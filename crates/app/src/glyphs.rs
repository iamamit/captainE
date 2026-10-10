//! Small drawn icons for the timeline toolbar and track headers (no icon font: they are lines and shapes).

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui};

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    Select,
    Trim,
    Roll,
    Slip,
    Slide,
    Blade,
    Hand,
    Zoom,
    Magnet,
    Link,
    Flag,
    Lift,
    Extract,
    Insert,
    Overwrite,
    Mic,
    Lock,
    Unlock,
    Eye,
    EyeOff,
    Speaker,
    SpeakerOff,
    Fit,
    Plus,
    Minus,
    Film,
    Wave,
    List,
    Target,
    Chevron,
}

/// Draw `g` inside `rect` (a square works best) in `ink`.
pub fn draw(p: &Painter, rect: Rect, g: Glyph, ink: Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let c = rect.center();
    let pt = |x: f32, y: f32| pos2(c.x + (x - 12.0) * s, c.y + (y - 12.0) * s);
    let st = Stroke::new((1.6 * s).max(1.0), ink);
    let line = |a: Pos2, b: Pos2| p.line_segment([a, b], st);
    let poly = |pts: Vec<Pos2>, fill: Color32| {
        p.add(Shape::convex_polygon(pts, fill, Stroke::NONE));
    };
    let outline = |pts: Vec<Pos2>| {
        p.add(Shape::closed_line(pts, st));
    };
    let arrow_l = |x: f32, y: f32, d: f32| {
        // a small arrow head pointing left (d = -1) or right (d = 1)
        line(pt(x, y - 3.0), pt(x + 3.0 * d, y));
        line(pt(x, y + 3.0), pt(x + 3.0 * d, y));
    };
    match g {
        Glyph::Select => poly(
            vec![
                pt(7.0, 4.0),
                pt(7.0, 19.0),
                pt(11.0, 15.0),
                pt(14.5, 21.0),
                pt(16.5, 20.0),
                pt(13.0, 14.0),
                pt(18.0, 14.0),
            ],
            ink,
        ),
        Glyph::Trim => {
            // a bracket and an arrow: the edge being pulled
            line(pt(9.0, 5.0), pt(9.0, 19.0));
            line(pt(9.0, 12.0), pt(19.0, 12.0));
            arrow_l(19.0, 12.0, -1.0);
            line(pt(5.0, 5.0), pt(9.0, 5.0));
            line(pt(5.0, 19.0), pt(9.0, 19.0));
        }
        Glyph::Roll => {
            // two neighbours and a cut with arrows both ways
            line(pt(12.0, 4.0), pt(12.0, 20.0));
            line(pt(4.0, 12.0), pt(20.0, 12.0));
            arrow_l(4.0, 12.0, 1.0);
            arrow_l(20.0, 12.0, -1.0);
        }
        Glyph::Slip => {
            outline(vec![
                pt(6.0, 6.0),
                pt(18.0, 6.0),
                pt(18.0, 18.0),
                pt(6.0, 18.0),
            ]);
            line(pt(9.0, 12.0), pt(15.0, 12.0));
            arrow_l(9.0, 12.0, 1.0);
            arrow_l(15.0, 12.0, -1.0);
        }
        Glyph::Slide => {
            outline(vec![
                pt(8.0, 7.0),
                pt(16.0, 7.0),
                pt(16.0, 17.0),
                pt(8.0, 17.0),
            ]);
            line(pt(3.0, 12.0), pt(8.0, 12.0));
            line(pt(16.0, 12.0), pt(21.0, 12.0));
            arrow_l(3.0, 12.0, 1.0);
            arrow_l(21.0, 12.0, -1.0);
        }
        Glyph::Blade => {
            // a razor: a slanted blade
            poly(
                vec![pt(5.0, 17.0), pt(15.0, 4.0), pt(19.0, 7.0), pt(9.0, 20.0)],
                ink,
            );
            line(pt(5.0, 20.0), pt(20.0, 20.0));
        }
        Glyph::Hand => {
            for x in [8.0, 11.0, 14.0, 17.0] {
                line(pt(x, 12.0), pt(x, 5.5 + (x - 8.0).abs() * 0.1));
            }
            outline(vec![
                pt(7.0, 12.0),
                pt(18.0, 12.0),
                pt(18.0, 17.0),
                pt(14.0, 21.0),
                pt(10.0, 21.0),
                pt(7.0, 17.0),
            ]);
        }
        Glyph::Zoom => {
            p.circle_stroke(pt(10.5, 10.5), 6.0 * s, st);
            line(pt(15.0, 15.0), pt(20.0, 20.0));
            line(pt(8.0, 10.5), pt(13.0, 10.5));
            line(pt(10.5, 8.0), pt(10.5, 13.0));
        }
        Glyph::Magnet => {
            // a horseshoe: two legs and an arc
            line(pt(6.0, 4.0), pt(6.0, 12.0));
            line(pt(18.0, 4.0), pt(18.0, 12.0));
            let arc: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * i as f32 / 12.0;
                    pt(12.0 - 6.0 * a.cos(), 12.0 + 8.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(arc, st));
            line(pt(4.0, 7.0), pt(8.0, 7.0));
            line(pt(16.0, 7.0), pt(20.0, 7.0));
        }
        Glyph::Link => {
            for dx in [-3.5f32, 3.5] {
                let r = Rect::from_center_size(pt(12.0 + dx, 12.0), vec2(10.0 * s, 6.0 * s));
                p.rect_stroke(r, 3.0 * s, st, egui::StrokeKind::Middle);
            }
            line(pt(9.5, 12.0), pt(14.5, 12.0));
        }
        Glyph::Flag => {
            line(pt(7.0, 4.0), pt(7.0, 20.0));
            poly(
                vec![
                    pt(7.0, 5.0),
                    pt(19.0, 5.0),
                    pt(15.5, 9.5),
                    pt(19.0, 14.0),
                    pt(7.0, 14.0),
                ],
                ink,
            );
        }
        Glyph::Lift | Glyph::Extract => {
            // a clip with a hole where it was taken out; extract also closes the hole
            outline(vec![
                pt(3.0, 14.0),
                pt(9.0, 14.0),
                pt(9.0, 20.0),
                pt(3.0, 20.0),
            ]);
            if g == Glyph::Lift {
                outline(vec![
                    pt(15.0, 14.0),
                    pt(21.0, 14.0),
                    pt(21.0, 20.0),
                    pt(15.0, 20.0),
                ]);
                line(pt(12.0, 3.0), pt(12.0, 10.0));
                arrow_l(12.0, 3.0, 1.0);
            } else {
                outline(vec![
                    pt(10.0, 14.0),
                    pt(16.0, 14.0),
                    pt(16.0, 20.0),
                    pt(10.0, 20.0),
                ]);
                line(pt(12.0, 3.0), pt(12.0, 10.0));
                arrow_l(12.0, 3.0, 1.0);
                line(pt(19.0, 17.0), pt(22.0, 17.0));
            }
        }
        Glyph::Insert | Glyph::Overwrite => {
            outline(vec![
                pt(3.0, 4.0),
                pt(21.0, 4.0),
                pt(21.0, 12.0),
                pt(3.0, 12.0),
            ]);
            let x = if g == Glyph::Insert { 8.0 } else { 12.0 };
            line(pt(x, 20.0), pt(x, 14.0));
            line(pt(x - 3.0, 17.0), pt(x, 14.0));
            line(pt(x + 3.0, 17.0), pt(x, 14.0));
            if g == Glyph::Overwrite {
                line(pt(5.0, 20.0), pt(19.0, 20.0));
            }
        }
        Glyph::Mic => {
            p.rect_stroke(
                Rect::from_center_size(pt(12.0, 9.0), vec2(6.0 * s, 11.0 * s)),
                3.0 * s,
                st,
                egui::StrokeKind::Middle,
            );
            let arc: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let a = std::f32::consts::PI * i as f32 / 10.0;
                    pt(12.0 - 6.5 * a.cos(), 11.0 + 6.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(arc, st));
            line(pt(12.0, 17.0), pt(12.0, 21.0));
            line(pt(8.0, 21.0), pt(16.0, 21.0));
        }
        Glyph::Lock | Glyph::Unlock => {
            p.rect_filled(
                Rect::from_center_size(pt(12.0, 15.0), vec2(12.0 * s, 8.0 * s)),
                1.5 * s,
                ink,
            );
            // the shackle: closed over the body, or swung open to the right
            let shift = if g == Glyph::Lock { 0.0 } else { 4.0 };
            let arc: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let a = std::f32::consts::PI * i as f32 / 10.0;
                    pt(12.0 + shift - 4.5 * a.cos(), 11.0 - 6.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(arc, st));
            line(pt(7.5 + shift, 11.0), pt(7.5 + shift, 12.0));
            if g == Glyph::Lock {
                line(pt(16.5, 11.0), pt(16.5, 12.0));
            }
        }
        Glyph::Eye | Glyph::EyeOff => {
            let top: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    pt(
                        3.0 + 18.0 * t,
                        12.0 - 7.0 * (std::f32::consts::PI * t).sin(),
                    )
                })
                .collect();
            let bottom: Vec<Pos2> = top.iter().map(|q| pos2(q.x, 2.0 * c.y - q.y)).collect();
            p.add(Shape::line(top, st));
            p.add(Shape::line(bottom, st));
            p.circle_filled(pt(12.0, 12.0), 2.6 * s, ink);
            if g == Glyph::EyeOff {
                line(pt(4.0, 20.0), pt(20.0, 4.0));
            }
        }
        Glyph::Speaker | Glyph::SpeakerOff => {
            poly(
                vec![
                    pt(4.0, 9.0),
                    pt(8.0, 9.0),
                    pt(13.0, 5.0),
                    pt(13.0, 19.0),
                    pt(8.0, 15.0),
                    pt(4.0, 15.0),
                ],
                ink,
            );
            if g == Glyph::Speaker {
                for r in [4.0f32, 7.5] {
                    let arc: Vec<Pos2> = (0..=8)
                        .map(|i| {
                            let a = -0.9 + 1.8 * i as f32 / 8.0;
                            pt(13.0 + r * a.cos(), 12.0 + r * a.sin())
                        })
                        .collect();
                    p.add(Shape::line(arc, st));
                }
            } else {
                line(pt(16.0, 9.0), pt(21.0, 15.0));
                line(pt(21.0, 9.0), pt(16.0, 15.0));
            }
        }
        Glyph::Fit => {
            outline(vec![
                pt(3.0, 6.0),
                pt(21.0, 6.0),
                pt(21.0, 18.0),
                pt(3.0, 18.0),
            ]);
            line(pt(7.0, 12.0), pt(17.0, 12.0));
            arrow_l(7.0, 12.0, 1.0);
            arrow_l(17.0, 12.0, -1.0);
        }
        Glyph::Plus => {
            line(pt(5.0, 12.0), pt(19.0, 12.0));
            line(pt(12.0, 5.0), pt(12.0, 19.0));
        }
        Glyph::Minus => {
            line(pt(5.0, 12.0), pt(19.0, 12.0));
        }
        Glyph::Film => {
            outline(vec![
                pt(3.0, 5.0),
                pt(21.0, 5.0),
                pt(21.0, 19.0),
                pt(3.0, 19.0),
            ]);
            for y in [8.0, 12.0, 16.0] {
                line(pt(5.0, y), pt(7.0, y));
                line(pt(17.0, y), pt(19.0, y));
            }
            line(pt(9.5, 5.0), pt(9.5, 19.0));
            line(pt(14.5, 5.0), pt(14.5, 19.0));
        }
        Glyph::Target => {
            outline(vec![
                pt(4.0, 4.0),
                pt(20.0, 4.0),
                pt(20.0, 20.0),
                pt(4.0, 20.0),
            ]);
            poly(
                vec![pt(8.5, 8.5), pt(15.5, 8.5), pt(15.5, 15.5), pt(8.5, 15.5)],
                ink,
            );
        }
        Glyph::Chevron => {
            line(pt(6.0, 9.0), pt(12.0, 15.0));
            line(pt(18.0, 9.0), pt(12.0, 15.0));
        }
        Glyph::List => {
            for y in [7.0f32, 12.0, 17.0] {
                p.circle_filled(pt(5.0, y), 1.3 * s, ink);
                line(pt(9.0, y), pt(20.0, y));
            }
        }
        Glyph::Wave => {
            for (i, h) in [3.0f32, 7.0, 11.0, 6.0, 9.0, 4.0, 2.0]
                .into_iter()
                .enumerate()
            {
                let x = 4.0 + i as f32 * 2.7;
                line(pt(x, 12.0 - h / 2.0), pt(x, 12.0 + h / 2.0));
            }
        }
    }
}

