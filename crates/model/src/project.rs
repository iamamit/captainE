use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use captaine_time::{Rational, RationalTime, Rounding};
use serde::{Deserialize, Serialize};

use crate::anim::{is_property, Anim};
use crate::effects::{check_effects, Effect};
use crate::Id;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub frame_rate: Rational,
    pub width: u32,
    pub height: u32,
    pub sample_rate: u32,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings {
            frame_rate: Rational::FPS_30,
            width: 1920,
            height: 1080,
            sample_rate: 48_000,
        }
    }
}

/// A source file in the media pool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaRef {
    pub id: Id,
    pub name: String,
    pub path: String,
    /// Length at the media's native rate, when probed. Used to bound trims.
    pub duration: Option<RationalTime>,
    #[serde(default)]
    pub has_audio: bool,
    /// Display size after rotation metadata is applied. 0 for audio-only media.
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default = "yes")]
    pub has_video: bool,
    /// Lightweight stand-in used for playback when proxy mode is on
    #[serde(default)]
    pub proxy: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum BlendMode {
    Normal,
    Add,
    Multiply,
    Screen,
}

/// Placement of a clip's picture on the canvas. The picture is first fitted inside the canvas
/// (letterboxed), then `scale`, `rotation_deg` and `position` apply.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// Offset from the canvas center, in canvas pixels (+x right, +y down).
    pub position: (f64, f64),
    /// 1.0 = the fitted size.
    pub scale: (f64, f64),
    /// Clockwise degrees around the picture center.
    pub rotation_deg: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            position: (0.0, 0.0),
            scale: (1.0, 1.0),
            rotation_deg: 0.0,
        }
    }
}

/// Basic per-clip grade, applied in linear light before blending. The defaults change nothing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorProps {
    /// Stops of exposure (+1 doubles the light).
    pub exposure: f64,
    /// 1.0 = unchanged; pivots around mid-grey.
    pub contrast: f64,
    /// Percent; 100 = unchanged, 0 = black and white.
    pub saturation: f64,
    /// Kelvin; 6500 = neutral, higher is warmer (more red, less blue), lower is cooler.
    pub temperature: f64,
}

impl ColorProps {
    pub const NEUTRAL_KELVIN: f64 = 6500.0;

    pub fn is_neutral(&self) -> bool {
        *self == ColorProps::default()
    }
}

