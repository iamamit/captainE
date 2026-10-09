//! Graphical controls for colour effects: three colour wheels and a curve editor.

use captaine_model::{Effect, EffectKind};
use egui::{epaint::Vertex, pos2, vec2, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui};

use crate::theme;

/// Offsets of the three channels (zero-sum, each -1..1) for a puck at `(x, y)` in a unit disc.
pub fn wheel_to_rgb(x: f32, y: f32) -> [f64; 3] {
    // red at the right, green at the lower left, blue at the upper left (screen y grows downwards)
    let r = x;
    let g = -0.5 * x + 0.866 * y;
    let b = -0.5 * x - 0.866 * y;
    [r as f64, g as f64, b as f64]
}

/// Where the puck sits for channel offsets `rgb` (the inverse of [`wheel_to_rgb`] for zero-sum offsets).
pub fn rgb_to_wheel(rgb: [f64; 3]) -> (f32, f32) {
    let x = (2.0 * rgb[0] - rgb[1] - rgb[2]) / 3.0;
    let y = (rgb[1] - rgb[2]) / 1.732;
    (x as f32, y as f32)
}

/// What the wheel looks like at a point: the tint a puck there would give, around a mid grey.
fn wheel_colour(x: f32, y: f32) -> Color32 {
    let v = wheel_to_rgb(x, y);
    let c = |k: usize| ((0.5 + 0.5 * v[k]).clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgb(c(0), c(1), c(2))
}

fn wheel_mesh(center: Pos2, radius: f32) -> Mesh {
    let mut mesh = Mesh::default();
    let (rings, segs) = (8usize, 72usize);
    mesh.vertices.push(Vertex {
        pos: center,
        uv: egui::epaint::WHITE_UV,
        color: wheel_colour(0.0, 0.0),
    });
    for ring in 1..=rings {
        let s = ring as f32 / rings as f32;
        for seg in 0..segs {
            let a = seg as f32 / segs as f32;
            // hue 0 (red) points right; the wheel runs red, yellow, green, cyan, blue, magenta
            let ang = a * std::f32::consts::TAU;
            mesh.vertices.push(Vertex {
                pos: center + vec2(ang.cos(), ang.sin()) * radius * s,
                uv: egui::epaint::WHITE_UV,
                color: wheel_colour(ang.cos() * s, ang.sin() * s),
            });
        }
    }
    let idx = |ring: usize, seg: usize| -> u32 {
        if ring == 0 {
            0
        } else {
            (1 + (ring - 1) * segs + seg % segs) as u32
        }
    };
    for seg in 0..segs {
        mesh.indices.extend([0, idx(1, seg), idx(1, seg + 1)]);
        for ring in 1..rings {
            let (a, b, c, d) = (
                idx(ring, seg),
                idx(ring, seg + 1),
                idx(ring + 1, seg),
                idx(ring + 1, seg + 1),
            );
            mesh.indices.extend([a, c, b, b, c, d]);
        }
    }
    mesh
}

/// One colour wheel: drag the puck toward a colour to push the tone that way; double-click to reset.
/// `rgb` holds the three channel offsets. Returns `(changed, done)`.
pub fn color_wheel(ui: &mut Ui, label: &str, rgb: &mut [f64; 3]) -> (bool, bool) {
    let size = 78.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, label));
    let center = rect.center();
    let radius = size / 2.0 - 3.0;
    ui.painter().add(Shape::mesh(wheel_mesh(center, radius)));
    ui.painter()
        .circle_stroke(center, radius, Stroke::new(1.0, theme::BORDER));
    let (mut changed, mut done) = (false, false);
    if resp.double_clicked() {
        *rgb = [0.0; 3];
        changed = true;
        done = true;
    } else if resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let mut d = (p - center) / radius;
            if d.length() > 1.0 {
                d = d / d.length();
            }
            // screen y points down; the wheel's hues run counter-clockwise from red on the right
            *rgb = wheel_to_rgb(d.x, d.y);
            changed = true;
        }
    }
    if resp.drag_stopped() || resp.clicked() {
        done = true;
    }
    // the puck
    let (px, py) = rgb_to_wheel(*rgb);
    let puck = center + vec2(px, py) * radius;
    ui.painter().circle_filled(puck, 4.5, Color32::WHITE);
    ui.painter()
        .circle_stroke(puck, 4.5, Stroke::new(1.0, Color32::BLACK));
    (changed, done)
}

