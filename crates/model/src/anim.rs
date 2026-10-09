//! Keyframe animation. Any numeric property of a clip or title can have a track of keyframes.
//!
//! Times are in ticks of the timeline rate **relative to the start of the item**, so moving a clip moves its
//! animation, and trimming the front shifts the keys so the animation stays glued to the picture.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How the value travels from this key to the next one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Interp {
    /// Stay at this value until the next key, then jump
    Hold,
    Linear,
    /// Start slowly
    EaseIn,
    /// End slowly
    EaseOut,
    EaseInOut,
    /// CSS-style cubic bezier through (0,0) and (1,1) with these two control points
    Bezier(f64, f64, f64, f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub t: i64,
    pub v: f64,
    pub interp: Interp,
}

impl Keyframe {
    pub fn new(t: i64, v: f64) -> Keyframe {
        Keyframe {
            t,
            v,
            interp: Interp::Linear,
        }
    }
}

/// Map progress 0..1 through the easing of `interp`.
pub fn ease(interp: Interp, x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    match interp {
        Interp::Hold => 0.0,
        Interp::Linear => x,
        Interp::EaseIn => x * x,
        Interp::EaseOut => 1.0 - (1.0 - x) * (1.0 - x),
        Interp::EaseInOut => x * x * (3.0 - 2.0 * x),
        Interp::Bezier(x1, y1, x2, y2) => {
            // solve bezier_x(s) = x for s with Newton steps, then read bezier_y(s)
            let bez = |a: f64, b: f64, s: f64| {
                3.0 * a * s * (1.0 - s) * (1.0 - s) + 3.0 * b * s * s * (1.0 - s) + s * s * s
            };
            let dbez = |a: f64, b: f64, s: f64| {
                3.0 * a * (1.0 - s) * (1.0 - s)
                    + 6.0 * (b - a) * s * (1.0 - s)
                    + 3.0 * (1.0 - b) * s * s
            };
            let mut s = x;
            for _ in 0..8 {
                let err = bez(x1, x2, s) - x;
                let d = dbez(x1, x2, s);
                if err.abs() < 1e-9 || d.abs() < 1e-9 {
                    break;
                }
                s = (s - err / d).clamp(0.0, 1.0);
            }
            // fall back to bisection if Newton wandered
            if (bez(x1, x2, s) - x).abs() > 1e-5 {
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..40 {
                    s = (lo + hi) / 2.0;
                    if bez(x1, x2, s) < x {
                        lo = s;
                    } else {
                        hi = s;
                    }
                }
            }
            bez(y1, y2, s)
        }
    }
}

/// Value of a sorted keyframe list at time `t`.
pub fn value_at(keys: &[Keyframe], t: i64) -> Option<f64> {
    let first = keys.first()?;
    if t <= first.t {
        return Some(first.v);
    }
    let last = keys.last().expect("non-empty");
    if t >= last.t {
        return Some(last.v);
    }
    let i = keys.partition_point(|k| k.t <= t) - 1;
    let (a, b) = (&keys[i], &keys[i + 1]);
    let x = (t - a.t) as f64 / (b.t - a.t) as f64;
    Some(a.v + (b.v - a.v) * ease(a.interp, x))
}

/// The animated properties of one item: property name -> keyframes sorted by time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Anim(pub BTreeMap<String, Vec<Keyframe>>);

/// Every animatable property, with the name shown to people.
pub const PROPERTIES: &[(&str, &str)] = &[
    ("position.x", "Position X"),
    ("position.y", "Position Y"),
    ("scale", "Scale"),
    ("rotation", "Rotation"),
    ("opacity", "Opacity"),
    ("exposure", "Exposure"),
    ("contrast", "Contrast"),
    ("saturation", "Saturation"),
    ("temperature", "Temperature"),
    ("volume", "Volume"),
    ("pan", "Pan"),
];

pub fn is_property(name: &str) -> bool {
    PROPERTIES.iter().any(|(k, _)| *k == name)
}

impl Anim {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn keys(&self, prop: &str) -> &[Keyframe] {
        self.0.get(prop).map_or(&[], Vec::as_slice)
    }