/// A square icon button. `label` is the accessible name and the tooltip start.
pub fn button(
    ui: &mut Ui,
    g: Glyph,
    active: bool,
    enabled: bool,
    label: &str,
    tip: &str,
) -> Response {
    let size = 28.0;
    let (rect, resp) = ui.allocate_exact_size(
        vec2(size, size),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let fill = if active {
        theme::ACCENT
    } else if resp.hovered() && enabled {
        theme::FIELD
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 5.0, fill);
    let ink = if !enabled {
        theme::FAINT
    } else if active || resp.hovered() {
        Color32::WHITE
    } else {
        theme::MUTED
    };
    draw(ui.painter(), rect.shrink(4.0), g, ink);
    let name = label.to_string();
    resp.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name.clone())
    });
    let text = if tip.is_empty() {
        label.to_string()
    } else {
        format!("{label}   {tip}")
    };
    resp.on_hover_text(text)
}

/// A small icon for a track header.
pub fn small(ui: &mut Ui, g: Glyph, on: bool, label: &str, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, theme::FIELD);
    }
    let ink = if on { Color32::WHITE } else { theme::FAINT };
    draw(ui.painter(), rect.shrink(2.0), g, ink);
    let name = label.to_string();
    resp.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
    });
    resp.on_hover_text(tip.to_string())
}
