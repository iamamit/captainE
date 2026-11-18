use rayon::prelude::*;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use std::hash::{Hash, Hasher};

use captaine_model::{Effect, EffectKind, Id, Project, Title, TransitionKind, VideoProps};

use crate::{
    apply_effects_at, draw_layer, draw_layer_fx, layers_at, render_title, Canvas, Frame8,
    FrameCache, FxTime, Layer, LayerFx, TimelineIndex, TransitionState, VideoReader,
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
    /// Renderers of the sequences played inside this one
    nested: HashMap<Id, Renderer>,
    /// Open matte videos (path -> reader), most recently used last
    mattes: HashMap<String, (VideoReader, u64)>,
    clock: u64,
    /// The tick rendered last: the next one in a row means playback or export, which reads ahead
    last_tick: Option<i64>,
    /// Decoders opened early for clips about to come on screen: clip -> the tick it is first needed at
    pending: HashMap<Id, i64>,
    profile: RenderProfile,
    /// Picture buffers handed back with [`recycle`](Renderer::recycle), reused instead of allocating a new
    /// canvas (133 MB at 4K) for every frame
    spare: Vec<Vec<f32>>,
    /// Size and modification time of each media file when its pictures were decoded, so a file replaced on
    /// disk is decoded again instead of showing the old pictures
    stamps: HashMap<Id, (u64, u64)>,
}

/// Size and modification time (nanoseconds) of a file, or zeros when it cannot be read.
fn file_stamp(path: &str) -> (u64, u64) {
    std::fs::metadata(path).map_or((0, 0), |m| {
        let t = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as u64);
        (m.len(), t)
    })
}

/// Where render time went, summed over frames (see [`Renderer::profile`]).
#[derive(Clone, Debug, Default)]
pub struct RenderProfile {
    pub frames: u64,
    pub canvas: std::time::Duration,
    pub decode: std::time::Duration,
    pub clip_effects: std::time::Duration,
    pub draw: std::time::Duration,
    pub adjustment: std::time::Duration,
    pub titles: std::time::Duration,
}