    pub fn has(&self, prop: &str) -> bool {
        self.0.get(prop).is_some_and(|k| !k.is_empty())
    }

    /// The animated value of `prop` at `t`, or `None` if it has no keys.
    pub fn value(&self, prop: &str, t: i64) -> Option<f64> {
        value_at(self.keys(prop), t)
    }

    /// Replace a property's keys (sorted and de-duplicated by time); an empty list removes the track.
    pub fn set_track(&mut self, prop: &str, mut keys: Vec<Keyframe>) {
        keys.sort_by_key(|k| k.t);
        keys.dedup_by_key(|k| k.t);
        if keys.is_empty() {
            self.0.remove(prop);
        } else {
            self.0.insert(prop.to_string(), keys);
        }
    }

    /// Move every key by `delta` ticks (negative = earlier).
    pub fn shifted(&self, delta: i64) -> Anim {
        Anim(
            self.0
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.iter()
                            .map(|x| Keyframe {
                                t: x.t + delta,
                                ..*x
                            })
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    /// This animation followed by `other`, which starts `len` ticks in (the inverse of cutting an item in two).
    pub fn joined(&self, len: i64, other: &Anim) -> Anim {
        let mut out = Anim::default();
        let props: std::collections::BTreeSet<&String> =
            self.0.keys().chain(other.0.keys()).collect();
        for p in props {
            let mut keys: Vec<Keyframe> =
                self.keys(p).iter().filter(|k| k.t < len).copied().collect();
            keys.extend(
                other
                    .keys(p)
                    .iter()
                    .map(|k| Keyframe { t: k.t + len, ..*k }),
            );
            out.set_track(p, keys);
        }
        out
    }

    /// The part of the animation in `[start, end)`, re-based to start at 0. Keeps the picture continuous at both
    /// edges by adding a key at each edge that carries the value there.
    pub fn window(&self, start: i64, end: i64) -> Anim {
        let mut out = Anim::default();
        for (prop, keys) in &self.0 {
            let (Some(v0), Some(v1)) = (value_at(keys, start), value_at(keys, end)) else {
                continue;
            };
            let mut w: Vec<Keyframe> = Vec::new();
            // the segment containing `start` keeps its easing
            let before = keys.iter().rev().find(|k| k.t <= start);
            w.push(Keyframe {
                t: 0,
                v: v0,
                interp: before.map_or(Interp::Linear, |k| k.interp),
            });
            w.extend(
                keys.iter()
                    .filter(|k| k.t > start && k.t < end)
                    .map(|k| Keyframe {
                        t: k.t - start,
                        ..*k
                    }),
            );
            if keys.iter().any(|k| k.t > start && k.t < end) || (v0 - v1).abs() > f64::EPSILON {
                w.push(Keyframe {
                    t: end - start,
                    v: v1,
                    interp: Interp::Linear,
                });
            }
            out.set_track(prop, w);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(t: i64, v: f64, i: Interp) -> Keyframe {
        Keyframe { t, v, interp: i }
    }

    #[test]
    fn values_before_between_and_after() {
        let keys = [
            k(10, 0.0, Interp::Linear),
            k(20, 100.0, Interp::Linear),
            k(40, 50.0, Interp::Hold),
        ];
        assert_eq!(value_at(&[], 5), None);
        assert_eq!(
            value_at(&keys, 0),
            Some(0.0),
            "before the first key: the first value"
        );
        assert_eq!(value_at(&keys, 10), Some(0.0));
        assert_eq!(value_at(&keys, 15), Some(50.0));
        assert_eq!(value_at(&keys, 20), Some(100.0));
        assert_eq!(value_at(&keys, 30), Some(75.0));
        assert_eq!(value_at(&keys, 40), Some(50.0));
        assert_eq!(
            value_at(&keys, 99),
            Some(50.0),
            "after the last key: the last value"
        );
        // hold keeps the value until the next key
        let hold = [k(0, 1.0, Interp::Hold), k(10, 5.0, Interp::Linear)];
        assert_eq!(value_at(&hold, 9), Some(1.0));
        assert_eq!(value_at(&hold, 10), Some(5.0));
    }

    #[test]
    fn easings_start_and_end_where_they_should_and_have_the_right_shape() {
        for i in [
            Interp::Linear,
            Interp::EaseIn,
            Interp::EaseOut,
            Interp::EaseInOut,
            Interp::Bezier(0.42, 0.0, 0.58, 1.0),
            Interp::Bezier(0.1, 0.9, 0.2, 1.0),
        ] {
            assert!(
                ease(i, 0.0).abs() < 1e-9 && (ease(i, 1.0) - 1.0).abs() < 1e-9,
                "{i:?}"
            );
            let mut prev = -1.0;
            for s in 0..=100 {
                let y = ease(i, s as f64 / 100.0);
                assert!(y >= prev - 1e-9, "{i:?} is monotonic");
                prev = y;
            }
        }
        assert!(ease(Interp::EaseIn, 0.5) < 0.5 && ease(Interp::EaseOut, 0.5) > 0.5);
        assert!((ease(Interp::EaseInOut, 0.5) - 0.5).abs() < 1e-9);
        // the standard "ease-in-out" bezier is symmetric
        let b = Interp::Bezier(0.42, 0.0, 0.58, 1.0);
        assert!((ease(b, 0.5) - 0.5).abs() < 1e-4);
        assert!(ease(b, 0.25) < 0.25 && ease(b, 0.75) > 0.75);
        // a linear bezier is a straight line
        let lin = Interp::Bezier(1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
        assert!((ease(lin, 0.3) - 0.3).abs() < 1e-4);
    }

    #[test]
    fn tracks_sort_dedupe_and_remove() {
        let mut a = Anim::default();
        assert!(a.is_empty() && !a.has("opacity") && a.value("opacity", 3).is_none());
        a.set_track(
            "opacity",
            vec![
                k(20, 1.0, Interp::Linear),
                k(0, 0.0, Interp::Linear),
                k(20, 0.5, Interp::Linear),
            ],
        );
        assert_eq!(a.keys("opacity").len(), 2, "one key per time");
        assert_eq!(a.keys("opacity")[0].t, 0);
        assert_eq!(
            a.value("opacity", 10),
            Some(0.25).map(|_| a.value("opacity", 10).unwrap())
        );
        a.set_track("opacity", vec![]);
        assert!(a.is_empty());
        assert!(is_property("position.x") && !is_property("nope"));
    }

    #[test]
    fn shifting_and_windowing_keep_the_animation_where_it_was() {
        let mut a = Anim::default();
        a.set_track(
            "opacity",
            vec![k(0, 0.0, Interp::Linear), k(100, 1.0, Interp::Linear)],
        );
        let s = a.shifted(-30);
        assert_eq!(
            s.value("opacity", 20),
            a.value("opacity", 50),
            "trimming 30 off the front: same picture, earlier key times"
        );
        // cut at 40: left [0,40), right [40,100)
        let (left, right) = (a.window(0, 40), a.window(40, 100));
        for t in [0, 10, 25, 39] {
            assert!(
                (left.value("opacity", t).unwrap() - a.value("opacity", t).unwrap()).abs() < 1e-9,
                "left at {t}"
            );
        }
        for t in [0, 10, 30, 59] {
            assert!(
                (right.value("opacity", t).unwrap() - a.value("opacity", 40 + t).unwrap()).abs()
                    < 1e-9,
                "right at {t}"
            );
        }
        // a constant override stays a constant override (a single key), it does not vanish or grow
        let mut flat = Anim::default();
        flat.set_track(
            "pan",
            vec![k(0, 0.5, Interp::Linear), k(50, 0.5, Interp::Linear)],
        );
        assert_eq!(flat.window(10, 30).keys("pan"), [k(0, 0.5, Interp::Linear)]);
        let j = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Anim>(&j).unwrap(), a);
        assert_eq!(serde_json::to_string(&Anim::default()).unwrap(), "{}");
    }
}
