use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use std::hash::{Hash, Hasher};

use captaine_model::{Effect, EffectKind, Id, Project, Title, TransitionKind, VideoProps};

use crate::{
    apply_effects, apply_effects_with, draw_layer, draw_layer_fx, layers_at, render_title, Canvas,
    Frame8, FrameCache, Layer, LayerFx, TimelineIndex, TransitionState, VideoReader,
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
/// How many frames a reversed clip decodes at a time.
const REVERSE_RUN: i64 = 24;

pub struct Renderer {
    project: Project,
    timeline: Id,
    index: TimelineIndex,
    opts: RenderOptions,
    cache: FrameCache,
    readers: HashMap<Id, (VideoReader, u64)>,
    titles: HashMap<u64, Arc<Frame8>>,
    /// Open matte videos (path -> reader), most recently used last
    mattes: HashMap<String, (VideoReader, u64)>,
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
            mattes: HashMap::new(),
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
        self.project.canvas(self.timeline)
    }

    fn fetch(
        &mut self,
        clip: Id,
        media: Id,
        frame: i64,
        back: bool,
    ) -> Result<Arc<Frame8>, RenderError> {
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
        // previews decode at the size they will be shown, not the size of the file
        let even = |v: u32| (((v as f64 * self.opts.decode_scale).round() as u32).max(2)) & !1;
        let (dw, dh) = if self.opts.decode_scale < 1.0 {
            (even(w), even(h))
        } else {
            (w, h)
        };
        if back {
            // a decoder only runs forwards: read a run of frames ending at this one and keep them all,
            // so the next frames of a reversed clip come from the cache
            let lo = (frame - REVERSE_RUN + 1).max(0);
            let mut r = VideoReader::open_with(&path, dw, dh, fps, lo, self.opts.hw_decode)
                .map_err(RenderError::Decode)?;
            let mut last = None;
            for i in lo..=frame {
                let f = r.frame(i).map_err(RenderError::Decode)?;
                self.cache.insert(media, i, f.clone());
                last = Some(f);
            }
            return last.ok_or_else(|| RenderError::Decode("no frames decoded".into()));
        }
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
            let r = VideoReader::open_with(&path, dw, dh, fps, frame, self.opts.hw_decode)
                .map_err(RenderError::Decode)?;
            self.readers.insert(clip, (r, self.clock));
        }
        let (reader, used) = self.readers.get_mut(&clip).expect("just ensured");
        *used = self.clock;
        let f = reader.frame(frame).map_err(RenderError::Decode)?;
        self.cache.insert(media, frame, f.clone());
        Ok(f)
    }

    /// Frame `index` of a matte video, or None if the file cannot be read.
    fn matte_frame(&mut self, path: &str, index: i64) -> Option<Arc<Frame8>> {
        self.clock += 1;
        if !self.mattes.contains_key(path) {
            if self.mattes.len() >= 8 {
                let oldest = self
                    .mattes
                    .iter()
                    .min_by_key(|(_, (_, t))| *t)
                    .map(|(k, _)| k.clone())?;
                self.mattes.remove(&oldest);
            }
            let (w, h, fps) = probe_video(path)?;
            let r = VideoReader::open(path, w, h, fps, index.max(0)).ok()?;
            self.mattes.insert(path.to_string(), (r, self.clock));
        }
        let (r, used) = self.mattes.get_mut(path)?;
        *used = self.clock;
        r.frame(index.max(0)).ok()
    }

    /// The picture of a title on the current canvas, drawn once and kept.
    fn title_picture(&mut self, t: &Title) -> Arc<Frame8> {
        let (w, h) = self.size();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!(
            "{}|{}|{:?}|{:?}|{w}x{h}",
            t.text, t.subtitle, t.style, t.shape
        )
        .hash(&mut hasher);
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
        #[allow(clippy::large_enum_variant)]
        enum Want {
            Clip {
                clip: Id,
                media: Id,
                frame: i64,
                props: VideoProps,
                back: bool,
                fx: LayerFx,
                effects: Vec<Effect>,
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
                    props: c.clip.video_at(c.local_tick),
                    back: c.clip.retime.backwards(),
                    fx: c.transition.map(transition_fx).unwrap_or_default(),
                    effects: c.clip.effects.clone(),
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
                    let mut props = t.title.video_at(t.local_tick);
                    props.opacity *= k;
                    let mut title = t.title.clone();
                    // typewriter: show only the first part of the text
                    if let Some(r) = title.anim.value("reveal", t.local_tick) {
                        let n = title.text.chars().count();
                        let shown = ((n as f64 * r / 100.0).ceil() as usize).min(n);
                        title.text = title.text.chars().take(shown).collect();
                    }
                    Want::Title { title, props }
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
                    back,
                    fx,
                    effects,
                } => {
                    let mut f = self.fetch(clip, media, frame, back)?;
                    let mattes: Vec<Option<Arc<Frame8>>> = effects
                        .iter()
                        .map(|e| {
                            (e.enabled && e.kind == EffectKind::Matte && !e.text.is_empty())
                                .then(|| self.matte_frame(&e.text, frame - e.params[3] as i64))
                                .flatten()
                        })
                        .collect();
                    if let Some(g) = apply_effects_with(&f, &effects, &mattes) {
                        f = Arc::new(g);
                    }
                    draw_layer_fx(&mut canvas, &f, &props, &fx);
                }
                Want::Title { title, props } => {
                    if title.adjustment {
                        // change everything drawn so far: through the effects and back, mixed by opacity
                        let before = Frame8 {
                            width: canvas.width,
                            height: canvas.height,
                            rgba: canvas.to_rgba8(),
                        };
                        if let Some(after) = apply_effects(&before, &title.effects) {
                            canvas.blend_adjusted(&after, props.opacity as f32);
                        }
                        continue;
                    }
                    let mut f = self.title_picture(&title);
                    if let Some(g) = apply_effects(&f, &title.effects) {
                        f = Arc::new(g);
                    }
                    draw_layer(&mut canvas, &f, &props);
                }
            }
        }
        Ok(canvas)
    }
}