impl Default for ColorProps {
    fn default() -> Self {
        ColorProps {
            exposure: 0.0,
            contrast: 1.0,
            saturation: 100.0,
            temperature: ColorProps::NEUTRAL_KELVIN,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoProps {
    pub transform: Transform,
    /// 0.0..=1.0
    pub opacity: f64,
    pub blend: BlendMode,
    #[serde(default)]
    pub color: ColorProps,
}

impl Default for VideoProps {
    fn default() -> Self {
        VideoProps {
            transform: Transform::default(),
            opacity: 1.0,
            blend: BlendMode::Normal,
            color: ColorProps::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioProps {
    pub muted: bool,
    pub gain_db: f64,
    /// -1.0 (left) ..= 1.0 (right)
    pub pan: f64,
    /// Linear fades, in ticks of the timeline rate. `fade_in + fade_out` may not exceed the clip.
    pub fade_in: i64,
    pub fade_out: i64,
}

impl Default for AudioProps {
    fn default() -> Self {
        AudioProps {
            muted: false,
            gain_db: 0.0,
            pan: 0.0,
            fade_in: 0,
            fade_out: 0,
        }
    }
}

/// A piece of media on a track.
///
/// `source_start` is at the media's native rate; `duration` is at the timeline rate.
/// (Speed changes will arrive with a time map; for now playback is 1x.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: Id,
    pub name: String,
    pub media: Id,
    pub source_start: RationalTime,
    pub duration: RationalTime,
    pub enabled: bool,
    #[serde(default)]
    pub video: VideoProps,
    #[serde(default)]
    pub audio: AudioProps,
    /// Keyframes, by property (times relative to the start of the clip)
    #[serde(default, skip_serializing_if = "Anim::is_empty")]
    pub anim: Anim,
    /// Speed, direction and freeze; the identity (normal speed, forward) is not stored
    #[serde(default, skip_serializing_if = "Retime::is_identity")]
    pub retime: Retime,
    /// A transition from the clip before this one, centred on the cut between them
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    /// Picture effects, applied in order before the transform
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// Clips with the same link id are edited together (a video clip and its separated audio)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Id>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransitionKind {
    /// The next picture fades in over the last
    Dissolve,
    /// Fade out to nothing, then in
    Dip,
    /// A hard edge sweeps across: left to right, or right to left
    WipeRight,
    WipeLeft,
    /// The next picture pushes the last one out sideways
    PushLeft,
    PushRight,
}

impl TransitionKind {
    pub const ALL: [TransitionKind; 6] = [
        TransitionKind::Dissolve,
        TransitionKind::Dip,
        TransitionKind::WipeRight,
        TransitionKind::WipeLeft,
        TransitionKind::PushLeft,
        TransitionKind::PushRight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TransitionKind::Dissolve => "Dissolve",
            TransitionKind::Dip => "Dip to black",
            TransitionKind::WipeRight => "Wipe right",
            TransitionKind::WipeLeft => "Wipe left",
            TransitionKind::PushLeft => "Push left",
            TransitionKind::PushRight => "Push right",
        }
    }
}

/// `duration` ticks of the timeline rate, half before the cut and half after.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub kind: TransitionKind,
    pub duration: i64,
}

impl Transition {
    /// How much of the transition falls before the cut, and how much after.
    pub fn split(&self) -> (i64, i64) {
        (self.duration / 2, self.duration - self.duration / 2)
    }
}

/// How a clip's source plays back. `speed` is a ratio (2.0 = twice as fast); a freeze shows one source
/// frame (the first of the window) for the whole clip and has no sound.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Retime {
    pub speed: f64,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub freeze: bool,
}

impl Default for Retime {
    fn default() -> Retime {
        Retime {
            speed: 1.0,
            reverse: false,
            freeze: false,
        }
    }
}

impl Retime {
    pub const MIN_SPEED: f64 = 0.05;
    pub const MAX_SPEED: f64 = 20.0;

    pub fn is_identity(&self) -> bool {
        *self == Retime::default()
    }

    /// Plays the window back to front (a freeze has no direction).
    pub fn backwards(&self) -> bool {
        self.reverse && !self.freeze
    }
}

impl Clip {
    /// Source frames (at the source rate) that `ticks` timeline ticks of this clip pass through, rounded
    /// down or up. Normal speed uses exact rational arithmetic, like the renderer always has.
    fn source_frames(&self, ticks: i64, up: bool) -> i64 {
        if self.retime.freeze {
            return 0;
        }
        let tl = self.duration.rate;
        let src = self.source_start.rate;
        let mode = if up { Rounding::Ceil } else { Rounding::Floor };
        if self.retime.speed == 1.0 {
            return RationalTime::new(ticks, tl)
                .rescale(src, mode)
                .map_or(0, |t| t.value);
        }
        let x = ticks as f64 * self.retime.speed * src.as_f64() / tl.as_f64();
        (if up { x.ceil() } else { x.floor() }) as i64
    }

    /// How far into the source the clip has played after `ticks` timeline ticks (rounded down).
    pub fn advance(&self, ticks: i64) -> i64 {
        self.source_frames(ticks, false)
    }

    /// Number of source frames the window covers: what the clip reads from the media.
    pub fn source_span(&self) -> i64 {
        if self.retime.freeze {
            1
        } else {
            self.source_frames(self.duration.value, true).max(1)
        }
    }

    /// The source frame `local` ticks into the clip, also before its start or after its end (negative or
    /// past the media: the caller clamps). Used for the overlap of a transition.
    pub fn source_frame_beyond(&self, local: i64) -> i64 {
        let ss = self.source_start.value;
        if self.retime.backwards() {
            ss + self.source_span() - 1 - self.advance(local)
        } else {
            ss + self.advance(local)
        }
    }

    /// The source frame shown `local` ticks into the clip.
    pub fn source_frame_at(&self, local: i64) -> i64 {
        let ss = self.source_start.value;
        if self.retime.backwards() {
            (ss + self.source_span() - 1 - self.advance(local)).max(ss)
        } else {
            ss + self.advance(local)
        }
    }

    pub fn new(
        id: Id,
        name: impl Into<String>,
        media: Id,
        source_start: RationalTime,
        duration: RationalTime,
    ) -> Clip {
        Clip {
            id,
            name: name.into(),
            media,
            source_start,
            duration,
            enabled: true,
            video: VideoProps::default(),
            anim: Anim::default(),
            retime: Retime::default(),
            transition: None,
            effects: Vec::new(),
            link: None,
            audio: AudioProps::default(),
        }
    }
}

/// Range checks shared by everything that has a picture: finite numbers, opacity 0..=1, sane colour.
pub fn check_video(v: &VideoProps) -> Result<(), &'static str> {
    let t = &v.transform;
    let c = &v.color;
    let all = [
        t.position.0,
        t.position.1,
        t.scale.0,
        t.scale.1,
        t.rotation_deg,
        v.opacity,
        c.exposure,
        c.contrast,
        c.saturation,
        c.temperature,
    ];
    if !all.iter().all(|x| x.is_finite()) {
        return Err("non-finite property");
    }
    if !(0.0..=1.0).contains(&v.opacity) {
        return Err("opacity must be 0..=1");
    }
    if c.contrast < 0.0 || !(0.0..=400.0).contains(&c.saturation) || c.temperature <= 0.0 {
        return Err("color values out of range");
    }
    if t.scale.0 == 0.0 || t.scale.1 == 0.0 {
        return Err("scale cannot be zero");
    }
    Ok(())
}

impl Clip {
    /// Property ranges: finite numbers, opacity 0..=1, pan -1..=1, fades inside the clip.
    pub fn check_props(&self, rate: Rational) -> Result<(), &'static str> {
        check_video(&self.video)?;
        if !self.audio.gain_db.is_finite() || !self.audio.pan.is_finite() {
            return Err("non-finite property");
        }
        if !(-1.0..=1.0).contains(&self.audio.pan) {
            return Err("pan must be -1..=1");
        }
        let (a, b) = (self.audio.fade_in, self.audio.fade_out);
        if a < 0 || b < 0 || self.duration.rate != rate || a + b > self.duration.value {
            return Err("fades must be non-negative and fit inside the clip");
        }
        check_effects(&self.effects)?;
        if self.transition.is_some_and(|t| t.duration < 2) {
            return Err("a transition needs at least two frames");
        }
        let sp = self.retime.speed;
        if !sp.is_finite() || !(Retime::MIN_SPEED..=Retime::MAX_SPEED).contains(&sp) {
            return Err("speed must be between 5% and 2000%");
        }
        check_anim(&self.anim, true)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TitleTemplate {
    /// Name plate in the lower left: a dark box with an accent bar, a bold line and a smaller line.
    LowerThird,
    /// Big centered text.
    Centered,
    /// Subtitle-style line at the bottom centre on a dark pill.
    Caption,
    /// Plain text with a thin accent line beneath it, lower left.
    Minimal,
    /// Centered text in a rounded plate with an accent border.
    Boxed,
    /// A large quotation mark, wrapped centered text and the speaker underneath.
    Quote,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl TitleTemplate {
    pub fn label(self) -> &'static str {
        match self {
            TitleTemplate::LowerThird => "Lower Third",
            TitleTemplate::Centered => "Centered Title",
            TitleTemplate::Caption => "Caption",
            TitleTemplate::Minimal => "Minimal",
            TitleTemplate::Boxed => "Boxed Title",
            TitleTemplate::Quote => "Quote",
        }
    }

    pub const ALL: [TitleTemplate; 6] = [
        TitleTemplate::LowerThird,
        TitleTemplate::Centered,
        TitleTemplate::Caption,
        TitleTemplate::Minimal,
        TitleTemplate::Boxed,
        TitleTemplate::Quote,
    ];

    /// Text size in pixels on a 1080-pixel-tall canvas.
    pub fn default_size(self) -> f64 {
        match self {
            TitleTemplate::LowerThird => 46.0,
            TitleTemplate::Centered => 110.0,
            TitleTemplate::Caption => 40.0,
            TitleTemplate::Minimal => 54.0,
            TitleTemplate::Boxed => 80.0,
            TitleTemplate::Quote => 56.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TitleStyle {
    pub template: TitleTemplate,
    /// Pixels on a 1080-pixel-tall canvas; scales with the canvas.
    pub size: f64,
    pub color: [u8; 3],
    /// Accent bar of the lower third
    pub accent: [u8; 3],
    /// Outline around the letters, pixels on a 1080-pixel-tall canvas (0 = none)
    #[serde(default)]
    pub outline: f64,
    #[serde(default = "white")]
    pub outline_color: [u8; 3],
    /// Drop shadow distance, pixels on a 1080-pixel-tall canvas (0 = none)
    #[serde(default)]
    pub shadow: f64,
    /// Extra space between letters, pixels on a 1080-pixel-tall canvas
    #[serde(default)]
    pub tracking: f64,
    /// Where the text sits horizontally (templates that centre by nature ignore Left/Right)
    #[serde(default)]
    pub align: TextAlign,
}

fn white() -> [u8; 3] {
    [255, 255, 255]
}

impl TitleStyle {
    pub fn new(template: TitleTemplate) -> TitleStyle {
        TitleStyle {
            template,
            size: template.default_size(),
            color: [255, 255, 255],
            accent: [0x5b, 0x4f, 0xd6],
            outline: 0.0,
            outline_color: [0, 0, 0],
            shadow: 0.0,
            tracking: 0.0,
            align: TextAlign::Center,
        }
    }
}

/// What a generated layer draws instead of text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    /// The whole frame in one colour
    Solid,
}

impl ShapeKind {
    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Ellipse => "Ellipse",
            ShapeKind::Solid => "Solid color",
        }
    }
}

/// A flat or gradient shape. Sizes are percentages of the frame; colours are RGBA.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub kind: ShapeKind,
    pub color: [u8; 4],
    /// Second colour: when set the fill runs from `color` at the top to this at the bottom
    pub gradient_to: Option<[u8; 4]>,
    pub width: f64,
    pub height: f64,
    /// Rounded corners of a rectangle, in percent of its shorter side
    pub corner: f64,
    pub outline: f64,
    pub outline_color: [u8; 4],
}

impl Shape {
    pub fn new(kind: ShapeKind) -> Shape {
        Shape {
            kind,
            color: match kind {
                ShapeKind::Solid => [40, 40, 48, 255],
                _ => [0x5b, 0x4f, 0xd6, 255],
            },
            gradient_to: None,
            width: 40.0,
            height: 40.0,
            corner: 0.0,
            outline: 0.0,
            outline_color: [255, 255, 255, 255],
        }
    }

