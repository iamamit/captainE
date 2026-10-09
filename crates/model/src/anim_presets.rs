//! Ready-made animations for titles and layers: how they come on and go off. A preset is only a set of
//! keyframes, so after applying one it can be edited like any other animation.

use crate::{Anim, Interp, Keyframe, VideoProps};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    Fade,
    SlideUp,
    SlideDown,
    SlideLeft,
    SlideRight,
    Pop,
    Zoom,
    /// Letters appear one after another (titles only)
    Typewriter,
}

impl Preset {
    pub const ALL: [Preset; 8] = [
        Preset::Fade,
        Preset::SlideUp,
        Preset::SlideDown,
        Preset::SlideLeft,
        Preset::SlideRight,
        Preset::Pop,
        Preset::Zoom,
        Preset::Typewriter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Fade => "Fade",
            Preset::SlideUp => "Slide up",
            Preset::SlideDown => "Slide down",
            Preset::SlideLeft => "Slide left",
            Preset::SlideRight => "Slide right",
            Preset::Pop => "Pop",
            Preset::Zoom => "Zoom",
            Preset::Typewriter => "Typewriter",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Preset::Fade => "fade",
            Preset::SlideUp => "slide_up",
            Preset::SlideDown => "slide_down",
            Preset::SlideLeft => "slide_left",
            Preset::SlideRight => "slide_right",
            Preset::Pop => "pop",
            Preset::Zoom => "zoom",
            Preset::Typewriter => "typewriter",
        }
    }

    pub fn from_key(k: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.key() == k)
    }
}

