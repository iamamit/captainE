//! The Titles tab of the media pool and the inspector for a selected title.

use captaine_model::*;
use captaine_ops::Op;
use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke, Ui};

use crate::app::App;
use crate::theme::{self, mono, ui_font};
use crate::ui::{chip, slider_row, LayerDrag, TitleDrag};

const TEMPLATES: [TitleTemplate; 6] = TitleTemplate::ALL;

pub fn pool_tab(app: &mut App, ui: &mut Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Double-click to add at the playhead, or drag onto a video track.")
                    .size(11.0)
                    .color(theme::MUTED),
            );
            ui.add_space(8.0);
            let gap = 8.0;
            let w = ((ui.available_width() - gap) / 2.0).floor();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for row in TEMPLATES.chunks(2) {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for t in row {
                                card(app, ui, *t, w);
                            }
                        });
                        ui.add_space(6.0);
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Shapes & layers").strong().size(12.0));
                    ui.add_space(6.0);
                    let kinds = [
                        Some(ShapeKind::Rectangle),
                        Some(ShapeKind::Ellipse),
                        Some(ShapeKind::Polygon),
                        Some(ShapeKind::Star),
                        Some(ShapeKind::Line),
                        Some(ShapeKind::Solid),
                        None,
                    ];
                    for row in kinds.chunks(2) {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for k in row {
                                layer_card(app, ui, *k, w);
                            }
                        });
                        ui.add_space(6.0);
                    }
                });
        });
}

fn layer_card(app: &mut App, ui: &mut Ui, kind: Option<ShapeKind>, w: f32) {
    let h = (w * 9.0 / 16.0).round();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h + 22.0), Sense::click_and_drag());
    let pic = Rect::from_min_size(rect.min, vec2(w, h));
    let p = ui.painter();
    p.rect_filled(pic, 3.0, Color32::from_rgb(0x2c, 0x3f, 0x57));
    p.rect_filled(
        Rect::from_min_max(pos2(pic.left(), pic.top() + h * 0.62), pic.max),
        3.0,
        Color32::from_rgb(0x59, 0x6b, 0x55),
    );
    match kind {
        Some(ShapeKind::Rectangle) => {
            p.rect_filled(
                Rect::from_center_size(pic.center(), vec2(w * 0.5, h * 0.5)),
                2.0,
                theme::ACCENT,
            );
        }
        Some(ShapeKind::Ellipse) => {
            p.circle_filled(pic.center(), h * 0.28, theme::ACCENT);
        }
        Some(ShapeKind::Solid) => {
            p.rect_filled(pic, 3.0, Color32::from_rgb(0x28, 0x28, 0x30));
        }
        Some(k @ (ShapeKind::Polygon | ShapeKind::Star)) => {
            let (n, c, r) = (
                if k == ShapeKind::Star { 10 } else { 6 },
                pic.center(),
                h * 0.3,
            );
            let pts: Vec<egui::Pos2> = (0..n)
                .map(|i| {
                    let a =
                        std::f32::consts::TAU * i as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
                    let r = if k == ShapeKind::Star && i % 2 == 1 {
                        r * 0.42
                    } else {
                        r
                    };
                    pos2(c.x + r * a.cos(), c.y + r * a.sin())
                })
                .collect();
            p.add(egui::Shape::convex_polygon(
                pts,
                theme::ACCENT,
                Stroke::NONE,
            ));
        }
        Some(ShapeKind::Line) => {
            p.rect_filled(
                Rect::from_center_size(pic.center(), vec2(w * 0.6, h * 0.08)),
                h * 0.04,
                theme::ACCENT,
            );
        }
        None => {
            // a pane of glass over the scene
            p.rect_filled(pic, 3.0, Color32::from_rgba_unmultiplied(255, 255, 255, 40));
            p.line_segment(
                [pic.left_bottom(), pic.right_top()],
                Stroke::new(1.5, Color32::from_white_alpha(160)),
            );
        }
    }
    let label = kind.map_or("Adjustment Layer", |k| k.label());
    p.text(
        rect.left_bottom() + vec2(0.0, -20.0),
        Align2::LEFT_TOP,
        label,
        ui_font(11.0),
        theme::TEXT,
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if resp.double_clicked() {
        app.add_layer(kind, None, None);
    }
    if resp.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), LayerDrag(kind));
    }
    resp.context_menu(|ui| {
        if ui.button("Add at Playhead").clicked() {
            app.add_layer(kind, None, None);
            ui.close();
        }
    });
}