    pub fn check(&self) -> Result<(), &'static str> {
        let nums = [self.width, self.height, self.corner, self.outline];
        if !nums.iter().all(|v| v.is_finite()) {
            return Err("shape sizes must be numbers");
        }
        if !(1.0..=400.0).contains(&self.width) || !(1.0..=400.0).contains(&self.height) {
            return Err("shape size must be 1-400 % of the frame");
        }
        if !(0.0..=50.0).contains(&self.corner) || !(0.0..=50.0).contains(&self.outline) {
            return Err("shape corner and outline are 0-50 %");
        }
        Ok(())
    }
}

/// A generated clip: text, or (with `shape`) a shape or solid colour, or (with `adjustment`) a layer whose
/// effects change everything beneath it. It has a picture (so it takes the usual transform and opacity)
/// and no sound.
///
/// A generated text clip. It has a picture (so it takes the usual transform and opacity) and no sound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Title {
    pub id: Id,
    pub name: String,
    pub duration: RationalTime,
    pub enabled: bool,
    pub text: String,
    pub subtitle: String,
    pub style: TitleStyle,
    pub video: VideoProps,
    /// Fades in ticks of the timeline rate (opacity ramps at both ends)
    pub fade_in: i64,
    /// Keyframes, by property (times relative to the start of the title)
    #[serde(default, skip_serializing_if = "Anim::is_empty")]
    pub anim: Anim,
    pub fade_out: i64,
    /// Draw this instead of text
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
    /// The typeface: None for the built-in one, `builtin:mono`, or the path of a .ttf / .otf file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// An adjustment layer draws nothing itself: its `effects` are applied to the picture below it
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub adjustment: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
}