/// The keyframes of one animated property that a preset writes, in the property's own units.
pub type PresetTracks = Vec<(&'static str, Vec<Keyframe>)>;

fn k(t: i64, v: f64, interp: Interp) -> Keyframe {
    Keyframe { t, v, interp }
}

/// Keyframes for `preset` coming on (`entering`) or going off over `len` ticks of a `total`-tick layer
/// that rests at `base`. `canvas` is the frame size in pixels. Typewriter only comes on.
pub fn tracks(
    preset: Preset,
    entering: bool,
    len: i64,
    total: i64,
    base: &VideoProps,
    canvas: (f64, f64),
) -> PresetTracks {
    let len = len.clamp(1, total.max(1));
    // time of the resting end and of the hidden end of the move
    let (rest, away) = if entering {
        (len, 0)
    } else {
        (total - len, total)
    };
    let mut out: PresetTracks = Vec::new();
    // `pair(hidden, shown)` as keyframes in the right order for coming on or going off
    let pair = |hidden: f64, shown: f64, ease: Interp| -> Vec<Keyframe> {
        if entering {
            vec![k(away, hidden, ease), k(rest, shown, Interp::Hold)]
        } else {
            vec![k(rest, shown, ease), k(away, hidden, Interp::Hold)]
        }
    };
    let (px, py) = base.transform.position;
    let pct = base.opacity * 100.0;
    let scale = base.transform.scale.0 * 100.0;
    match preset {
        Preset::Fade => out.push(("opacity", pair(0.0, pct, Interp::EaseInOut))),
        Preset::SlideUp => {
            out.push((
                "position.y",
                pair(
                    py + canvas.1 * 0.12,
                    py,
                    if entering {
                        Interp::EaseOut
                    } else {
                        Interp::EaseIn
                    },
                ),
            ));
            out.push(("opacity", pair(0.0, pct, Interp::Linear)));
        }
        Preset::SlideDown => {
            out.push((
                "position.y",
                pair(
                    py - canvas.1 * 0.12,
                    py,
                    if entering {
                        Interp::EaseOut
                    } else {
                        Interp::EaseIn
                    },
                ),
            ));
            out.push(("opacity", pair(0.0, pct, Interp::Linear)));
        }
        Preset::SlideLeft => {
            out.push((
                "position.x",
                pair(
                    px + canvas.0 * 0.12,
                    px,
                    if entering {
                        Interp::EaseOut
                    } else {
                        Interp::EaseIn
                    },
                ),
            ));
            out.push(("opacity", pair(0.0, pct, Interp::Linear)));
        }
        Preset::SlideRight => {
            out.push((
                "position.x",
                pair(
                    px - canvas.0 * 0.12,
                    px,
                    if entering {
                        Interp::EaseOut
                    } else {
                        Interp::EaseIn
                    },
                ),
            ));
            out.push(("opacity", pair(0.0, pct, Interp::Linear)));
        }
        Preset::Pop => {
            // overshoot, then settle (on the way off: swell a little, then vanish)
            let mid = len * 2 / 3;
            let keys = if entering {
                vec![
                    k(0, scale * 0.5, Interp::EaseOut),
                    k(mid.max(1), scale * 1.08, Interp::EaseInOut),
                    k(len, scale, Interp::Hold),
                ]
            } else {
                vec![
                    k(total - len, scale, Interp::EaseIn),
                    k(total - len + (len / 3).max(1), scale * 1.06, Interp::EaseIn),
                    k(total, scale * 0.4, Interp::Hold),
                ]
            };
            out.push(("scale", keys));
            out.push(("opacity", pair(0.0, pct, Interp::Linear)));
        }
        Preset::Zoom => {
            out.push((
                "scale",
                pair(
                    scale * if entering { 1.5 } else { 0.6 },
                    scale,
                    Interp::EaseOut,
                ),
            ));
            out.push(("opacity", pair(0.0, pct, Interp::Linear)));
        }
        Preset::Typewriter => {
            if entering {
                out.push((
                    "reveal",
                    vec![k(0, 0.0, Interp::Linear), k(len, 100.0, Interp::Hold)],
                ));
            }
        }
    }
    out
}

/// Write the preset's tracks into `anim`, keeping whatever other keyframes are there: a coming-on preset
/// replaces the keys in its time span, a going-off one the keys in its.
pub fn apply(anim: &mut Anim, new: PresetTracks, entering: bool, len: i64, total: i64) {
    for (prop, keys) in new {
        let mut all: Vec<Keyframe> = anim
            .keys(prop)
            .iter()
            .copied()
            .filter(|old| {
                if entering {
                    old.t > len
                } else {
                    old.t < total - len
                }
            })
            .collect();
        all.extend(keys);
        all.sort_by_key(|x| x.t);
        anim.set_track(prop, all);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> VideoProps {
        VideoProps::default()
    }

    #[test]
    fn coming_on_starts_hidden_and_ends_at_rest() {
        for p in Preset::ALL {
            let t = tracks(p, true, 12, 100, &base(), (1920.0, 1080.0));
            assert!(!t.is_empty(), "{p:?}");
            for (prop, keys) in &t {
                assert!(
                    keys.first().unwrap().t == 0 && keys.last().unwrap().t == 12,
                    "{prop}"
                );
            }
        }
        let fade = tracks(Preset::Fade, true, 12, 100, &base(), (1920.0, 1080.0));
        assert_eq!((fade[0].1[0].v, fade[0].1[1].v), (0.0, 100.0));
        let up = tracks(Preset::SlideUp, true, 12, 100, &base(), (1920.0, 1080.0));
        assert_eq!(up[0].0, "position.y");
        assert!(
            up[0].1[0].v > 0.0 && up[0].1[1].v == 0.0,
            "starts below, ends at rest"
        );
    }

    #[test]
    fn going_off_ends_hidden_and_typewriter_only_comes_on() {
        let out = tracks(Preset::Fade, false, 12, 100, &base(), (1920.0, 1080.0));
        assert_eq!(
            (out[0].1[0].t, out[0].1[0].v, out[0].1[1].t, out[0].1[1].v),
            (88, 100.0, 100, 0.0)
        );
        assert!(tracks(Preset::Typewriter, false, 12, 100, &base(), (1.0, 1.0)).is_empty());
    }

    #[test]
    fn in_and_out_presets_share_a_property_without_clobbering_each_other() {
        let mut anim = Anim::default();
        let (len, total) = (12, 100);
        apply(
            &mut anim,
            tracks(Preset::Fade, true, len, total, &base(), (1.0, 1.0)),
            true,
            len,
            total,
        );
        apply(
            &mut anim,
            tracks(Preset::Fade, false, len, total, &base(), (1.0, 1.0)),
            false,
            len,
            total,
        );
        assert_eq!(anim.value("opacity", 0), Some(0.0));
        assert_eq!(anim.value("opacity", 50), Some(100.0));
        assert_eq!(anim.value("opacity", 100), Some(0.0));
        assert_eq!(anim.keys("opacity").len(), 4);
        // applying the same preset again replaces rather than piles up
        apply(
            &mut anim,
            tracks(Preset::Fade, true, len, total, &base(), (1.0, 1.0)),
            true,
            len,
            total,
        );
        assert_eq!(anim.keys("opacity").len(), 4);
    }
}