/// The three wheels of a Color wheels effect (lift, gamma, gain), each over its master slider's value.
/// Returns `(changed, done)`.
pub fn wheels_row(ui: &mut Ui, e: &mut Effect) -> (bool, bool) {
    debug_assert_eq!(e.kind, EffectKind::ColorWheels);
    let (mut changed, mut done) = (false, false);
    ui.horizontal(|ui| {
        for (i, name) in ["Lift", "Gamma", "Gain"].into_iter().enumerate() {
            ui.vertical(|ui| {
                let mut rgb = [
                    e.params[i * 4 + 1],
                    e.params[i * 4 + 2],
                    e.params[i * 4 + 3],
                ];
                let (c, d) = color_wheel(ui, &format!("{name} wheel"), &mut rgb);
                if c {
                    // the wheel's reach is half a stop-ish: keep the numbers in the effect's -1..1 range
                    for (k, v) in rgb.iter().enumerate() {
                        e.params[i * 4 + 1 + k] = (v * 0.5).clamp(-1.0, 1.0);
                    }
                }
                ui.label(egui::RichText::new(name).size(11.0).color(theme::MUTED));
                changed |= c;
                done |= d;
            });
        }
    });
    (changed, done)
}

/// A curve editor for one of the four curves of a Curves effect: five points to drag up and down.
/// `which` is 0 for the master curve and 1..=3 for red, green, blue. Returns `(changed, done)`.
pub fn curve_editor(ui: &mut Ui, e: &mut Effect, which: usize) -> (bool, bool) {
    debug_assert_eq!(e.kind, EffectKind::Curves);
    let size = vec2(ui.available_width().min(220.0), 130.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, theme::FIELD);
    for q in 1..4 {
        let t = q as f32 / 4.0;
        p.line_segment(
            [
                pos2(rect.left() + rect.width() * t, rect.top()),
                pos2(rect.left() + rect.width() * t, rect.bottom()),
            ],
            Stroke::new(1.0, theme::BORDER),
        );
        p.line_segment(
            [
                pos2(rect.left(), rect.top() + rect.height() * t),
                pos2(rect.right(), rect.top() + rect.height() * t),
            ],
            Stroke::new(1.0, theme::BORDER),
        );
    }
    p.line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(1.0, theme::FAINT),
    );
    let col = [
        Color32::from_gray(230),
        Color32::from_rgb(0xe5, 0x48, 0x4d),
        Color32::from_rgb(0x3d, 0xc2, 0x7a),
        Color32::from_rgb(0x4f, 0x8c, 0xf5),
    ][which];
    let base = which * 5;
    let at = |i: usize, v: f64| {
        pos2(
            rect.left() + rect.width() * i as f32 / 4.0,
            rect.bottom() - rect.height() * v as f32,
        )
    };
    // the smooth curve through the points
    let ys: Vec<f64> = e.params[base..base + 5].to_vec();
    let pts: Vec<Pos2> = (0..=48)
        .map(|k| {
            let x = k as f64 / 48.0;
            let seg = ((x * 4.0) as usize).min(3);
            let t = x * 4.0 - seg as f64;
            // cosine blend between neighbouring points: a good-enough preview of the real monotone spline
            let w = (1.0 - (t * std::f64::consts::PI).cos()) / 2.0;
            let y = ys[seg] * (1.0 - w) + ys[seg + 1] * w;
            pos2(
                rect.left() + rect.width() * x as f32,
                rect.bottom() - rect.height() * y as f32,
            )
        })
        .collect();
    ui.painter().add(Shape::line(pts, Stroke::new(1.6, col)));
    let (mut changed, mut done) = (false, false);
    for i in 0..5 {
        let pos = at(i, e.params[base + i]);
        let hit = Rect::from_center_size(pos, vec2(16.0, 16.0));
        let resp = ui.interact(
            hit,
            ui.id().with(("curve-point", which, i)),
            Sense::click_and_drag(),
        );
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Slider,
                true,
                format!(
                    "Curve point {} {}",
                    ["Master", "Red", "Green", "Blue"][which],
                    i + 1
                ),
            )
        });
        if resp.dragged() {
            if let Some(m) = resp.interact_pointer_pos() {
                let v = ((rect.bottom() - m.y) / rect.height()).clamp(0.0, 1.0) as f64;
                e.params[base + i] = v;
                changed = true;
            }
        }
        if resp.double_clicked() {
            e.params[base + i] = i as f64 / 4.0;
            changed = true;
            done = true;
        }
        if resp.drag_stopped() {
            done = true;
        }
        ui.painter().circle_filled(pos, 4.0, col);
        ui.painter()
            .circle_stroke(pos, 4.0, Stroke::new(1.0, Color32::BLACK));
    }
    (changed, done)
}
