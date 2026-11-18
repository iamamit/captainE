use captaine_model::{BlendMode, ColorProps, VideoProps};
use rayon::prelude::*;

use crate::color::to_linear;
use crate::{Canvas, Frame8};

/// Per channel: sRGB byte -> linear light, with the per-channel part of a grade already applied.
type Lut = [[f32; 256]; 3];

/// Premultiplied linear texel; coordinates clamp to the picture edge (coverage is applied separately).
#[inline]
fn texel(f: &Frame8, lut: &Lut, x: i64, y: i64) -> [f32; 4] {
    let x = x.clamp(0, f.width as i64 - 1) as u32;
    let y = y.clamp(0, f.height as i64 - 1) as u32;
    let i = ((y * f.width + x) * 4) as usize;
    let a = f.rgba[i + 3] as f32 / 255.0;
    [
        lut[0][f.rgba[i] as usize] * a,
        lut[1][f.rgba[i + 1] as usize] * a,
        lut[2][f.rgba[i + 2] as usize] * a,
        a,
    ]
}

/// Bilinear sample at continuous source coordinates (pixel centers at +0.5).
#[inline]
fn sample(f: &Frame8, lut: &Lut, sx: f64, sy: f64) -> [f32; 4] {
    let (x, y) = (sx - 0.5, sy - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
    let (x0, y0) = (x0 as i64, y0 as i64);
    if fx == 0.0 && fy == 0.0 {
        return texel(f, lut, x0, y0);
    }
    let (a, b, c, d) = (
        texel(f, lut, x0, y0),
        texel(f, lut, x0 + 1, y0),
        texel(f, lut, x0, y0 + 1),
        texel(f, lut, x0 + 1, y0 + 1),
    );
    let mut o = [0.0; 4];
    for k in 0..4 {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bot = c[k] + (d[k] - c[k]) * fx;
        o[k] = top + (bot - top) * fy;
    }
    o
}

/// A clip's colour settings turned into the numbers the per-pixel grade needs, worked out once per layer
/// instead of once per pixel (same arithmetic, same results).
struct Grade<'a> {
    c: &'a ColorProps,
    gain: f32,
    wr: f32,
    wb: f32,
    tint: [f32; 3],
}

