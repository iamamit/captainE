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

/// How far the wheel reaches: the rim gives offsets of this size (half of the effect's -1..1 range), a gentle
/// push rather than a full colour swing.
const WHEEL_REACH: f64 = 0.5;

/// Stored channel offsets -> wheel offsets (the puck's scale).
pub fn wheel_from_params(p: [f64; 3]) -> [f64; 3] {
    p.map(|v| v / WHEEL_REACH)
}

/// Wheel offsets -> stored channel offsets, inside the effect's range.
pub fn params_from_wheel(rgb: [f64; 3]) -> [f64; 3] {
    rgb.map(|v| (v * WHEEL_REACH).clamp(-1.0, 1.0))
}

/// The three wheels of a Color wheels effect (lift, gamma, gain), each over its master slider's value.
/// Returns `(changed, done)`.
pub fn wheels_row(ui: &mut Ui, e: &mut Effect) -> (bool, bool) {
    debug_assert_eq!(e.kind, EffectKind::ColorWheels);
    let (mut changed, mut done) = (false, false);
    ui.horizontal(|ui| {
        for (i, name) in ["Lift", "Gamma", "Gain"].into_iter().enumerate() {
            ui.vertical(|ui| {
                // the puck shows the stored offsets at the wheel's scale, so it stays where it was dropped
                let mut rgb = wheel_from_params([
                    e.params[i * 4 + 1],
                    e.params[i * 4 + 2],
                    e.params[i * 4 + 3],
                ]);
                let (c, d) = color_wheel(ui, &format!("{name} wheel"), &mut rgb);
                if c {
                    for (k, v) in params_from_wheel(rgb).into_iter().enumerate() {
                        e.params[i * 4 + 1 + k] = v;
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
    // exactly the curve the effect applies (the renderer's own table), not an approximation
    let table = captaine_render::curve_table(&ys);
    let pts: Vec<Pos2> = (0..=64)
        .map(|k| {
            let x = k as f64 / 64.0;
            let y = table[((x * 255.0).round() as usize).min(255)];
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

/// The frequency response of an EQ effect: log frequency across, ±18 dB up and down.
pub fn eq_graph(ui: &mut Ui, e: &Effect, sample_rate: u32) {
    debug_assert_eq!(e.kind, EffectKind::Eq);
    let size = vec2(ui.available_width().min(260.0), 90.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "EQ response graph")
    });
    let p = ui.painter();
    p.rect_filled(rect, 3.0, theme::FIELD);
    let y_of =
        |db: f64| rect.center().y - (db.clamp(-18.0, 18.0) / 18.0) as f32 * rect.height() / 2.0;
    for db in [-12.0, -6.0, 0.0, 6.0, 12.0] {
        let col = if db == 0.0 {
            theme::FAINT
        } else {
            theme::BORDER
        };
        p.line_segment(
            [pos2(rect.left(), y_of(db)), pos2(rect.right(), y_of(db))],
            Stroke::new(1.0, col),
        );
    }
    for hz in [100.0f64, 1000.0, 10000.0] {
        let x = rect.left() + rect.width() * ((hz / 20.0).log10() / 3.0) as f32;
        p.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, theme::BORDER),
        );
        p.text(
            pos2(x + 2.0, rect.bottom() - 2.0),
            egui::Align2::LEFT_BOTTOM,
            if hz >= 1000.0 {
                format!("{}k", hz / 1000.0)
            } else {
                format!("{hz}")
            },
            crate::theme::mono(8.5),
            theme::FAINT,
        );
    }
    let curve = captaine_audio::eq_response(&e.params, sample_rate, 96);
    let pts: Vec<Pos2> = curve
        .iter()
        .enumerate()
        .map(|(i, (_, db))| pos2(rect.left() + rect.width() * i as f32 / 95.0, y_of(*db)))
        .collect();
    p.add(Shape::line(
        pts,
        Stroke::new(
            1.8,
            if e.enabled {
                theme::ACCENT
            } else {
                theme::FAINT
            },
        ),
    ));
}

/// The .cube files in a folder, by name (the LUT browser's list).
pub fn lut_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("cube"))
        })
        .collect();
    v.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
    v
}

/// Where a new corner goes: on the outline, after the edge nearest to `p` (all in percent).
pub fn polygon_insert(pts: &mut Vec<(f64, f64)>, p: (f64, f64)) {
    if pts.len() >= 64 {
        return;
    }
    let mut best = (f64::MAX, 0);
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let (ex, ey) = (b.0 - a.0, b.1 - a.1);
        let l2 = ex * ex + ey * ey;
        let t = if l2 == 0.0 {
            0.0
        } else {
            (((p.0 - a.0) * ex + (p.1 - a.1) * ey) / l2).clamp(0.0, 1.0)
        };
        let d = ((p.0 - a.0 - t * ex).powi(2) + (p.1 - a.1 - t * ey).powi(2)).sqrt();
        if d < best.0 {
            best = (d, i);
        }
    }
    pts.insert(best.1 + 1, p);
}

/// A small canvas to draw a mask on: drag corners, click the outline to add one, right-click a corner to remove it.
pub fn polygon_editor(ui: &mut Ui, e: &mut Effect) -> (bool, bool) {
    let mut pts = captaine_model::parse_polygon(&e.text)
        .unwrap_or_else(|| vec![(25.0, 25.0), (75.0, 25.0), (75.0, 75.0), (25.0, 75.0)]);
    let (mut changed, mut done) = (e.text.is_empty(), e.text.is_empty());
    let w = ui.available_width().min(240.0);
    let size = vec2(w, w * 9.0 / 16.0);
    let (rect, bg) = ui.allocate_exact_size(size, Sense::click());
    let to_screen = |p: (f64, f64)| {
        pos2(
            rect.left() + rect.width() * p.0 as f32 / 100.0,
            rect.top() + rect.height() * p.1 as f32 / 100.0,
        )
    };
    ui.painter().rect_filled(rect, 3.0, theme::FIELD);
    let outline: Vec<Pos2> = pts.iter().map(|&p| to_screen(p)).collect();
    ui.painter().add(Shape::convex_polygon(
        outline.clone(),
        Color32::from_rgba_unmultiplied(0x4f, 0x8c, 0xf5, 50),
        Stroke::new(1.4, Color32::from_rgb(0x4f, 0x8c, 0xf5)),
    ));
    bg.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            "Mask outline: click to add a corner",
        )
    });
    let mut remove = None;
    for i in 0..pts.len() {
        let pos = to_screen(pts[i]);
        let resp = ui.interact(
            Rect::from_center_size(pos, vec2(14.0, 14.0)),
            ui.id().with(("mask-point", i)),
            Sense::click_and_drag(),
        );
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Slider,
                true,
                format!("Mask corner {}", i + 1),
            )
        });
        if resp.dragged() {
            if let Some(m) = resp.interact_pointer_pos() {
                pts[i] = (
                    ((m.x - rect.left()) / rect.width() * 100.0).clamp(0.0, 100.0) as f64,
                    ((m.y - rect.top()) / rect.height() * 100.0).clamp(0.0, 100.0) as f64,
                );
                changed = true;
            }
        }
        if resp.drag_stopped() {
            done = true;
        }
        if resp.secondary_clicked() && pts.len() > 3 {
            remove = Some(i);
        }
        ui.painter().circle_filled(pos, 4.0, Color32::WHITE);
        ui.painter()
            .circle_stroke(pos, 4.0, Stroke::new(1.0, Color32::BLACK));
    }
    if let Some(i) = remove {
        pts.remove(i);
        changed = true;
        done = true;
    } else if bg.clicked() {
        if let Some(m) = bg.interact_pointer_pos() {
            let p = (
                ((m.x - rect.left()) / rect.width() * 100.0) as f64,
                ((m.y - rect.top()) / rect.height() * 100.0) as f64,
            );
            polygon_insert(&mut pts, p);
            changed = true;
            done = true;
        }
    }
    if changed {
        e.text = captaine_model::polygon_text(&pts);
    }
    (changed, done)
}

#[cfg(test)]
mod wheel_tests {
    use super::*;

    #[test]
    fn the_puck_stays_where_it_was_dropped() {
        // drop the puck at points around the wheel, store, read back: it must be drawn at the same point
        for (x, y) in [
            (0.0, 0.0),
            (0.6, 0.0),
            (-0.3, 0.5),
            (0.0, -0.9),
            (0.7, 0.7),
            (-1.0, 0.0),
        ] {
            let stored = params_from_wheel(wheel_to_rgb(x, y));
            let (px, py) = rgb_to_wheel(wheel_from_params(stored));
            assert!(
                (px - x).abs() < 1e-5 && (py - y).abs() < 1e-5,
                "dropped at ({x}, {y}), drawn at ({px}, {py})"
            );
        }
    }

    #[test]
    fn the_rim_gives_half_strength_offsets() {
        let stored = params_from_wheel(wheel_to_rgb(1.0, 0.0));
        assert!((stored[0] - 0.5).abs() < 1e-9, "red at the rim: {stored:?}");
    }
}