/// The inspector of a shape, solid colour or adjustment layer.
fn layer_controls(app: &mut App, ui: &mut Ui, title: &Title, track_name: &str) {
    ui.label(RichText::new(&title.name).strong().size(13.0));
    ui.label(
        RichText::new(format!(
            "{track_name} · {} · {}",
            app.tc(title.duration.value),
            if title.adjustment {
                "adjustment layer"
            } else {
                "shape"
            }
        ))
        .font(mono(10.5))
        .color(theme::MUTED),
    );
    let mut t = title.clone();
    let (mut changed, mut done) = (false, false);
    ui.add_space(8.0);
    if let Some(sh) = t.shape.as_mut() {
        ui.horizontal(|ui| {
            ui.add_sized(
                [80.0, 22.0],
                egui::Label::new(RichText::new("Fill").size(12.0)),
            );
            let mut rgb = [sh.color[0], sh.color[1], sh.color[2]];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                sh.color = [rgb[0], rgb[1], rgb[2], sh.color[3]];
                changed = true;
                done = true;
            }
            let on = sh.gradient_to.is_some();
            if chip(ui, "Gradient", on, true).clicked() {
                sh.gradient_to = if on { None } else { Some([0, 0, 0, 255]) };
                changed = true;
                done = true;
            }
            if let Some(g) = sh.gradient_to.as_mut() {
                let mut rgb = [g[0], g[1], g[2]];
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    *g = [rgb[0], rgb[1], rgb[2], g[3]];
                    changed = true;
                    done = true;
                }
            }
        });
        let mut row = |ui: &mut Ui,
                       label: &str,
                       v: &mut f64,
                       range: std::ops::RangeInclusive<f64>,
                       def: f64,
                       suf: &str| {
            let r = slider_row(ui, label, v, range, def, 0, suf, true);
            changed |= r.changed;
            done |= r.done;
        };
        let mut alpha = sh.color[3] as f64 / 255.0 * 100.0;
        let before = alpha;
        row(ui, "Fill opacity", &mut alpha, 0.0..=100.0, 100.0, "%");
        if alpha != before {
            sh.color[3] = (alpha / 100.0 * 255.0).round() as u8;
        }
        if sh.kind != ShapeKind::Solid {
            row(ui, "Width", &mut sh.width, 1.0..=200.0, 40.0, "%");
            row(ui, "Height", &mut sh.height, 1.0..=200.0, 40.0, "%");
        }
        if sh.kind == ShapeKind::Rectangle {
            row(ui, "Corner radius", &mut sh.corner, 0.0..=50.0, 0.0, "%");
        }
        if matches!(sh.kind, ShapeKind::Polygon | ShapeKind::Star) {
            let mut n = sh.sides as f64;
            row(ui, "Sides", &mut n, 3.0..=16.0, 5.0, "");
            sh.sides = n.round() as u32;
        }
        if sh.kind != ShapeKind::Solid {
            row(ui, "Outline", &mut sh.outline, 0.0..=50.0, 0.0, " px");
            if sh.outline > 0.0 {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [80.0, 22.0],
                        egui::Label::new(RichText::new("Outline color").size(12.0)),
                    );
                    let mut rgb = [
                        sh.outline_color[0],
                        sh.outline_color[1],
                        sh.outline_color[2],
                    ];
                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                        sh.outline_color = [rgb[0], rgb[1], rgb[2], 255];
                        changed = true;
                        done = true;
                    }
                });
            }
        }
    } else {
        ui.label(
            RichText::new("Everything on the tracks below this layer goes through its effects. Add a Mask to limit it to part of the picture.")
                .size(10.5)
                .color(theme::FAINT),
        );
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Transform").strong().size(12.5));
    let (cw, ch) = (app.canvas().0 as f64, app.canvas().1 as f64);
    let mut row = |ui: &mut Ui,
                   label: &str,
                   v: &mut f64,
                   range: std::ops::RangeInclusive<f64>,
                   def: f64,
                   dec: usize,
                   suf: &str| {
        let r = slider_row(
            ui,
            label,
            v,
            range,
            def,
            dec,
            suf,
            !title.adjustment || label == "Opacity",
        );
        changed |= r.changed;
        done |= r.done;
    };
    row(
        ui,
        "Position X",
        &mut t.video.transform.position.0,
        -cw..=cw,
        0.0,
        1,
        "",
    );
    row(
        ui,
        "Position Y",
        &mut t.video.transform.position.1,
        -ch..=ch,
        0.0,
        1,
        "",
    );
    let mut scale = t.video.transform.scale.0 * 100.0;
    let before = scale;
    row(ui, "Scale", &mut scale, 5.0..=400.0, 100.0, 1, "%");
    if scale != before {
        t.video.transform.scale = (scale / 100.0, scale / 100.0);
    }
    row(
        ui,
        "Rotation",
        &mut t.video.transform.rotation_deg,
        -180.0..=180.0,
        0.0,
        1,
        "°",
    );
    let mut opacity = t.video.opacity * 100.0;
    let before = opacity;
    row(ui, "Opacity", &mut opacity, 0.0..=100.0, 100.0, 0, "%");
    if opacity != before {
        t.video.opacity = opacity / 100.0;
    }
    if t != *title || changed {
        let key = format!("t:{}", title.id.to_full_string());
        app.do_merging(
            Op::SetTitle {
                item: title.id,
                title: t,
            },
            &key,
        );
    }
    if done {
        app.end_merge();
    }
    animate_controls(app, ui, title);
    crate::ui::effects_stack(app, ui, title.id, &title.effects, true, false);
}

