use std::fmt;

use serde::{Deserialize, Serialize};

/// A positive rational number, always stored in lowest terms with `den > 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawRational", into = "RawRational")]
pub struct Rational {
    num: i64,
    den: i64,
}

#[derive(Serialize, Deserialize)]
struct RawRational {
    num: i64,
    den: i64,
}

impl From<Rational> for RawRational {
    fn from(r: Rational) -> Self {
        RawRational {
            num: r.num,
            den: r.den,
        }
    }
}

impl TryFrom<RawRational> for Rational {
    type Error = String;
    fn try_from(r: RawRational) -> Result<Self, String> {
        Rational::new(r.num, r.den).ok_or_else(|| format!("invalid rational {}/{}", r.num, r.den))
    }
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl Rational {
    pub const FPS_24: Rational = Rational { num: 24, den: 1 };
    pub const FPS_25: Rational = Rational { num: 25, den: 1 };
    pub const FPS_30: Rational = Rational { num: 30, den: 1 };
    pub const FPS_60: Rational = Rational { num: 60, den: 1 };
    /// 23.976 fps
    pub const FPS_23_976: Rational = Rational {
        num: 24000,
        den: 1001,
    };
    /// 29.97 fps
    pub const FPS_29_97: Rational = Rational {
        num: 30000,
        den: 1001,
    };
    /// 59.94 fps
    pub const FPS_59_94: Rational = Rational {
        num: 60000,
        den: 1001,
    };

    /// Builds `num/den` in lowest terms. Returns `None` unless both are positive.
    pub fn new(num: i64, den: i64) -> Option<Rational> {
        if num <= 0 || den <= 0 {
            return None;
        }
        let g = gcd(num as i128, den as i128) as i64;
        Some(Rational {
            num: num / g,
            den: den / g,
        })
    }

    pub fn from_int(n: i64) -> Option<Rational> {
        Rational::new(n, 1)
    }

    pub fn num(self) -> i64 {
        self.num
    }

    pub fn den(self) -> i64 {
        self.den
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Nominal integer frame rate: 30000/1001 -> 30, 24 -> 24.
    pub fn nominal(self) -> i64 {
        (self.num + self.den / 2) / self.den
    }

    /// True for the NTSC-style rates (`N*1000/1001`) that have a drop-frame timecode.
    pub fn is_ntsc(self) -> bool {
        self.den == 1001 && self.num % 1000 == 0
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}
