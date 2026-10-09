use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use captaine_model::{Id, Project};

use crate::{draw_layer, layers_at, Canvas, Frame8, FrameCache, TimelineIndex, VideoReader};

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
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            use_proxy: false,
            cache_bytes: 512 << 20,
            max_readers: 8,
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
            clock: 0,
        })
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
            let r = VideoReader::open(&path, w, h, fps, frame).map_err(RenderError::Decode)?;
            self.readers.insert(clip, (r, self.clock));
        }
        let (reader, used) = self.readers.get_mut(&clip).expect("just ensured");
        *used = self.clock;
        let f = reader.frame(frame).map_err(RenderError::Decode)?;
        self.cache.insert(media, frame, f.clone());
        Ok(f)
    }

    /// The composited picture at timeline tick `tick`.
    pub fn render(&mut self, tick: i64) -> Result<Canvas, RenderError> {
        let (w, h) = self.size();
        let mut canvas = Canvas::black(w, h);
        // collect first: layers borrow the project, fetching needs `&mut self`
        let wanted: Vec<_> = layers_at(&self.project, self.timeline, &self.index, tick)
            .into_iter()
            .map(|l| (l.clip.id, l.media.id, l.source_frame, l.clip.video))
            .collect();
        // drop decoders of clips that are no longer on screen (sequential export keeps this small)
        self.readers
            .retain(|id, _| wanted.iter().any(|w| w.0 == *id));
        for (clip, media, frame, props) in wanted {
            let f = self.fetch(clip, media, frame)?;
            draw_layer(&mut canvas, &f, &props);
        }
        Ok(canvas)
    }
}
