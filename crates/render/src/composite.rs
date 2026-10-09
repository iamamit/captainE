use captaine_model::{BlendMode, ColorProps, VideoProps};

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

/// Linear-light grade of one un-premultiplied color.
fn grade(rgb: [f32; 3], c: &ColorProps) -> [f32; 3] {
    let gain = 2f32.powf(c.exposure as f32);
    // temperature: +-1500 K moves red and blue a quarter stop in opposite directions
    let warm =
        (((c.temperature - ColorProps::NEUTRAL_KELVIN) / 1500.0).clamp(-4.0, 4.0) * 0.25) as f32;
    let (wr, wb) = (2f32.powf(warm), 2f32.powf(-warm));
    let mut out = [rgb[0] * gain * wr, rgb[1] * gain, rgb[2] * gain * wb];
    const PIVOT: f32 = 0.18;
    if c.contrast != 1.0 {
        let k = c.contrast as f32;
        for v in &mut out {
            *v = if *v > 0.0 {
                PIVOT * (*v / PIVOT).powf(k)
            } else {
                0.0
            };
        }
    }
    if c.tint != 0.0 {
        // positive tint adds magenta: a little less green, a little more red and blue
        let t = (c.tint / 100.0) as f32 * 0.3;
        out[0] *= 2f32.powf(t * 0.5);
        out[1] *= 2f32.powf(-t);
        out[2] *= 2f32.powf(t * 0.5);
    }
    if c.highlights != 0.0 || c.shadows != 0.0 {
        let y = (0.2126 * out[0] + 0.7152 * out[1] + 0.0722 * out[2]).max(0.0);
        let smooth = |a: f32, b: f32, x: f32| {
            let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let w_shadow = 1.0 - smooth(0.0, 0.35, y);
        let w_light = smooth(0.25, 1.0, y);
        let stops =
            1.5 * ((c.shadows / 100.0) as f32 * w_shadow + (c.highlights / 100.0) as f32 * w_light);
        let g = 2f32.powf(stops);
        for v in &mut out {
            *v *= g;
        }
    }
    if c.saturation != 100.0 {
        let s = (c.saturation / 100.0) as f32;
        let y = 0.2126 * out[0] + 0.7152 * out[1] + 0.0722 * out[2];
        for v in &mut out {
            *v = (y + (*v - y) * s).max(0.0);
        }
    }
    if c.vibrance != 0.0 {
        let y = 0.2126 * out[0] + 0.7152 * out[1] + 0.0722 * out[2];
        let (mx, mn) = (
            out[0].max(out[1]).max(out[2]),
            out[0].min(out[1]).min(out[2]),
        );
        let sat = if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
        let s = 1.0 + (c.vibrance / 100.0) as f32 * (1.0 - sat);
        for v in &mut out {
            *v = (y + (*v - y) * s).max(0.0);
        }
    }
    out
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
    draw_layer_fx(canvas, frame, props, &LayerFx::default());
}

/// Extra, per-draw changes that a transition makes to a layer without touching its properties.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerFx {
    /// Multiplies the layer's opacity
    pub opacity: f64,
    /// Moves the layer, as fractions of the canvas width and height
    pub shift: (f64, f64),
    /// Only this part of the canvas is drawn: `[left, top, right, bottom]` as fractions
    pub mask: Option<[f64; 4]>,
    /// Multiplies the layer's size
    pub scale: f64,
    /// Only a circle of this radius, as a fraction of half the canvas diagonal, around the middle is drawn
    pub iris: Option<f64>,
}

impl Default for LayerFx {
    fn default() -> LayerFx {
        LayerFx {
            opacity: 1.0,
            shift: (0.0, 0.0),
            mask: None,
            scale: 1.0,
            iris: None,
        }
    }
}

