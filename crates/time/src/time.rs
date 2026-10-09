use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::Rational;

/// How to resolve a value that falls between two ticks of the target rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    Floor,
    Ceil,
    /// Nearest tick; exact halves round away from zero.
    Nearest,
}

/// `value` ticks at `rate` ticks per second, i.e. `value / rate` seconds.
///
/// Equality and ordering compare the instant in time, not the representation:
/// `48 @ 24` equals `2 @ 1`. Hashing is therefore not provided.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RationalTime {
    pub value: i64,
    pub rate: Rational,
}

fn div_round(n: i128, d: i128, mode: Rounding) -> i128 {
    debug_assert!(d > 0);
    let q = n.div_euclid(d);
    let r = n.rem_euclid(d);
    if r == 0 {
        return q;
    }
    match mode {
        Rounding::Floor => q,
        Rounding::Ceil => q + 1,
        Rounding::Nearest => match (2 * r).cmp(&d) {
            Ordering::Less => q,
            Ordering::Greater => q + 1,
            // exact half: away from zero
            Ordering::Equal => {
                if n >= 0 {
                    q + 1
                } else {
                    q
                }
            }
        },
    }
}

impl RationalTime {
    pub fn new(value: i64, rate: Rational) -> Self {
        RationalTime { value, rate }
    }

    pub fn zero(rate: Rational) -> Self {
        RationalTime { value: 0, rate }
    }

    /// Converts to ticks at `rate`, resolving fractions per `mode`.
    /// Returns `None` if the result does not fit in `i64`.
    pub fn rescale(self, rate: Rational, mode: Rounding) -> Option<RationalTime> {
        // value / self.rate  =  x / rate   =>   x = value * rate.num * self.den / (rate.den * self.num)
        let n = self.value as i128 * rate.num() as i128 * self.rate.den() as i128;
        let d = rate.den() as i128 * self.rate.num() as i128;
        let v = i64::try_from(div_round(n, d, mode)).ok()?;
        Some(RationalTime { value: v, rate })
    }

    /// Like [`rescale`](Self::rescale) but `None` unless the conversion is exact.
    pub fn rescale_exact(self, rate: Rational) -> Option<RationalTime> {
        let f = self.rescale(rate, Rounding::Floor)?;
        let c = self.rescale(rate, Rounding::Ceil)?;
        (f.value == c.value).then_some(f)
    }

    /// Exact sum, expressed at `self.rate`. `None` if `rhs` is not representable
    /// at that rate (or on overflow) — callers must choose a rounding explicitly.
    pub fn checked_add(self, rhs: RationalTime) -> Option<RationalTime> {
        let r = rhs.rescale_exact(self.rate)?;
        Some(RationalTime {
            value: self.value.checked_add(r.value)?,
            rate: self.rate,
        })
    }

    pub fn checked_sub(self, rhs: RationalTime) -> Option<RationalTime> {
        let r = rhs.rescale_exact(self.rate)?;
        Some(RationalTime {
            value: self.value.checked_sub(r.value)?,
            rate: self.rate,
        })
    }

    /// Frame (or sample) index at `rate`, rounding down.
    pub fn to_ticks(self, rate: Rational) -> Option<i64> {
        self.rescale(rate, Rounding::Floor).map(|t| t.value)
    }

    /// Seconds as a float. For display and heuristics only, never for the model.
    pub fn to_seconds_f64(self) -> f64 {
        self.value as f64 / self.rate.as_f64()
    }
}

