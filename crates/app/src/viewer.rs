//! The program viewer: the picture, the timecode chip, the selection outline and the transport bar.

use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke};

use crate::app::App;
use crate::theme::{self, mono, ui_font};
use crate::ui::{chip, icon_button, Icon};

pub fn central(app: &mut App, root: &mut egui::Ui) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::VIEWER_BG))
        .show(root, |ui| {
            let full = ui.max_rect();
            app.central_rect = full;
            // header
            let head = Rect::from_min_size(full.min, vec2(full.width(), 30.0));
            let p = ui.painter();
            p.rect_filled(head, 0.0, theme::PANEL_DARK);
            p.hline(
                full.x_range(),
                head.bottom(),
                Stroke::new(1.0, theme::BORDER),
            );
            let tl_name = app.timeline().map_or(String::new(), |t| t.name.clone());
            p.text(
                pos2(head.left() + 14.0, head.center().y),
                Align2::LEFT_CENTER,
                format!("Program · {tl_name}"),
                ui_font(12.0),
                Color32::WHITE,
            );
            let s = &app.project().settings;
            p.text(
                pos2(head.right() - 14.0, head.center().y),
                Align2::RIGHT_CENTER,
                format!("{} × {} · Fit", s.width, s.height),
                mono(11.0),
                theme::MUTED,
            );

            let transport_h = 70.0;
            let area = Rect::from_min_max(
                pos2(full.left(), head.bottom()),
                pos2(full.right(), full.bottom() - transport_h),
            )
            .shrink(18.0);
            let (cw, ch) = (s.width as f32, s.height as f32);
            let scale = (area.width() / cw).min(area.height() / ch).max(0.01);
            let pic = Rect::from_center_size(area.center(), vec2(cw * scale, ch * scale));
            let p = ui.painter();
            p.rect_filled(pic, 0.0, Color32::BLACK);
            if let Some(tex) = &app.program {
                p.image(
                    tex.id(),
                    pic,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            if app.project().media.is_empty() {
                p.text(
                    pic.center(),
                    Align2::CENTER_CENTER,
                    "Import media to begin\nDrop files here or press ⌘I",
                    ui_font(13.0),
                    theme::MUTED,
                );
            }
            selection_outline(app, ui, pic, scale);
            // timecode chip
            let tc = app.tc(app.playhead);
            let galley = ui.painter().layout_no_wrap(tc, mono(11.5), Color32::WHITE);
            let chip_rect =
                Rect::from_min_size(pic.min + vec2(12.0, 12.0), galley.size() + vec2(14.0, 8.0));
            ui.painter().rect_filled(
                chip_rect,
                4.0,
                Color32::from_rgba_unmultiplied(20, 20, 24, 215),
            );
            ui.painter()
                .galley(chip_rect.min + vec2(7.0, 4.0), galley, Color32::WHITE);
            if app.preview_override.is_some() {
                let r =
                    Rect::from_min_size(pic.left_bottom() + vec2(12.0, -34.0), vec2(190.0, 22.0));
                ui.painter().rect_filled(r, 4.0, theme::ACCENT);
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    "Previewing Copilot edit",
                    ui_font(11.5),
                    Color32::WHITE,
                );
            }
            transport(
                app,
                ui,
                Rect::from_min_max(pos2(full.left(), full.bottom() - transport_h), full.max),
            );
        });
}

/// Dashed outline of the selected clip's picture, following its transform.
fn selection_outline(app: &App, ui: &mut egui::Ui, pic: Rect, scale: f32) {
    let Some((clip, _, start)) = app.selected() else {
        return;
    };
    let ph = app.playhead;
    if ph < start || ph >= start + clip.duration.value {
        return;
    }
    let Some(m) = app
        .project()
        .media
        .get(&clip.media)
        .filter(|m| m.has_video && m.width > 0)
    else {
        return;
    };
    let s = &app.project().settings;
    let (cw, ch) = (s.width as f64, s.height as f64);
    let fit = (cw / m.width as f64).min(ch / m.height as f64);
    let t = &clip.video.transform;
    let (hw, hh) = (
        m.width as f64 * fit * t.scale.0 / 2.0,
        m.height as f64 * fit * t.scale.1 / 2.0,
    );
    let (sin, cos) = t.rotation_deg.to_radians().sin_cos();
    let to_screen = |x: f64, y: f64| {
        let (rx, ry) = (x * cos - y * sin, x * sin + y * cos);
        let (px, py) = (cw / 2.0 + t.position.0 + rx, ch / 2.0 + t.position.1 + ry);
        pos2(
            pic.left() + px as f32 * scale,
            pic.top() + py as f32 * scale,
        )
    };
    let pts = [
        to_screen(-hw, -hh),
        to_screen(hw, -hh),
        to_screen(hw, hh),
        to_screen(-hw, hh),
        to_screen(-hw, -hh),
    ];
    let painter = ui.painter().with_clip_rect(pic);
    painter.extend(egui::Shape::dashed_line(
        &pts,
        Stroke::new(1.0, Color32::from_white_alpha(150)),
        5.0,
        4.0,
    ));
}

fn transport(app: &mut App, ui: &mut egui::Ui, rect: Rect) {
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::from_rgb(0x11, 0x11, 0x14));
    p.hline(rect.x_range(), rect.top(), Stroke::new(1.0, theme::BORDER));
    p.text(
        pos2(rect.left() + 16.0, rect.center().y),
        Align2::LEFT_CENTER,
        app.tc(app.playhead),
        mono(26.0),
        Color32::WHITE,
    );
    let c = rect.center();
    let mut at = |dx: f32, icon: Icon, d: f32, primary: bool| {
        let r = Rect::from_center_size(pos2(c.x + dx, c.y), vec2(d, d));
        ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
            icon_button(ui, icon, d, primary)
        })
        .inner
    };
    if at(-108.0, Icon::ToStart, 32.0, false)
        .on_hover_text("Go to start (Home)")
        .clicked()
    {
        app.set_playhead(0);
    }
    if at(-62.0, Icon::Back, 32.0, false)
        .on_hover_text("Previous frame (←)")
        .clicked()
    {
        app.set_playhead(app.playhead - 1);
    }
    let main = if app.playing { Icon::Pause } else { Icon::Play };
    if at(0.0, main, 42.0, true)
        .on_hover_text("Play / pause (Space)")
        .clicked()
    {
        app.toggle_play();
    }
    if at(62.0, Icon::Forward, 32.0, false)
        .on_hover_text("Next frame (→)")
        .clicked()
    {
        app.set_playhead(app.playhead + 1);
    }
    if at(108.0, Icon::ToEnd, 32.0, false)
        .on_hover_text("Go to end (End)")
        .clicked()
    {
        app.set_playhead(app.duration_ticks());
    }
    let right = Rect::from_min_max(pos2(rect.right() - 250.0, rect.top()), rect.max);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(right.shrink2(vec2(16.0, 0.0))),
        |ui| {
            ui.horizontal_centered(|ui| {
                if chip(ui, "Mark In", app.mark_in.is_some(), true)
                    .on_hover_text("I")
                    .clicked()
                {
                    app.mark_in = Some(app.playhead);
                }
                if chip(ui, "Mark Out", app.mark_out.is_some(), true)
                    .on_hover_text("O")
                    .clicked()
                {
                    app.mark_out = Some(app.playhead);
                }
                let sh = if app.playing { app.shuttle } else { 1.0 };
                ui.label(
                    RichText::new(format!("{sh:.1}x"))
                        .font(mono(11.0))
                        .color(theme::MUTED),
                );
            });
        },
    );
    let _ = Sense::hover();
}
