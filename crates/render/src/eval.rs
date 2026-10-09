use captaine_model::{Clip, Id, Item, MediaRef, Project, Title, TrackKind, TransitionKind};

/// `(start, end, item index)` of each clip on one track, sorted by start.
type Spans = Vec<(i64, i64, usize)>;

/// Where every clip sits on each video track, for fast "what is visible at tick t" lookups.
pub struct TimelineIndex {
    /// per video track (bottom first): (track index in the timeline, sorted `(start, end, item index)` of clips)
    tracks: Vec<(usize, Spans)>,
}

impl TimelineIndex {
    pub fn new(project: &Project, timeline: Id) -> Option<TimelineIndex> {
        let tl = project.timeline(timeline)?;
        let tracks = tl
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == TrackKind::Video && t.enabled)
            .map(|(ti, t)| {
                let spans = t
                    .items
                    .iter()
                    .zip(t.item_starts())
                    .enumerate()
                    .filter(|(_, (it, _))| {
                        matches!(it, Item::Clip(c) if c.enabled)
                            || matches!(it, Item::Title(t) if t.enabled)
                    })
                    .map(|(i, (it, st))| (st, st + it.duration().value, i))
                    .collect();
                (ti, spans)
            })
            .collect();
        Some(TimelineIndex { tracks })
    }
}

/// One clip contributing to a frame.
pub struct ClipLayer<'a> {
    pub clip: &'a Clip,
    pub media: &'a MediaRef,
    /// Frame number in the media at its own frame rate
    pub source_frame: i64,
    /// Ticks since the clip began (animation time)
    pub local_tick: i64,
    pub track: usize,
    /// Set while a transition is drawing this clip
    pub transition: Option<TransitionState>,
}

/// Which transition a layer belongs to and how far along it is.
#[derive(Clone, Copy, Debug)]
pub struct TransitionState {
    pub kind: TransitionKind,
    /// 0 at the start of the transition, 1 at its end
    pub progress: f64,
    /// true for the clip that is coming in
    pub incoming: bool,
}

/// A title contributing to a frame.
pub struct TitleLayer<'a> {
    pub title: &'a Title,
    /// Ticks since the title began
    pub local_tick: i64,
    pub track: usize,
}

pub enum Layer<'a> {
    Clip(ClipLayer<'a>),
    Title(TitleLayer<'a>),
}

impl Layer<'_> {
    pub fn track(&self) -> usize {
        match self {
            Layer::Clip(c) => c.track,
            Layer::Title(t) => t.track,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Layer::Clip(c) => &c.clip.name,
            Layer::Title(t) => &t.title.name,
        }
    }
}

