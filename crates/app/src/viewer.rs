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

            let tabs = Rect::from_min_size(
                pos2(head.right() - 420.0, head.top() + 3.0),
                vec2(180.0, 24.0),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(tabs), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if chip(ui, "Program", !app.source.open, true).clicked() {
                        app.source.open = false;
                    }
                    if chip(ui, "Source", app.source.open, true).clicked() {
                        app.source.open = true;
                        if app.source.media.is_none() {
                            app.source.media = app.selected_media;
                        }
                    }
                });
            });
            let btn = Rect::from_min_size(
                pos2(head.right() - 232.0, head.top() + 3.0),
                vec2(80.0, 24.0),
            );
            let toggled = ui
                .scope_builder(egui::UiBuilder::new().max_rect(btn), |ui| {
                    chip(ui, "Scopes", app.scopes_open, true).clicked()
                })
                .inner;
            if toggled {
                app.scopes_open = !app.scopes_open;
                app.scope_dirty = true;
                app.scope_src = None;
                app.last_requested = None; // draw the current frame again so there is something to read
            }

            let transport_h = 70.0;
            let area = Rect::from_min_max(
                pos2(full.left(), head.bottom()),
                pos2(full.right(), full.bottom() - transport_h),
            )
            .shrink(18.0);
            let (cw, ch) = (app.canvas().0 as f32, app.canvas().1 as f32);
            let scale = (area.width() / cw).min(area.height() / ch).max(0.01);
            let pic = Rect::from_center_size(area.center(), vec2(cw * scale, ch * scale));
            if app.source.open {
                let transport_rect =
                    Rect::from_min_max(pos2(full.left(), full.bottom() - transport_h), full.max);
                let p = ui.painter();
                p.rect_filled(transport_rect, 0.0, Color32::from_rgb(0x11, 0x11, 0x14));
                p.hline(
                    transport_rect.x_range(),
                    transport_rect.top(),
                    Stroke::new(1.0, theme::BORDER),
                );
                crate::source::show(app, ui, area, transport_rect);
                return;
            }
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
            // the captainE badge, faint, in the picture's bottom-right corner (viewer only: never rendered
            // into exports)
            if pic.width() > 160.0 {
                let side = 22.0;
                let badge = Rect::from_min_size(
                    pic.right_bottom() - vec2(side + 12.0, side + 12.0),
                    vec2(side, side),
                );
                let [r, g, b, _] = theme::ACCENT.to_array();
                ui.painter()
                    .rect_filled(badge, 5.0, Color32::from_rgba_unmultiplied(r, g, b, 150));
                ui.painter().text(
                    badge.center(),
                    Align2::CENTER_CENTER,
                    "cE",
                    ui_font(9.5),
                    Color32::from_white_alpha(210),
                );
            }
            if app.preview_override.is_some() {
                let r =
                    Rect::from_min_size(pic.left_bottom() + vec2(12.0, -34.0), vec2(190.0, 22.0));
                ui.painter().rect_filled(r, 4.0, theme::ACCENT);
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    "Previewing CoCaptain edit",
                    ui_font(11.5),
                    Color32::WHITE,
                );
            }
            if app.scopes_open {
                if app.scope_dirty {
                    app.refresh_scope(ui.ctx());
                }
                scopes_panel(app, ui, pic);
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
    // the timecode shrinks to fit the room to the left of the buttons
    let room = rect.center().x - 158.0 - 22.0 - (rect.left() + 16.0);
    let size = (room / (11.0 * 0.62)).min(26.0);
    // a narrow viewer (a side panel open) has no room for it; the timecode on the picture still shows
    if size >= 11.0 {
        p.text(
            pos2(rect.left() + 16.0, rect.center().y),
            Align2::LEFT_CENTER,
            app.tc(app.playhead),
            mono(size),
            Color32::WHITE,
        );
    }
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
    // in and out marks as bracket buttons either side of the transport; lit while a mark is set
    for (dx, icon, set, tip) in [
        (-158.0, Icon::MarkIn, app.mark_in.is_some(), "Mark In (I)"),
        (158.0, Icon::MarkOut, app.mark_out.is_some(), "Mark Out (O)"),
    ] {
        let r = at(dx, icon, 32.0, false);
        if set {
            r.ctx.layer_painter(r.layer_id).circle_stroke(
                r.rect.center(),
                15.5,
                Stroke::new(1.5, theme::ACCENT),
            );
        }
        if r.on_hover_text(tip).clicked() {
            if matches!(icon, Icon::MarkIn) {
                app.mark_in = Some(app.playhead);
            } else {
                app.mark_out = Some(app.playhead);
            }
        }
    }
    // the room right of the `]` button decides how much of the playback settings fits
    let room = rect.right() - (c.x + 158.0 + 16.0 + 12.0);
    if room >= 70.0 {
        let compact = room < 210.0;
        let w = if compact { 70.0 } else { 210.0 };
        let right = Rect::from_min_max(pos2(rect.right() - w, rect.top()), rect.max);
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(right.shrink2(vec2(8.0, 0.0))),
            |ui| {
                ui.horizontal_centered(|ui| {
                    if compact {
                        // one menu holds the resolution and the proxy switch
                        ui.menu_button(
                            format!("{} ▾", app.prefs.playback_quality.label()),
                            |ui| playback_menu(app, ui),
                        )
                        .response
                        .on_hover_text("Playback resolution and proxies");
                        return;
                    }
                    let sh = if app.playing { app.shuttle } else { 1.0 };
                    ui.label(
                        RichText::new(format!("{sh:.1}x"))
                            .font(mono(11.0))
                            .color(theme::MUTED),
                    );
                    // playback resolution, as in other editors: stopped frames are always full quality
                    let q = app.prefs.playback_quality;
                    let mut picked = q;
                    egui::ComboBox::from_id_salt("playback-quality")
                        .width(58.0)
                        .selected_text(q.label())
                        .show_ui(ui, |ui| {
                            for o in crate::keymap::PlaybackQuality::ALL {
                                ui.selectable_value(&mut picked, o, o.label());
                            }
                        })
                        .response
                        .on_hover_text(
                            "Playback resolution while playing. Auto: as sharp as the viewer can show (up to 1920 wide); \
                             1/2 and 1/4 of that for smoother playback; Full: the timeline's own size. \
                             When stopped, the frame is always made from the original footage at full quality.",
                        );
                    if picked != q {
                        app.prefs.playback_quality = picked;
                        let _ = app.prefs.save();
                    }
                    if chip(ui, "Proxy", app.prefs.use_proxies, true)
                        .on_hover_text("Play from proxy files (lighter, for slower machines); made if missing")
                        .clicked()
                    {
                        toggle_proxies(app);
                    }
                });
            },
        );
    }
    let _ = Sense::hover();
}