fn card(app: &mut App, ui: &mut Ui, template: TitleTemplate, w: f32) {
    let h = (w * 9.0 / 16.0).round();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h + 22.0), Sense::click_and_drag());
    let pic = Rect::from_min_size(rect.min, vec2(w, h));
    let p = ui.painter();
    // a stand-in scene so the title reads as a title
    p.rect_filled(pic, 3.0, Color32::from_rgb(0x2c, 0x3f, 0x57));
    p.rect_filled(
        Rect::from_min_max(pos2(pic.left(), pic.top() + h * 0.62), pic.max),
        3.0,
        Color32::from_rgb(0x59, 0x6b, 0x55),
    );
    let s = h / 90.0;
    match template {
        TitleTemplate::LowerThird => {
            let plate = Rect::from_min_max(
                pos2(pic.left() + w * 0.07, pic.top() + h * 0.58),
                pos2(pic.left() + w * 0.66, pic.top() + h * 0.86),
            );
            p.rect_filled(plate, 3.0, Color32::from_rgba_unmultiplied(24, 24, 28, 225));
            p.rect_filled(
                Rect::from_min_size(plate.min, vec2(3.0 * s, plate.height())),
                0.0,
                theme::ACCENT,
            );
            p.text(
                plate.left_top() + vec2(9.0 * s, 4.0 * s),
                Align2::LEFT_TOP,
                "Your Title",
                ui_font(11.0 * s.max(0.8)),
                Color32::WHITE,
            );
            p.text(
                plate.left_top() + vec2(9.0 * s, 17.0 * s),
                Align2::LEFT_TOP,
                "Subtitle",
                ui_font(8.0 * s.max(0.8)),
                Color32::from_gray(190),
            );
        }
        TitleTemplate::Centered => {
            p.text(
                pic.center(),
                Align2::CENTER_CENTER,
                "Title",
                ui_font(24.0 * s.max(0.8)),
                Color32::WHITE,
            );
        }
        TitleTemplate::Minimal => {
            let y = pic.top() + h * 0.7;
            p.text(
                pos2(pic.left() + w * 0.08, y),
                Align2::LEFT_CENTER,
                "Minimal title",
                ui_font(14.0 * s.max(0.8)),
                Color32::WHITE,
            );
            p.rect_filled(
                Rect::from_min_size(
                    pos2(pic.left() + w * 0.08, y + 9.0 * s),
                    vec2(w * 0.4, 2.0 * s.max(0.8)),
                ),
                1.0,
                theme::ACCENT,
            );
        }
        TitleTemplate::Boxed => {
            let plate = Rect::from_center_size(pic.center(), vec2(w * 0.62, 24.0 * s.max(0.8)));
            p.rect_filled(plate.expand(1.5), 5.0, theme::ACCENT);
            p.rect_filled(plate, 4.0, Color32::from_rgb(24, 24, 28));
            p.text(
                plate.center(),
                Align2::CENTER_CENTER,
                "Boxed title",
                ui_font(12.0 * s.max(0.8)),
                Color32::WHITE,
            );
        }
        TitleTemplate::Quote => {
            p.text(
                pic.center() + vec2(0.0, -14.0 * s),
                Align2::CENTER_CENTER,
                "\u{201C}",
                ui_font(30.0 * s.max(0.8)),
                theme::ACCENT,
            );
            p.text(
                pic.center() + vec2(0.0, 6.0 * s),
                Align2::CENTER_CENTER,
                "Make it simple",
                ui_font(10.0 * s.max(0.8)),
                Color32::WHITE,
            );
            p.text(
                pic.center() + vec2(0.0, 18.0 * s),
                Align2::CENTER_CENTER,
                "\u{2014} Speaker",
                ui_font(7.0 * s.max(0.8)),
                Color32::from_gray(190),
            );
        }
        TitleTemplate::Caption => {
            let pill = Rect::from_center_size(
                pos2(pic.center().x, pic.top() + h * 0.82),
                vec2(w * 0.7, 15.0 * s.max(0.8)),
            );
            p.rect_filled(pill, pill.height() / 2.0, Color32::from_black_alpha(160));
            p.text(
                pill.center(),
                Align2::CENTER_CENTER,
                "Caption text",
                ui_font(9.5 * s.max(0.8)),
                Color32::WHITE,
            );
        }
    }
    if resp.hovered() {
        p.rect_stroke(
            pic,
            3.0,
            Stroke::new(1.0, theme::FAINT),
            egui::StrokeKind::Inside,
        );
    }
    p.text(
        pos2(rect.left(), pic.bottom() + 4.0),
        Align2::LEFT_TOP,
        template.label(),
        ui_font(11.0),
        theme::TEXT,
    );
    p.text(
        pos2(rect.left(), pic.bottom() + 18.0),
        Align2::LEFT_TOP,
        "00:00:03",
        mono(10.0),
        theme::MUTED,
    );
    if resp.double_clicked() {
        app.add_title(template, None, None);
    }
    if resp.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), TitleDrag(template));
    }
    resp.context_menu(|ui| {
        if ui.button("Add at Playhead").clicked() {
            app.add_title(template, None, None);
            ui.close();
        }
    });
}