/// Things visible at `tick`, bottom layer first. Audio-only media contributes no picture.
pub fn layers_at<'a>(
    project: &'a Project,
    timeline: Id,
    index: &TimelineIndex,
    tick: i64,
) -> Vec<Layer<'a>> {
    let Some(tl) = project.timeline(timeline) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (ti, spans) in &index.tracks {
        let pos = spans.partition_point(|(_, end, _)| *end <= tick);
        let Some(&(start, _, item)) = spans.get(pos) else {
            continue;
        };
        if tick < start {
            continue;
        }
        match &tl.tracks[*ti].items[item] {
            Item::Clip(clip) => {
                let items = &tl.tracks[*ti].items;
                let make = |c: &'a Clip, at: i64, state: Option<TransitionState>| {
                    let media = project.media.get(&c.media).filter(|m| m.has_video)?;
                    let local = tick - at;
                    let mut frame = c.source_frame_beyond(local).max(0);
                    if let Some(len) = media.duration {
                        frame = frame.min((len.value - 1).max(0));
                    }
                    Some(Layer::Clip(ClipLayer {
                        clip: c,
                        media,
                        source_frame: frame,
                        local_tick: local.clamp(0, (c.duration.value - 1).max(0)),
                        track: *ti,
                        transition: state,
                    }))
                };
                // is this tick inside a transition at the cut before `pos` or the one after?
                let neighbour = |p: usize| -> Option<(i64, i64, usize)> { spans.get(p).copied() };
                let mut cands = Vec::new();
                if let (Some(t), true) = (clip.transition, pos > 0) {
                    if let Some((_, pend, pi)) = neighbour(pos - 1) {
                        if pi + 1 == item && pend == start {
                            cands.push((pi, item, start, t));
                        }
                    }
                }
                {
                    if let Some((nstart, _, ni)) = neighbour(pos + 1) {
                        if ni == item + 1 && nstart == start + clip.duration.value {
                            if let Item::Clip(n) = &items[ni] {
                                if let Some(t) = n.transition {
                                    cands.push((item, ni, nstart, t));
                                }
                            }
                        }
                    }
                }
                let mut blended = false;
                for (oi, ii, cut, t) in cands {
                    if let (Item::Clip(o), Item::Clip(i)) = (&items[oi], &items[ii]) {
                        let d = t.duration.min(2 * o.duration.value.min(i.duration.value));
                        let (a, b) = (d / 2, d - d / 2);
                        if d >= 2 && tick >= cut - a && tick < cut + b {
                            let progress = ((tick - (cut - a)) as f64 + 0.5) / d as f64;
                            let st = |incoming| {
                                Some(TransitionState {
                                    kind: t.kind,
                                    progress,
                                    incoming,
                                })
                            };
                            let ostart = cut - o.duration.value;
                            out.extend(make(o, ostart, st(false)));
                            out.extend(make(i, cut, st(true)));
                            blended = true;
                            break;
                        }
                    }
                }
                if !blended {
                    out.extend(make(clip, start, None));
                }
            }
            Item::Title(title) => out.push(Layer::Title(TitleLayer {
                title,
                local_tick: tick - start,
                track: *ti,
            })),
            Item::Gap(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use captaine_model::*;
    use captaine_time::{Rational, RationalTime};

    use super::*;

    fn t(v: i64, r: Rational) -> RationalTime {
        RationalTime::new(v, r)
    }

    fn project() -> (Project, Id) {
        let mut p = Project::new(ProjectSettings::default());
        let m = Id::from_u128(1);
        p.media.insert(
            m,
            MediaRef {
                id: m,
                name: "m".into(),
                path: "/m".into(),
                duration: Some(t(1000, Rational::FPS_24)),
                has_audio: false,
                width: 4,
                height: 4,
                has_video: true,
                proxy: None,
            },
        );
        let mut tl = Timeline::new(Id::from_u128(2), "tl", Rational::FPS_30);
        let mut v1 = Track::new(Id::from_u128(3), "V1", TrackKind::Video);
        // gap 0..10, clip 10..40 (source from frame 100 @24)
        v1.items.push(Item::Gap(Gap {
            id: Id::from_u128(10),
            duration: t(10, Rational::FPS_30),
        }));
        v1.items.push(Item::Clip(Clip::new(
            Id::from_u128(11),
            "a",
            m,
            t(100, Rational::FPS_24),
            t(30, Rational::FPS_30),
        )));
        let mut v2 = Track::new(Id::from_u128(4), "V2", TrackKind::Video);
        v2.items.push(Item::Clip(Clip::new(
            Id::from_u128(12),
            "b",
            m,
            t(0, Rational::FPS_24),
            t(20, Rational::FPS_30),
        )));
        tl.tracks.push(Arc::new(v1));
        tl.tracks.push(Arc::new(v2));
        let id = tl.id;
        p.timelines.insert(id, Arc::new(tl));
        (p, id)
    }

    #[test]
    fn finds_layers_bottom_first() {
        let (p, tl) = project();
        let idx = TimelineIndex::new(&p, tl).unwrap();
        let names = |tick| {
            layers_at(&p, tl, &idx, tick)
                .iter()
                .map(|l| l.name().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(0), ["b"]); // V1 has a gap here
        assert_eq!(names(10), ["a", "b"]);
        assert_eq!(names(19), ["a", "b"]);
        assert_eq!(names(20), ["a"]); // end of b is exclusive
        assert_eq!(names(39), ["a"]);
        assert!(names(40).is_empty());
    }

    #[test]
    fn source_frame_follows_the_media_rate() {
        let (p, tl) = project();
        let idx = TimelineIndex::new(&p, tl).unwrap();
        let f = |tick| {
            layers_at(&p, tl, &idx, tick).iter().find_map(|l| match l {
                Layer::Clip(c) if c.clip.name == "a" => Some(c.source_frame),
                _ => None,
            })
        };
        assert_eq!(f(10), Some(100));
        assert_eq!(f(15), Some(104)); // 5 frames @30 = 4 frames @24
        assert_eq!(f(39), Some(123)); // floor(29 * 0.8) = 23
    }

    #[test]
    fn disabled_tracks_and_clips_are_skipped() {
        let (mut p, tl) = project();
        p.timeline_mut(tl).unwrap().track_mut(1).enabled = false;
        let idx = TimelineIndex::new(&p, tl).unwrap();
        assert!(layers_at(&p, tl, &idx, 0).is_empty());
    }
}