/// How far ahead (seconds) the decoder of the next clip is started during playback and export, so its
/// seek and first frames are ready by the cut instead of stalling it.
const LOOKAHEAD_SECS: f64 = 0.8;

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
            nested: HashMap::new(),
            mattes: HashMap::new(),
            clock: 0,
            last_tick: None,
            pending: HashMap::new(),
            profile: RenderProfile::default(),
            spare: Vec::new(),
            stamps: project
                .media
                .values()
                .map(|m| (m.id, file_stamp(&m.path)))
                .collect(),
        })
    }

    /// Switch to another snapshot of the project (after an edit) while keeping decoders and cached frames:
    /// decoded pictures do not depend on the timeline.
    pub fn set_project(&mut self, project: &Project) -> Result<(), RenderError> {
        let index = TimelineIndex::new(project, self.timeline)
            .ok_or(RenderError::NoTimeline(self.timeline))?;
        self.project = project.clone();
        self.index = index;
        // a media file replaced on disk (same name, new picture): drop its cached pictures, and the open
        // decoders (they are per clip; this is rare, so all of them are reopened)
        let mut replaced = false;
        for m in project.media.values() {
            let stamp = file_stamp(&m.path);
            if self
                .stamps
                .insert(m.id, stamp)
                .is_some_and(|old| old != stamp)
            {
                self.cache.forget(m.id);
                replaced = true;
            }
        }
        if replaced {
            self.readers.clear();
            self.pending.clear();
        }
        // sequences played inside this one follow the edit too; one that is gone is forgotten
        self.nested.retain(|_, r| r.set_project(project).is_ok());
        Ok(())
    }

    /// Give a rendered picture back once it has been used, so its memory serves a later frame.
    pub fn recycle(&mut self, canvas: Canvas) {
        if self.spare.len() < 3 {
            self.spare.push(canvas.data);
        }
    }

    /// Time spent in each stage since the renderer was made.
    pub fn profile(&self) -> &RenderProfile {
        &self.profile
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
        if let Some(inner) = self.project.media[&media].nested_timeline() {
            return self.nested_frame(inner, frame);
        }
        let (path, dw, dh, fps, space, hw) = self.decode_setup(media, Some(clip));
        // a cached picture of this media made at another size (another clip shows it bigger or smaller) does
        // not count
        if let Some(f) = self
            .cache
            .get(media, frame)
            .filter(|f| f.width == dw && f.height == dh)
        {
            return Ok(f);
        }
        self.clock += 1;
        if back {
            // a decoder only runs forwards: read a run of frames ending at this one and keep them all,
            // so the next frames of a reversed clip come from the cache
            let lo = (frame - REVERSE_RUN + 1).max(0);
            let mut r = VideoReader::open_in(&path, dw, dh, fps, lo, hw, space)
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
            let r = VideoReader::open_in(&path, dw, dh, fps, frame, hw, space)
                .map_err(RenderError::Decode)?;
            self.readers.insert(clip, (r, self.clock));
        }
        self.pending.remove(&clip);
        let (reader, used) = self.readers.get_mut(&clip).expect("just ensured");
        *used = self.clock;
        let f = reader.frame(frame).map_err(RenderError::Decode)?;
        self.cache.insert(media, frame, f.clone());
        Ok(f)
    }

    /// How big `clip` is shown on this canvas relative to its media's own size (its scale, the smallest of its
    /// scale keys, and a camera-shake zoom): the size worth decoding it at. At most 1.
    fn shown_scale(&self, clip: Id, media: &captaine_model::MediaRef) -> f64 {
        let (cw, ch) = self.size();
        let (mw, mh) = (media.width.max(1) as f64, media.height.max(1) as f64);
        let fit = (cw as f64 / mw).min(ch as f64 / mh);
        let zoom = self
            .project
            .timelines
            .values()
            .flat_map(|t| t.tracks.iter())
            .flat_map(|t| t.items.iter())
            .find_map(|i| i.as_clip().filter(|c| c.id == clip))
            .map_or(1.0, |c| {
                let t = c.video.transform.scale;
                // keyframed scale: size for the smallest key, where the clip spends its time; a short punch-in
                // above it is stretched a little rather than the whole clip being shrunk (and softened)
                let keys = c.anim.keys("scale");
                let base = if keys.is_empty() {
                    t.0.abs().max(t.1.abs())
                } else {
                    keys.iter().map(|k| k.v / 100.0).fold(f64::MAX, f64::min)
                };
                let shake = c
                    .effects
                    .iter()
                    .filter(|e| e.enabled && e.kind == EffectKind::CameraShake)
                    .map(|e| e.params[3] / 100.0)
                    .fold(1.0f64, f64::max);
                base * shake
            });
        (fit * zoom).min(1.0)
    }

    /// Where and how a media item is decoded: file (or proxy), decode size, frame rate, colour encoding.
    /// `clip` (when known) decides the size: as big as it is shown, so the picture is scaled down once, by
    /// FFmpeg's Lanczos filter, and the compositor draws it at about 1:1 instead of averaging pixels.
    fn decode_setup(
        &self,
        media: Id,
        clip: Option<Id>,
    ) -> (
        String,
        u32,
        u32,
        captaine_time::Rational,
        captaine_model::InputSpace,
        bool,
    ) {
        let m = &self.project.media[&media];
        let (path, proxy) = match (&m.proxy, self.opts.use_proxy) {
            (Some(p), true) => (p.clone(), true),
            _ => (m.path.clone(), false),
        };
        // the video chip pays off for camera originals (long-GOP HEVC/H.264, big frames: about twice as fast to
        // the first frame); a proxy is small and every frame is a keyframe, where starting the hardware decoder
        // (~75 ms) costs more than it saves, so proxies decode in software
        let hw = self.opts.hw_decode && !proxy;
        let fps = m
            .duration
            .map(|d| d.rate)
            .unwrap_or(self.project.settings.frame_rate);
        // the colour encoding: what the user said, else what the file says (a proxy is already Rec.709)
        let space = match self.project.meta.get(&media).map(|x| x.input) {
            Some(s) if s != captaine_model::InputSpace::Auto => s,
            _ if m.proxy.is_some() && self.opts.use_proxy => captaine_model::InputSpace::Rec709,
            _ => captaine_media::detect_space(&path),
        };
        // decode at the size the clip is shown at (never bigger than the file, nor than a preview asks for)
        let mut ds = self.opts.decode_scale.min(1.0);
        if let Some(c) = clip {
            // exactly the shown size: the compositor then maps it 1:1 (any leftover resampling, even a few
            // percent, softens the picture more than a clean Lanczos resize does)
            ds = ds.min(self.shown_scale(c, m));
        }
        let even = |v: u32| (((v as f64 * ds).round() as u32).max(2)) & !1;
        let (dw, dh) = if ds < 0.999 {
            (even(m.width), even(m.height))
        } else {
            (m.width, m.height)
        };
        (path, dw, dh, fps, space, hw)
    }

    /// During playback and export: start the decoders of clips that come on screen within
    /// [`LOOKAHEAD_SECS`], at the frame each will first show, so a cut does not wait for a seek.
    fn read_ahead(&mut self, tick: i64, on_screen: &[Id]) {
        let rate = self.project.timeline(self.timeline).map(|t| t.rate);
        let Some(rate) = rate else { return };
        let look = tick + (LOOKAHEAD_SECS * rate.as_f64()).round().max(1.0) as i64;
        // (clip, media, first tick on screen, source frame then)
        let mut upcoming: Vec<(Id, Id, i64, i64)> = Vec::new();
        for l in layers_at(&self.project, self.timeline, &self.index, look) {
            let Layer::Clip(c) = l else { continue };
            if c.clip.retime.backwards()
                || c.media.nested_timeline().is_some()
                || !c.media.has_video
                || on_screen.contains(&c.clip.id)
                || self.readers.contains_key(&c.clip.id)
            {
                continue;
            }
            let first = (look - c.local_tick).max(tick + 1);
            upcoming.push((c.clip.id, c.media.id, first, c.source_frame));
        }
        for (clip, media, first, fallback) in upcoming {
            let frame = layers_at(&self.project, self.timeline, &self.index, first)
                .into_iter()
                .find_map(|l| match l {
                    Layer::Clip(d) if d.clip.id == clip => Some(d.source_frame),
                    _ => None,
                })
                .unwrap_or(fallback);
            let (path, dw, dh, fps, space, hw) = self.decode_setup(media, Some(clip));
            if self
                .cache
                .get(media, frame)
                .is_some_and(|f| f.width == dw && f.height == dh)
            {
                continue;
            }
            if let Ok(r) = VideoReader::open_in(&path, dw, dh, fps, frame, hw, space) {
                self.clock += 1;
                self.readers.insert(clip, (r, self.clock));
                self.pending.insert(clip, first);
            }
        }
    }

    /// The picture of another sequence of the project at its tick `tick`. Not cached: the sequence can
    /// change between two renders, and its own decoders are already cached.
    fn nested_frame(&mut self, timeline: Id, tick: i64) -> Result<Arc<Frame8>, RenderError> {
        if !self.nested.contains_key(&timeline) {
            let r = Renderer::new(&self.project, timeline, self.opts.clone())?;
            self.nested.insert(timeline, r);
        }
        let r = self.nested.get_mut(&timeline).expect("just ensured");
        let canvas = r.render(tick.max(0))?;
        Ok(Arc::new(Frame8 {
            width: canvas.width,
            height: canvas.height,
            rgba: canvas.to_rgba8(),
        }))
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
        let t0 = std::time::Instant::now();
        let n = (w as usize) * (h as usize) * 4;
        let mut canvas = match self.spare.pop() {
            Some(mut data) if data.len() == n => {
                data.par_chunks_mut(4 * 4096).for_each(|run| {
                    for p in run.chunks_exact_mut(4) {
                        p.copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
                    }
                });
                Canvas {
                    width: w,
                    height: h,
                    data,
                }
            }
            _ => Canvas::black(w, h),
        };
        self.profile.canvas += t0.elapsed();
        self.profile.frames += 1;
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
                at: FxTime,
            },
            Title {
                title: Title,
                props: VideoProps,
                at: FxTime,
            },
        }
        let fps = self
            .project
            .timeline(self.timeline)
            .map_or(self.project.settings.frame_rate, |t| t.rate)
            .as_f64();
        let wanted: Vec<Want> = layers_at(&self.project, self.timeline, &self.index, tick)
            .into_iter()
            .map(|l| match l {
                Layer::Clip(c) => Want::Clip {
                    clip: c.clip.id,
                    media: c.media.id,
                    frame: c.source_frame,
                    props: c.clip.video_at(c.local_tick),
                    back: c.clip.retime.backwards(),
                    fx: match c.transition {
                        Some(t) => transition_fx(t),
                        None => LayerFx {
                            snap: ["position.x", "position.y", "scale", "rotation"]
                                .iter()
                                .all(|p| c.clip.anim.keys(p).is_empty()),
                            ..LayerFx::default()
                        },
                    },
                    effects: {
                        let mut e = c.clip.effects_at(c.local_tick).into_owned();
                        if let Some(t) = c.transition {
                            e.extend(transition_effects(t));
                        }
                        e
                    },
                    at: FxTime::new(c.source_frame.max(0) as u64, c.local_tick as f64 / fps),
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
                    title.effects = t.title.effects_at(t.local_tick).into_owned();
                    // typewriter: show only the first part of the text
                    if let Some(r) = title.anim.value("reveal", t.local_tick) {
                        let n = title.text.chars().count();
                        let shown = ((n as f64 * r / 100.0).ceil() as usize).min(n);
                        title.text = title.text.chars().take(shown).collect();
                    }
                    let at = FxTime::new(t.local_tick.max(0) as u64, t.local_tick as f64 / fps);
                    Want::Title { title, props, at }
                }
            })
            .collect();
        // drop decoders of clips that are no longer on screen (sequential export keeps this small), except
        // the ones started early for a clip still to come
        let on_screen: Vec<Id> = wanted
            .iter()
            .filter_map(|w| match w {
                Want::Clip { clip, .. } => Some(*clip),
                _ => None,
            })
            .collect();
        self.pending.retain(|_, first| *first >= tick);
        let pending = &self.pending;
        self.readers
            .retain(|id, _| on_screen.contains(id) || pending.contains_key(id));
        let sequential = self.last_tick.is_some_and(|t| tick == t + 1);
        self.last_tick = Some(tick);
        if sequential {
            self.read_ahead(tick, &on_screen);
        }
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
                    at,
                } => {
                    let t = std::time::Instant::now();
                    let mut f = self.fetch(clip, media, frame, back)?;
                    self.profile.decode += t.elapsed();
                    let t = std::time::Instant::now();
                    let mattes: Vec<Option<Arc<Frame8>>> = effects
                        .iter()
                        .map(|e| {
                            (e.enabled && e.kind == EffectKind::Matte && !e.text.is_empty())
                                .then(|| self.matte_frame(&e.text, frame - e.params[3] as i64))
                                .flatten()
                        })
                        .collect();
                    if let Some(g) = apply_effects_at(&f, &effects, &mattes, at) {
                        f = Arc::new(g);
                    }
                    if fx.motion_blur > 0.0 {
                        let len = (fx.motion_blur * f.width as f64).round() as usize;
                        f = Arc::new(crate::composite::horizontal_blur(&f, len, fx.wrap_x));
                    }
                    self.profile.clip_effects += t.elapsed();
                    let t = std::time::Instant::now();
                    draw_shadow(&mut canvas, &f, &effects, &props, &fx);
                    draw_layer_fx(&mut canvas, &f, &props, &fx);
                    self.profile.draw += t.elapsed();
                }
                Want::Title { title, props, at } => {
                    let t = std::time::Instant::now();
                    if title.adjustment {
                        // change everything drawn so far: through the effects and back, mixed by opacity
                        let before = Frame8 {
                            width: canvas.width,
                            height: canvas.height,
                            rgba: canvas.to_rgba8(),
                        };
                        if title
                            .effects
                            .iter()
                            .any(|e| e.enabled && !e.kind.is_audio())
                        {
                            let after = crate::effects::apply_effects_owned(
                                before,
                                &title.effects,
                                &[],
                                at,
                            );
                            canvas.blend_adjusted(&after, props.opacity as f32);
                        }
                        self.profile.adjustment += t.elapsed();
                        continue;
                    }
                    let mut f = self.title_picture(&title);
                    if let Some(g) = apply_effects_at(&f, &title.effects, &[], at) {
                        f = Arc::new(g);
                    }
                    draw_shadow(&mut canvas, &f, &title.effects, &props, &LayerFx::default());
                    draw_layer(&mut canvas, &f, &props);
                    self.profile.titles += t.elapsed();
                }
            }
        }
        Ok(canvas)
    }
}