impl Title {
    /// A shape, solid colour or adjustment layer lasting `duration`.
    pub fn generator(id: Id, kind: Option<ShapeKind>, duration: RationalTime) -> Title {
        let mut t = Title::new(id, TitleTemplate::Centered, duration);
        t.text = String::new();
        match kind {
            Some(k) => {
                t.name = k.label().to_string();
                t.shape = Some(Shape::new(k));
            }
            None => {
                t.name = "Adjustment Layer".into();
                t.adjustment = true;
            }
        }
        t
    }

    pub fn new(id: Id, template: TitleTemplate, duration: RationalTime) -> Title {
        let (text, subtitle) = match template {
            TitleTemplate::LowerThird => ("Your Title", "Subtitle"),
            TitleTemplate::Centered => ("Title", ""),
            TitleTemplate::Caption => ("Caption text", ""),
            TitleTemplate::Minimal => ("Minimal title", ""),
            TitleTemplate::Boxed => ("Boxed title", ""),
            TitleTemplate::Quote => ("Make it simple, but significant.", "Don Draper"),
        };
        Title {
            id,
            name: format!("Title: {text}"),
            duration,
            enabled: true,
            text: text.into(),
            subtitle: subtitle.into(),
            style: TitleStyle::new(template),
            video: VideoProps::default(),
            fade_in: 0,
            anim: Anim::default(),
            fade_out: 0,
            shape: None,
            font: None,
            adjustment: false,
            effects: Vec::new(),
        }
    }