/// What a transition does to the layer it is drawing at this moment.
fn transition_fx(t: TransitionState) -> LayerFx {
    let p = t.progress.clamp(0.0, 1.0);
    let mut fx = LayerFx::default();
    match (t.kind, t.incoming) {
        (TransitionKind::Dissolve, true) => fx.opacity = p,
        (TransitionKind::Dissolve, false) => {}
        (TransitionKind::Dip, true) => fx.opacity = (2.0 * p - 1.0).max(0.0),
        (TransitionKind::Dip, false) => fx.opacity = (1.0 - 2.0 * p).max(0.0),
        (TransitionKind::WipeRight, true) => fx.mask = Some([0.0, 0.0, p, 1.0]),
        (TransitionKind::WipeLeft, true) => fx.mask = Some([1.0 - p, 0.0, 1.0, 1.0]),
        (TransitionKind::WipeRight | TransitionKind::WipeLeft, false) => {}
        (TransitionKind::PushLeft, true) => fx.shift.0 = 1.0 - p,
        (TransitionKind::PushLeft, false) => fx.shift.0 = -p,
        (TransitionKind::PushRight, true) => fx.shift.0 = -(1.0 - p),
        (TransitionKind::PushRight, false) => fx.shift.0 = p,
    }
    fx
}

/// Size and frame rate of a video file, by asking ffprobe.
fn probe_video(path: &str) -> Option<(u32, u32, captaine_time::Rational)> {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut f = text.trim().split(',');
    let (w, h) = (f.next()?.parse().ok()?, f.next()?.parse().ok()?);
    let (n, d) = f.next()?.split_once('/')?;
    Some((
        w,
        h,
        captaine_time::Rational::new(n.parse().ok()?, d.parse().ok()?)?,
    ))
}
