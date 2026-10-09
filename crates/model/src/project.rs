use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use captaine_time::{Rational, RationalTime};
use serde::{Deserialize, Serialize};

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
}

impl Clip {
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
            audio: AudioProps::default(),
        }
    }
}

impl Clip {
    /// Property ranges: finite numbers, opacity 0..=1, pan -1..=1, fades inside the clip.
    pub fn check_props(&self, rate: Rational) -> Result<(), &'static str> {
        let t = &self.video.transform;
        let finite = [
            t.position.0,
            t.position.1,
            t.scale.0,
            t.scale.1,
            t.rotation_deg,
            self.video.opacity,
            self.audio.gain_db,
            self.audio.pan,
            self.video.color.exposure,
            self.video.color.contrast,
            self.video.color.saturation,
            self.video.color.temperature,
        ]
        .iter()
        .all(|v| v.is_finite());
        if !finite {
            return Err("non-finite property");
        }
        if !(0.0..=1.0).contains(&self.video.opacity) {
            return Err("opacity must be 0..=1");
        }
        if !(-1.0..=1.0).contains(&self.audio.pan) {
            return Err("pan must be -1..=1");
        }
        let c = &self.video.color;
        if c.contrast < 0.0 || !(0.0..=400.0).contains(&c.saturation) || c.temperature <= 0.0 {
            return Err("color values out of range");
        }
        if t.scale.0 == 0.0 || t.scale.1 == 0.0 {
            return Err("scale cannot be zero");
        }
        let (a, b) = (self.audio.fade_in, self.audio.fade_out);
        if a < 0 || b < 0 || self.duration.rate != rate || a + b > self.duration.value {
            return Err("fades must be non-negative and fit inside the clip");
        }
        Ok(())
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
}

impl Item {
    pub fn id(&self) -> Id {
        match self {
            Item::Clip(c) => c.id,
            Item::Gap(g) => g.id,
        }
    }

    pub fn duration(&self) -> RationalTime {
        match self {
            Item::Clip(c) => c.duration,
            Item::Gap(g) => g.duration,
        }
    }

    pub fn as_clip(&self) -> Option<&Clip> {
        match self {
            Item::Clip(c) => Some(c),
            Item::Gap(_) => None,
        }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub id: Id,
    pub name: String,
    /// All item durations on this timeline are whole ticks of this rate.
    pub rate: Rational,
    pub tracks: Vec<Arc<Track>>,
}

impl Timeline {
    pub fn new(id: Id, name: impl Into<String>, rate: Rational) -> Timeline {
        Timeline {
            id,
            name: name.into(),
            rate,
            tracks: Vec::new(),
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
                                if let (Some(len), Some(src_dur)) = (
                                    m.duration,
                                    c.duration.rescale(
                                        c.source_start.rate,
                                        captaine_time::Rounding::Ceil,
                                    ),
                                ) {
                                    let end = RationalTime::new(
                                        c.source_start.value + src_dur.value,
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
