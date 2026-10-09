use std::fmt;

use crate::Rational;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimecodeError {
    /// Frame rate is not a SMPTE-style rate (nominal rate must be 1..=120)
    BadRate,
    /// Drop-frame only exists for 30000/1001 and 60000/1001
    DropFrameNotSupported,
    /// Field out of range, or a frame number that drop-frame skips
    InvalidFields,
    Parse,
    Negative,
}

impl fmt::Display for TimecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TimecodeError::BadRate => "unsupported frame rate",
            TimecodeError::DropFrameNotSupported => "drop-frame needs 29.97 or 59.94 fps",
            TimecodeError::InvalidFields => "invalid timecode fields",
            TimecodeError::Parse => "cannot parse timecode",
            TimecodeError::Negative => "negative frame count",
        };
        f.write_str(s)
    }
}
impl std::error::Error for TimecodeError {}

/// SMPTE timecode at a given rate. Drop-frame skips frame *numbers* (not frames)
/// so that 29.97/59.94 timecode tracks wall-clock time; it prints with `;` before frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timecode {
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
    pub frames: u32,
    pub rate: Rational,
    pub drop_frame: bool,
}

fn drop_count(rate: Rational, drop_frame: bool) -> Result<i64, TimecodeError> {
    if !drop_frame {
        return Ok(0);
    }
    match (rate.num(), rate.den()) {
        (30000, 1001) => Ok(2),
        (60000, 1001) => Ok(4),
        _ => Err(TimecodeError::DropFrameNotSupported),
    }
}

fn nominal(rate: Rational) -> Result<i64, TimecodeError> {
    let n = rate.nominal();
    if (1..=120).contains(&n) {
        Ok(n)
    } else {
        Err(TimecodeError::BadRate)
    }
}

impl Timecode {
    /// Timecode for a zero-based frame count.
    pub fn from_frames(
        frames: i64,
        rate: Rational,
        drop_frame: bool,
    ) -> Result<Timecode, TimecodeError> {
        if frames < 0 {
            return Err(TimecodeError::Negative);
        }
        let nom = nominal(rate)?;
        let drop = drop_count(rate, drop_frame)?;
        let mut n = frames;
        if drop > 0 {
            let per_min = nom * 60 - drop;
            let per_10min = nom * 600 - drop * 9;
            let d = n / per_10min;
            let m = n % per_10min;
            n += 9 * drop * d;
            if m >= drop {
                n += drop * ((m - drop) / per_min);
            }
        }
        Ok(Timecode {
            hours: ((n / (nom * 3600)) % 24) as u32,
            minutes: ((n / (nom * 60)) % 60) as u32,
            seconds: ((n / nom) % 60) as u32,
            frames: (n % nom) as u32,
            rate,
            drop_frame,
        })
    }

    /// Zero-based frame count (hours wrap at 24 on the way in, so this is < 24 h).
    pub fn to_frames(&self) -> Result<i64, TimecodeError> {
        let nom = nominal(self.rate)?;
        let drop = drop_count(self.rate, self.drop_frame)?;
        let (h, m, s, f) = (
            self.hours as i64,
            self.minutes as i64,
            self.seconds as i64,
            self.frames as i64,
        );
        if m >= 60 || s >= 60 || f >= nom || h >= 24 {
            return Err(TimecodeError::InvalidFields);
        }
        // drop-frame skips frame numbers 0..drop at the start of every minute except every 10th
        if drop > 0 && s == 0 && f < drop && m % 10 != 0 {
            return Err(TimecodeError::InvalidFields);
        }
        let total_minutes = 60 * h + m;
        Ok(nom * 3600 * h + nom * 60 * m + nom * s + f
            - drop * (total_minutes - total_minutes / 10))
    }

    /// Parses `HH:MM:SS:FF` or `HH:MM:SS;FF` (`;` or `.` before frames means drop-frame).
    pub fn parse(s: &str, rate: Rational) -> Result<Timecode, TimecodeError> {
        let b = s.trim().as_bytes();
        if b.len() != 11 || b[2] != b':' || b[5] != b':' {
            return Err(TimecodeError::Parse);
        }
        let drop_frame = match b[8] {
            b':' => false,
            b';' | b'.' => true,
            _ => return Err(TimecodeError::Parse),
        };
        let field = |a: usize| -> Result<u32, TimecodeError> {
            std::str::from_utf8(&b[a..a + 2])
                .ok()
                .filter(|t| t.bytes().all(|c| c.is_ascii_digit()))
                .and_then(|t| t.parse().ok())
                .ok_or(TimecodeError::Parse)
        };
        let tc = Timecode {
            hours: field(0)?,
            minutes: field(3)?,
            seconds: field(6)?,
            frames: field(9)?,
            rate,
            drop_frame,
        };
        tc.to_frames()?; // validate
        Ok(tc)
    }
}

impl fmt::Display for Timecode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sep = if self.drop_frame { ';' } else { ':' };
        write!(
            f,
            "{:02}:{:02}:{:02}{}{:02}",
            self.hours, self.minutes, self.seconds, sep, self.frames
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn df(n: i64) -> String {
        Timecode::from_frames(n, Rational::FPS_29_97, true)
            .unwrap()
            .to_string()
    }

    #[test]
    fn non_drop() {
        let tc = Timecode::from_frames(24 * 3600 + 24 * 61 + 5, Rational::FPS_24, false).unwrap();
        assert_eq!(tc.to_string(), "01:01:01:05");
        assert_eq!(tc.to_frames().unwrap(), 24 * 3600 + 24 * 61 + 5);
    }

    #[test]
    fn drop_frame_known_values() {
        assert_eq!(df(0), "00:00:00;00");
        assert_eq!(df(1799), "00:00:59;29");
        // minute 1 skips ;00 and ;01
        assert_eq!(df(1800), "00:01:00;02");
        assert_eq!(df(17981), "00:09:59;29");
        // every 10th minute keeps its frames
        assert_eq!(df(17982), "00:10:00;00");
        // one hour of 29.97 DF is 107892 frames
        assert_eq!(df(107892), "01:00:00;00");
    }

    #[test]
    fn drop_frame_round_trip() {
        for n in (0..300_000).step_by(7) {
            let tc = Timecode::from_frames(n, Rational::FPS_29_97, true).unwrap();
            assert_eq!(tc.to_frames().unwrap(), n, "frame {n} -> {tc}");
        }
        for n in (0..600_000).step_by(11) {
            let tc = Timecode::from_frames(n, Rational::FPS_59_94, true).unwrap();
            assert_eq!(tc.to_frames().unwrap(), n);
        }
    }

    #[test]
    fn parse_and_reject_dropped_numbers() {
        let tc = Timecode::parse("00:01:00;02", Rational::FPS_29_97).unwrap();
        assert_eq!(tc.to_frames().unwrap(), 1800);
        assert_eq!(
            Timecode::parse("00:01:00;00", Rational::FPS_29_97),
            Err(TimecodeError::InvalidFields)
        );
        assert!(Timecode::parse("00:10:00;00", Rational::FPS_29_97).is_ok());
        assert_eq!(
            Timecode::parse("00:00:00:24", Rational::FPS_24),
            Err(TimecodeError::InvalidFields)
        );
        assert_eq!(
            Timecode::parse("bogus", Rational::FPS_24),
            Err(TimecodeError::Parse)
        );
    }

    #[test]
    fn drop_frame_needs_ntsc_rate() {
        assert_eq!(
            Timecode::from_frames(10, Rational::FPS_24, true),
            Err(TimecodeError::DropFrameNotSupported)
        );
    }
}
