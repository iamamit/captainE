//! Timeline interchange (docs/13): the formats other editing systems read and write.
//!
//! * [`edl`]: CMX3600 edit decision lists (cuts, speed changes), the lowest common denominator.
//! * [`otio`]: OpenTimelineIO JSON (tracks, clips, gaps, transitions, retimes), also carrying captainE's
//!   own properties so a round trip loses nothing.
//! * [`fcpxml`]: Final Cut Pro XML 1.9, export only.

pub mod edl;
pub mod fcpxml;
pub mod otio;
pub mod srt;

use captaine_time::Rational;

/// The standard frame rate nearest to `fps` (23.976 is 24000/1001, and so on).
pub fn rate_from_f64(fps: f64) -> Rational {
    const STANDARD: [(i64, i64); 10] = [
        (24000, 1001),
        (24, 1),
        (25, 1),
        (30000, 1001),
        (30, 1),
        (48, 1),
        (50, 1),
        (60000, 1001),
        (60, 1),
        (120, 1),
    ];
    for (n, d) in STANDARD {
        if (fps - n as f64 / d as f64).abs() < 0.002 {
            return Rational::new(n, d).expect("valid");
        }
    }
    Rational::new((fps * 1000.0).round() as i64, 1000).unwrap_or(Rational::FPS_24)
}

/// A path as a `file://` URL, with the characters that need it escaped.
pub fn file_url(path: &str) -> String {
    let mut out = String::from("file://");
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The inverse of [`file_url`] (also accepts a plain path).
pub fn path_from_url(url: &str) -> String {
    let raw = url.strip_prefix("file://").unwrap_or(url);
    let raw = raw.strip_prefix("localhost").unwrap_or(raw);
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&raw[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