pub fn draw_layer_fx(canvas: &mut Canvas, frame: &Frame8, props: &VideoProps, fx: &LayerFx) {
    let (cw, ch) = (canvas.width as f64, canvas.height as f64);
    let (fw, fh) = (frame.width as f64, frame.height as f64);
    if fw == 0.0 || fh == 0.0 || props.opacity * fx.opacity <= 0.0 {
        return;
    }
    let t = &props.transform;
    let fit = (cw / fw).min(ch / fh);
    let (sx, sy) = (fit * t.scale.0 * fx.scale, fit * t.scale.1 * fx.scale);
    let (sin, cos) = t.rotation_deg.to_radians().sin_cos();

    // bounding box of the drawn picture on the canvas
    let (hw, hh) = (fw * sx.abs() / 2.0, fh * sy.abs() / 2.0);
    let (cx, cy) = (
        cw / 2.0 + t.position.0 + fx.shift.0 * cw,
        ch / 2.0 + t.position.1 + fx.shift.1 * ch,
    );
    let ex = hw * cos.abs() + hh * sin.abs();
    let ey = hw * sin.abs() + hh * cos.abs();
    let x0 = ((cx - ex - 1.0).floor().max(0.0)) as u32;
    let y0 = ((cy - ey - 1.0).floor().max(0.0)) as u32;
    let x1 = ((cx + ex + 1.0).ceil().min(cw)).max(0.0) as u32;
    let y1 = ((cy + ey + 1.0).ceil().min(ch)).max(0.0) as u32;

    let opacity = (props.opacity * fx.opacity) as f32;
    let mask = fx
        .mask
        .map(|m| [m[0] * cw, m[1] * ch, m[2] * cw, m[3] * ch]);
    let iris = fx.iris.map(|f| f * (cw * cw + ch * ch).sqrt() / 2.0);
    let graded_neutral = props.color.is_neutral();
    for y in y0..y1 {
        for x in x0..x1 {
            if let Some(m) = mask {
                let (mx, my) = (x as f64 + 0.5, y as f64 + 0.5);
                if mx < m[0] || mx >= m[2] || my < m[1] || my >= m[3] {
                    continue;
                }
            }
            if let Some(r) = iris {
                let (ix, iy) = (x as f64 + 0.5 - cw / 2.0, y as f64 + 0.5 - ch / 2.0);
                if ix * ix + iy * iy > r * r {
                    continue;
                }
            }
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
            let mut s = sample(frame, px, py);
            if !graded_neutral && s[3] > 0.0 {
                let a = s[3];
                let g = grade([s[0] / a, s[1] / a, s[2] / a], &props.color);
                s = [g[0] * a, g[1] * a, g[2] * a, a];
            }
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
    fn neutral_grade_changes_nothing_and_each_control_does_its_job() {
        let px = |color: ColorProps| {
            let mut c = Canvas::black(1, 1);
            draw_layer(
                &mut c,
                &Frame8::solid(1, 1, [188, 100, 60, 255]),
                &VideoProps { color, ..props() },
            );
            c.pixel(0, 0)
        };
        let base = px(ColorProps::default());
        let near = |a: f32, b: f32| assert!((a - b).abs() < 1e-5, "{a} vs {b}");
        let lin = [to_linear(188), to_linear(100), to_linear(60)];
        for k in 0..3 {
            near(base[k], lin[k]);
        }
        let bright = px(ColorProps {
            exposure: 1.0,
            ..Default::default()
        });
        for k in 0..3 {
            near(bright[k], lin[k] * 2.0);
        }
        let grey = px(ColorProps {
            saturation: 0.0,
            ..Default::default()
        });
        near(grey[0], grey[1]);
        near(grey[1], grey[2]);
        near(grey[1], 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2]);
        let warm = px(ColorProps {
            temperature: 8000.0,
            ..Default::default()
        });
        assert!(warm[0] > base[0] && warm[2] < base[2]);
        near(warm[1], base[1]);
        let cool = px(ColorProps {
            temperature: 5000.0,
            ..Default::default()
        });
        assert!(cool[0] < base[0] && cool[2] > base[2]);
        // contrast leaves mid-grey alone and pushes the rest away from it
        let punchy = px(ColorProps {
            contrast: 1.5,
            ..Default::default()
        });
        assert!(punchy[0] > base[0] && punchy[2] < base[2]);
    }

    #[test]
    fn negative_scale_flips_the_picture() {
        let mut f = Frame8::solid(4, 2, [0, 0, 255, 255]);
        for y in 0..2 {
            for x in 0..2 {
                let i = (y * 4 + x) * 4;
                f.rgba[i..i + 3].copy_from_slice(&[255, 0, 0]); // left half red
            }
        }
        let mut c = Canvas::black(4, 2);
        draw_layer(&mut c, &f, &props());
        assert_eq!(px(&c, 0, 0), [255, 0, 0]);
        let mut flipped = props();
        flipped.transform.scale = (-1.0, 1.0);
        let mut c = Canvas::black(4, 2);
        draw_layer(&mut c, &f, &flipped);
        assert_eq!(px(&c, 0, 0), [0, 0, 255], "red is now on the right");
        assert_eq!(px(&c, 3, 0), [255, 0, 0]);
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

#[cfg(test)]
mod grade_tests {
    use captaine_model::{BlendMode, ColorProps, Transform, VideoProps};

    use super::*;

    /// One grey or tinted pixel, graded, as 8-bit RGB.
    fn px(rgb: [u8; 3], color: ColorProps) -> [u8; 3] {
        let f = Frame8::solid(2, 2, [rgb[0], rgb[1], rgb[2], 255]);
        let mut c = Canvas::black(2, 2);
        let props = VideoProps {
            transform: Transform::default(),
            color,
            opacity: 1.0,
            blend: BlendMode::Normal,
        };
        draw_layer(&mut c, &f, &props);
        let v = c.to_rgba8();
        [v[0], v[1], v[2]]
    }

    #[test]
    fn tint_moves_green_against_magenta() {
        let grey = [128, 128, 128];
        let magenta = px(
            grey,
            ColorProps {
                tint: 80.0,
                ..ColorProps::default()
            },
        );
        let green = px(
            grey,
            ColorProps {
                tint: -80.0,
                ..ColorProps::default()
            },
        );
        assert!(magenta[1] < grey[1] && magenta[0] > grey[0], "{magenta:?}");
        assert!(green[1] > grey[1] && green[0] < grey[0], "{green:?}");
    }

    #[test]
    fn shadows_and_highlights_work_on_their_own_ends_of_the_tone_range() {
        let (dark, bright) = ([30, 30, 30], [235, 235, 235]);
        let open = ColorProps {
            shadows: 80.0,
            ..ColorProps::default()
        };
        assert!(px(dark, open)[0] > dark[0] + 15, "shadows lifted");
        assert!(
            px(bright, open)[0].abs_diff(bright[0]) <= 2,
            "highlights untouched: {:?}",
            px(bright, open)
        );
        let recover = ColorProps {
            highlights: -80.0,
            ..ColorProps::default()
        };
        assert!(
            px(bright, recover)[0] < bright[0] - 15,
            "highlights pulled down"
        );
        assert!(
            px(dark, recover)[0].abs_diff(dark[0]) <= 2,
            "shadows untouched"
        );
    }

    #[test]
    fn vibrance_lifts_dull_colours_more_than_vivid_ones() {
        let (dull, vivid) = ([140, 120, 110], [250, 40, 30]);
        let v = ColorProps {
            vibrance: 100.0,
            ..ColorProps::default()
        };
        let gain = |c: [u8; 3]| {
            let g = px(c, v);
            let chroma =
                |p: [u8; 3]| p.iter().max().unwrap().abs_diff(*p.iter().min().unwrap()) as f32;
            chroma(g) / chroma(c)
        };
        assert!(
            gain(dull) > gain(vivid) + 0.15,
            "{} vs {}",
            gain(dull),
            gain(vivid)
        );
        assert_eq!(
            px([100, 100, 100], v),
            px([100, 100, 100], ColorProps::default()),
            "grey stays grey"
        );
    }
}
