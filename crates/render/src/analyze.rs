//! Looking at pictures: the numbers an auto-correction needs, and the correction they suggest.

use captaine_model::ColorProps;

use crate::color::to_linear;
use crate::Frame8;

/// What a handful of frames look like, measured on the pixels that matter (opaque, not crushed or clipped).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorStats {
    /// Median brightness in linear light
    pub median_lin: f64,
    /// 1st and 99th percentile of brightness in display values (0..1): how much of the range is used
    pub low: f64,
    pub high: f64,
    /// Average red, green and blue of the mid-tones in linear light
    pub mean_lin: [f64; 3],
    /// Average saturation (0..1) of the mid-tones
    pub saturation: f64,
}

impl ColorStats {
    pub fn of(frames: &[&Frame8]) -> Option<ColorStats> {
        let mut luma: Vec<f32> = Vec::new();
        let (mut sum, mut n, mut sat) = ([0.0f64; 3], 0.0f64, 0.0f64);
        for f in frames {
            // about 40 000 pixels per frame is plenty
            let step = (f.rgba.len() / 4 / 40_000).max(1);
            for px in f.rgba.chunks_exact(4).step_by(step) {
                if px[3] < 128 {
                    continue;
                }
                let (r, g, b) = (
                    px[0] as f32 / 255.0,
                    px[1] as f32 / 255.0,
                    px[2] as f32 / 255.0,
                );
                let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                luma.push(y);
                if (0.1..0.9).contains(&y) {
                    let lin = [
                        to_linear(px[0]) as f64,
                        to_linear(px[1]) as f64,
                        to_linear(px[2]) as f64,
                    ];
                    for c in 0..3 {
                        sum[c] += lin[c];
                    }
                    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
                    sat += if mx > 0.0 {
                        ((mx - mn) / mx) as f64
                    } else {
                        0.0
                    };
                    n += 1.0;
                }
            }
        }
        if luma.len() < 16 {
            return None;
        }
        luma.sort_by(|a, b| a.total_cmp(b));
        let at = |q: f64| luma[((luma.len() - 1) as f64 * q) as usize] as f64;
        // the median in linear light is the linear value of the median brightness
        let med_srgb = at(0.5);
        let med_lin = to_linear((med_srgb * 255.0).round().clamp(0.0, 255.0) as u8) as f64;
        let (mean_lin, saturation) = if n > 0.0 {
            ([sum[0] / n, sum[1] / n, sum[2] / n], sat / n)
        } else {
            ([med_lin; 3], 0.0)
        };
        Some(ColorStats {
            median_lin: med_lin.max(1e-4),
            low: at(0.01),
            high: at(0.99),
            mean_lin,
            saturation,
        })
    }
}

/// Keep a correction moderate: `strength` of the way there.
fn damp(v: f64, neutral: f64, strength: f64) -> f64 {
    neutral + (v - neutral) * strength
}

/// The grade that makes a picture look balanced: well exposed, using its range, without a colour cast and
/// with healthy colour. Always a gentle move; scenes that are dark or warm on purpose stay recognisable.
pub fn auto_correction(s: &ColorStats) -> ColorProps {
    let exposure = damp(((0.16 / s.median_lin).log2()).clamp(-2.0, 2.0), 0.0, 0.75);
    let spread = (s.high - s.low).max(0.05);
    let contrast = damp((0.88 / spread).clamp(0.85, 1.4), 1.0, 0.6);
    let [r, _, b] = s.mean_lin;
    let warm = 0.5 * (b.max(1e-4) / r.max(1e-4)).log2();
    let temperature =
        ColorProps::NEUTRAL_KELVIN + damp((warm / 0.25 * 1500.0).clamp(-5000.0, 5000.0), 0.0, 0.85);
    let saturation = 100.0 * damp((0.42 / s.saturation.max(0.05)).clamp(0.85, 1.25), 1.0, 0.6);
    ColorProps {
        exposure,
        contrast,
        saturation,
        temperature,
    }
}

/// The grade that makes `target` look like `reference`: the same brightness, colour balance and colour.
pub fn match_correction(reference: &ColorStats, target: &ColorStats) -> ColorProps {
    let exposure = ((reference.median_lin / target.median_lin).log2()).clamp(-2.5, 2.5) * 0.9;
    let spread_t = (target.high - target.low).max(0.05);
    let spread_r = (reference.high - reference.low).max(0.05);
    let contrast = (spread_r / spread_t).clamp(0.8, 1.5).powf(0.7);
    let warm = 0.5
        * ((reference.mean_lin[0].max(1e-4) * target.mean_lin[2].max(1e-4))
            / (reference.mean_lin[2].max(1e-4) * target.mean_lin[0].max(1e-4)))
        .log2();
    let temperature =
        ColorProps::NEUTRAL_KELVIN + (warm / 0.25 * 1500.0).clamp(-5000.0, 5000.0) * 0.95;
    let saturation =
        100.0 * (reference.saturation.max(0.05) / target.saturation.max(0.05)).clamp(0.6, 1.6);
    ColorProps {
        exposure,
        contrast,
        saturation,
        temperature,
    }
}