/// What a transition does to the layer it is drawing at this moment.
/// A Drop shadow effect's shadow, drawn under the layer: the layer's silhouette, softened and padded so the blur
/// is not cut at the layer's edge, moved by the distance and angle (sizes for a 1080-pixel-tall canvas).
fn draw_shadow(
    canvas: &mut Canvas,
    f: &Frame8,
    effects: &[Effect],
    props: &VideoProps,
    fx: &LayerFx,
) {
    let Some(e) = effects
        .iter()
        .find(|e| e.enabled && e.kind == EffectKind::DropShadow)
    else {
        return;
    };
    let p = &e.params;
    let ck = canvas.height as f64 / 1080.0;
    let (fw, fh) = (f.width as f64, f.height as f64);
    let fit = (canvas.width as f64 / fw).min(canvas.height as f64 / fh);
    let t = &props.transform;
    // softness is given on the canvas; the shadow is made at the layer's own size
    let layer_k = ck / (fit * t.scale.1.abs()).max(1e-6);
    let pad = (p[2] * layer_k * 1.5).ceil().max(0.0) as u32;
    let mut padded = Frame8 {
        width: f.width + 2 * pad,
        height: f.height + 2 * pad,
        rgba: vec![0; ((f.width + 2 * pad) * (f.height + 2 * pad) * 4) as usize],
    };
    let pw = padded.width as usize;
    for y in 0..f.height as usize {
        let src = &f.rgba[y * f.width as usize * 4..(y + 1) * f.width as usize * 4];
        let at = ((y + pad as usize) * pw + pad as usize) * 4;
        padded.rgba[at..at + src.len()].copy_from_slice(src);
    }
    let shadow = crate::library::shadow_of(&padded, p, layer_k);
    let pfit = (canvas.width as f64 / padded.width as f64)
        .min(canvas.height as f64 / padded.height as f64);
    let mut sp = *props;
    let grow = fit / pfit;
    sp.transform.scale = (t.scale.0 * grow, t.scale.1 * grow);
    let (dy, dx) = p[1].to_radians().sin_cos();
    sp.transform.position = (t.position.0 + dx * p[0] * ck, t.position.1 + dy * p[0] * ck);
    sp.opacity *= p[3] / 100.0;
    sp.color = Default::default();
    draw_layer_fx(canvas, &shadow, &sp, fx);
}

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
        (TransitionKind::WipeDown, true) => fx.mask = Some([0.0, 0.0, 1.0, p]),
        (TransitionKind::WipeUp, true) => fx.mask = Some([0.0, 1.0 - p, 1.0, 1.0]),
        (TransitionKind::WipeDown | TransitionKind::WipeUp, false) => {}
        (TransitionKind::SlideLeft, true) => fx.shift.0 = 1.0 - p,
        (TransitionKind::SlideRight, true) => fx.shift.0 = -(1.0 - p),
        (TransitionKind::SlideLeft | TransitionKind::SlideRight, false) => {}
        (TransitionKind::Iris, true) => fx.iris = Some(p),
        (TransitionKind::Iris, false) => {}
        (TransitionKind::ZoomIn, true) => {
            fx.scale = 0.3 + 0.7 * p;
            fx.opacity = p;
        }
        (TransitionKind::ZoomIn, false) => {}
        (TransitionKind::WhipLeft | TransitionKind::WhipRight, incoming) => {
            // like a real whip pan: the last shot swings away for the first half, the next one swings in for the
            // second, each repeated sideways so the frame never shows an edge; the switch hides in the peak blur
            let e = p * p * (3.0 - 2.0 * p);
            let dir = if t.kind == TransitionKind::WhipLeft {
                -1.0
            } else {
                1.0
            };
            if incoming != (p >= 0.5) {
                fx.opacity = 0.0;
            }
            fx.shift.0 = if incoming { -dir * (1.0 - e) } else { dir * e };
            fx.motion_blur = 0.25 * (4.0 * p * (1.0 - p));
            fx.wrap_x = true;
        }
        (TransitionKind::BlurDissolve | TransitionKind::LightLeak, true) => fx.opacity = p,
        (TransitionKind::BlurDissolve | TransitionKind::LightLeak, false) => {}
        // the switch hides in the brightest moment
        (TransitionKind::DipToWhite | TransitionKind::Flash | TransitionKind::Glitch, incoming) => {
            if incoming != (p >= 0.5) {
                fx.opacity = 0.0;
            }
        }
        (TransitionKind::CrossZoom, incoming) => {
            if incoming != (p >= 0.5) {
                fx.opacity = 0.0;
            }
            let e = (1.0 - (2.0 * p - 1.0).abs()).powi(2);
            fx.scale = 1.0 + 2.0 * e;
        }
        (TransitionKind::Spin, incoming) => {
            if incoming != (p >= 0.5) {
                fx.opacity = 0.0;
            }
            let e = p * p * (3.0 - 2.0 * p);
            fx.spin = if incoming {
                -180.0 * (1.0 - e)
            } else {
                180.0 * e
            };
            // big enough that the turned picture still covers the frame's corners
            fx.scale = 1.0 + 0.8 * (1.0 - (2.0 * p - 1.0).abs());
        }
        (TransitionKind::ClockWipe, true) => fx.clock = Some(p),
        (TransitionKind::BarnDoors, true) => {
            fx.mask = Some([0.5 - p / 2.0, 0.0, 0.5 + p / 2.0, 1.0])
        }
        (
            TransitionKind::ClockWipe | TransitionKind::BarnDoors | TransitionKind::LumaFade,
            false,
        ) => {}
        (TransitionKind::LumaFade, true) => {}
    }
    fx
}

