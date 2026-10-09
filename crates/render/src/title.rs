//! Title templates: lay out a [`Title`] on a canvas of a given size and draw it into a transparent picture.
//! Sizes are in pixels on a 1080-pixel-tall canvas and scale with the canvas height.

use captaine_model::{Shape, ShapeKind, Title, TitleTemplate};

use crate::text::{ascent_descent, draw_text, fill_rounded_rect, text_width};
use crate::Frame8;

const SUBTITLE: [u8; 3] = [196, 196, 205];
const PLATE: [u8; 3] = [24, 24, 28];

/// The title as a canvas-sized picture, transparent except for the title itself.
pub fn render_title(t: &Title, width: u32, height: u32) -> Frame8 {
    let mut f = Frame8::solid(width, height, [0, 0, 0, 0]);
    if let Some(shape) = &t.shape {
        return render_shape(shape, width, height);
    }
    if t.adjustment {
        return f; // an adjustment layer has no picture of its own
    }
    if t.text.trim().is_empty() && t.subtitle.trim().is_empty() {
        return f;
    }
    let (w, h) = (width as f32, height as f32);
    let s = (t.style.size as f32) * h / 1080.0;
    match t.style.template {
        TitleTemplate::LowerThird => lower_third(&mut f, t, w, h, s),
        TitleTemplate::Centered => centered(&mut f, t, w, h, s),
        TitleTemplate::Caption => caption(&mut f, t, w, h, s),
    }
    f
}

fn lower_third(f: &mut Frame8, t: &Title, w: f32, h: f32, s: f32) {
    let sub_size = s * 0.58;
    let has_sub = !t.subtitle.trim().is_empty();
    let (asc, _) = ascent_descent(s);
    let (sub_asc, sub_desc) = ascent_descent(sub_size);
    let pad_x = s * 0.55;
    let pad_y = s * 0.42;
    let bar = (s * 0.12).max(2.0);
    let line_gap = s * 0.14;
    let text_w = text_width(&t.text, s, true).max(if has_sub {
        text_width(&t.subtitle, sub_size, false)
    } else {
        0.0
    });
    let box_w = bar + pad_x * 2.0 + text_w;
    let content_h = asc
        + s * 0.25
        + if has_sub {
            line_gap + sub_asc + sub_desc
        } else {
            0.0
        };
    let box_h = pad_y * 2.0 + content_h;
    let (x0, y1) = (w * 0.06, h * 0.89);
    let y0 = y1 - box_h;
    fill_rounded_rect(f, x0, y0, x0 + box_w, y1, s * 0.1, PLATE, 0.86);
    fill_rounded_rect(f, x0, y0, x0 + bar, y1, 0.0, t.style.accent, 1.0);
    let tx = x0 + bar + pad_x;
    let base = y0 + pad_y + asc;
    draw_text(f, &t.text, tx, base, s, t.style.color, 1.0, true);
    if has_sub {
        draw_text(
            f,
            &t.subtitle,
            tx,
            base + s * 0.25 + line_gap + sub_asc,
            sub_size,
            SUBTITLE,
            1.0,
            false,
        );
    }
}

fn centered(f: &mut Frame8, t: &Title, w: f32, h: f32, s: f32) {
    let has_sub = !t.subtitle.trim().is_empty();
    let sub_size = s * 0.42;
    let (asc, _) = ascent_descent(s);
    let total = asc + if has_sub { s * 0.3 + sub_size } else { 0.0 };
    let base = (h - total) / 2.0 + asc;
    let x = (w - text_width(&t.text, s, true)) / 2.0;
    let off = (s * 0.025).max(1.0);
    draw_text(f, &t.text, x + off, base + off, s, [0, 0, 0], 0.5, true); // soft shadow
    draw_text(f, &t.text, x, base, s, t.style.color, 1.0, true);
    if has_sub {
        let sx = (w - text_width(&t.subtitle, sub_size, false)) / 2.0;
        let sb = base + s * 0.3 + sub_size * 0.8;
        draw_text(
            f,
            &t.subtitle,
            sx + off,
            sb + off,
            sub_size,
            [0, 0, 0],
            0.5,
            false,
        );
        draw_text(f, &t.subtitle, sx, sb, sub_size, SUBTITLE, 1.0, false);
    }
}

