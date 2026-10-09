//! Rational time for CaptainE (docs/03).
//!
//! Time is never stored as float seconds. A [`RationalTime`] is `value` ticks
//! at a [`Rational`] rate (ticks per second), so 29.97 fps is exactly
//! `30000/1001`. A [`TimeRange`] is half-open: `[start, start + duration)`.

mod range;
mod rate;
mod time;
mod timecode;

pub use range::TimeRange;
pub use rate::Rational;
pub use time::{RationalTime, Rounding};
pub use timecode::{Timecode, TimecodeError};
