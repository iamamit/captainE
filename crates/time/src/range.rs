use crate::RationalTime;

/// Half-open time range `[start, start + duration)`. Defined once, used everywhere
/// (docs/03: avoids inclusive/exclusive end-frame bugs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRange {
    pub start: RationalTime,
    pub duration: RationalTime,
}

impl TimeRange {
    /// `None` if `duration` is negative.
    pub fn new(start: RationalTime, duration: RationalTime) -> Option<TimeRange> {
        (duration.value >= 0).then_some(TimeRange { start, duration })
    }

    /// Range from `start` up to, but excluding, `end`. `None` if `end < start`.
    pub fn from_start_end(start: RationalTime, end: RationalTime) -> Option<TimeRange> {
        if end < start {
            return None;
        }
        let duration = end.checked_sub(start).or_else(|| {
            // different, incompatible rates: express the duration at the end's rate
            let s = start.rescale(end.rate, crate::Rounding::Floor)?;
            end.checked_sub(s)
        })?;
        Some(TimeRange { start, duration })
    }

    /// First instant not in the range.
    pub fn end_exclusive(&self) -> Option<RationalTime> {
        self.start.checked_add(self.duration)
    }

    pub fn is_empty(&self) -> bool {
        self.duration.value == 0
    }

    pub fn contains(&self, t: RationalTime) -> bool {
        match self.end_exclusive() {
            Some(end) => self.start <= t && t < end,
            None => false,
        }
    }

    pub fn overlaps(&self, other: &TimeRange) -> bool {
        match (self.end_exclusive(), other.end_exclusive()) {
            (Some(a_end), Some(b_end)) => {
                !self.is_empty() && !other.is_empty() && self.start < b_end && other.start < a_end
            }
            _ => false,
        }
    }

    /// Overlapping part, if any; expressed at `self`'s start rate.
    pub fn intersection(&self, other: &TimeRange) -> Option<TimeRange> {
        if !self.overlaps(other) {
            return None;
        }
        let start = self.start.max(other.start);
        let end = self.end_exclusive()?.min(other.end_exclusive()?);
        TimeRange::from_start_end(start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rational;

    fn t(v: i64) -> RationalTime {
        RationalTime::new(v, Rational::FPS_24)
    }
    fn r(s: i64, d: i64) -> TimeRange {
        TimeRange::new(t(s), t(d)).unwrap()
    }

    #[test]
    fn half_open() {
        let a = r(10, 5);
        assert!(a.contains(t(10)));
        assert!(a.contains(t(14)));
        assert!(!a.contains(t(15)));
        assert_eq!(a.end_exclusive().unwrap(), t(15));
    }

    #[test]
    fn adjacent_ranges_do_not_overlap() {
        assert!(!r(0, 5).overlaps(&r(5, 5)));
        assert!(r(0, 6).overlaps(&r(5, 5)));
    }

    #[test]
    fn intersection() {
        let i = r(0, 10).intersection(&r(6, 10)).unwrap();
        assert_eq!(i, r(6, 4));
        assert!(r(0, 5).intersection(&r(5, 5)).is_none());
    }

    #[test]
    fn negative_duration_rejected() {
        assert!(TimeRange::new(t(0), t(-1)).is_none());
        assert!(TimeRange::from_start_end(t(5), t(3)).is_none());
    }

    #[test]
    fn mixed_rates() {
        let a = TimeRange::from_start_end(t(0), RationalTime::new(30, Rational::FPS_30)).unwrap();
        assert_eq!(
            a.duration,
            RationalTime::new(1, Rational::from_int(1).unwrap())
        );
        assert!(a.contains(t(23)));
        assert!(!a.contains(t(24)));
    }
}
