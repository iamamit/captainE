use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use std::hash::{Hash, Hasher};

use captaine_model::{Id, Project, Title, VideoProps};

use crate::{
    draw_layer, layers_at, render_title, Canvas, Frame8, FrameCache, Layer, TimelineIndex,
    VideoReader,
};

#[derive(Debug)]
pub enum RenderError {
    NoTimeline(Id),
    Decode(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::NoTimeline(id) => write!(f, "timeline {id} not found"),
            RenderError::Decode(s) => write!(f, "{s}"),
        }
    }
}
impl std::error::Error for RenderError {}

#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Read `MediaRef::proxy` instead of the original when a media item has one
    pub use_proxy: bool,
    pub cache_bytes: usize,
    /// Open decoder processes kept alive at once
    pub max_readers: usize,
    /// Decode pictures at this fraction of their size (1.0 = full). Previews use the canvas scale, which
    /// moves far fewer pixels through the decoder pipe; export leaves it at 1.0.
    pub decode_scale: f64,
    /// Use the platform's hardware video decoder. Not bit-exact with software decoding, so never for export.
    pub hw_decode: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            use_proxy: false,
            cache_bytes: 512 << 20,
            max_readers: 8,
            decode_scale: 1.0,
            hw_decode: false,
        }
    }
}

/// Renders frames of one timeline from a project snapshot.
pub struct Renderer {
    project: Project,
    timeline: Id,
    index: TimelineIndex,
    opts: RenderOptions,
    cache: FrameCache,
    readers: HashMap<Id, (VideoReader, u64)>,
    titles: HashMap<u64, Arc<Frame8>>,
    clock: u64,
}

impl Renderer {
    pub fn new(
        project: &Project,
        timeline: Id,
        opts: RenderOptions,
    ) -> Result<Renderer, RenderError> {
        let index =
            TimelineIndex::new(project, timeline).ok_or(RenderError::NoTimeline(timeline))?;
        Ok(Renderer {
            project: project.clone(),
            timeline,
            index,
            cache: FrameCache::new(opts.cache_bytes),
            opts,
            readers: HashMap::new(),
            titles: HashMap::new(),
            clock: 0,
        })
    }

    /// Switch to another snapshot of the project (after an edit) while keeping decoders and cached frames:
    /// decoded pictures do not depend on the timeline.
    pub fn set_project(&mut self, project: &Project) -> Result<(), RenderError> {
        let index = TimelineIndex::new(project, self.timeline)
            .ok_or(RenderError::NoTimeline(self.timeline))?;
        self.project = project.clone();
        self.index = index;
        Ok(())
    }

    pub fn size(&self) -> (u32, u32) {
        (self.project.settings.width, self.project.settings.height)
    }

    fn fetch(&mut self, clip: Id, media: Id, frame: i64) -> Result<Arc<Frame8>, RenderError> {
        if let Some(f) = self.cache.get(media, frame) {
            return Ok(f);
        }
        let m = &self.project.media[&media];
        let (path, w, h) = match (&m.proxy, self.opts.use_proxy) {
            (Some(p), true) => (p.clone(), m.width, m.height),
            _ => (m.path.clone(), m.width, m.height),
        };
        let fps = m
            .duration
            .map(|d| d.rate)
            .unwrap_or(self.project.settings.frame_rate);
        self.clock += 1;
        if !self.readers.contains_key(&clip) {
            if self.readers.len() >= self.opts.max_readers {
                let oldest = *self
                    .readers
                    .iter()
                    .min_by_key(|(_, (_, t))| *t)
                    .map(|(k, _)| k)
                    .expect("non-empty");
                self.readers.remove(&oldest);
            }
            // previews decode at the size they will be shown, not the size of the file
            let even = |v: u32| (((v as f64 * self.opts.decode_scale).round() as u32).max(2)) & !1;
            let (w, h) = if self.opts.decode_scale < 1.0 {
                (even(w), even(h))
            } else {
                (w, h)
            };
            let r = VideoReader::open_with(&path, w, h, fps, frame, self.opts.hw_decode)
                .map_err(RenderError::Decode)?;
            self.readers.insert(clip, (r, self.clock));
        }
        let (reader, used) = self.readers.get_mut(&clip).expect("just ensured");
        *used = self.clock;
        let f = reader.frame(frame).map_err(RenderError::Decode)?;
        self.cache.insert(media, frame, f.clone());
        Ok(f)
    }

    /// The picture of a title on the current canvas, drawn once and kept.
    fn title_picture(&mut self, t: &Title) -> Arc<Frame8> {
        let (w, h) = self.size();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!("{}|{}|{:?}|{w}x{h}", t.text, t.subtitle, t.style).hash(&mut hasher);
        let key = hasher.finish();
        if self.titles.len() > 32 {
            self.titles.clear();
        }
        self.titles
            .entry(key)
            .or_insert_with(|| Arc::new(render_title(t, w, h)))
            .clone()
    }

    /// The composited picture at timeline tick `tick`.
    pub fn render(&mut self, tick: i64) -> Result<Canvas, RenderError> {
        let (w, h) = self.size();
        let mut canvas = Canvas::black(w, h);
        // collect first: layers borrow the project, fetching needs `&mut self`
        enum Want {
            Clip {
                clip: Id,
                media: Id,
                frame: i64,
                props: VideoProps,
            },
            Title {
                title: Title,
                props: VideoProps,
            },
        }
        let wanted: Vec<Want> = layers_at(&self.project, self.timeline, &self.index, tick)
            .into_iter()
            .map(|l| match l {
                Layer::Clip(c) => Want::Clip {
                    clip: c.clip.id,
                    media: c.media.id,
                    frame: c.source_frame,
                    props: c.clip.video,
                },
                Layer::Title(t) => {
                    let dur = t.title.duration.value;
                    let mut k = 1.0f64;
                    if t.title.fade_in > 0 && t.local_tick < t.title.fade_in {
                        k = k.min(t.local_tick as f64 / t.title.fade_in as f64);
                    }
                    if t.title.fade_out > 0 && dur - t.local_tick <= t.title.fade_out {
                        k = k.min((dur - t.local_tick) as f64 / t.title.fade_out as f64);
                    }
                    let mut props = t.title.video;
                    props.opacity *= k;
                    Want::Title {
                        title: t.title.clone(),
                        props,
                    }
                }
            })
            .collect();
        // drop decoders of clips that are no longer on screen (sequential export keeps this small)
        self.readers.retain(|id, _| {
            wanted
                .iter()
                .any(|w| matches!(w, Want::Clip { clip, .. } if clip == id))
        });
        for want in wanted {
            match want {
                Want::Clip {
                    clip,
                    media,
                    frame,
                    props,
                } => {
                    let f = self.fetch(clip, media, frame)?;
                    draw_layer(&mut canvas, &f, &props);
                }
                Want::Title { title, props } => {
                    let f = self.title_picture(&title);
                    draw_layer(&mut canvas, &f, &props);
                }
            }
        }
        Ok(canvas)
    }
}