impl<'a> Grade<'a> {
    fn new(c: &'a ColorProps) -> Grade<'a> {
        let gain = 2f32.powf(c.exposure as f32);
        // temperature: +-1500 K moves red and blue a quarter stop in opposite directions
        let warm = (((c.temperature - ColorProps::NEUTRAL_KELVIN) / 1500.0).clamp(-4.0, 4.0) * 0.25)
            as f32;
        // positive tint adds magenta: a little less green, a little more red and blue
        let t = (c.tint / 100.0) as f32 * 0.3;
        Grade {
            c,
            gain,
            wr: 2f32.powf(warm),
            wb: 2f32.powf(-warm),
            tint: [2f32.powf(t * 0.5), 2f32.powf(-t), 2f32.powf(t * 0.5)],
        }
    }

    /// The part of the grade that treats each channel on its own (exposure, temperature, contrast, tint).
    fn channel(&self, ch: usize, v: f32) -> f32 {
        let c = self.c;
        let w = [self.wr, 1.0, self.wb][ch];
        let mut v = if ch == 1 {
            v * self.gain
        } else {
            v * self.gain * w
        };
        const PIVOT: f32 = 0.18;
        if c.contrast != 1.0 {
            v = if v > 0.0 {
                PIVOT * (v / PIVOT).powf(c.contrast as f32)
            } else {
                0.0
            };
        }
        if c.tint != 0.0 {
            v *= self.tint[ch];
        }
        v
    }

    /// sRGB byte -> graded linear light, per channel: the per-channel part of the grade worked out for all
    /// 256 values once, so drawing a pixel is a table lookup instead of several powers.
    fn lut(&self) -> Lut {
        std::array::from_fn(|ch| std::array::from_fn(|i| self.channel(ch, to_linear(i as u8))))
    }

    /// Whether the grade has parts that mix the channels (and so must run per pixel).
    fn mixes(&self) -> bool {
        let c = self.c;
        c.highlights != 0.0 || c.shadows != 0.0 || c.saturation != 100.0 || c.vibrance != 0.0
    }

    /// The channel-mixing part of the grade (highlights, shadows, saturation, vibrance), after [`channel`](Self::channel).
    fn mix(&self, mut out: [f32; 3]) -> [f32; 3] {
        let c = self.c;
        if c.highlights != 0.0 || c.shadows != 0.0 {
            let y = (0.2126 * out[0] + 0.7152 * out[1] + 0.0722 * out[2]).max(0.0);
            let smooth = |a: f32, b: f32, x: f32| {
                let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            let w_shadow = 1.0 - smooth(0.0, 0.35, y);
            let w_light = smooth(0.25, 1.0, y);
            let stops = 1.5
                * ((c.shadows / 100.0) as f32 * w_shadow + (c.highlights / 100.0) as f32 * w_light);
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
    /// Smear the layer sideways, as when the camera whips: the length of the streak as a fraction of its width
    pub motion_blur: f64,
    /// Repeat the picture sideways so a layer moved off-centre still fills the canvas (no empty edge)
    pub wrap_x: bool,
    /// Extra turn, in degrees clockwise, on top of the layer's own rotation
    pub spin: f64,
    /// Only the part a clock hand has swept, starting at twelve o'clock, is drawn: the fraction of the full turn
    pub clock: Option<f64>,
    /// The layer stands still (no moving position, scale or rotation): when it is drawn at about 1:1, put its
    /// pixels exactly on the canvas grid, so an off-centre placement does not average neighbouring pixels
    pub snap: bool,
}

impl Default for LayerFx {
    fn default() -> LayerFx {
        LayerFx {
            opacity: 1.0,
            shift: (0.0, 0.0),
            mask: None,
            scale: 1.0,
            iris: None,
            motion_blur: 0.0,
            wrap_x: false,
            spin: 0.0,
            clock: None,
            snap: false,
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
    let (mut sx, mut sy) = (fit * t.scale.0 * fx.scale, fit * t.scale.1 * fx.scale);
    let turn = t.rotation_deg + fx.spin;
    let (sin, cos) = turn.to_radians().sin_cos();

    let (mut cx, mut cy) = (
        cw / 2.0 + t.position.0 + fx.shift.0 * cw,
        ch / 2.0 + t.position.1 + fx.shift.1 * ch,
    );
    // a still picture decoded at the size it is shown: draw it 1:1 with whole-pixel placement. A fraction of a
    // pixel of offset (or a 0.1% size difference from even decode sizes) would otherwise blur the whole frame
    if fx.snap
        && turn.rem_euclid(360.0) == 0.0
        && (sx - 1.0).abs() < 0.003
        && (sy - 1.0).abs() < 0.003
    {
        (sx, sy) = (1.0, 1.0);
        cx = (cx - fw / 2.0).round() + fw / 2.0;
        cy = (cy - fh / 2.0).round() + fh / 2.0;
    }

    // bounding box of the drawn picture on the canvas
    let (hw, hh) = (fw * sx.abs() / 2.0, fh * sy.abs() / 2.0);
    let ex = hw * cos.abs() + hh * sin.abs();
    let ey = hw * sin.abs() + hh * cos.abs();
    let (x0, x1) = if fx.wrap_x {
        (0, canvas.width)
    } else {
        (
            ((cx - ex - 1.0).floor().max(0.0)) as u32,
            ((cx + ex + 1.0).ceil().min(cw)).max(0.0) as u32,
        )
    };
    let y0 = ((cy - ey - 1.0).floor().max(0.0)) as u32;
    let y1 = ((cy + ey + 1.0).ceil().min(ch)).max(0.0) as u32;

    let opacity = (props.opacity * fx.opacity) as f32;
    let mask = fx
        .mask
        .map(|m| [m[0] * cw, m[1] * ch, m[2] * cw, m[3] * ch]);
    let iris = fx.iris.map(|f| f * (cw * cw + ch * ch).sqrt() / 2.0);
    let graded_neutral = props.color.is_neutral();
    let grader = Grade::new(&props.color);
    // sRGB -> linear with the per-channel grade folded in; channel mixing (saturation...) runs per pixel
    let lut: Lut = if graded_neutral {
        let plain: [f32; 256] = std::array::from_fn(|i| to_linear(i as u8));
        [plain; 3]
    } else {
        grader.lut()
    };
    let mixes = !graded_neutral && grader.mixes();
    let normal = props.blend == BlendMode::Normal;
    let (inv_sx, inv_sy) = (1.0 / sx, 1.0 / sy);
    let width = canvas.width as usize;
    // each row of the canvas is independent: spread them over every core
    canvas
        .data
        .par_chunks_mut(width * 4)
        .enumerate()
        .filter(|(y, _)| (*y as u32) >= y0 && (*y as u32) < y1)
        .for_each(|(y, row)| {
            let y = y as u32;
            // the parts of the inverse transform that only depend on the row
            let dy = y as f64 + 0.5 - cy;
            let (row_x, row_y) = (dy * sin, dy * cos);
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
                if let Some(c) = fx.clock {
                    let (ix, iy) = (x as f64 + 0.5 - cw / 2.0, y as f64 + 0.5 - ch / 2.0);
                    // angle clockwise from straight up, as a fraction of a turn
                    let a = (ix.atan2(-iy) / std::f64::consts::TAU).rem_euclid(1.0);
                    if a >= c {
                        continue;
                    }
                }
                let dx = x as f64 + 0.5 - cx;
                // inverse of (scale, then clockwise rotation): rotate back, then unscale
                let (ux, uy) = (dx * cos + row_x, -dx * sin + row_y);
                let (mut px, py) = (ux * inv_sx + fw / 2.0, uy * inv_sy + fh / 2.0);
                // how much of this canvas pixel the picture covers (distance to the nearest edge, in canvas pixels)
                let ex = if fx.wrap_x {
                    px = px.rem_euclid(fw);
                    1.0
                } else {
                    (px.min(fw - px) * sx.abs() + 0.5).clamp(0.0, 1.0)
                };
                let ey = (py.min(fh - py) * sy.abs() + 0.5).clamp(0.0, 1.0);
                let k = (ex * ey) as f32 * opacity;
                if k <= 0.0 {
                    continue;
                }
                let mut s = sample(frame, &lut, px, py);
                if mixes && s[3] > 0.0 {
                    let a = s[3];
                    if a >= 1.0 {
                        // opaque (camera footage): nothing to un-premultiply
                        let g = grader.mix([s[0], s[1], s[2]]);
                        s = [g[0], g[1], g[2], a];
                    } else {
                        let g = grader.mix([s[0] / a, s[1] / a, s[2] / a]);
                        s = [g[0] * a, g[1] * a, g[2] * a, a];
                    }
                }
                let s = [s[0] * k, s[1] * k, s[2] * k, s[3] * k];
                let i = x as usize * 4;
                let d = &mut row[i..i + 4];
                if normal {
                    // the usual case, without going through the general blend
                    let k = 1.0 - s[3];
                    d[0] = s[0] + d[0] * k;
                    d[1] = s[1] + d[1] * k;
                    d[2] = s[2] + d[2] * k;
                    d[3] = s[3] + d[3] - s[3] * d[3];
                } else {
                    let o = blend(props.blend, s, [d[0], d[1], d[2], d[3]]);
                    d.copy_from_slice(&o);
                }
            }
        });
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
    fn wrapped_layer_fills_the_canvas_when_moved() {
        // a half-width picture moved right: without wrap the left of the canvas stays black, with wrap it repeats
        let f = Frame8::solid(4, 4, [255, 255, 255, 255]);
        let p = VideoProps::default();
        let mut moved = LayerFx {
            shift: (0.25, 0.0),
            ..LayerFx::default()
        };
        let mut plain = Canvas::black(8, 4);
        draw_layer_fx(&mut plain, &f, &p, &moved);
        assert_eq!(px(&plain, 0, 2), [0, 0, 0]);
        moved.wrap_x = true;
        let mut wrapped = Canvas::black(8, 4);
        draw_layer_fx(&mut wrapped, &f, &p, &moved);
        assert_eq!(px(&wrapped, 0, 2), [255, 255, 255]);
        assert_eq!(px(&wrapped, 7, 2), [255, 255, 255]);
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
    fn still_layer_moved_by_part_of_a_pixel_stays_sharp() {
        // a 1-pixel-wide white line on an 8x4 picture, drawn 1:1 but 2.5 px to the right
        let mut f = Frame8::solid(8, 4, [0, 0, 0, 255]);
        for y in 0..4 {
            let i = (y * 8 + 3) * 4;
            f.rgba[i..i + 3].copy_from_slice(&[255, 255, 255]);
        }
        let mut p = props();
        p.transform.position = (2.5, 0.0);
        let fx = LayerFx {
            snap: true,
            ..LayerFx::default()
        };
        let mut c = Canvas::black(8, 4);
        draw_layer_fx(&mut c, &f, &p, &fx);
        // whole-pixel placement: the line is still one pure white pixel wide, not two grey ones
        let row: Vec<[u8; 3]> = (0..8).map(|x| px(&c, x, 1)).collect();
        assert_eq!(row.iter().filter(|v| **v == [255, 255, 255]).count(), 1);
        assert!(row.iter().all(|v| *v == [255, 255, 255] || *v == [0, 0, 0]));
        // without snapping (a moving layer) it is placed exactly, and so spreads over two pixels
        let mut c = Canvas::black(8, 4);
        draw_layer_fx(&mut c, &f, &p, &LayerFx::default());
        assert_eq!(px(&c, 5, 1), px(&c, 6, 1));
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

/// Average every pixel with its neighbours along the row over `len` pixels: a horizontal motion streak.
/// Straight-alpha RGBA in, same size out. `len` < 2 returns a copy. With `wrap` the row is treated as a loop
/// (for a picture repeated sideways, so the seam where it repeats is smeared like the rest).
pub fn horizontal_blur(frame: &Frame8, len: usize, wrap: bool) -> Frame8 {
    let (w, h) = (frame.width as usize, frame.height as usize);
    if len < 2 || w == 0 || h == 0 {
        return frame.clone();
    }
    let len = len.min(w);
    let (before, after) = (len / 2, len - len / 2);
    let mut out = vec![0u8; w * h * 4];
    out.par_chunks_mut(w * 4)
        .zip(frame.rgba.par_chunks(w * 4))
        .for_each(|(dst, src)| {
            // running sums over a window that is clamped at the picture's edges
            let mut sum = [0u32; 4];
            let at = |x: isize| {
                if wrap {
                    x.rem_euclid(w as isize) as usize * 4
                } else {
                    x.clamp(0, w as isize - 1) as usize * 4
                }
            };
            for x in -(before as isize)..(after as isize) {
                let i = at(x);
                for c in 0..4 {
                    sum[c] += src[i + c] as u32;
                }
            }
            for x in 0..w {
                for c in 0..4 {
                    dst[x * 4 + c] = ((sum[c] + len as u32 / 2) / len as u32) as u8;
                }
                let (add, sub) = (
                    at(x as isize + after as isize),
                    at(x as isize - before as isize),
                );
                for c in 0..4 {
                    sum[c] = sum[c] + src[add + c] as u32 - src[sub + c] as u32;
                }
            }
        });
    Frame8 {
        width: frame.width,
        height: frame.height,
        rgba: out,
    }
}

#[cfg(test)]
mod blur_tests {
    use super::*;

    #[test]
    fn horizontal_blur_spreads_along_rows_only() {
        // one white column in the middle of a black 9x3 picture
        let (w, h) = (9u32, 3u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            let i = (y * w as usize + 4) * 4;
            rgba[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
        for px in rgba.chunks_mut(4) {
            px[3] = 255;
        }
        let f = Frame8 {
            width: w,
            height: h,
            rgba,
        };
        let b = horizontal_blur(&f, 3, false);
        let px = |x: usize, y: usize| b.rgba[(y * w as usize + x) * 4];
        assert_eq!(px(4, 1), 85);
        assert_eq!(px(3, 1), 85);
        assert_eq!(px(5, 1), 85);
        assert_eq!(px(1, 1), 0);
        assert_eq!(px(4, 0), px(4, 2), "rows are treated alike");
        assert_eq!(b.rgba[3], 255, "alpha of an opaque picture stays opaque");
        assert_eq!(horizontal_blur(&f, 1, false).rgba, f.rgba);
        // looped: the white column at the left edge bleeds into the right edge
        let mut edge = f.clone();
        for y in 0..h as usize {
            let i = (y * w as usize + 4) * 4;
            edge.rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
            edge.rgba[y * w as usize * 4..y * w as usize * 4 + 3].copy_from_slice(&[255, 255, 255]);
        }
        let looped = horizontal_blur(&edge, 3, true);
        assert_eq!(looped.rgba[(w as usize - 1) * 4], 85);
        assert_eq!(
            horizontal_blur(&edge, 3, false).rgba[(w as usize - 1) * 4],
            0
        );
    }
}