    pub fn check_props(&self, rate: Rational) -> Result<(), &'static str> {
        check_video(&self.video)?;
        if !self.style.size.is_finite() || self.style.size <= 0.0 {
            return Err("title size must be positive");
        }
        let st = &self.style;
        if ![st.outline, st.shadow, st.tracking]
            .iter()
            .all(|v| v.is_finite())
            || !(0.0..=40.0).contains(&st.outline)
            || !(0.0..=60.0).contains(&st.shadow)
            || !(-20.0..=80.0).contains(&st.tracking)
        {
            return Err("title outline, shadow or letter spacing out of range");
        }
        let (a, b) = (self.fade_in, self.fade_out);
        if a < 0 || b < 0 || self.duration.rate != rate || a + b > self.duration.value {
            return Err("fades must be non-negative and fit inside the title");
        }
        if let Some(sh) = &self.shape {
            sh.check()?;
        }
        if self.shape.is_some() && self.adjustment {
            return Err("a layer is either a shape or an adjustment layer");
        }
        check_effects(&self.effects)?;
        if self.effects.iter().any(|e| e.kind.is_audio()) {
            return Err("a title has no sound to process");
        }
        check_anim(&self.anim, false)
    }
}

/// `base` with every property that has keyframes replaced by its animated value at `t`.
pub fn video_with_anim(base: &VideoProps, anim: &Anim, t: i64) -> VideoProps {
    if anim.is_empty() {
        return *base;
    }
    let mut v = *base;
    let get = |p: &str| anim.value(p, t);
    if let Some(x) = get("position.x") {
        v.transform.position.0 = x;
    }
    if let Some(x) = get("position.y") {
        v.transform.position.1 = x;
    }
    if let Some(x) = get("scale") {
        v.transform.scale = (x / 100.0, x / 100.0);
    }
    if let Some(x) = get("rotation") {
        v.transform.rotation_deg = x;
    }
    if let Some(x) = get("opacity") {
        v.opacity = (x / 100.0).clamp(0.0, 1.0);
    }
    if let Some(x) = get("exposure") {
        v.color.exposure = x;
    }
    if let Some(x) = get("contrast") {
        v.color.contrast = x.max(0.0);
    }
    if let Some(x) = get("saturation") {
        v.color.saturation = x.clamp(0.0, 400.0);
    }
    if let Some(x) = get("temperature") {
        v.color.temperature = x.max(1.0);
    }
    v
}

/// `base` with animated volume and pan applied at `t`.
pub fn audio_with_anim(base: &AudioProps, anim: &Anim, t: i64) -> AudioProps {
    let mut a = *base;
    if let Some(x) = anim.value("volume", t) {
        a.gain_db = x;
    }
    if let Some(x) = anim.value("pan", t) {
        a.pan = x.clamp(-1.0, 1.0);
    }
    a
}

/// The value a property has when it is not animated, in the units animation uses
/// (scale and opacity are percentages).
pub fn base_value(video: &VideoProps, audio: Option<&AudioProps>, prop: &str) -> Option<f64> {
    Some(match prop {
        "position.x" => video.transform.position.0,
        "position.y" => video.transform.position.1,
        "scale" => video.transform.scale.0 * 100.0,
        "rotation" => video.transform.rotation_deg,
        "opacity" => video.opacity * 100.0,
        "exposure" => video.color.exposure,
        "contrast" => video.color.contrast,
        "saturation" => video.color.saturation,
        "temperature" => video.color.temperature,
        "volume" => audio?.gain_db,
        "pan" => audio?.pan,
        _ => return None,
    })
}

/// Write `value` (in animation units) into the un-animated base of a property.
pub fn set_base_value(video: &mut VideoProps, audio: &mut AudioProps, prop: &str, value: f64) {
    match prop {
        "position.x" => video.transform.position.0 = value,
        "position.y" => video.transform.position.1 = value,
        "scale" => video.transform.scale = (value / 100.0, value / 100.0),
        "rotation" => video.transform.rotation_deg = value,
        "opacity" => video.opacity = value / 100.0,
        "exposure" => video.color.exposure = value,
        "contrast" => video.color.contrast = value,
        "saturation" => video.color.saturation = value,
        "temperature" => video.color.temperature = value,
        "volume" => audio.gain_db = value,
        "pan" => audio.pan = value,
        _ => {}
    }
}

/// Range and name checks for an item's keyframes.
pub fn check_anim(anim: &Anim, has_audio: bool) -> Result<(), &'static str> {
    for (prop, keys) in &anim.0 {
        if !is_property(prop) {
            return Err("unknown animated property");
        }
        if !has_audio && matches!(prop.as_str(), "volume" | "pan") {
            return Err("this item has no sound to animate");
        }
        if has_audio && prop == "reveal" {
            return Err("only titles can reveal their text");
        }
        for k in keys {
            if !k.v.is_finite() {
                return Err("keyframe values must be finite numbers");
            }
            let ok = match prop.as_str() {
                "opacity" | "reveal" => (0.0..=100.0).contains(&k.v),
                "pan" => (-1.0..=1.0).contains(&k.v),
                "contrast" => k.v >= 0.0,
                "saturation" => (0.0..=400.0).contains(&k.v),
                "temperature" => k.v > 0.0,
                "scale" => k.v != 0.0,
                _ => true,
            };
            if !ok {
                return Err("keyframe value out of range");
            }
            if let crate::Interp::Bezier(a, b, c, d) = k.interp {
                if ![a, b, c, d].iter().all(|x| x.is_finite())
                    || !(0.0..=1.0).contains(&a)
                    || !(0.0..=1.0).contains(&c)
                {
                    return Err("bezier control points must be finite with x in 0..=1");
                }
            }
        }
        if keys.windows(2).any(|w| w[0].t >= w[1].t) {
            return Err("keyframes must be in strictly increasing time order");
        }
    }
    Ok(())
}