fn caption(f: &mut Frame8, t: &Title, w: f32, h: f32, s: f32) {
    let text = if t.text.trim().is_empty() {
        &t.subtitle
    } else {
        &t.text
    };
    let (asc, desc) = ascent_descent(s);
    let tw = text_width(text, s, false);
    let (pad_x, pad_y) = (s * 0.6, s * 0.3);
    let box_h = asc + desc + pad_y * 2.0;
    let y1 = h * 0.91;
    let y0 = y1 - box_h;
    let x0 = (w - tw) / 2.0 - pad_x;
    fill_rounded_rect(
        f,
        x0,
        y0,
        x0 + tw + pad_x * 2.0,
        y1,
        box_h / 2.0,
        [0, 0, 0],
        0.62,
    );
    draw_text(
        f,
        text,
        x0 + pad_x,
        y0 + pad_y + asc,
        s,
        t.style.color,
        1.0,
        false,
    );
}

/// A shape (or the whole frame in one colour) as a canvas-sized picture with smooth edges.
pub fn render_shape(sh: &Shape, width: u32, height: u32) -> Frame8 {
    let mut f = Frame8::solid(width, height, [0, 0, 0, 0]);
    let (w, h) = (width as f32, height as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (bw, bh) = if sh.kind == ShapeKind::Solid {
        (w, h)
    } else {
        (
            (sh.width as f32 / 100.0) * w,
            (sh.height as f32 / 100.0) * h,
        )
    };
    let (hx, hy) = (bw / 2.0, bh / 2.0);
    let radius = (sh.corner as f32 / 100.0) * bw.min(bh);
    let outline = (sh.outline as f32) * h / 1080.0;
    let mix = |a: [u8; 4], b: [u8; 4], t: f32| -> [f32; 4] {
        std::array::from_fn(|i| a[i] as f32 + (b[i] as f32 - a[i] as f32) * t)
    };
    for y in 0..height {
        for x in 0..width {
            let (px, py) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            // signed distance to the edge in pixels (negative inside)
            let d = match sh.kind {
                ShapeKind::Solid => -1.0,
                ShapeKind::Rectangle => {
                    let (qx, qy) = (px.abs() - (hx - radius), py.abs() - (hy - radius));
                    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
                }
                ShapeKind::Ellipse => {
                    let (a, b) = (hx.max(0.5), hy.max(0.5));
                    ((px / a).powi(2) + (py / b).powi(2))
                        .sqrt()
                        .mul_add(1.0, -1.0)
                        * a.min(b)
                }
            };
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let t = ((py + hy) / bh.max(1.0)).clamp(0.0, 1.0);
            let fill = match sh.gradient_to {
                Some(to) => mix(sh.color, to, t),
                None => sh.color.map(|v| v as f32),
            };
            // the outline covers the band just inside the edge
            let ring = if outline > 0.0 {
                (0.5 + d + outline).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let c: [f32; 4] =
                std::array::from_fn(|i| fill[i] + (sh.outline_color[i] as f32 - fill[i]) * ring);
            let a = c[3] / 255.0 * cover;
            let i = ((y * width + x) * 4) as usize;
            f.rgba[i] = c[0].round() as u8;
            f.rgba[i + 1] = c[1].round() as u8;
            f.rgba[i + 2] = c[2].round() as u8;
            f.rgba[i + 3] = (a * 255.0).round() as u8;
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use captaine_model::{Id, Title};
    use captaine_time::{Rational, RationalTime};

    fn title(template: TitleTemplate, text: &str, sub: &str) -> Title {
        let mut t = Title::new(
            Id::from_u128(1),
            template,
            RationalTime::new(72, Rational::FPS_24),
        );
        t.text = text.into();
        t.subtitle = sub.into();
        t
    }

    fn alpha(f: &Frame8, x: u32, y: u32) -> u8 {
        f.rgba[((y * f.width + x) * 4 + 3) as usize]
    }

    /// Bounding box of everything that is not fully transparent.
    fn bounds(f: &Frame8) -> Option<(u32, u32, u32, u32)> {
        let mut b: Option<(u32, u32, u32, u32)> = None;
        for y in 0..f.height {
            for x in 0..f.width {
                if alpha(f, x, y) > 0 {
                    b = Some(match b {
                        None => (x, y, x, y),
                        Some((a, c, d, e)) => (a.min(x), c.min(y), d.max(x), e.max(y)),
                    });
                }
            }
        }
        b
    }

    #[test]
    fn lower_third_sits_bottom_left_with_an_accent_bar() {
        let t = title(
            TitleTemplate::LowerThird,
            "Coastline, Day 3",
            "Lower third · 3 s",
        );
        let f = render_title(&t, 1920, 1080);
        let (x0, y0, x1, y1) = bounds(&f).unwrap();
        assert!(
            (x0 as f32 - 1920.0 * 0.06).abs() < 3.0,
            "left edge at 6%: {x0}"
        );
        assert!(
            (y1 as f32 - 1080.0 * 0.89).abs() < 3.0,
            "bottom at 89%: {y1}"
        );
        assert!(
            y0 > 700 && x1 < 1000,
            "a compact plate in the lower left: {:?}",
            (x0, y0, x1, y1)
        );
        // the bar on the left edge is the accent colour, solid
        let i = (((y0 + y1) / 2 * 1920 + x0 + 3) * 4) as usize;
        assert_eq!(&f.rgba[i..i + 4], &[0x5b, 0x4f, 0xd6, 255]);
        // the plate is translucent dark; the text is white
        assert!(
            f.rgba
                .chunks(4)
                .any(|p| p[3] == 255 && p[0] > 240 && p[1] > 240 && p[2] > 240),
            "white text pixels"
        );
        assert!(
            f.rgba
                .chunks(4)
                .any(|p| p[3] > 200 && p[3] < 230 && p[0] < 40),
            "plate pixels"
        );
    }

    #[test]
    fn a_title_scales_with_the_canvas_height() {
        let t = title(TitleTemplate::LowerThird, "Coastline", "Day 3");
        let big = bounds(&render_title(&t, 1920, 1080)).unwrap();
        let small = bounds(&render_title(&t, 960, 540)).unwrap();
        let ratio = |a: u32, b: u32| a as f32 / b as f32;
        assert!((ratio(big.2 - big.0, small.2 - small.0) - 2.0).abs() < 0.06);
        assert!((ratio(big.3 - big.1, small.3 - small.1) - 2.0).abs() < 0.08);
        // a vertical 1080x1920 canvas scales by height too
        let tall = bounds(&render_title(&t, 1080, 1920)).unwrap();
        assert!(
            tall.2 - tall.0 > big.2 - big.0,
            "taller canvas, bigger type"
        );
    }

    #[test]
    fn centered_title_is_centred_and_empty_titles_draw_nothing() {
        let t = title(TitleTemplate::Centered, "HELLO", "");
        let f = render_title(&t, 1920, 1080);
        let (x0, y0, x1, y1) = bounds(&f).unwrap();
        assert!(
            ((x0 + x1) as f32 / 2.0 - 960.0).abs() < 12.0,
            "horizontally centred"
        );
        assert!(
            ((y0 + y1) as f32 / 2.0 - 540.0).abs() < 40.0,
            "vertically near the middle"
        );
        let empty = title(TitleTemplate::Centered, "  ", "");
        assert!(bounds(&render_title(&empty, 640, 360)).is_none());
    }

    #[test]
    fn caption_is_a_pill_near_the_bottom_centre() {
        let t = title(TitleTemplate::Caption, "Hello there, this is a caption", "");
        let (x0, y0, x1, y1) = bounds(&render_title(&t, 1920, 1080)).unwrap();
        assert!(((x0 + x1) as f32 / 2.0 - 960.0).abs() < 6.0);
        assert!(y0 > 850 && y1 < 1010);
        assert!(x1 - x0 > 300);
    }

    #[test]
    fn style_changes_the_picture_and_rendering_is_repeatable() {
        let mut t = title(TitleTemplate::LowerThird, "Name", "Role");
        let a = render_title(&t, 1280, 720);
        assert_eq!(a, render_title(&t, 1280, 720));
        t.style.accent = [255, 0, 0];
        assert_ne!(a, render_title(&t, 1280, 720));
        t.style.size = 80.0;
        let big = bounds(&render_title(&t, 1280, 720)).unwrap();
        let small = bounds(&a).unwrap();
        assert!(big.2 - big.0 > small.2 - small.0);
    }
}