/// The scopes panel, floating at the top right of the picture.
fn scopes_panel(app: &mut App, ui: &mut egui::Ui, pic: Rect) {
    use crate::scopes::{stats, vector_pos, ScopeKind};
    let side = app.scope_side.clamp(200.0, 560.0);
    let size = vec2(side + 14.0, side + 14.0 + 34.0 + 22.0);
    let default = pic.right_top() + vec2(-size.x - 12.0, 12.0);
    let mut pos = app.scope_pos.unwrap_or(default);
    // keep it inside the picture area
    let area = pic.expand(40.0);
    pos.x = pos
        .x
        .clamp(area.left(), (area.right() - size.x).max(area.left()));
    pos.y = pos
        .y
        .clamp(area.top(), (area.bottom() - size.y).max(area.top()));
    let r = Rect::from_min_size(pos, size);
    ui.painter()
        .rect_filled(r, 6.0, Color32::from_rgba_unmultiplied(14, 14, 18, 235));
    // drag the window by its background; the corner resizes it
    let grab = ui.interact(r, egui::Id::new("scopes-move"), Sense::drag());
    if grab.dragged() {
        app.scope_pos = Some(pos + grab.drag_delta());
    }
    let corner = Rect::from_min_size(r.right_bottom() - vec2(16.0, 16.0), vec2(16.0, 16.0));
    let rz = ui.interact(corner, egui::Id::new("scopes-size"), Sense::drag());
    if rz.hovered() || rz.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
    }
    if rz.dragged() {
        let d = rz.drag_delta();
        app.scope_side = (app.scope_side + (d.x + d.y) / 2.0).clamp(200.0, 560.0);
    }
    for k in 0..3 {
        let o = 4.0 + k as f32 * 4.0;
        ui.painter().line_segment(
            [
                r.right_bottom() - vec2(o, 3.0),
                r.right_bottom() - vec2(3.0, o),
            ],
            Stroke::new(1.0, theme::FAINT),
        );
    }
    let tabs = Rect::from_min_size(r.min + vec2(6.0, 4.0), vec2(r.width() - 12.0, 26.0));
    let mut picked = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(tabs));
    child.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for k in ScopeKind::ALL {
            if chip(ui, k.label(), app.scope_kind == k, true).clicked() {
                picked = Some(k);
            }
        }
    });
    if let Some(k) = picked {
        app.scope_kind = k;
        app.scope_dirty = true;
    }
    let img = Rect::from_min_size(r.min + vec2(7.0, 34.0), vec2(side, side));
    if let Some(t) = &app.scope_tex {
        ui.painter().image(
            t.id(),
            img,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    // the scale, drawn sharp at any size
    let p = ui.painter().with_clip_rect(r);
    let faint = Color32::from_white_alpha(110);
    match app.scope_kind {
        ScopeKind::Waveform | ScopeKind::Parade => {
            for (i, label) in ["100", "75", "50", "25", "0"].into_iter().enumerate() {
                let y = img.top() + img.height() * i as f32 / 4.0;
                p.text(
                    pos2(img.left() + 3.0, (y + 1.0).min(img.bottom() - 7.0)),
                    Align2::LEFT_CENTER,
                    label,
                    mono(9.0),
                    faint,
                );
            }
        }
        ScopeKind::Histogram => {
            for (i, label) in ["0", "64", "128", "192", "255"].into_iter().enumerate() {
                let x = img.left() + img.width() * i as f32 / 4.0;
                p.text(
                    pos2(
                        x.clamp(img.left() + 8.0, img.right() - 10.0),
                        img.bottom() - 6.0,
                    ),
                    Align2::CENTER_CENTER,
                    label,
                    mono(9.0),
                    faint,
                );
            }
        }
        ScopeKind::Vectorscope => {
            let c = img.center();
            let rad = img.width() / 2.0;
            for f in [0.5f32, 1.0] {
                p.circle_stroke(
                    c,
                    rad * f * 0.98,
                    Stroke::new(1.0, Color32::from_white_alpha(35)),
                );
            }
            // skin tones sit along one line out from the middle
            let (sx, sy) = vector_pos(1.0, 0.72, 0.58);
            let end = pos2(
                img.left() + (c.x - img.left()) + (sx - 0.5) * img.width() * 2.2,
                img.top() + (c.y - img.top()) + (sy - 0.5) * img.height() * 2.2,
            );
            p.line_segment(
                [c, end],
                Stroke::new(1.0, Color32::from_rgb(0xd9, 0xa7, 0x7e)),
            );
            // the six colour bars at 75 % saturation
            for (name, (r, g, b)) in [
                ("R", (0.75, 0.0, 0.0)),
                ("Y", (0.75, 0.75, 0.0)),
                ("G", (0.0, 0.75, 0.0)),
                ("C", (0.0, 0.75, 0.75)),
                ("B", (0.0, 0.0, 0.75)),
                ("M", (0.75, 0.0, 0.75)),
            ] {
                let (x, y) = vector_pos(r, g, b);
                let at = pos2(img.left() + x * img.width(), img.top() + y * img.height());
                p.rect_stroke(
                    Rect::from_center_size(at, vec2(14.0, 14.0)),
                    2.0,
                    Stroke::new(1.0, Color32::from_white_alpha(120)),
                    egui::StrokeKind::Middle,
                );
                p.text(
                    at,
                    Align2::CENTER_CENTER,
                    name,
                    mono(8.5),
                    Color32::from_white_alpha(150),
                );
            }
        }
    }
    // numbers under the picture: how much is blown out or crushed
    if let Some(src) = &app.scope_src {
        let s = stats(src);
        let line = format!(
            "highlights clipped {:.1}%   blacks crushed {:.1}%   average {:.0}%",
            s.clipped_high, s.clipped_low, s.mean_luma
        );
        let warn = s.clipped_high > 1.0 || s.clipped_low > 5.0;
        p.text(
            pos2(r.left() + 8.0, r.bottom() - 11.0),
            Align2::LEFT_CENTER,
            line,
            mono(9.5),
            if warn {
                Color32::from_rgb(0xf0, 0x8c, 0x3a)
            } else {
                theme::MUTED
            },
        );
    }
}

/// The playback settings as a menu (for a narrow viewer).
fn playback_menu(app: &mut App, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("Playback resolution")
            .color(theme::MUTED)
            .size(11.0),
    );
    ui.label(
        RichText::new("While playing. A paused frame is always full quality.")
            .color(theme::FAINT)
            .size(10.5),
    );
    for o in crate::keymap::PlaybackQuality::ALL {
        if ui
            .selectable_label(app.prefs.playback_quality == o, o.label())
            .clicked()
        {
            app.prefs.playback_quality = o;
            let _ = app.prefs.save();
            ui.close();
        }
    }
    ui.separator();
    if ui
        .selectable_label(app.prefs.use_proxies, "Play from proxies")
        .clicked()
    {
        toggle_proxies(app);
        ui.close();
    }
}

/// Turn proxy playback on or off; turning it on makes the proxies that are missing.
fn toggle_proxies(app: &mut App) {
    app.prefs.use_proxies = !app.prefs.use_proxies;
    let _ = app.prefs.save();
    if app.prefs.use_proxies {
        let missing: Vec<captaine_model::Id> = app
            .project()
            .media
            .values()
            .filter(|m| m.has_video && m.proxy.is_none() && m.nested_timeline().is_none())
            .map(|m| m.id)
            .collect();
        if !missing.is_empty() {
            app.make_proxies(&missing, app.prefs.proxy_height);
        }
    }
}