fn field(ui: &mut Ui, label: &str, text: &mut String) -> (bool, bool) {
    ui.label(RichText::new(label).size(11.5).color(theme::MUTED));
    let r = ui.add(
        egui::TextEdit::singleline(text)
            .desired_width(f32::INFINITY)
            .margin(egui::Margin::symmetric(8, 5)),
    );
    (r.changed(), r.lost_focus())
}

/// Inspector for the selected title. Edits are one undo step per drag or per text-field focus.
/// Animation presets: how the title comes on and goes off.
fn animate_controls(app: &mut App, ui: &mut Ui, title: &Title) {
    use captaine_model::anim_presets::{apply, tracks, Preset};
    ui.add_space(10.0);
    ui.label(RichText::new("Animate").strong().size(12.5));
    let fps = app.fps();
    let mut secs = app.preset_secs;
    let r = slider_row(ui, "Length", &mut secs, 0.1..=3.0, 0.6, 1, " s", true);
    if r.changed {
        app.preset_secs = secs;
    }
    let total = title.duration.value;
    let len = ((app.preset_secs * fps).round() as i64).clamp(1, total);
    let canvas = (app.canvas().0 as f64, app.canvas().1 as f64);
    let mut picked: Option<(Preset, bool)> = None;
    for (label, entering) in [("In", true), ("Out", false)] {
        ui.horizontal_wrapped(|ui| {
            ui.add_sized(
                [28.0, 22.0],
                egui::Label::new(RichText::new(label).size(12.0).color(theme::MUTED)),
            );
            for p in Preset::ALL {
                if !entering && p == Preset::Typewriter {
                    continue;
                }
                if chip(ui, p.label(), false, true).clicked() {
                    picked = Some((p, entering));
                }
            }
        });
    }
    let clear = ui
        .horizontal(|ui| chip(ui, "Clear animation", false, !title.anim.is_empty()).clicked())
        .inner;
    if let Some((preset, entering)) = picked {
        let new = tracks(preset, entering, len, total, &title.video, canvas);
        let mut anim = title.anim.clone();
        let props: Vec<&str> = new.iter().map(|(p, _)| *p).collect();
        apply(&mut anim, new, entering, len, total);
        let ops: Vec<Op> = props
            .into_iter()
            .map(|p| Op::SetAnimTrack {
                item: title.id,
                prop: p.to_string(),
                keys: anim.keys(p).to_vec(),
            })
            .collect();
        app.do_op(Op::Transaction {
            description: format!("{} {}", preset.label(), if entering { "in" } else { "out" }),
            ops,
        });
    }
    if clear {
        let ops: Vec<Op> = title
            .anim
            .0
            .keys()
            .map(|p| Op::SetAnimTrack {
                item: title.id,
                prop: p.clone(),
                keys: Vec::new(),
            })
            .collect();
        app.do_op(Op::Transaction {
            description: "Clear animation".into(),
            ops,
        });
    }
}