impl Clip {
    /// Picture properties at `local` ticks into the clip, with keyframes applied.
    pub fn video_at(&self, local: i64) -> VideoProps {
        video_with_anim(&self.video, &self.anim, local)
    }

    pub fn audio_at(&self, local: i64) -> AudioProps {
        audio_with_anim(&self.audio, &self.anim, local)
    }
}

impl Title {
    pub fn video_at(&self, local: i64) -> VideoProps {
        video_with_anim(&self.video, &self.anim, local)
    }
}

/// Explicit empty space. Tracks are sequential (OTIO style), so gaps are real items.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gap {
    pub id: Id,
    pub duration: RationalTime,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Item {
    Clip(Clip),
    Gap(Gap),
    Title(Title),
}

impl Item {
    pub fn id(&self) -> Id {
        match self {
            Item::Clip(c) => c.id,
            Item::Gap(g) => g.id,
            Item::Title(t) => t.id,
        }
    }

    pub fn duration(&self) -> RationalTime {
        match self {
            Item::Clip(c) => c.duration,
            Item::Gap(g) => g.duration,
            Item::Title(t) => t.duration,
        }
    }

    pub fn as_clip(&self) -> Option<&Clip> {
        match self {
            Item::Clip(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_title(&self) -> Option<&Title> {
        match self {
            Item::Title(t) => Some(t),
            _ => None,
        }
    }

    /// Anything but empty space.
    pub fn is_content(&self) -> bool {
        !matches!(self, Item::Gap(_))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub name: String,
    pub kind: TrackKind,
    pub items: Vec<Item>,
    /// Video: visible. Audio: not muted.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Applied to every clip's audio on this track.
    #[serde(default)]
    pub gain_db: f64,
}

impl Track {
    pub fn new(id: Id, name: impl Into<String>, kind: TrackKind) -> Track {
        Track {
            id,
            name: name.into(),
            kind,
            items: Vec::new(),
            enabled: true,
            gain_db: 0.0,
        }
    }

    /// Sum of item durations in ticks of the timeline rate (every item is stored at that rate).
    pub fn duration_ticks(&self) -> i64 {
        self.items.iter().map(|i| i.duration().value).sum()
    }

    /// Start of each item, in ticks of the timeline rate.
    pub fn item_starts(&self) -> Vec<i64> {
        let mut t = 0;
        self.items
            .iter()
            .map(|i| {
                let s = t;
                t += i.duration().value;
                s
            })
            .collect()
    }
}

/// A note pinned to a moment (or a stretch) of the timeline. Markers stay where they are when the edit
/// around them changes; they are also the chapters of an exported video.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: Id,
    /// Where it sits, in ticks of the timeline rate
    pub tick: i64,
    /// 0 for a point; otherwise the length of the range it marks
    #[serde(default)]
    pub duration: i64,
    pub name: String,
    #[serde(default)]
    pub note: String,
    /// Colour index into [`Marker::COLORS`]
    #[serde(default)]
    pub color: u8,
}

impl Marker {
    pub const COLORS: [(&'static str, [u8; 3]); 6] = [
        ("Blue", [0x4f, 0x8c, 0xf5]),
        ("Green", [0x3d, 0xc2, 0x7a]),
        ("Yellow", [0xe6, 0xb4, 0x2e]),
        ("Red", [0xe5, 0x48, 0x4d]),
        ("Purple", [0x9b, 0x6b, 0xf0]),
        ("Cyan", [0x3c, 0xc4, 0xd8]),
    ];

    pub fn new(id: Id, tick: i64, name: impl Into<String>) -> Marker {
        Marker {
            id,
            tick,
            duration: 0,
            name: name.into(),
            note: String::new(),
            color: 0,
        }
    }

    pub fn check(&self) -> Result<(), &'static str> {
        if self.tick < 0 || self.duration < 0 {
            return Err("a marker cannot sit before the start or have a negative length");
        }
        if self.color as usize >= Marker::COLORS.len() {
            return Err("unknown marker colour");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub id: Id,
    pub name: String,
    /// All item durations on this timeline are whole ticks of this rate.
    pub rate: Rational,
    pub tracks: Vec<Arc<Track>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// Frame size of this sequence when it differs from the project's (a vertical cut of a horizontal film)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<(u32, u32)>,
}

impl Timeline {
    pub fn new(id: Id, name: impl Into<String>, rate: Rational) -> Timeline {
        Timeline {
            id,
            name: name.into(),
            rate,
            tracks: Vec::new(),
            markers: Vec::new(),
            canvas: None,
        }
    }

    pub fn track_mut(&mut self, index: usize) -> &mut Track {
        Arc::make_mut(&mut self.tracks[index])
    }

    pub fn duration(&self) -> RationalTime {
        let ticks = self
            .tracks
            .iter()
            .map(|t| t.duration_ticks())
            .max()
            .unwrap_or(0);
        RationalTime::new(ticks, self.rate)
    }
}

/// Where an item lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemLoc {
    pub timeline: Id,
    pub track: usize,
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub settings: ProjectSettings,
    pub media: BTreeMap<Id, MediaRef>,
    pub timelines: BTreeMap<Id, Arc<Timeline>>,
}

impl Project {
    pub fn new(settings: ProjectSettings) -> Project {
        Project {
            settings,
            media: BTreeMap::new(),
            timelines: BTreeMap::new(),
        }
    }

    pub fn timeline(&self, id: Id) -> Option<&Timeline> {
        self.timelines.get(&id).map(|t| t.as_ref())
    }

    pub fn timeline_mut(&mut self, id: Id) -> Option<&mut Timeline> {
        self.timelines.get_mut(&id).map(Arc::make_mut)
    }

    /// `(timeline id, track index)` of a track.
    pub fn find_track(&self, id: Id) -> Option<(Id, usize)> {
        self.timelines.values().find_map(|tl| {
            tl.tracks
                .iter()
                .position(|t| t.id == id)
                .map(|i| (tl.id, i))
        })
    }

    pub fn find_item(&self, id: Id) -> Option<ItemLoc> {
        self.timelines.values().find_map(|tl| {
            tl.tracks.iter().enumerate().find_map(|(ti, t)| {
                t.items
                    .iter()
                    .position(|i| i.id() == id)
                    .map(|index| ItemLoc {
                        timeline: tl.id,
                        track: ti,
                        index,
                    })
            })
        })
    }

    pub fn item(&self, loc: ItemLoc) -> Option<&Item> {
        self.timeline(loc.timeline)?
            .tracks
            .get(loc.track)?
            .items
            .get(loc.index)
    }

    pub fn id_in_use(&self, id: Id) -> bool {
        self.media.contains_key(&id)
            || self.timelines.contains_key(&id)
            || self.find_track(id).is_some()
            || self.find_item(id).is_some()
            || self
                .timelines
                .values()
                .any(|tl| tl.markers.iter().any(|m| m.id == id))
    }

    /// The frame size of a timeline: its own, or the project's.
    pub fn canvas(&self, timeline: Id) -> (u32, u32) {
        self.timeline(timeline)
            .and_then(|t| t.canvas)
            .unwrap_or((self.settings.width, self.settings.height))
    }

    pub fn media_in_use(&self, media: Id) -> bool {
        self.timelines.values().any(|tl| {
            tl.tracks.iter().any(|t| {
                t.items
                    .iter()
                    .any(|i| i.as_clip().is_some_and(|c| c.media == media))
            })
        })
    }

    /// Checks every invariant; returns all violations found.
    ///
    /// - ids are unique across media, timelines, tracks and items
    /// - every item has a positive duration in whole ticks of its timeline's rate
    /// - every clip references existing media and stays inside it (when its length is known)
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        let mut seen = HashSet::new();
        let mut note = |id: Id, what: &str, errs: &mut Vec<String>| {
            if !seen.insert(id) {
                errs.push(format!("duplicate id {id} ({what})"));
            }
        };
        for m in self.media.values() {
            note(m.id, "media", &mut errs);
        }
        for tl in self.timelines.values() {
            note(tl.id, "timeline", &mut errs);
            if let Some((w, h)) = tl.canvas {
                if w < 16 || h < 16 || w > 16384 || h > 16384 || w % 2 != 0 || h % 2 != 0 {
                    errs.push(format!("timeline {} has a bad frame size {w}x{h}", tl.name));
                }
            }
            if tl
                .markers
                .windows(2)
                .any(|w| (w[0].tick, w[0].id) > (w[1].tick, w[1].id))
            {
                errs.push(format!("markers of {} are not in time order", tl.name));
            }
            for m in &tl.markers {
                note(m.id, "marker", &mut errs);
                if let Err(e) = m.check() {
                    errs.push(format!("marker {}: {e}", m.id));
                }
            }
            for t in &tl.tracks {
                note(t.id, "track", &mut errs);
                for item in &t.items {
                    note(item.id(), "item", &mut errs);
                    let d = item.duration();
                    if d.rate != tl.rate || d.value <= 0 {
                        errs.push(format!(
                            "item {} has bad duration {}@{}",
                            item.id(),
                            d.value,
                            d.rate
                        ));
                    }
                    if let Item::Title(t) = item {
                        if let Err(e) = t.check_props(tl.rate) {
                            errs.push(format!("title {}: {e}", t.id));
                        }
                    }
                    if let Item::Clip(c) = item {
                        if let Err(e) = c.check_props(tl.rate) {
                            errs.push(format!("clip {}: {e}", c.id));
                        }
                        match self.media.get(&c.media) {
                            None => errs.push(format!(
                                "clip {} references missing media {}",
                                c.id, c.media
                            )),
                            Some(m) => {
                                if c.source_start.value < 0 {
                                    errs.push(format!("clip {} starts before its media", c.id));
                                }
                                if let Some(len) = m.duration {
                                    let end = RationalTime::new(
                                        c.source_start.value + c.source_span(),
                                        c.source_start.rate,
                                    );
                                    if end > len {
                                        errs.push(format!(
                                            "clip {} runs past the end of its media",
                                            c.id
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let mut p = Project::new(ProjectSettings::default());
        let m = Id::from_u128(1);
        p.media.insert(
            m,
            MediaRef {
                id: m,
                name: "a.mov".into(),
                path: "/a.mov".into(),
                duration: Some(RationalTime::new(300, Rational::FPS_30)),
                has_audio: true,
                width: 1920,
                height: 1080,
                has_video: true,
                proxy: None,
            },
        );
        let mut tl = Timeline::new(Id::from_u128(2), "main", Rational::FPS_30);
        let mut tr = Track::new(Id::from_u128(3), "V1", TrackKind::Video);
        tr.items.push(Item::Clip(Clip {
            id: Id::from_u128(4),
            name: "c".into(),
            media: m,
            source_start: RationalTime::new(0, Rational::FPS_30),
            duration: RationalTime::new(30, Rational::FPS_30),
            enabled: true,
            video: VideoProps::default(),
            audio: AudioProps::default(),
            anim: Default::default(),
            retime: Default::default(),
            transition: None,
            effects: Vec::new(),
            link: None,
        }));
        tl.tracks.push(Arc::new(tr));
        p.timelines.insert(tl.id, Arc::new(tl));
        assert_eq!(p.validate(), Ok(()));
        let back: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn validate_catches_problems() {
        let mut p = Project::new(ProjectSettings::default());
        let mut tl = Timeline::new(Id::from_u128(2), "main", Rational::FPS_30);
        let mut tr = Track::new(Id::from_u128(3), "V1", TrackKind::Video);
        tr.items.push(Item::Gap(Gap {
            id: Id::from_u128(4),
            duration: RationalTime::new(0, Rational::FPS_30),
        }));
        tr.items.push(Item::Gap(Gap {
            id: Id::from_u128(4),
            duration: RationalTime::new(5, Rational::FPS_24),
        }));
        tl.tracks.push(Arc::new(tr));
        p.timelines.insert(tl.id, Arc::new(tl));
        assert_eq!(p.validate().unwrap_err().len(), 3);
    }

    #[test]
    fn clone_is_copy_on_write() {
        let mut p = Project::new(ProjectSettings::default());
        let mut tl = Timeline::new(Id::from_u128(2), "main", Rational::FPS_30);
        tl.tracks.push(Arc::new(Track::new(
            Id::from_u128(3),
            "V1",
            TrackKind::Video,
        )));
        tl.tracks.push(Arc::new(Track::new(
            Id::from_u128(5),
            "V2",
            TrackKind::Video,
        )));
        p.timelines.insert(tl.id, Arc::new(tl));
        let old = p.clone();
        p.timeline_mut(Id::from_u128(2)).unwrap().track_mut(0).name = "changed".into();
        // untouched track is shared, touched one is not
        let (a, b) = (
            old.timeline(Id::from_u128(2)).unwrap(),
            p.timeline(Id::from_u128(2)).unwrap(),
        );
        assert!(Arc::ptr_eq(&a.tracks[1], &b.tracks[1]));
        assert!(!Arc::ptr_eq(&a.tracks[0], &b.tracks[0]));
        assert_eq!(a.tracks[0].name, "V1");
    }
}