impl PartialEq for RationalTime {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for RationalTime {}

impl PartialOrd for RationalTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RationalTime {
    fn cmp(&self, other: &Self) -> Ordering {
        // a.v * a.den / a.num  vs  b.v * b.den / b.num   (rates positive)
        let l = self.value as i128 * self.rate.den() as i128 * other.rate.num() as i128;
        let r = other.value as i128 * other.rate.den() as i128 * self.rate.num() as i128;
        l.cmp(&r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R24: Rational = Rational::FPS_24;

    #[test]
    fn serde_round_trip_and_validation() {
        let t = RationalTime::new(5, Rational::FPS_29_97);
        let j = serde_json::to_string(&t).unwrap();
        let back: RationalTime = serde_json::from_str(&j).unwrap();
        assert_eq!(back.value, 5);
        assert_eq!(back.rate, Rational::FPS_29_97);
        assert!(serde_json::from_str::<Rational>(r#"{"num":1,"den":0}"#).is_err());
    }

    #[test]
    fn equality_is_by_instant() {
        let a = RationalTime::new(48, R24);
        let b = RationalTime::new(2, Rational::from_int(1).unwrap());
        assert_eq!(a, b);
        assert!(RationalTime::new(47, R24) < b);
    }

    #[test]
    fn ntsc_is_exact() {
        // 30000 frames at 29.97 is exactly 1001 s
        let t = RationalTime::new(30000, Rational::FPS_29_97);
        assert_eq!(t, RationalTime::new(1001, Rational::from_int(1).unwrap()));
        assert_eq!(t.to_seconds_f64(), 1001.0);
    }

    #[test]
    fn rescale_rounding() {
        // 1 frame @ 24 = 1/24 s = 1.25 frames @ 30
        let t = RationalTime::new(1, R24);
        let r30 = Rational::FPS_30;
        assert_eq!(t.rescale(r30, Rounding::Floor).unwrap().value, 1);
        assert_eq!(t.rescale(r30, Rounding::Ceil).unwrap().value, 2);
        assert_eq!(t.rescale(r30, Rounding::Nearest).unwrap().value, 1);
        assert!(t.rescale_exact(r30).is_none());
        // 4 frames @ 24 = 5 frames @ 30 exactly
        assert_eq!(
            RationalTime::new(4, R24).rescale_exact(r30).unwrap().value,
            5
        );
    }

    #[test]
    fn nearest_half_rounds_away_from_zero() {
        // 1 frame @ 24 -> samples at rate 2 -> 1/12 s... use 1 @ 2 -> rate 1: 0.5
        let two = Rational::from_int(2).unwrap();
        let one = Rational::from_int(1).unwrap();
        assert_eq!(
            RationalTime::new(1, two)
                .rescale(one, Rounding::Nearest)
                .unwrap()
                .value,
            1
        );
        assert_eq!(
            RationalTime::new(-1, two)
                .rescale(one, Rounding::Nearest)
                .unwrap()
                .value,
            -1
        );
        assert_eq!(
            RationalTime::new(-1, two)
                .rescale(one, Rounding::Floor)
                .unwrap()
                .value,
            -1
        );
        assert_eq!(
            RationalTime::new(-1, two)
                .rescale(one, Rounding::Ceil)
                .unwrap()
                .value,
            0
        );
    }

    #[test]
    fn checked_add_requires_exactness() {
        let a = RationalTime::new(10, R24);
        let b = RationalTime::new(12, R24);
        assert_eq!(a.checked_add(b).unwrap().value, 22);
        // 1 frame @ 30 is not a whole frame @ 24
        assert!(a
            .checked_add(RationalTime::new(1, Rational::FPS_30))
            .is_none());
        assert_eq!(a.checked_sub(b).unwrap().value, -2);
    }

    #[test]
    fn audio_samples() {
        let sr = Rational::from_int(48000).unwrap();
        let t = RationalTime::new(1, Rational::FPS_29_97); // 1001/30000 s
        assert_eq!(t.to_ticks(sr), Some(1601)); // 1601.6
        assert_eq!(t.rescale(sr, Rounding::Nearest).unwrap().value, 1602);
    }
}

#[cfg(test)]
mod overflow_tests {
    use super::*;

    #[test]
    fn rescale_that_does_not_fit_is_none() {
        let t = RationalTime::new(i64::MAX, Rational::new(1, 1).unwrap());
        assert!(t
            .rescale(Rational::new(1_000_000, 1).unwrap(), Rounding::Floor)
            .is_none());
        assert!(RationalTime::new(i64::MAX, Rational::FPS_24)
            .checked_add(RationalTime::new(1, Rational::FPS_24))
            .is_none());
        assert!(RationalTime::new(i64::MIN, Rational::FPS_24)
            .checked_sub(RationalTime::new(1, Rational::FPS_24))
            .is_none());
        assert_eq!(RationalTime::zero(Rational::FPS_24).value, 0);
    }
}
