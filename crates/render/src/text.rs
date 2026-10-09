//! Text and simple shapes drawn into an RGBA picture with straight alpha, using the bundled font.
//! Pure CPU and table-free arithmetic, so the same title draws identically everywhere.

use std::sync::OnceLock;

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};

use crate::Frame8;

fn font() -> &'static FontRef<'static> {
    static F: OnceLock<FontRef<'static>> = OnceLock::new();
    F.get_or_init(|| {
        FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("bundled font parses")
    })
}

/// Distance the faux-bold second pass is shifted, as a fraction of the text size.
const BOLD_SHIFT: f32 = 0.034;

/// Width of `text` in pixels at `size` pixels tall.
pub fn text_width(text: &str, size: f32, bold: bool) -> f32 {
    let f = font().as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let id = f.glyph_id(c);
        if let Some(p) = prev {
            w += f.kern(p, id);
        }
        w += f.h_advance(id);
        prev = Some(id);
    }
    if bold {
        w += size * BOLD_SHIFT * 2.0;
    }
    w
}

/// Pixels from the baseline up to the top of a capital letter, and down to the lowest descender.
pub fn ascent_descent(size: f32) -> (f32, f32) {
    let f = font().as_scaled(PxScale::from(size));
    (f.ascent(), -f.descent())
}

/// Composite one source pixel over a straight-alpha destination.
fn over(dst: &mut [u8], rgb: [u8; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = a + da * (1.0 - a);
    for k in 0..3 {
        let v = (rgb[k] as f32 * a + dst[k] as f32 * da * (1.0 - a)) / out_a;
        dst[k] = v.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

#[allow(clippy::too_many_arguments)]
/// Draw `text` with its left end at `x` and its baseline at `baseline`.
pub fn draw_text(
    frame: &mut Frame8,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    rgb: [u8; 3],
    alpha: f32,
    bold: bool,
) {
    let scale = PxScale::from(size);
    let f = font().as_scaled(scale);
    let passes: &[f32] = if bold {
        &[0.0, size * BOLD_SHIFT, size * BOLD_SHIFT * 2.0]
    } else {
        &[0.0]
    };
    let (w, h) = (frame.width as i32, frame.height as i32);
    for &shift in passes {
        let mut caret = x + shift;
        let mut prev = None;
        for c in text.chars() {
            let id = f.glyph_id(c);
            if let Some(p) = prev {
                caret += f.kern(p, id);
            }
            let glyph = id.with_scale_and_position(scale, point(caret, baseline));
            if let Some(outlined) = font().outline_glyph(glyph) {
                let b = outlined.px_bounds();
                outlined.draw(|gx, gy, cov| {
                    let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && px < w && py < h && cov > 0.0 {
                        let i = ((py * w + px) * 4) as usize;
                        over(&mut frame.rgba[i..i + 4], rgb, cov * alpha);
                    }
                });
            }
            caret += f.h_advance(id);
            prev = Some(id);
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Fill the rectangle `[x0,x1) x [y0,y1)` with rounded corners of `radius`, anti-aliased.
pub fn fill_rounded_rect(
    frame: &mut Frame8,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
    rgb: [u8; 3],
    alpha: f32,
) {
    let (w, h) = (frame.width as i32, frame.height as i32);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (hx, hy) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let r = radius.min(hx).min(hy).max(0.0);
    for py in (y0.floor() as i32).max(0)..(y1.ceil() as i32).min(h) {
        for px in (x0.floor() as i32).max(0)..(x1.ceil() as i32).min(w) {
            // signed distance to the rounded box, measured from the pixel centre
            let (dx, dy) = (
                (px as f32 + 0.5 - cx).abs() - (hx - r),
                (py as f32 + 0.5 - cy).abs() - (hy - r),
            );
            let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
            let d = outside + dx.max(dy).min(0.0) - r;
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov > 0.0 {
                let i = ((py * w + px) * 4) as usize;
                over(&mut frame.rgba[i..i + 4], rgb, cov * alpha);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(w: u32, h: u32) -> Frame8 {
        Frame8::solid(w, h, [0, 0, 0, 0])
    }

    #[test]
    fn width_grows_with_text_and_size() {
        assert!(text_width("WWWW", 40.0, false) > text_width("iiii", 40.0, false));
        assert!(text_width("Hello", 80.0, false) > text_width("Hello", 40.0, false) * 1.9);
        assert!(text_width("Hello", 40.0, true) > text_width("Hello", 40.0, false));
        assert_eq!(text_width("", 40.0, false), 0.0);
    }

    #[test]
    fn text_lands_between_baseline_and_ascent() {
        let mut f = blank(300, 100);
        draw_text(&mut f, "HE", 10.0, 70.0, 40.0, [255, 255, 255], 1.0, false);
        let inked = |y: u32| (0..300u32).any(|x| f.rgba[((y * 300 + x) * 4 + 3) as usize] > 128);
        assert!(inked(60), "the letters stand on the baseline");
        assert!(
            !inked(20) && !inked(90),
            "and nothing is far above or below"
        );
        // white ink: rgb stays white wherever there is any alpha
        assert!(f.rgba.chunks(4).filter(|p| p[3] > 0).all(|p| p[0] >= 250));
    }

    #[test]
    fn bold_is_heavier_and_drawing_is_deterministic() {
        let ink = |bold: bool| {
            let mut f = blank(200, 80);
            draw_text(&mut f, "Hi", 5.0, 60.0, 50.0, [255; 3], 1.0, bold);
            f
        };
        let (a, b) = (ink(false), ink(true));
        let sum = |f: &Frame8| f.rgba.chunks(4).map(|p| p[3] as u32).sum::<u32>();
        assert!(sum(&b) > sum(&a));
        assert_eq!(ink(true), ink(true));
    }

    #[test]
    fn clipping_at_the_edges_is_safe() {
        let mut f = blank(40, 20);
        draw_text(
            &mut f,
            "overflowing text",
            -30.0,
            10.0,
            40.0,
            [255; 3],
            1.0,
            true,
        );
        fill_rounded_rect(&mut f, -10.0, -10.0, 100.0, 100.0, 8.0, [9, 9, 9], 1.0);
        assert_eq!(f.rgba.len(), 40 * 20 * 4);
    }

    #[test]
    fn rounded_rect_has_soft_round_corners() {
        let mut f = blank(40, 40);
        fill_rounded_rect(&mut f, 4.0, 4.0, 36.0, 36.0, 10.0, [200, 100, 50], 1.0);
        let a = |x: u32, y: u32| f.rgba[((y * 40 + x) * 4 + 3) as usize];
        assert_eq!(a(20, 20), 255);
        assert_eq!(a(5, 5), 0, "the very corner is cut away");
        assert!(a(20, 4) > 200 && a(2, 20) == 0);
        assert_eq!(
            &f.rgba[((20 * 40 + 20) * 4) as usize..][..3],
            &[200, 100, 50]
        );
    }

    #[test]
    fn translucent_text_over_a_box_blends() {
        let mut f = Frame8::solid(10, 10, [255, 0, 0, 255]);
        over(&mut f.rgba[0..4], [0, 0, 255], 0.5);
        assert_eq!(&f.rgba[0..4], &[128, 0, 128, 255]);
    }
}
