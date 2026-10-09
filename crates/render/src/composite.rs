use captaine_model::{BlendMode, VideoProps};

use crate::color::to_linear;
use crate::{Canvas, Frame8};

/// Premultiplied linear texel; coordinates clamp to the picture edge (coverage is applied separately).
#[inline]
fn texel(f: &Frame8, x: i64, y: i64) -> [f32; 4] {
    let x = x.clamp(0, f.width as i64 - 1) as u32;
    let y = y.clamp(0, f.height as i64 - 1) as u32;
    let i = ((y * f.width + x) * 4) as usize;
    let a = f.rgba[i + 3] as f32 / 255.0;
    [
        to_linear(f.rgba[i]) * a,
        to_linear(f.rgba[i + 1]) * a,
        to_linear(f.rgba[i + 2]) * a,
        a,
    ]
}

/// Bilinear sample at continuous source coordinates (pixel centers at +0.5).
#[inline]
fn sample(f: &Frame8, sx: f64, sy: f64) -> [f32; 4] {
    let (x, y) = (sx - 0.5, sy - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
    let (x0, y0) = (x0 as i64, y0 as i64);
    if fx == 0.0 && fy == 0.0 {
        return texel(f, x0, y0);
    }
    let (a, b, c, d) = (
        texel(f, x0, y0),
        texel(f, x0 + 1, y0),
        texel(f, x0, y0 + 1),
        texel(f, x0 + 1, y0 + 1),
    );
    let mut o = [0.0; 4];
    for k in 0..4 {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bot = c[k] + (d[k] - c[k]) * fx;
        o[k] = top + (bot - top) * fy;
    }
    o
}

#[inline]
fn blend(mode: BlendMode, s: [f32; 4], d: [f32; 4]) -> [f32; 4] {
    let (sa, da) = (s[3], d[3]);
    let a = sa + da - sa * da;
    let mut o = [0.0, 0.0, 0.0, a];
    for k in 0..3 {
        o[k] = match mode {
            BlendMode::Normal => s[k] + d[k] * (1.0 - sa),
            BlendMode::Add => s[k] + d[k],
            BlendMode::Multiply => s[k] * d[k] + s[k] * (1.0 - da) + d[k] * (1.0 - sa),
            BlendMode::Screen => s[k] + d[k] - s[k] * d[k],
        };
    }
    o
}

/// Draw `frame` onto `canvas`.
///
/// The picture is fitted inside the canvas (letterboxed), scaled by `transform.scale`, rotated clockwise
/// by `rotation_deg` about its center, moved by `position`, then blended with `opacity`.
pub fn draw_layer(canvas: &mut Canvas, frame: &Frame8, props: &VideoProps) {
    let (cw, ch) = (canvas.width as f64, canvas.height as f64);
    let (fw, fh) = (frame.width as f64, frame.height as f64);
    if fw == 0.0 || fh == 0.0 || props.opacity <= 0.0 {
        return;
    }
    let t = &props.transform;
    let fit = (cw / fw).min(ch / fh);
    let (sx, sy) = (fit * t.scale.0, fit * t.scale.1);
    let (sin, cos) = t.rotation_deg.to_radians().sin_cos();

    // bounding box of the drawn picture on the canvas
    let (hw, hh) = (fw * sx.abs() / 2.0, fh * sy.abs() / 2.0);
    let (cx, cy) = (cw / 2.0 + t.position.0, ch / 2.0 + t.position.1);
    let ex = hw * cos.abs() + hh * sin.abs();
    let ey = hw * sin.abs() + hh * cos.abs();
    let x0 = ((cx - ex - 1.0).floor().max(0.0)) as u32;
    let y0 = ((cy - ey - 1.0).floor().max(0.0)) as u32;
    let x1 = ((cx + ex + 1.0).ceil().min(cw)).max(0.0) as u32;
    let y1 = ((cy + ey + 1.0).ceil().min(ch)).max(0.0) as u32;

    let opacity = props.opacity as f32;
    for y in y0..y1 {
        for x in x0..x1 {
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            // inverse of (scale, then clockwise rotation): rotate back, then unscale
            let (ux, uy) = (dx * cos + dy * sin, -dx * sin + dy * cos);
            let (px, py) = (ux / sx + fw / 2.0, uy / sy + fh / 2.0);
            // how much of this canvas pixel the picture covers (distance to the nearest edge, in canvas pixels)
            let ex = (px.min(fw - px) * sx.abs() + 0.5).clamp(0.0, 1.0);
            let ey = (py.min(fh - py) * sy.abs() + 0.5).clamp(0.0, 1.0);
            let k = (ex * ey) as f32 * opacity;
            if k <= 0.0 {
                continue;
            }
            let s = sample(frame, px, py);
            let s = [s[0] * k, s[1] * k, s[2] * k, s[3] * k];
            let i = ((y * canvas.width + x) * 4) as usize;
            let d = [
                canvas.data[i],
                canvas.data[i + 1],
                canvas.data[i + 2],
                canvas.data[i + 3],
            ];
            canvas.data[i..i + 4].copy_from_slice(&blend(props.blend, s, d));
        }
    }
}

#[cfg(test)]
mod tests {
    use captaine_model::Transform;

    use super::*;
    use crate::color::to_linear;

    fn props() -> VideoProps {
        VideoProps::default()
    }
    fn px(c: &Canvas, x: u32, y: u32) -> [u8; 3] {
        let p = c.pixel(x, y);
        [
            crate::color::to_srgb8(p[0]),
            crate::color::to_srgb8(p[1]),
            crate::color::to_srgb8(p[2]),
        ]
    }

    #[test]
    fn same_size_layer_is_an_exact_copy() {
        let mut f = Frame8::solid(4, 4, [0, 0, 0, 255]);
        for (i, v) in f.rgba.iter_mut().enumerate() {
            if i % 4 != 3 {
                *v = (i * 7 % 256) as u8;
            }
        }
        let mut c = Canvas::black(4, 4);
        draw_layer(&mut c, &f, &props());
        assert_eq!(c.to_rgba8(), f.rgba);
    }

    #[test]
    fn letterboxes_to_fit() {
        // 2x1 picture on a 4x4 canvas becomes 4x2, centered
        let mut c = Canvas::black(4, 4);
        draw_layer(&mut c, &Frame8::solid(2, 1, [255, 255, 255, 255]), &props());
        assert_eq!(px(&c, 0, 0), [0, 0, 0]);
        assert_eq!(px(&c, 0, 1), [255, 255, 255]);
        assert_eq!(px(&c, 3, 2), [255, 255, 255]);
        assert_eq!(px(&c, 3, 3), [0, 0, 0]);
    }

    #[test]
    fn opacity_blends_in_linear_light() {
        let mut c = Canvas::black(2, 2);
        draw_layer(
            &mut c,
            &Frame8::solid(2, 2, [255, 255, 255, 255]),
            &VideoProps {
                opacity: 0.5,
                ..props()
            },
        );
        // half white over black is 0.5 linear = 188 in sRGB (not 128)
        assert_eq!(px(&c, 0, 0), [188, 188, 188]);
        let mut c = Canvas::black(2, 2);
        draw_layer(
            &mut c,
            &Frame8::solid(2, 2, [255, 255, 255, 255]),
            &VideoProps {
                opacity: 0.0,
                ..props()
            },
        );
        assert_eq!(px(&c, 0, 0), [0, 0, 0]);
    }

    #[test]
    fn picture_alpha_is_respected() {
        let mut c = Canvas::black(2, 2);
        draw_layer(&mut c, &Frame8::solid(2, 2, [255, 255, 255, 0]), &props());
        assert_eq!(px(&c, 1, 1), [0, 0, 0]);
    }

    #[test]
    fn position_and_scale() {
        let white = Frame8::solid(2, 2, [255, 255, 255, 255]);
        let mut c = Canvas::black(8, 8);
        let t = Transform {
            position: (2.0, 0.0),
            scale: (0.5, 0.5),
            rotation_deg: 0.0,
        };
        draw_layer(
            &mut c,
            &white,
            &VideoProps {
                transform: t,
                ..props()
            },
        );
        // 8x8 fitted picture halved = 4x4 square, centered at (6, 4): covers x 4..8, y 2..6
        assert_eq!(px(&c, 4, 2), [255, 255, 255]);
        assert_eq!(px(&c, 7, 5), [255, 255, 255]);
        assert_eq!(px(&c, 3, 2), [0, 0, 0]);
        assert_eq!(px(&c, 4, 6), [0, 0, 0]);
    }

    #[test]
    fn rotation_is_clockwise() {
        // left half red, right half blue
        let mut f = Frame8::solid(2, 1, [255, 0, 0, 255]);
        f.rgba[4..8].copy_from_slice(&[0, 0, 255, 255]);
        let mut c = Canvas::black(4, 4);
        let t = Transform {
            rotation_deg: 90.0,
            ..Transform::default()
        };
        draw_layer(
            &mut c,
            &f,
            &VideoProps {
                transform: t,
                ..props()
            },
        );
        // clockwise 90: the left (red) end moves to the top
        assert_eq!(px(&c, 1, 0), [255, 0, 0]);
        assert_eq!(px(&c, 2, 3), [0, 0, 255]);
        assert_eq!(px(&c, 0, 0), [0, 0, 0]);
        assert_eq!(px(&c, 3, 3), [0, 0, 0]);
    }

    #[test]
    fn blend_modes() {
        let grey = Frame8::solid(1, 1, [188, 188, 188, 255]);
        let g = to_linear(188);
        let mut base = Canvas::black(1, 1);
        base.data[..3].copy_from_slice(&[g, g, g]);
        let run = |mode| {
            let mut c = base.clone();
            draw_layer(
                &mut c,
                &grey,
                &VideoProps {
                    blend: mode,
                    ..props()
                },
            );
            c.pixel(0, 0)[0]
        };
        let near = |a: f32, b: f32| assert!((a - b).abs() < 1e-5, "{a} vs {b}");
        near(run(BlendMode::Normal), g);
        near(run(BlendMode::Add), 2.0 * g);
        near(run(BlendMode::Multiply), g * g);
        near(run(BlendMode::Screen), 2.0 * g - g * g);
    }

    #[test]
    fn offscreen_layer_is_harmless() {
        let mut c = Canvas::black(4, 4);
        let t = Transform {
            position: (1000.0, -1000.0),
            ..Transform::default()
        };
        draw_layer(
            &mut c,
            &Frame8::solid(2, 2, [255; 4]),
            &VideoProps {
                transform: t,
                ..props()
            },
        );
        assert_eq!(c, Canvas::black(4, 4));
    }
}