/// Effects a transition runs on the layers while it plays: blur that peaks at the cut, the flash of white, the
/// zoom and spin streaks, glitching, the wash of light, and the brightness key that lets the next shot in.
fn transition_effects(t: TransitionState) -> Vec<Effect> {
    let p = t.progress.clamp(0.0, 1.0);
    let peak = 1.0 - (2.0 * p - 1.0).abs();
    let fx = |kind: EffectKind, params: &[(usize, f64)]| {
        let mut e = Effect::new(kind);
        for (i, v) in params {
            let d = &kind.params()[*i];
            e.params[*i] = v.clamp(d.min, d.max);
        }
        e
    };
    match t.kind {
        TransitionKind::BlurDissolve => vec![fx(EffectKind::Blur, &[(0, 40.0 * peak)])],
        TransitionKind::DipToWhite => vec![fx(EffectKind::Levels, &[(3, 255.0 * peak)])],
        TransitionKind::Flash => {
            let m = (-((p - 0.5) / 0.14).powi(2)).exp();
            vec![
                fx(EffectKind::Levels, &[(3, 240.0 * m)]),
                fx(EffectKind::Glow, &[(0, 50.0), (2, 150.0 * m)]),
            ]
        }
        TransitionKind::CrossZoom => vec![fx(EffectKind::ZoomBlur, &[(0, 60.0 * peak)])],
        TransitionKind::Spin => vec![fx(EffectKind::RadialBlur, &[(0, 25.0 * peak)])],
        TransitionKind::Glitch => vec![fx(
            EffectKind::Glitch,
            &[(0, 100.0 * peak.powf(0.7)), (2, 20.0)],
        )],
        TransitionKind::LightLeak => {
            vec![fx(EffectKind::LightLeak, &[(0, 100.0 * peak), (1, 2.0)])]
        }
        TransitionKind::LumaFade if t.incoming => vec![fx(
            EffectKind::LumaKey,
            &[(0, 100.0 * (1.0 - p)), (1, 20.0 * (1.0 - p)), (2, 0.0)],
        )],
        _ => Vec::new(),
    }
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