pub fn controls(app: &mut App, ui: &mut Ui) {
    let Some((title, track, _)) = app.selected_title() else {
        return;
    };
    let track_name = app
        .timeline()
        .and_then(|t| t.tracks.iter().find(|x| x.id == track))
        .map(|t| t.name.clone())
        .unwrap_or_default();
    if title.shape.is_some() || title.adjustment {
        layer_controls(app, ui, &title, &track_name);
        return;
    }
    ui.label(RichText::new(&title.name).strong().size(13.0));
    ui.label(
        RichText::new(format!(
            "{track_name} · {} · title",
            app.tc(title.duration.value)
        ))
        .font(mono(10.5))
        .color(theme::MUTED),
    );
    let mut t = title.clone();
    let changed = std::cell::Cell::new(false);
    let done = std::cell::Cell::new(false);
    ui.add_space(8.0);
    let (c, d) = field(ui, "Text", &mut t.text);
    changed.set(changed.get() | c);
    done.set(done.get() | d);
    ui.add_space(4.0);
    let (c, d) = field(ui, "Second line", &mut t.subtitle);
    changed.set(changed.get() | c);
    done.set(done.get() | d);

    ui.add_space(10.0);
    ui.label(RichText::new("Template").strong().size(12.5));
    ui.horizontal_wrapped(|ui| {
        for tpl in TEMPLATES {
            if chip(ui, tpl.label(), t.style.template == tpl, true).clicked() {
                t.style.template = tpl;
                t.style.size = tpl.default_size();
                changed.set(true);
                done.set(true);
            }
        }
    });
    let row = |ui: &mut Ui,
               label: &str,
               v: &mut f64,
               range: std::ops::RangeInclusive<f64>,
               def: f64,
               dec: usize,
               suffix: &str| {
        let r = slider_row(ui, label, v, range, def, dec, suffix, true);
        changed.set(changed.get() | r.changed);
        done.set(done.get() | r.done);
    };
    let def_size = t.style.template.default_size();
    row(
        ui,
        "Size",
        &mut t.style.size,
        12.0..=300.0,
        def_size,
        0,
        " px",
    );
    ui.horizontal(|ui| {
        ui.add_sized(
            [80.0, 22.0],
            egui::Label::new(RichText::new("Text colour").size(12.0)),
        );
        let r1 = ui.color_edit_button_srgb(&mut t.style.color);
        ui.add_space(10.0);
        ui.label(RichText::new("Accent").size(12.0));
        let r2 = ui.color_edit_button_srgb(&mut t.style.accent);
        if r1.changed() || r2.changed() {
            changed.set(true);
        }
    });

    row(
        ui,
        "Outline",
        &mut t.style.outline,
        0.0..=40.0,
        0.0,
        0,
        " px",
    );
    if t.style.outline > 0.0 {
        ui.horizontal(|ui| {
            ui.add_sized(
                [80.0, 22.0],
                egui::Label::new(RichText::new("Outline color").size(12.0)),
            );
            if ui
                .color_edit_button_srgb(&mut t.style.outline_color)
                .changed()
            {
                changed.set(true);
                done.set(true);
            }
        });
    }
    row(ui, "Shadow", &mut t.style.shadow, 0.0..=60.0, 0.0, 0, " px");
    row(ui, "Glow", &mut t.style.glow, 0.0..=80.0, 0.0, 0, " px");
    row(
        ui,
        "Letter spacing",
        &mut t.style.tracking,
        -20.0..=80.0,
        0.0,
        0,
        " px",
    );
    ui.horizontal(|ui| {
        ui.add_sized(
            [80.0, 22.0],
            egui::Label::new(RichText::new("Align").size(12.0)),
        );
        for (a, l) in [
            (TextAlign::Left, "Left"),
            (TextAlign::Center, "Center"),
            (TextAlign::Right, "Right"),
        ] {
            if chip(ui, l, t.style.align == a, true).clicked() {
                t.style.align = a;
                changed.set(true);
                done.set(true);
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_sized(
            [80.0, 22.0],
            egui::Label::new(RichText::new("Typeface").size(12.0)),
        );
        let cur = t.font.clone();
        if chip(ui, "Default", cur.is_none(), true).clicked() {
            t.font = None;
            changed.set(true);
            done.set(true);
        }
        if chip(ui, "Mono", cur.as_deref() == Some("builtin:mono"), true).clicked() {
            t.font = Some("builtin:mono".into());
            changed.set(true);
            done.set(true);
        }
        let file_name = cur
            .as_deref()
            .filter(|f| !f.starts_with("builtin:"))
            .map(|f| {
                std::path::Path::new(f)
                    .file_name()
                    .map_or(f.to_string(), |n| n.to_string_lossy().into())
            });
        if chip(
            ui,
            &file_name.clone().unwrap_or_else(|| "Font file…".into()),
            file_name.is_some(),
            true,
        )
        .clicked()
        {
            if let Some(f) = rfd::FileDialog::new()
                .add_filter("font", &["ttf", "otf"])
                .pick_file()
            {
                t.font = Some(f.to_string_lossy().into());
                changed.set(true);
                done.set(true);
            }
        }
    });

    ui.add_space(10.0);
    ui.label(RichText::new("Fades").strong().size(12.5));
    let fps = app.fps();
    let max_fade = title.duration.value as f64 / fps;
    let (mut fin, mut fout) = (t.fade_in as f64 / fps, t.fade_out as f64 / fps);
    let (before_in, before_out) = (fin, fout);
    row(ui, "Fade in", &mut fin, 0.0..=max_fade, 0.0, 2, " s");
    row(ui, "Fade out", &mut fout, 0.0..=max_fade, 0.0, 2, " s");
    if fin != before_in || fout != before_out {
        t.fade_in = (fin * fps).round() as i64;
        t.fade_out = (fout * fps).round() as i64;
        let over = t.fade_in + t.fade_out - title.duration.value;
        if over > 0 {
            if fin != before_in {
                t.fade_out = (t.fade_out - over).max(0);
            } else {
                t.fade_in = (t.fade_in - over).max(0);
            }
        }
    }

    ui.add_space(10.0);
    ui.label(RichText::new("Transform").strong().size(12.5));
    let (cw, ch) = (app.canvas().0 as f64, app.canvas().1 as f64);
    row(
        ui,
        "Position X",
        &mut t.video.transform.position.0,
        -cw..=cw,
        0.0,
        1,
        "",
    );
    row(
        ui,
        "Position Y",
        &mut t.video.transform.position.1,
        -ch..=ch,
        0.0,
        1,
        "",
    );
    let mut scale = t.video.transform.scale.0 * 100.0;
    let before = scale;
    row(ui, "Scale", &mut scale, 5.0..=400.0, 100.0, 1, "%");
    if scale != before {
        t.video.transform.scale = (scale / 100.0, scale / 100.0);
    }
    row(
        ui,
        "Rotation",
        &mut t.video.transform.rotation_deg,
        -180.0..=180.0,
        0.0,
        1,
        "°",
    );
    let mut opacity = t.video.opacity * 100.0;
    let before = opacity;
    row(ui, "Opacity", &mut opacity, 0.0..=100.0, 100.0, 0, "%");
    if opacity != before {
        t.video.opacity = opacity / 100.0;
    }

    if t != title {
        changed.set(true);
    }
    if changed.get() {
        // the name follows the text while it still has the automatic form
        if title.name == format!("Title: {}", title.text) {
            t.name = format!("Title: {}", t.text);
        }
        if let Some(op) = rename_op(&title, &t) {
            app.do_merging(op, &format!("t:{}", title.id.to_full_string()));
        }
    }
    if done.get() || ui.input(|i| i.pointer.any_released()) {
        app.end_merge();
    }
    animate_controls(app, ui, &title);
    crate::ui::effects_stack(app, ui, title.id, &title.effects, true, false);
}

/// `SetTitle` keeps the id and length; the name is part of the title content so it rides along.
fn rename_op(old: &Title, new: &Title) -> Option<Op> {
    (old != new).then(|| Op::SetTitle {
        item: old.id,
        title: new.clone(),
    })
}