#[cfg(test)]
mod tests {
    use captaine_model::{BlendMode, Transform, VideoProps};

    use super::*;
    use crate::{draw_layer, Canvas};

    /// A smooth-ish test picture: a horizontal gradient of `base` scaled between dark and light.
    fn scene(base: [f32; 3], gain: f32) -> Frame8 {
        let (w, h) = (96u32, 54u32);
        let mut f = Frame8::solid(w, h, [0, 0, 0, 255]);
        for y in 0..h {
            for x in 0..w {
                let t = x as f32 / (w - 1) as f32;
                let i = ((y * w + x) * 4) as usize;
                for (c, b) in base.iter().enumerate() {
                    f.rgba[i + c] = ((b * (0.25 + 0.75 * t) * gain).clamp(0.0, 1.0) * 255.0) as u8;
                }
            }
        }
        f
    }

    fn graded(f: &Frame8, color: ColorProps) -> Frame8 {
        let mut c = Canvas::black(f.width, f.height);
        let props = VideoProps {
            transform: Transform::default(),
            color,
            opacity: 1.0,
            blend: BlendMode::Normal,
        };
        draw_layer(&mut c, f, &props);
        Frame8 {
            width: c.width,
            height: c.height,
            rgba: c.to_rgba8(),
        }
    }

    #[test]
    fn a_dark_blue_picture_gets_brighter_and_warmer() {
        let dark_blue = scene([0.5, 0.6, 0.95], 0.35);
        let s = ColorStats::of(&[&dark_blue]).unwrap();
        let fix = auto_correction(&s);
        assert!(fix.exposure > 0.3, "{fix:?}");
        assert!(
            fix.temperature > ColorProps::NEUTRAL_KELVIN + 300.0,
            "warmer to cancel the cast: {fix:?}"
        );
        let after = ColorStats::of(&[&graded(&dark_blue, fix)]).unwrap();
        assert!(after.median_lin > s.median_lin * 1.3, "brighter");
        let cast = |m: &ColorStats| (m.mean_lin[2] / m.mean_lin[0]).ln().abs();
        assert!(
            cast(&after) < cast(&s) * 0.75,
            "less blue cast: {} -> {}",
            cast(&s),
            cast(&after)
        );
    }

    #[test]
    fn a_picture_that_is_already_fine_is_left_nearly_alone() {
        let ok = scene([0.9, 0.9, 0.9], 1.0);
        let fix = auto_correction(&ColorStats::of(&[&ok]).unwrap());
        assert!(fix.exposure.abs() < 0.9, "{fix:?}");
        assert!(
            (fix.temperature - ColorProps::NEUTRAL_KELVIN).abs() < 150.0,
            "{fix:?}"
        );
        assert!((fix.saturation - 100.0).abs() < 30.0);
    }

    #[test]
    fn matching_pulls_one_shot_toward_the_other() {
        let reference = scene([1.0, 0.85, 0.6], 0.9); // warm and bright
        let target = scene([0.6, 0.7, 1.0], 0.4); // cool and dark
        let (r, t) = (
            ColorStats::of(&[&reference]).unwrap(),
            ColorStats::of(&[&target]).unwrap(),
        );
        let fix = match_correction(&r, &t);
        assert!(fix.exposure > 0.5 && fix.temperature > 7000.0, "{fix:?}");
        let after = ColorStats::of(&[&graded(&target, fix)]).unwrap();
        let dist = |a: &ColorStats, b: &ColorStats| {
            (a.median_lin / b.median_lin).ln().abs()
                + ((a.mean_lin[0] / a.mean_lin[2]) / (b.mean_lin[0] / b.mean_lin[2]))
                    .ln()
                    .abs()
        };
        assert!(
            dist(&after, &r) < dist(&t, &r) * 0.5,
            "{} -> {}",
            dist(&t, &r),
            dist(&after, &r)
        );
        // too little picture to measure
        assert!(ColorStats::of(&[&Frame8::solid(2, 2, [0, 0, 0, 255])]).is_none());
    }
}
