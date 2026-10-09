//! Shared widgets and the non-timeline panels: top bar, media pool, inspector, dialogs, toasts, shortcuts.

use std::sync::{Arc, Mutex};

use captaine_export::Codec;
use captaine_model::*;
use captaine_ops::{edit, Op};
use captaine_time::{Rational, RationalTime};
use egui::{
    pos2, vec2, Align, Align2, Color32, Context, Key, Layout, Modifiers, Rect, Response, RichText,
    Sense, Stroke, Ui,
};

use crate::app::*;
use crate::theme::{self, mono, ui_font};

// ---- small widgets -----------------------------------------------------------------------------

/// A text tab with an accent underline when active.
pub fn tab(ui: &mut Ui, label: &str, active: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        ui_font(12.5),
        if active { Color32::WHITE } else { theme::MUTED },
    );
    let size = vec2(galley.size().x + 22.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, active, label)
    });
    let color = if active {
        Color32::WHITE
    } else if resp.hovered() {
        theme::TEXT
    } else {
        theme::MUTED
    };
    ui.painter().galley(
        pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    if active {
        ui.painter().rect_filled(
            Rect::from_min_size(
                pos2(rect.left() + 10.0, rect.bottom() - 2.0),
                vec2(rect.width() - 20.0, 2.0),
            ),
            1.0,
            theme::ACCENT,
        );
    }
    resp
}

/// A small rounded button; `active` paints it in the accent colour.
pub fn chip(ui: &mut Ui, label: &str, active: bool, enabled: bool) -> Response {
    let color = if !enabled {
        theme::FAINT
    } else if active {
        Color32::WHITE
    } else {
        theme::TEXT
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), ui_font(12.0), color);
    let size = vec2(galley.size().x + 22.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, label)
    });
    let fill = if active {
        theme::ACCENT
    } else if resp.hovered() && enabled {
        rgb_lift(theme::FIELD)
    } else {
        theme::FIELD
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    if !active {
        ui.painter().rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, theme::BORDER),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

fn rgb_lift(c: Color32) -> Color32 {
    Color32::from_rgb(
        c.r().saturating_add(14),
        c.g().saturating_add(14),
        c.b().saturating_add(16),
    )
}

/// Text-only toggle, as used for Snap / Link / Markers.
pub fn toggle_text(ui: &mut Ui, label: &str, on: bool, enabled: bool) -> Response {
    let color = if !enabled {
        theme::FAINT
    } else if on {
        Color32::WHITE
    } else {
        theme::MUTED
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), ui_font(12.0), color);
    let (rect, resp) = ui.allocate_exact_size(
        vec2(galley.size().x + 16.0, 24.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, label));
    if on {
        ui.painter()
            .rect_filled(rect.shrink2(vec2(2.0, 3.0)), 4.0, theme::ACCENT_SOFT);
    } else if resp.hovered() && enabled {
        ui.painter()
            .rect_filled(rect.shrink2(vec2(2.0, 3.0)), 4.0, theme::FIELD);
    }
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

#[derive(Clone, Copy)]
pub enum Icon {
    ToStart,
    Back,
    Play,
    Pause,
    Forward,
    ToEnd,
}

pub fn icon_button(ui: &mut Ui, icon: Icon, diameter: f32, primary: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(diameter, diameter), Sense::click());
    let p = ui.painter();
    let fill = if primary {
        if resp.hovered() {
            theme::ACCENT_HOVER
        } else {
            theme::ACCENT
        }
    } else if resp.hovered() {
        rgb_lift(theme::FIELD)
    } else {
        theme::FIELD
    };
    p.circle_filled(rect.center(), diameter / 2.0, fill);
    if !primary {
        p.circle_stroke(
            rect.center(),
            diameter / 2.0 - 0.5,
            Stroke::new(1.0, theme::BORDER),
        );
    }
    let c = rect.center();
    let s = diameter * 0.17;
    let ink = Color32::WHITE;
    let tri = |dx: f32, dir: f32| {
        p.add(egui::Shape::convex_polygon(
            vec![
                pos2(c.x + dx - dir * s, c.y - s * 1.2),
                pos2(c.x + dx - dir * s, c.y + s * 1.2),
                pos2(c.x + dx + dir * s, c.y),
            ],
            ink,
            Stroke::NONE,
        ));
    };
    let bar = |dx: f32| {
        p.rect_filled(
            Rect::from_center_size(pos2(c.x + dx, c.y), vec2(1.6, s * 2.4)),
            0.5,
            ink,
        );
    };
    match icon {
        Icon::Play => tri(s * 0.35, 1.0),
        Icon::Pause => {
            bar(-s * 0.6);
            bar(s * 0.6);
        }
        Icon::Back => tri(0.0, -1.0),
        Icon::Forward => tri(0.0, 1.0),
        Icon::ToStart => {
            bar(-s);
            tri(s * 0.4, -1.0);
        }
        Icon::ToEnd => {
            bar(s);
            tri(-s * 0.4, 1.0);
        }
    }
    resp
}

pub struct SliderOut {
    pub changed: bool,
    pub done: bool,
}

/// Keyframe toggle: filled on a key at the playhead, outlined when animated, faint otherwise.
fn diamond(
    ui: &mut Ui,
    label: &str,
    animated: bool,
    on_key: bool,
    enabled: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        vec2(16.0, 22.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            enabled,
            format!("Keyframe {label}"),
        )
    });
    let c = rect.center();
    let r = if resp.hovered() && enabled { 5.5 } else { 4.5 };
    let pts = vec![
        pos2(c.x, c.y - r),
        pos2(c.x + r, c.y),
        pos2(c.x, c.y + r),
        pos2(c.x - r, c.y),
    ];
    let col = if animated {
        theme::ACCENT
    } else {
        theme::FAINT
    };
    let fill = if on_key { col } else { Color32::TRANSPARENT };
    ui.painter().add(egui::Shape::convex_polygon(
        pts,
        fill,
        Stroke::new(1.2, col),
    ));
    resp
}

/// `label  ───●───  value`: drag the bar, drag/type the number, double-click the bar to reset.
#[allow(clippy::too_many_arguments)]
pub fn slider_row(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    default: f64,
    decimals: usize,
    suffix: &str,
    enabled: bool,
) -> SliderOut {
    let mut out = SliderOut {
        changed: false,
        done: false,
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_sized(
            [86.0, 22.0],
            egui::Label::new(
                RichText::new(label)
                    .color(if enabled { theme::TEXT } else { theme::FAINT })
                    .size(12.0),
            )
            .truncate(),
        );
        let w = (ui.available_width() - 84.0).max(40.0);
        let (rect, resp) = ui.allocate_exact_size(
            vec2(w, 22.0),
            if enabled {
                Sense::click_and_drag()
            } else {
                Sense::hover()
            },
        );
        let (lo, hi) = (*range.start(), *range.end());
        let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
        let (x0, x1, y) = (rect.left() + 6.0, rect.right() - 6.0, rect.center().y);
        let p = ui.painter();
        p.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(3.0, theme::FIELD));
        let x = x0 + (x1 - x0) * t;
        let fill = if enabled { theme::ACCENT } else { theme::FAINT };
        p.line_segment([pos2(x0, y), pos2(x, y)], Stroke::new(3.0, fill));
        p.circle_filled(
            pos2(x, y),
            5.5,
            if enabled {
                Color32::WHITE
            } else {
                theme::MUTED
            },
        );
        if enabled {
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let nt = ((pos.x - x0) / (x1 - x0)).clamp(0.0, 1.0) as f64;
                    let v = lo + (hi - lo) * nt;
                    if (v - *value).abs() > f64::EPSILON {
                        *value = v;
                        out.changed = true;
                    }
                }
            }
            if resp.double_clicked() {
                *value = default;
                out.changed = true;
            }
            if resp.drag_stopped() || resp.clicked() || resp.double_clicked() {
                out.done = true;
            }
        }
        // a flat, right-aligned number: no box until it is hovered
        let speed = if enabled { (hi - lo) / 400.0 } else { 0.0 };
        let r = ui
            .scope(|ui| {
                let w = &mut ui.style_mut().visuals.widgets;
                for st in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                    st.bg_fill = Color32::TRANSPARENT;
                    st.weak_bg_fill = Color32::TRANSPARENT;
                    st.bg_stroke = Stroke::NONE;
                }
                w.hovered.bg_stroke = Stroke::new(1.0, theme::BORDER);
                ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [60.0, 22.0],
                        egui::DragValue::new(value)
                            .range(range.clone())
                            .fixed_decimals(decimals)
                            .suffix(suffix)
                            .speed(speed),
                    )
                })
                .inner
            })
            .inner;
        if r.changed() {
            out.changed = true;
        }
        if r.drag_stopped() || r.lost_focus() {
            out.done = true;
        }
    });
    out
}

// ---- top bar -----------------------------------------------------------------------------------

pub fn top_bar(app: &mut App, root: &mut Ui) {
    egui::Panel::top("top_bar")
        .exact_size(40.0)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL_DARK)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .inner_margin(egui::Margin::symmetric(12, 0)),
        )
        .show(root, |ui| {
            let full = ui.max_rect();
            ui.horizontal_centered(|ui| {
                let (logo, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                ui.painter().rect_filled(logo, 6.0, theme::ACCENT);
                ui.painter().text(
                    logo.center(),
                    Align2::CENTER_CENTER,
                    "cE",
                    ui_font(10.5),
                    Color32::WHITE,
                );
                ui.label(RichText::new("captainE").strong().size(14.0));
                ui.add_space(10.0);
                menus(app, ui);
            });
            // page tabs, centred on the whole bar
            let tabs_w = 6.0 * 64.0;
            let tabs_rect = Rect::from_min_size(
                pos2(full.center().x - tabs_w / 2.0, full.top() + 6.0),
                vec2(tabs_w, 28.0),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(tabs_rect), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (page, label) in [
                        (Page::Media, "Media"),
                        (Page::Cut, "Cut"),
                        (Page::Edit, "Edit"),
                        (Page::Color, "Color"),
                        (Page::Effects, "Effects"),
                        (Page::Deliver, "Deliver"),
                    ] {
                        if tab(ui, label, app.page == page)
                            .on_hover_text(if matches!(page, Page::Edit | Page::Deliver) {
                                ""
                            } else {
                                "Not built yet"
                            })
                            .clicked()
                        {
                            match page {
                                Page::Edit => app.page = Page::Edit,
                                Page::Deliver => app.export.open = true,
                                _ => app.toast(
                                    format!("The {label} page is planned for a later phase"),
                                    false,
                                ),
                            }
                        }
                    }
                });
            });
            ui.scope_builder(egui::UiBuilder::new().max_rect(full), |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Render").strong().color(Color32::WHITE),
                            )
                            .fill(theme::ACCENT)
                            .min_size(vec2(72.0, 26.0)),
                        )
                        .clicked()
                    {
                        app.export.open = true;
                    }
                    let label = RichText::new("Copilot")
                        .size(12.0)
                        .color(if app.show_copilot {
                            Color32::WHITE
                        } else {
                            theme::TEXT
                        });
                    let r = ui.add(
                        egui::Button::new(label)
                            .fill(if app.show_copilot {
                                theme::ACCENT_SOFT
                            } else {
                                theme::FIELD
                            })
                            .min_size(vec2(84.0, 26.0)),
                    );
                    let dot = pos2(r.rect.left() + 14.0, r.rect.center().y);
                    ui.painter().circle_filled(dot, 3.5, theme::OK);
                    if r.clicked() {
                        app.show_copilot = !app.show_copilot;
                    }
                    ui.add_space(8.0);
                    let pill = crate::branches_ui::pill(app, ui);
                    app.branch_pill = pill.rect;
                    if pill.clicked() {
                        app.branch_menu = !app.branch_menu;
                    }
                    ui.add_space(8.0);
                    let rate = app.rate();
                    ui.label(
                        RichText::new(format!("{} · {} fps", app.project_title(), fps_label(rate)))
                            .font(mono(11.5))
                            .color(theme::MUTED),
                    );
                });
            });
        });
}

pub fn fps_label(r: Rational) -> String {
    let f = r.as_f64();
    if (f - f.round()).abs() < 1e-9 {
        format!("{}", f as i64)
    } else {
        format!("{f:.3}").trim_end_matches('0').to_string()
    }
}

fn menus(app: &mut App, ui: &mut Ui) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.menu_button("File", |ui| {
            if ui.button("New Project…").clicked() {
                app.new_project = Some(("Untitled".into(), 1920, 1080, "30".into()));
                ui.close();
            }
            if ui.button("Open Project…").clicked() {
                ui.close();
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("captainE project", &["json"])
                    .pick_file()
                {
                    crate::open_project(app, p);
                }
            }
            if ui.button("Import Media…   ⌘I").clicked() {
                ui.close();
                import_dialog(app);
            }
            ui.separator();
            if ui.button("Export…   ⌘E").clicked() {
                app.export.open = true;
                ui.close();
            }
            ui.menu_button("Export Timeline As", |ui| {
                for (label, ext) in [
                    ("OpenTimelineIO (.otio)", "otio"),
                    ("Edit Decision List (.edl)", "edl"),
                    ("Final Cut Pro XML (.fcpxml)", "fcpxml"),
                ] {
                    if ui.button(label).clicked() {
                        ui.close();
                        export_interchange(app, ext);
                    }
                }
            });
            if ui.button("Import Timeline…").clicked() {
                ui.close();
                import_interchange(app);
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui
                .add_enabled(
                    app.session.editor.can_undo(),
                    egui::Button::new("Undo   ⌘Z"),
                )
                .clicked()
            {
                app.undo();
                ui.close();
            }
            if ui
                .add_enabled(
                    app.session.editor.can_redo(),
                    egui::Button::new("Redo   ⇧⌘Z"),
                )
                .clicked()
            {
                app.redo();
                ui.close();
            }
        });
        ui.menu_button("Trim", |ui| {
            let sel = app.selected();
            if ui
                .add_enabled(sel.is_some(), egui::Button::new("Trim Start to Playhead"))
                .clicked()
            {
                trim_to_playhead(app, captaine_ops::Edge::In);
                ui.close();
            }
            if ui
                .add_enabled(sel.is_some(), egui::Button::new("Trim End to Playhead"))
                .clicked()
            {
                trim_to_playhead(app, captaine_ops::Edge::Out);
                ui.close();
            }
        });
        ui.menu_button("Timeline", |ui| {
            if ui.button("Split at Playhead   ⌘B").clicked() {
                split_at_playhead(app);
                ui.close();
            }
            ui.separator();
            if ui.button("Lift   ⌫").clicked() {
                lift_selection(app);
                ui.close();
            }
            if ui.button("Extract   ⇧⌫").clicked() {
                extract_selection(app);
                ui.close();
            }
            ui.separator();
            if ui.button("Add Video Track").clicked() {
                add_track(app, TrackKind::Video);
                ui.close();
            }
            if ui.button("Add Audio Track").clicked() {
                add_track(app, TrackKind::Audio);
                ui.close();
            }
            ui.separator();
            if ui.button("Clean Up Gaps").clicked() {
                tidy_gaps(app);
                ui.close();
            }
        });
        ui.menu_button("Clip", |ui| {
            let has = app.selected().is_some();
            if ui
                .add_enabled(has, egui::Button::new("Enable / Disable"))
                .clicked()
            {
                toggle_enabled(app);
                ui.close();
            }
            if ui
                .add_enabled(has, egui::Button::new("Reset Transform and Color"))
                .clicked()
            {
                if let Some((c, ..)) = app.selected() {
                    app.do_op(Op::SetVideo {
                        item: c.id,
                        video: VideoProps {
                            opacity: c.video.opacity,
                            blend: c.video.blend,
                            ..VideoProps::default()
                        },
                    });
                }
                ui.close();
            }
        });
        ui.menu_button("Mark", |ui| {
            if ui.button("Mark In   I").clicked() {
                app.mark_in = Some(app.playhead);
                ui.close();
            }
            if ui.button("Mark Out   O").clicked() {
                app.mark_out = Some(app.playhead);
                ui.close();
            }
            if ui.button("Clear In and Out   ⌥X").clicked() {
                app.mark_in = None;
                app.mark_out = None;
                ui.close();
            }
        });
        ui.menu_button("View", |ui| {
            if ui.button("Zoom In   =").clicked() {
                app.pps = (app.pps * 1.25).min(600.0);
                ui.close();
            }
            if ui.button("Zoom Out   -").clicked() {
                app.pps = (app.pps / 1.25).max(1.0);
                ui.close();
            }
            if ui.button("Fit Timeline   ⇧Z").clicked() {
                crate::timeline::fit(app);
                ui.close();
            }
            ui.separator();
            if ui.button("Copilot   ⌘J").clicked() {
                app.show_copilot = !app.show_copilot;
                ui.close();
            }
        });
        ui.menu_button("Help", |ui| {
            if ui.button("Keyboard Shortcuts   ?").clicked() {
                app.show_shortcuts = true;
                ui.close();
            }
        });
    });
}

/// Write the current timeline in an interchange format other editors read.
fn export_interchange(app: &mut App, ext: &str) {
    let Some(tl) = app.timeline_id() else { return };
    let text = match ext {
        "otio" => captaine_interchange::otio::export(app.project(), tl),
        "edl" => captaine_interchange::edl::export(app.project(), tl, false),
        _ => captaine_interchange::fcpxml::export(app.project(), tl),
    };
    let text = match text {
        Ok(t) => t,
        Err(e) => return app.toast(e, true),
    };
    if let Some(path) = rfd::FileDialog::new()
        .set_file_name(format!("{}.{ext}", app.project_title()))
        .add_filter("timeline", &[ext])
        .save_file()
    {
        match std::fs::write(&path, text) {
            Ok(()) => app.toast(format!("Wrote {}", path.display()), false),
            Err(e) => app.toast(format!("cannot write {}: {e}", path.display()), true),
        }
    }
}

/// Read a timeline from an OTIO or EDL file: OTIO becomes a new timeline, an EDL is laid onto the current one.
fn import_interchange(app: &mut App) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("timeline", &["otio", "edl"])
        .pick_file()
    else {
        return;
    };
    app.import_timeline_file(&path);
}

pub fn import_dialog(app: &mut App) {
    if let Some(files) = rfd::FileDialog::new()
        .add_filter(
            "media",
            &[
                "mp4", "mov", "m4v", "mkv", "avi", "webm", "mxf", "wav", "mp3", "m4a", "aac",
                "flac", "aif", "aiff",
            ],
        )
        .pick_files()
    {
        app.import(files);
    }
}

// ---- editing commands shared by menus, keys and buttons ------------------------------------------

/// Hold the frame under the playhead for two seconds: cut the clip there and slot in a still.
pub fn freeze_frame(app: &mut App) {
    let Some((clip, track, start)) = app.selected() else {
        app.toast(
            "Select a clip first, then put the playhead on the frame to hold",
            false,
        );
        return;
    };
    let local = app.playhead - start;
    if local < 0 || local >= clip.duration.value {
        app.toast("Move the playhead inside the selected clip", false);
        return;
    }
    if !app
        .project()
        .media
        .get(&clip.media)
        .is_some_and(|m| m.has_video)
    {
        app.toast("Only video clips can be frozen", false);
        return;
    }
    let Some(tl) = app.timeline() else { return };
    let Some(index) = tl
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.items.iter().position(|i| i.id() == clip.id))
    else {
        return;
    };
    let rate = clip.duration.rate;
    let mut still = Clip::new(
        Id::new(),
        format!("{} (freeze)", clip.name),
        clip.media,
        RationalTime::new(clip.source_frame_at(local), clip.source_start.rate),
        RationalTime::new(((2.0 * app.fps()).round() as i64).max(1), rate),
    );
    still.retime = Retime {
        freeze: true,
        ..Retime::default()
    };
    still.video = clip.video_at(local);
    let mut ops = Vec::new();
    let at = if local > 0 {
        ops.push(Op::Split {
            item: clip.id,
            at: RationalTime::new(local, rate),
            new_id: Id::new(),
        });
        index + 1
    } else {
        index
    };
    ops.push(Op::InsertItem {
        track,
        index: at,
        item: Item::Clip(still),
    });
    app.do_op(Op::Transaction {
        description: "Freeze frame".into(),
        ops,
    });
}

/// Cut the selected clip wherever the picture changes abruptly.
pub fn split_at_scenes(app: &mut App) {
    let Some((clip, ..)) = app.selected() else {
        app.toast("Select a video clip first", false);
        return;
    };
    let Some(media) = app.project().media.get(&clip.media).cloned() else {
        return;
    };
    match captaine_agent::split_at_scenes(&clip, &media, app.fps(), 0.3, 0.5) {
        Ok((ops, _)) if ops.is_empty() => app.toast("No scene changes found in this clip", false),
        Ok((ops, secs)) => {
            let n = secs.len();
            app.do_op(Op::Transaction {
                description: "Split at scene changes".into(),
                ops,
            });
            app.toast(
                format!("Cut at {n} scene change{}", if n == 1 { "" } else { "s" }),
                false,
            );
        }
        Err(e) => app.toast(e, true),
    }
}

pub fn split_at_playhead(app: &mut App) {
    let Some(tl) = app.timeline().cloned() else {
        return;
    };
    let at = app.playhead;
    let selected = app.selection_tracks();
    let tracks: Vec<Id> = if selected.is_empty() {
        tl.tracks.iter().map(|t| t.id).collect()
    } else {
        selected
    };
    let mut cuts = Vec::new();
    for t in tracks {
        if let Ok(op) = edit::split_at(app.project(), t, app.time(at)) {
            cuts.push(op);
        }
    }
    match cuts.len() {
        0 => app.toast(
            "Nothing to cut here: move the playhead inside a clip",
            false,
        ),
        1 => {
            app.do_op(cuts.remove(0));
        }
        _ => {
            app.do_op(Op::Transaction {
                description: "Split at playhead".into(),
                ops: cuts,
            });
        }
    }
}

pub fn lift_selection(app: &mut App) {
    if app.selection_ids().len() > 1 {
        app.delete_selection(false);
        return;
    }
    if let (Some(a), Some(b)) = (app.mark_in, app.mark_out) {
        if app.selected_clip.is_none() {
            let Some(tl) = app.timeline_id() else { return };
            let (s, e) = (app.time(a.min(b)), app.time(a.max(b)));
            app.do_plan(|p| edit::lift_range(p, tl, s, e));
            return;
        }
    }
    if let Some(id) = app.selected_clip {
        if app.do_plan(|p| edit::lift(p, id)) {
            app.selected_clip = None;
        }
    }
}

pub fn extract_selection(app: &mut App) {
    if app.selection_ids().len() > 1 {
        app.delete_selection(true);
        return;
    }
    if let (Some(a), Some(b)) = (app.mark_in, app.mark_out) {
        if app.selected_clip.is_none() {
            let Some(tl) = app.timeline_id() else { return };
            let (s, e) = (app.time(a.min(b)), app.time(a.max(b)));
            app.do_plan(|p| edit::extract_range(p, tl, s, e));
            return;
        }
    }
    if let Some(id) = app.selected_clip {
        if app.do_op(Op::RemoveItem { item: id }) {
            app.selected_clip = None;
        }
    }
}

pub fn trim_to_playhead(app: &mut App, edge: captaine_ops::Edge) {
    let Some((item, _, start, len)) = app.selected_span() else {
        return;
    };
    let end = start + len;
    let delta = match edge {
        captaine_ops::Edge::In => app.playhead - start,
        captaine_ops::Edge::Out => app.playhead - end,
    };
    if delta == 0 || app.playhead <= start || app.playhead >= end {
        app.toast("Put the playhead inside the selected clip first", false);
        return;
    }
    let delta = app.time(delta);
    app.do_op(Op::Trim { item, edge, delta });
}

pub fn add_track(app: &mut App, kind: TrackKind) {
    let Some(tl) = app.timeline().cloned() else {
        return;
    };
    let n = tl.tracks.iter().filter(|t| t.kind == kind).count() + 1;
    let name = format!("{}{n}", if kind == TrackKind::Video { "V" } else { "A" });
    // video tracks stack above the existing video tracks; audio tracks go after the last audio track
    let index = match kind {
        TrackKind::Video => tl
            .tracks
            .iter()
            .rposition(|t| t.kind == TrackKind::Video)
            .map_or(0, |i| i + 1),
        TrackKind::Audio => tl.tracks.len(),
    };
    app.do_op(Op::AddTrack {
        timeline: tl.id,
        index,
        track: Track::new(Id::new(), name, kind),
    });
}

pub fn tidy_gaps(app: &mut App) {
    let Some(tl) = app.timeline().cloned() else {
        return;
    };
    let ops: Vec<Op> = tl
        .tracks
        .iter()
        .filter_map(|t| edit::clean_gaps(app.project(), t.id).ok().flatten())
        .collect();
    if ops.is_empty() {
        app.toast("No empty gaps to clean up", false);
    } else {
        app.do_op(Op::Transaction {
            description: "Clean up gaps".into(),
            ops,
        });
    }
}

pub fn toggle_enabled(app: &mut App) {
    let Some(id) = app.selected_clip else { return };
    let current = app
        .selected()
        .map(|(c, ..)| c.enabled)
        .or_else(|| app.selected_title().map(|(t, ..)| t.enabled));
    if let Some(on) = current {
        app.do_op(Op::SetEnabled {
            item: id,
            enabled: !on,
        });
    }
}

// ---- keyboard ------------------------------------------------------------------------------------

pub fn handle_shortcuts(app: &mut App, ctx: &Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
    let none = Modifiers::NONE;
    if app.multicam.is_some() {
        let keys = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        for (i, k) in keys.into_iter().enumerate() {
            if pressed(none, k) {
                app.multicam_cut(i, None);
            }
        }
        if pressed(none, Key::Escape) {
            app.end_multicam();
        }
    }
    if app.source.open {
        // the source monitor has its own marks and its own frame stepping
        if pressed(none, Key::I) {
            app.source.mark_in = Some(app.source.frame);
        }
        if pressed(none, Key::O) {
            app.source.mark_out = Some(app.source.frame);
        }
        if pressed(none, Key::ArrowLeft) {
            app.source_step(-1);
        }
        if pressed(none, Key::ArrowRight) {
            app.source_step(1);
        }
        if pressed(Modifiers::SHIFT, Key::ArrowLeft) {
            app.source_step(-10);
        }
        if pressed(Modifiers::SHIFT, Key::ArrowRight) {
            app.source_step(10);
        }
        if pressed(none, Key::Comma) {
            app.source_edit(true);
        }
        if pressed(none, Key::Period) {
            app.source_edit(false);
        }
    }
    if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) {
        app.redo();
    }
    if pressed(Modifiers::COMMAND, Key::Z) {
        app.undo();
    }
    if pressed(Modifiers::COMMAND, Key::B) {
        split_at_playhead(app);
    }
    if pressed(none, Key::M) {
        app.add_marker(None, None);
    }
    if pressed(Modifiers::ALT, Key::ArrowLeft) {
        app.jump_marker(-1);
    }
    if pressed(Modifiers::ALT, Key::ArrowRight) {
        app.jump_marker(1);
    }
    if pressed(Modifiers::COMMAND, Key::C) {
        app.copy_selection();
    }
    if pressed(Modifiers::COMMAND, Key::V) {
        app.paste_at(app.playhead);
    }
    if pressed(Modifiers::COMMAND, Key::D) {
        app.duplicate_selection();
    }
    if pressed(Modifiers::COMMAND, Key::A) {
        app.select_all();
    }
    if pressed(Modifiers::SHIFT, Key::F) {
        freeze_frame(app);
    }
    if pressed(Modifiers::COMMAND, Key::T) {
        toggle_transition(app);
    }
    if pressed(Modifiers::COMMAND, Key::I) {
        import_dialog(app);
    }
    if pressed(Modifiers::COMMAND, Key::E) {
        app.export.open = true;
    }
    if pressed(Modifiers::COMMAND, Key::J) {
        app.show_copilot = !app.show_copilot;
    }
    if pressed(Modifiers::SHIFT, Key::Delete) || pressed(Modifiers::SHIFT, Key::Backspace) {
        extract_selection(app);
    }
    if pressed(none, Key::Delete) || pressed(none, Key::Backspace) {
        lift_selection(app);
    }
    if pressed(none, Key::Space) {
        app.toggle_play();
    }
    if pressed(none, Key::K) {
        app.pause();
    }
    if pressed(none, Key::L) {
        let s = if app.playing && app.shuttle > 0.0 {
            (app.shuttle * 2.0).min(8.0)
        } else {
            1.0
        };
        app.play(s);
    }
    if pressed(none, Key::J) {
        let s = if app.playing && app.shuttle < 0.0 {
            (app.shuttle * 2.0).max(-8.0)
        } else {
            -1.0
        };
        app.play(s);
    }
    // Shift variants first: a plain-key pattern also matches the same key with Shift held
    let fps = app.fps().round() as i64;
    if pressed(Modifiers::SHIFT, Key::ArrowLeft) {
        app.set_playhead(app.playhead - fps);
    }
    if pressed(Modifiers::SHIFT, Key::ArrowRight) {
        app.set_playhead(app.playhead + fps);
    }
    if pressed(none, Key::ArrowLeft) {
        app.set_playhead(app.playhead - 1);
    }
    if pressed(none, Key::ArrowRight) {
        app.set_playhead(app.playhead + 1);
    }
    if pressed(none, Key::Home) {
        app.set_playhead(0);
    }
    if pressed(none, Key::End) {
        app.set_playhead(app.duration_ticks());
    }
    if pressed(none, Key::I) {
        app.mark_in = Some(app.playhead);
    }
    if pressed(none, Key::O) {
        app.mark_out = Some(app.playhead);
    }
    if pressed(Modifiers::ALT, Key::X) {
        app.mark_in = None;
        app.mark_out = None;
    }
    for (k, t) in [
        (Key::V, Tool::Select),
        (Key::T, Tool::Trim),
        (Key::B, Tool::Blade),
        (Key::H, Tool::Hand),
        (Key::Z, Tool::Zoom),
    ] {
        if pressed(none, k) {
            app.tool = t;
        }
    }
    if pressed(none, Key::S) {
        app.snap = !app.snap;
    }
    if pressed(none, Key::Equals) || pressed(none, Key::Plus) {
        app.pps = (app.pps * 1.25).min(600.0);
    }
    if pressed(none, Key::Minus) {
        app.pps = (app.pps / 1.25).max(1.0);
    }
    if pressed(Modifiers::SHIFT, Key::Z) {
        crate::timeline::fit(app);
    }
    if pressed(none, Key::Escape) {
        app.selected_clip = None;
        app.show_shortcuts = false;
    }
    if pressed(Modifiers::SHIFT, Key::Slash) {
        app.show_shortcuts = true;
    }
}

// ---- media pool ------------------------------------------------------------------------------------

/// Payload while a media item is being dragged toward the timeline.
#[derive(Clone, Copy)]
pub struct MediaDrag(pub Id);

/// Payload while a title template is being dragged toward the timeline.
#[derive(Clone, Copy)]
pub struct TitleDrag(pub TitleTemplate);

/// A shape, solid colour (`Some`) or adjustment layer (`None`) being dragged from the pool.
pub struct LayerDrag(pub Option<ShapeKind>);

pub fn media_pool(app: &mut App, root: &mut Ui) {
    egui::Panel::left("media_pool")
        .default_size(330.0)
        .min_size(240.0)
        .resizable(true)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(8.0);
                for (t, l) in [
                    (PoolTab::Media, "Media Pool"),
                    (PoolTab::Effects, "Effects"),
                    (PoolTab::Titles, "Titles"),
                    (PoolTab::Transitions, "Transitions"),
                ] {
                    if tab(ui, l, app.pool_tab == t).clicked() {
                        app.pool_tab = t;
                    }
                }
            });
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.cursor().top(),
                Stroke::new(1.0, theme::BORDER),
            );
            if app.pool_tab == PoolTab::Titles {
                crate::titles_ui::pool_tab(app, ui);
                return;
            }
            if app.pool_tab != PoolTab::Media {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Coming in a later phase").color(theme::MUTED));
                    ui.label(
                        RichText::new(
                            "Titles, transitions and effects are not part of the engine yet.",
                        )
                        .size(11.0)
                        .color(theme::FAINT),
                    );
                });
                return;
            }
            egui::Frame::new()
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let w = ui.available_width() - 34.0;
                        ui.add_sized(
                            [w, 26.0],
                            egui::TextEdit::singleline(&mut app.search)
                                .hint_text("Search clips")
                                .margin(egui::Margin::symmetric(8, 4)),
                        );
                        if chip(ui, "+", false, true)
                            .on_hover_text("Import media (⌘I) — or drop files onto the window")
                            .clicked()
                        {
                            import_dialog(app);
                        }
                    });
                    ui.add_space(6.0);
                    bins(app, ui);
                    ui.add_space(6.0);
                    sequences_list(app, ui);
                    grid(app, ui);
                });
        });
}

/// The project's sequences: insert one into the current sequence, or step into it.
fn sequences_list(app: &mut App, ui: &mut Ui) {
    let seqs = app.sequences();
    if seqs.len() < 2 {
        return;
    }
    let cur = app.timeline_id();
    ui.label(RichText::new("Sequences").size(12.0).color(theme::MUTED));
    for (id, name) in seqs {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(RichText::new(&name).size(12.0).color(if Some(id) == cur {
                Color32::WHITE
            } else {
                theme::MUTED
            }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if Some(id) != cur {
                    if chip(ui, "Open", false, true).clicked() {
                        app.set_active_timeline(id);
                    }
                    if chip(ui, "Insert", false, true)
                        .on_hover_text("Put this sequence in the current one at the playhead")
                        .clicked()
                    {
                        app.insert_sequence(id, None);
                    }
                }
            });
        });
    }
    ui.add_space(6.0);
}

fn bins(app: &mut App, ui: &mut Ui) {
    let (mut footage, mut audio) = (0, 0);
    for m in app
        .project()
        .media
        .values()
        .filter(|m| m.nested_timeline().is_none())
    {
        if m.has_video {
            footage += 1;
        } else {
            audio += 1;
        }
    }
    ui.label(RichText::new("Master").size(12.0).color(theme::MUTED));
    for (bin, name, n) in [
        (Bin::Footage, "Footage", footage),
        (Bin::Audio, "Audio", audio),
        (Bin::Graphics, "Graphics", 0),
        (Bin::Stills, "Stills", 0),
    ] {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let on = app.bin == bin;
            let text = RichText::new(format!("{name} ({n})"))
                .size(12.0)
                .color(if on { Color32::WHITE } else { theme::MUTED });
            if ui
                .add(egui::Label::new(text).sense(Sense::click()))
                .clicked()
            {
                app.bin = if on { Bin::All } else { bin };
            }
        });
    }
}

fn grid(app: &mut App, ui: &mut Ui) {
    let q = app.search.to_lowercase();
    let mut items: Vec<MediaRef> = app
        .project()
        .media
        .values()
        .filter(|m| m.nested_timeline().is_none())
        .filter(|m| q.is_empty() || m.name.to_lowercase().contains(&q))
        .filter(|m| match app.bin {
            Bin::All => true,
            Bin::Footage => m.has_video,
            Bin::Audio => !m.has_video,
            Bin::Graphics | Bin::Stills => false,
        })
        .cloned()
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    if items.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(if app.project().media.is_empty() {
                    "No media yet"
                } else {
                    "Nothing matches"
                })
                .color(theme::MUTED),
            );
            if app.project().media.is_empty() {
                ui.label(
                    RichText::new("Drop video or audio files here, or press +")
                        .size(11.0)
                        .color(theme::FAINT),
                );
            }
        });
        return;
    }
    let gap = 8.0;
    let cell_w = ((ui.available_width() - gap) / 2.0).floor();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for row in items.chunks(2) {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for m in row {
                        media_cell(app, ui, m, cell_w);
                    }
                });
                ui.add_space(4.0);
            }
        });
}

fn media_cell(app: &mut App, ui: &mut Ui, m: &MediaRef, w: f32) {
    let thumb_h = (w * 9.0 / 16.0).round();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, thumb_h + 34.0), Sense::click_and_drag());
    let thumb = Rect::from_min_size(rect.min, vec2(w, thumb_h));
    let p = ui.painter();
    p.rect_filled(thumb, 3.0, theme::PANEL_DARK);
    match (m.has_video, app.thumbs.get(&m.id)) {
        (true, Some(tex)) => {
            p.image(
                tex.id(),
                thumb,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        (true, None) => {
            p.text(
                thumb.center(),
                Align2::CENTER_CENTER,
                "…",
                ui_font(14.0),
                theme::FAINT,
            );
        }
        (false, _) => {
            p.text(
                thumb.center(),
                Align2::CENTER_CENTER,
                extension(&m.name),
                mono(11.0),
                theme::AUDIO_WAVE,
            );
            if let Some(peaks) = app.waves.get(&m.id) {
                crate::timeline::draw_wave(
                    p,
                    thumb.shrink2(vec2(8.0, 14.0)),
                    peaks,
                    0.0,
                    peaks.len() as f32,
                    theme::AUDIO_WAVE.gamma_multiply(0.45),
                );
            }
        }
    }
    let selected = app.selected_media == Some(m.id);
    if selected {
        p.rect_stroke(
            thumb.expand(1.0),
            3.0,
            Stroke::new(2.0, theme::ACCENT),
            egui::StrokeKind::Outside,
        );
    } else if resp.hovered() {
        p.rect_stroke(
            thumb,
            3.0,
            Stroke::new(1.0, theme::FAINT),
            egui::StrokeKind::Inside,
        );
    }
    let name_pos = pos2(rect.left(), thumb.bottom() + 4.0);
    let name = ui
        .painter()
        .layout(m.name.clone(), ui_font(11.0), theme::TEXT, w);
    ui.painter()
        .with_clip_rect(Rect::from_min_size(name_pos, vec2(w, 14.0)))
        .galley(name_pos, name, theme::TEXT);
    let dur = m.duration.map_or(0.0, |d| d.to_seconds_f64());
    let tc = format!(
        "{:02}:{:02}:{:02}",
        (dur / 3600.0) as i64,
        (dur / 60.0) as i64 % 60,
        dur as i64 % 60
    );
    ui.painter().text(
        pos2(rect.left(), thumb.bottom() + 20.0),
        Align2::LEFT_TOP,
        tc,
        mono(10.0),
        theme::MUTED,
    );
    if resp.clicked() {
        app.selected_media = Some(m.id);
    }
    if resp.double_clicked() {
        app.open_source(m.id);
    }
    if resp.drag_started() {
        app.selected_media = Some(m.id);
        egui::DragAndDrop::set_payload(ui.ctx(), MediaDrag(m.id));
    }
    resp.context_menu(|ui| {
        if ui.button("Open in Source Monitor").clicked() {
            app.open_source(m.id);
            ui.close();
        }
        if ui.button("Add to Timeline").clicked() {
            app.place_media(m.id, None, None);
            ui.close();
        }
    });
}

fn extension(name: &str) -> String {
    name.rsplit('.')
        .next()
        .map(|e| e.to_uppercase())
        .unwrap_or_default()
}

// ---- inspector -----------------------------------------------------------------------------------

pub fn inspector(app: &mut App, root: &mut Ui) {
    egui::Panel::right("inspector")
        .default_size(300.0)
        .min_size(240.0)
        .resizable(true)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(8.0);
                for (t, l) in [
                    (InspectorTab::Inspector, "Inspector"),
                    (InspectorTab::History, "History"),
                    (InspectorTab::Metadata, "Metadata"),
                    (InspectorTab::Mixer, "Mixer"),
                    (InspectorTab::Transcript, "Transcript"),
                ] {
                    if tab(ui, l, app.inspector_tab == t).clicked() {
                        app.inspector_tab = t;
                    }
                }
            });
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.cursor().top(),
                Stroke::new(1.0, theme::BORDER),
            );
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| match app.inspector_tab {
                            InspectorTab::Inspector => clip_controls(app, ui),
                            InspectorTab::History => crate::branches_ui::history_tab(app, ui),
                            InspectorTab::Metadata => metadata(app, ui),
                            InspectorTab::Mixer => mixer_tab(app, ui),
                            InspectorTab::Transcript => crate::transcript_ui::tab(app, ui),
                        });
                });
        });
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong().size(12.5));
    ui.add_space(2.0);
}

/// The clip right before `clip` on its track, if there is one without a gap between.
/// Track faders, mute buttons and level meters.
fn mixer_tab(app: &mut App, ui: &mut Ui) {
    let Some(tl) = app.timeline().cloned() else {
        return;
    };
    section(ui, "Tracks");
    // the top track first, like the timeline
    for (ix, t) in tl.tracks.iter().enumerate().rev() {
        let level = app.meters.get(ix).copied().unwrap_or(0.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&t.name).strong().size(12.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if t.kind == TrackKind::Audio && chip(ui, "Mute", !t.enabled, true).clicked() {
                    app.do_op(Op::SetTrack {
                        track: t.id,
                        enabled: !t.enabled,
                        gain_db: t.gain_db,
                    });
                }
            });
        });
        let mut gain = t.gain_db;
        let r = slider_row(ui, "Fader", &mut gain, -60.0..=12.0, 0.0, 1, " dB", true);
        if r.changed {
            app.do_merging(
                Op::SetTrack {
                    track: t.id,
                    enabled: t.enabled,
                    gain_db: gain,
                },
                &format!("g:{}", t.id.to_full_string()),
            );
        }
        if r.done {
            app.end_merge();
        }
        meter(ui, level);
        ui.add_space(6.0);
    }
    section(ui, "Master");
    meter(ui, app.meters.last().copied().unwrap_or(0.0));
    ui.label(
        RichText::new("Levels follow playback. Add EQ, compression, a limiter, a de-esser or noise reduction to a clip from the Inspector.")
            .size(10.5)
            .color(theme::FAINT),
    );
}

/// A horizontal level bar from -60 dB to 0 dB, red above -3 dB.
fn meter(ui: &mut Ui, peak: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
    ui.painter().rect_filled(rect, 2.0, theme::FIELD);
    let db = 20.0 * peak.max(1e-6).log10();
    let frac = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let fill = Rect::from_min_size(rect.min, vec2(rect.width() * frac, rect.height()));
    let colour = if db > -3.0 {
        Color32::from_rgb(0xe5, 0x48, 0x4d)
    } else if db > -12.0 {
        Color32::from_rgb(0xe6, 0xb4, 0x2e)
    } else {
        Color32::from_rgb(0x3d, 0xc2, 0x7a)
    };
    ui.painter().rect_filled(fill, 2.0, colour);
}

fn clip_before(app: &App, track: Id, clip: Id) -> Option<Clip> {
    let tl = app.timeline()?;
    let t = tl.tracks.iter().find(|t| t.id == track)?;
    let i = t.items.iter().position(|it| it.id() == clip)?;
    match t.items.get(i.checked_sub(1)?)? {
        Item::Clip(c) => Some(c.clone()),
        _ => None,
    }
}

/// Add (or toggle off) a one-second dissolve before the selected clip.
pub fn toggle_transition(app: &mut App) {
    let Some((clip, track, _)) = app.selected() else {
        app.toast("Select the clip that the transition leads into", false);
        return;
    };
    let Some(prev) = clip_before(app, track, clip.id) else {
        app.toast("A transition needs a clip right before this one", false);
        return;
    };
    let transition = if clip.transition.is_some() {
        None
    } else {
        let max = 2 * prev.duration.value.min(clip.duration.value);
        Some(Transition {
            kind: TransitionKind::Dissolve,
            duration: (app.fps().round() as i64).min(max).max(2),
        })
    };
    if let Some(t) = &transition {
        if let Err(e) = captaine_ops::check_transition(app.project(), clip.id, t) {
            app.toast(e.to_string(), true);
            return;
        }
    }
    app.do_op(Op::SetTransition {
        item: clip.id,
        transition,
    });
}

fn effects_controls(app: &mut App, ui: &mut Ui, clip: &Clip, has_video: bool, has_audio: bool) {
    effects_stack(app, ui, clip.id, &clip.effects, has_video, has_audio);
    if has_video {
        subject_controls(app, ui, clip);
    }
}

/// AI tools that need Apple's Vision framework: background removal now, tracking and reframing below.
fn subject_controls(app: &mut App, ui: &mut Ui, clip: &Clip) {
    section(ui, "AI");
    let busy = app.vision_jobs.iter().any(|j| j.clip == clip.id);
    ui.horizontal_wrapped(|ui| {
        if chip(ui, "Stabilize", false, !busy)
            .on_hover_text("Steady shaky handheld footage (makes a slightly zoomed, smoothed copy)")
            .clicked()
        {
            app.stabilize_clip(clip.id);
        }
        if busy {
            ui.spinner();
            ui.label(RichText::new("Working…").size(11.0).color(theme::MUTED));
        }
    });
    if let Some(why) = captaine_vision::unavailable() {
        ui.label(RichText::new(why).size(10.5).color(theme::FAINT));
        return;
    }
    ui.horizontal_wrapped(|ui| {
        if chip(ui, "Remove background", false, !busy).clicked() {
            app.remove_background(clip.id, captaine_vision::MatteMode::Subject);
        }
        if chip(ui, "People only", false, !busy).clicked() {
            app.remove_background(clip.id, captaine_vision::MatteMode::Person);
        }
    });
    ui.horizontal_wrapped(|ui| {
        let both = {
            let ids = app.selection_ids();
            ids.len() >= 2
                && app.timeline().is_some_and(|t| {
                    t.tracks
                        .iter()
                        .flat_map(|tr| tr.items.iter())
                        .any(|i| i.as_title().is_some() && ids.contains(&i.id()))
                })
        };
        if chip(ui, "Attach title to subject", false, both && !busy)
            .on_hover_text(
                "Shift-click a title as well, then the title follows what is moving in this clip",
            )
            .clicked()
        {
            app.track_attach_selected();
        }
    });
    ui.label(
        RichText::new("Runs on this Mac with Apple's vision model. The cut-out is an effect: switch it off or tune Feather and Choke.")
            .size(10.5)
            .color(theme::FAINT),
    );
}

/// The effects list of a clip or of a generated layer, with add / remove / on-off and sliders.
pub fn effects_stack(
    app: &mut App,
    ui: &mut Ui,
    id: Id,
    current: &[Effect],
    has_video: bool,
    has_audio: bool,
) {
    section(ui, "Effects");
    let mut fx = current.to_vec();
    let key = format!("fx:{}", id.to_full_string());
    let (mut changed, mut done, mut structural) = (false, false, false);
    let mut remove = None;
    for (i, e) in fx.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(e.kind.label())
                    .strong()
                    .size(12.0)
                    .color(if e.enabled { theme::TEXT } else { theme::FAINT }),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if chip(ui, "Remove", false, true).clicked() {
                    remove = Some(i);
                }
                if chip(ui, if e.enabled { "On" } else { "Off" }, e.enabled, true).clicked() {
                    e.enabled = !e.enabled;
                    structural = true;
                }
            });
        });
        if e.kind == EffectKind::Lut {
            ui.horizontal(|ui| {
                let name = std::path::Path::new(&e.text)
                    .file_name()
                    .map_or("No LUT chosen".to_string(), |n| n.to_string_lossy().into());
                ui.label(RichText::new(name).size(11.0).color(theme::MUTED));
                if chip(ui, "Choose .cube…", false, true).clicked() {
                    if let Some(f) = rfd::FileDialog::new()
                        .add_filter("LUT", &["cube"])
                        .pick_file()
                    {
                        e.text = f.to_string_lossy().into();
                        structural = true;
                    }
                }
            });
        }
        for (v, d) in e.params.iter_mut().zip(e.kind.params()) {
            let r = slider_row(
                ui,
                d.name,
                v,
                d.min..=d.max,
                d.default,
                d.decimals,
                "",
                e.enabled,
            );
            changed |= r.changed;
            done |= r.done;
        }
        ui.add_space(4.0);
    }
    if let Some(i) = remove {
        fx.remove(i);
        structural = true;
    }
    ui.horizontal_wrapped(|ui| {
        for kind in EffectKind::ALL {
            if kind.is_audio() && !has_audio || !kind.is_audio() && !has_video {
                continue;
            }
            if chip(ui, &format!("+ {}", kind.label()), false, fx.len() < 16).clicked() {
                fx.push(Effect::new(kind));
                structural = true;
            }
        }
    });
    if structural {
        app.end_merge();
        app.do_op(Op::SetEffects {
            item: id,
            effects: fx,
        });
    } else if changed {
        app.do_merging(
            Op::SetEffects {
                item: id,
                effects: fx,
            },
            &key,
        );
    }
    if done {
        app.end_merge();
    }
}

fn transition_controls(app: &mut App, ui: &mut Ui, clip: &Clip, track: Id) {
    section(ui, "Transition");
    let Some(prev) = clip_before(app, track, clip.id) else {
        ui.label(
            RichText::new("Needs a clip right before this one.")
                .size(10.5)
                .color(theme::FAINT),
        );
        return;
    };
    let max = 2 * prev.duration.value.min(clip.duration.value);
    let default_len = (app.fps().round() as i64).min(max).max(2);
    let cur = clip.transition;
    let mut want = cur;
    ui.horizontal_wrapped(|ui| {
        if chip(ui, "None", cur.is_none(), true).clicked() {
            want = None;
        }
        for kind in TransitionKind::ALL {
            if chip(
                ui,
                kind.label(),
                cur.is_some_and(|t| t.kind == kind),
                max >= 2,
            )
            .clicked()
            {
                want = Some(Transition {
                    kind,
                    duration: cur.map_or(default_len, |t| t.duration).min(max),
                });
            }
        }
    });
    if let Some(t) = cur {
        let mut frames = t.duration as f64;
        let r = slider_row(
            ui,
            "Length",
            &mut frames,
            2.0..=max as f64,
            default_len as f64,
            0,
            " f",
            true,
        );
        if r.changed {
            app.do_merging(
                Op::SetTransition {
                    item: clip.id,
                    transition: Some(Transition {
                        duration: frames.round() as i64,
                        ..t
                    }),
                },
                &format!("x:{}", clip.id.to_full_string()),
            );
        }
        if r.done {
            app.end_merge();
        }
    }
    if want != cur {
        app.do_op(Op::SetTransition {
            item: clip.id,
            transition: want,
        });
    }
}

fn clip_controls(app: &mut App, ui: &mut Ui) {
    if app.selected_title().is_some() {
        crate::titles_ui::controls(app, ui);
        return;
    }
    let Some((clip, track, start)) = app.selected() else {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No clip selected").color(theme::MUTED));
            ui.label(
                RichText::new("Click a clip in the timeline to edit it.")
                    .size(11.0)
                    .color(theme::FAINT),
            );
        });
        return;
    };
    let track_name = app
        .project()
        .timeline(app.timeline_id().unwrap_or_default())
        .and_then(|t| t.tracks.iter().find(|x| x.id == track))
        .map(|t| t.name.clone())
        .unwrap_or_default();
    let media = app.project().media.get(&clip.media).cloned();
    ui.label(RichText::new(&clip.name).strong().size(13.0));
    let size = media
        .as_ref()
        .filter(|m| m.has_video)
        .map(|m| format!(" · {} × {}", m.width, m.height))
        .unwrap_or_default();
    ui.label(
        RichText::new(format!(
            "{track_name} · {}{size}",
            app.tc(clip.duration.value)
        ))
        .font(mono(10.5))
        .color(theme::MUTED),
    );
    let _ = start;
    let has_video = media.as_ref().is_some_and(|m| m.has_video);
    let has_audio = media.as_ref().is_some_and(|m| m.has_audio);
    let (cw, ch) = (app.canvas().0 as f64, app.canvas().1 as f64);

    let mut v = clip.video;
    let mut a = clip.audio;
    let (mut vdone, mut adone) = (false, false);
    let mut vchanged = false;
    let mut achanged = false;
    let key = clip.id.to_full_string();
    let inside = app.playhead >= start && app.playhead < start + clip.duration.value;
    let local = (app.playhead - start).clamp(0, clip.duration.value.max(1) - 1);
    let (cur_v, cur_a) = (clip.video_at(local), clip.audio_at(local));
    let mut edits: Vec<(&'static str, f64)> = Vec::new();
    let mut toggles: Vec<(&'static str, f64)> = Vec::new();

    // (property, label, range, default, decimals, suffix, is audio)
    type Row = (
        &'static str,
        &'static str,
        (f64, f64),
        f64,
        usize,
        &'static str,
        bool,
    );
    let rows: [Row; 11] = [
        ("position.x", "Position X", (-cw, cw), 0.0, 1, "", false),
        ("position.y", "Position Y", (-ch, ch), 0.0, 1, "", false),
        ("scale", "Scale", (5.0, 400.0), 100.0, 1, "%", false),
        ("rotation", "Rotation", (-180.0, 180.0), 0.0, 1, "°", false),
        ("opacity", "Opacity", (0.0, 100.0), 100.0, 0, "%", false),
        ("exposure", "Exposure", (-3.0, 3.0), 0.0, 2, "", false),
        ("contrast", "Contrast", (0.2, 2.0), 1.0, 2, "", false),
        (
            "saturation",
            "Saturation",
            (0.0, 200.0),
            100.0,
            0,
            "",
            false,
        ),
        (
            "temperature",
            "Temperature",
            (2500.0, 10000.0),
            ColorProps::NEUTRAL_KELVIN,
            0,
            "K",
            false,
        ),
        ("volume", "Volume", (-60.0, 12.0), 0.0, 1, " dB", true),
        ("pan", "Pan", (-1.0, 1.0), 0.0, 2, "", true),
    ];
    let app_selection_count = app.selection_ids().len();
    let mut anim_row = |ui: &mut Ui, r: &Row, edits: &mut Vec<_>, toggles: &mut Vec<_>| {
        let (prop, label, (lo, hi), def, dec, suf, audio) = *r;
        let enabled = if audio { has_audio } else { has_video };
        let cur = base_value(&cur_v, Some(&cur_a), prop).unwrap_or(def);
        let mut val = cur;
        let animated = clip.anim.has(prop);
        let on_key = inside && clip.anim.keys(prop).iter().any(|k| k.t == local);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if diamond(ui, label, animated, on_key, enabled && inside).clicked() {
                toggles.push((prop, cur));
            }
            let out = slider_row(ui, label, &mut val, lo..=hi, def, dec, suf, enabled);
            if out.changed && (val - cur).abs() > 1e-9 {
                edits.push((prop, val));
            }
            if out.done {
                if audio {
                    adone = true;
                } else {
                    vdone = true;
                }
            }
        });
    };
    section(ui, "Transform");
    for r in &rows[..5] {
        anim_row(ui, r, &mut edits, &mut toggles);
    }
    let mut quick = None;
    if media.as_ref().is_some_and(|m| m.has_video) {
        ui.horizontal_wrapped(|ui| {
            let neg = (v.transform.scale.0 < 0.0, v.transform.scale.1 < 0.0);
            if chip(ui, "Fit", false, true)
                .on_hover_text("Whole picture inside the frame")
                .clicked()
            {
                quick = Some(0);
            }
            if chip(ui, "Fill", false, true)
                .on_hover_text("Cover the frame, cropping the overflow")
                .clicked()
            {
                quick = Some(1);
            }
            if chip(ui, "Flip H", neg.0, true).clicked() {
                quick = Some(2);
            }
            if chip(ui, "Flip V", neg.1, true).clicked() {
                quick = Some(3);
            }
        });
    }
    section(ui, "Color");
    for r in &rows[5..9] {
        anim_row(ui, r, &mut edits, &mut toggles);
    }
    let mut color_action = None;
    if has_video {
        ui.horizontal_wrapped(|ui| {
            if chip(ui, "Auto color", false, true)
                .on_hover_text(
                    "Balance exposure, contrast, colour cast and saturation from the footage",
                )
                .clicked()
            {
                color_action = Some(0);
            }
            let two = app_selection_count >= 2;
            if chip(ui, "Match to other selected", false, two)
                .on_hover_text("Shift-click a second clip: this one is changed to look like it")
                .clicked()
            {
                color_action = Some(1);
            }
        });
    }
    section(ui, "Audio & Speed");
    for r in &rows[9..] {
        anim_row(ui, r, &mut edits, &mut toggles);
    }
    match color_action {
        Some(0) => app.auto_color(clip.id),
        Some(_) => app.match_color_selected(),
        None => {}
    }
    if let (Some(q), Some(m)) = (quick, media.as_ref()) {
        // how much bigger than "fit inside the frame" the picture must be to cover the frame
        let (rx, ry) = (cw / m.width.max(1) as f64, ch / m.height.max(1) as f64);
        let cover = rx.max(ry) / rx.min(ry);
        let (sx, sy) = (v.transform.scale.0.signum(), v.transform.scale.1.signum());
        match q {
            0 => v.transform.scale = (sx, sy),
            1 => v.transform.scale = (cover * sx, cover * sy),
            2 => v.transform.scale.0 = -v.transform.scale.0,
            _ => v.transform.scale.1 = -v.transform.scale.1,
        }
        vchanged = true;
        vdone = true;
    }
    let row = |ui: &mut Ui,
               label: &str,
               val: &mut f64,
               range: std::ops::RangeInclusive<f64>,
               def: f64,
               dec: usize,
               suf: &str,
               enabled: bool,
               changed: &mut bool,
               done: &mut bool| {
        let r = slider_row(ui, label, val, range, def, dec, suf, enabled);
        *changed |= r.changed;
        *done |= r.done;
    };
    let fps = app.fps();
    let max_fade = clip.duration.value as f64 / fps;
    let mut fin = a.fade_in as f64 / fps;
    let mut fout = a.fade_out as f64 / fps;
    let mut fc = false;
    row(
        ui,
        "Fade in",
        &mut fin,
        0.0..=max_fade,
        0.0,
        2,
        " s",
        has_audio,
        &mut fc,
        &mut adone,
    );
    row(
        ui,
        "Fade out",
        &mut fout,
        0.0..=max_fade,
        0.0,
        2,
        " s",
        has_audio,
        &mut fc,
        &mut adone,
    );
    if fc {
        a.fade_in = (fin * fps).round() as i64;
        a.fade_out = (fout * fps).round() as i64;
        // fades may not overlap: the one being dragged wins
        let over = a.fade_in + a.fade_out - clip.duration.value;
        if over > 0 {
            if (fin * fps).round() as i64 != clip.audio.fade_in {
                a.fade_out = (a.fade_out - over).max(0);
            } else {
                a.fade_in = (a.fade_in - over).max(0);
            }
        }
        achanged = true;
    }
    let mut rt = clip.retime;
    let mut speed = rt.speed * 100.0;
    let sp = slider_row(
        ui,
        "Speed",
        &mut speed,
        5.0..=400.0,
        100.0,
        0,
        "%",
        has_video || has_audio,
    );
    let mut retime_now = false;
    if sp.changed {
        // dragging re-times the clip live, as one undo step
        let to = Retime {
            speed: (speed / 100.0).clamp(Retime::MIN_SPEED, Retime::MAX_SPEED),
            ..rt
        };
        if to != clip.retime {
            app.do_merging(
                Op::SetRetime {
                    item: clip.id,
                    retime: to,
                },
                &format!("s:{key}"),
            );
        }
    }
    if sp.done {
        app.end_merge();
    }
    ui.horizontal(|ui| {
        if chip(ui, "Reverse", rt.reverse, !rt.freeze).clicked() {
            rt.reverse = !rt.reverse;
            retime_now = true;
        }
        if chip(ui, "Freeze", rt.freeze, true).clicked() {
            rt.freeze = !rt.freeze;
            retime_now = true;
        }
        if chip(ui, "Hold frame at playhead", false, inside).clicked() {
            freeze_frame(app);
        }
    });
    if retime_now && rt != clip.retime {
        app.do_op(Op::SetRetime {
            item: clip.id,
            retime: rt,
        });
    }
    section(ui, "Blend");
    ui.horizontal(|ui| {
        for (mode, name) in [
            (BlendMode::Normal, "Normal"),
            (BlendMode::Add, "Add"),
            (BlendMode::Multiply, "Multiply"),
            (BlendMode::Screen, "Screen"),
        ] {
            if chip(ui, name, v.blend == mode, has_video).clicked() {
                v.blend = mode;
                vchanged = true;
                vdone = true;
            }
        }
    });

    for (prop, val) in edits {
        if clip.anim.has(prop) {
            let mut keys = clip.anim.keys(prop).to_vec();
            match keys.iter_mut().find(|k| k.t == local) {
                Some(k) => k.v = val,
                None => {
                    keys.push(Keyframe::new(local, val));
                    keys.sort_by_key(|k| k.t);
                }
            }
            app.do_merging(
                Op::SetAnimTrack {
                    item: clip.id,
                    prop: prop.into(),
                    keys,
                },
                &format!("k:{key}:{prop}"),
            );
        } else {
            set_base_value(&mut v, &mut a, prop, val);
            if matches!(prop, "volume" | "pan") {
                achanged = true;
            } else {
                vchanged = true;
            }
        }
    }
    for (prop, cur) in toggles {
        let mut keys = clip.anim.keys(prop).to_vec();
        match keys.iter().position(|k| k.t == local) {
            Some(i) => {
                keys.remove(i);
            }
            None => {
                keys.push(Keyframe::new(local, cur));
                keys.sort_by_key(|k| k.t);
            }
        }
        app.do_op(Op::SetAnimTrack {
            item: clip.id,
            prop: prop.into(),
            keys,
        });
    }
    if has_video {
        transition_controls(app, ui, &clip, track);
    }
    effects_controls(app, ui, &clip, has_video, has_audio);
    if vchanged && has_video {
        app.do_merging(
            Op::SetVideo {
                item: clip.id,
                video: v,
            },
            &format!("v:{key}"),
        );
    }
    if achanged && has_audio {
        app.do_merging(
            Op::SetAudio {
                item: clip.id,
                audio: a,
            },
            &format!("a:{key}"),
        );
    }
    if vdone || adone {
        app.end_merge();
    }
}

fn metadata(app: &mut App, ui: &mut Ui) {
    let Some(m) = app
        .selected()
        .and_then(|(c, ..)| app.project().media.get(&c.media).cloned())
        .or_else(|| {
            app.selected_media
                .and_then(|id| app.project().media.get(&id).cloned())
        })
    else {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Select a clip or media item").color(theme::MUTED))
        });
        return;
    };
    let rows = [
        ("Name", m.name.clone()),
        ("Path", m.path.clone()),
        (
            "Frame size",
            if m.has_video {
                format!("{} × {}", m.width, m.height)
            } else {
                "—".into()
            },
        ),
        (
            "Length",
            m.duration
                .map_or("—".into(), |d| format!("{:.2} s", d.to_seconds_f64())),
        ),
        (
            "Rate",
            m.duration.map_or("—".into(), |d| {
                format!(
                    "{} {}",
                    fps_label(d.rate),
                    if m.has_video { "fps" } else { "Hz" }
                )
            }),
        ),
        (
            "Audio",
            if m.has_audio {
                "yes".into()
            } else {
                "no".into()
            },
        ),
        ("Proxy", m.proxy.clone().unwrap_or_else(|| "none".into())),
    ];
    for (k, v) in rows {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(k).color(theme::MUTED).size(11.5));
            ui.label(RichText::new(v).font(mono(11.0)));
        });
    }
}

// ---- dialogs ---------------------------------------------------------------------------------------

pub fn dialogs(app: &mut App, ctx: &Context) {
    crate::markers_ui::windows(app, ctx);
    if let Some((id, mut name)) = app.rename_sequence.take() {
        let (mut keep, mut done) = (true, false);
        egui::Window::new("Rename sequence")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let r = ui.text_edit_singleline(&mut name);
                r.request_focus();
                ui.horizontal(|ui| {
                    if ui.button("Rename").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = true;
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        keep = false;
                    }
                });
            });
        if done {
            app.rename_sequence(id, name);
        } else if keep {
            app.rename_sequence = Some((id, name));
        }
    }
    if app.show_shortcuts {
        let mut open = true;
        egui::Window::new("Keyboard shortcuts")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Grid::new("keys")
                    .num_columns(2)
                    .spacing([24.0, 4.0])
                    .show(ui, |ui| {
                        for (k, d) in [
                            ("Space / K", "Play / pause"),
                            (
                                "L / J",
                                "Play forward / reverse (press again for 2x, 4x, 8x)",
                            ),
                            ("← →", "Step one frame (⇧ for one second)"),
                            ("Home / End", "Go to start / end"),
                            ("I / O", "Mark in / out (⌥X clears)"),
                            ("V T B H Z", "Select, Trim, Blade, Hand, Zoom tools"),
                            ("⌘B", "Split at playhead"),
                            ("⌫", "Lift the selected clip (or the marked range)"),
                            ("⇧⌫", "Extract: delete and close the gap"),
                            ("⌘Z / ⇧⌘Z", "Undo / redo"),
                            ("S", "Toggle snapping"),
                            ("= / -", "Zoom the timeline"),
                            ("⇧Z", "Fit the timeline"),
                            ("⌘I / ⌘E", "Import media / export"),
                            ("⌘J", "Copilot"),
                        ] {
                            ui.label(RichText::new(k).font(mono(11.5)).color(theme::ACCENT_HOVER));
                            ui.label(d);
                            ui.end_row();
                        }
                    });
            });
        app.show_shortcuts = open;
    }
    new_project_dialog(app, ctx);
    export_dialog(app, ctx);
}

fn new_project_dialog(app: &mut App, ctx: &Context) {
    let Some((mut name, mut w, mut h, mut fps)) = app.new_project.clone() else {
        return;
    };
    let mut create = false;
    let mut cancel = false;
    egui::Window::new("New project")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            egui::Grid::new("np")
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut name);
                    ui.end_row();
                    ui.label("Frame size");
                    ui.horizontal(|ui| {
                        for (label, (pw, ph)) in [
                            ("1080p", (1920, 1080)),
                            ("4K", (3840, 2160)),
                            ("Vertical", (1080, 1920)),
                            ("720p", (1280, 720)),
                        ] {
                            if chip(ui, label, (w, h) == (pw, ph), true).clicked() {
                                (w, h) = (pw, ph);
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Frame rate");
                    ui.horizontal(|ui| {
                        for f in ["24", "25", "30", "60", "23.976", "29.97"] {
                            if chip(ui, f, fps == f, true).clicked() {
                                fps = f.to_string();
                            }
                        }
                    });
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Create").color(Color32::WHITE))
                            .fill(theme::ACCENT),
                    )
                    .clicked()
                {
                    create = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
    if create {
        app.new_project = None;
        let Some(dir) = rfd::FileDialog::new()
            .set_title("Where should the project be saved?")
            .pick_folder()
        else {
            return;
        };
        crate::new_project(app, &dir, &name, w, h, &fps);
    } else if cancel {
        app.new_project = None;
    } else {
        app.new_project = Some((name, w, h, fps));
    }
}

fn export_dialog(app: &mut App, ctx: &Context) {
    if !app.export.open {
        return;
    }
    let mut open = true;
    let busy = app
        .export
        .jobs
        .iter()
        .any(|j| j.progress.lock().expect("export").2.is_none());
    let codec = app.export.codec;
    egui::Window::new("Export")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label(RichText::new("Presets").size(11.0).color(theme::MUTED));
            ui.horizontal_wrapped(|ui| {
                for p in captaine_export::PRESETS {
                    let on = app.export.codec == p.codec
                        && (!p.codec.uses_quality() || app.export.crf == p.crf);
                    if chip(ui, p.name, on, true).on_hover_text(p.note).clicked() {
                        app.export.codec = p.codec;
                        app.export.crf = p.crf;
                        app.export.path = None;
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new("Format").size(11.0).color(theme::MUTED));
            ui.horizontal_wrapped(|ui| {
                for c in Codec::ALL {
                    if chip(ui, c.label(), codec == c, true).clicked() {
                        app.export.codec = c;
                        app.export.path = None;
                    }
                }
            });
            if codec.uses_quality() {
                let mut crf = app.export.crf as f64;
                let r = slider_row(
                    ui,
                    "Quality (CRF)",
                    &mut crf,
                    12.0..=30.0,
                    18.0,
                    0,
                    "",
                    true,
                );
                if r.changed {
                    app.export.crf = crf.round() as u32;
                }
                ui.label(
                    RichText::new(
                        "Lower is better quality and a larger file. 18 is visually lossless.",
                    )
                    .size(10.5)
                    .color(theme::FAINT),
                );
            }
            let marks = app
                .mark_in
                .zip(app.mark_out)
                .map(|(a, b)| (a.min(b), a.max(b)));
            ui.horizontal(|ui| {
                ui.label("Range");
                if chip(
                    ui,
                    "Whole timeline",
                    !app.export.marks_only || marks.is_none(),
                    true,
                )
                .clicked()
                {
                    app.export.marks_only = false;
                }
                if chip(
                    ui,
                    "In to Out marks",
                    app.export.marks_only && marks.is_some(),
                    marks.is_some(),
                )
                .clicked()
                {
                    app.export.marks_only = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(if codec == Codec::PngSequence {
                    "Folder"
                } else {
                    "File"
                });
                let text = app
                    .export
                    .path
                    .as_ref()
                    .map_or("Choose…".to_string(), |p| p.display().to_string());
                if ui.button(text).clicked() {
                    let picked = if codec == Codec::PngSequence {
                        rfd::FileDialog::new()
                            .set_file_name(app.project_title())
                            .save_file()
                    } else {
                        let ext = codec.extension();
                        rfd::FileDialog::new()
                            .set_file_name(format!("{}.{ext}", app.project_title()))
                            .add_filter("export", &[ext])
                            .save_file()
                    };
                    if let Some(p) = picked {
                        app.export.path = Some(p);
                    }
                }
            });
            ui.add_space(4.0);
            if ui
                .add_enabled(
                    app.export.path.is_some(),
                    egui::Button::new(
                        RichText::new(if busy { "Add to queue" } else { "Export" })
                            .color(Color32::WHITE),
                    )
                    .fill(theme::ACCENT),
                )
                .clicked()
            {
                let range = if app.export.marks_only { marks } else { None };
                queue_export(app, ctx, range);
            }
            if !app.export.jobs.is_empty() {
                ui.separator();
                ui.label(RichText::new("Queue").size(11.0).color(theme::MUTED));
                let mut remove = None;
                for (i, j) in app.export.jobs.iter().enumerate() {
                    let (done, total, result) = j.progress.lock().expect("export").clone();
                    let started = j.started.load(std::sync::atomic::Ordering::Relaxed);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&j.name).size(12.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = if result.is_some() { "Clear" } else { "Cancel" };
                            if chip(ui, label, false, true).clicked() {
                                if result.is_some() {
                                    remove = Some(i);
                                } else {
                                    j.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                                }
                            }
                        });
                    });
                    match &result {
                        Some(Ok(())) => {
                            ui.colored_label(theme::OK, format!("Finished: {}", j.path.display()));
                        }
                        Some(Err(e)) => {
                            ui.colored_label(Color32::LIGHT_RED, e);
                        }
                        None if !started => {
                            ui.label(RichText::new("Waiting…").size(11.0).color(theme::MUTED));
                        }
                        None => {
                            ui.add(
                                egui::ProgressBar::new(if total > 0 {
                                    done as f32 / total as f32
                                } else {
                                    0.0
                                })
                                .show_percentage(),
                            );
                        }
                    }
                }
                if let Some(i) = remove {
                    app.export.jobs.remove(i);
                }
            }
        });
    app.export.open = open;
    if busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// Serialises exports: a job waits here until the ones before it are done.
static EXPORT_TURN: Mutex<()> = Mutex::new(());

fn queue_export(app: &mut App, ctx: &Context, range: Option<(i64, i64)>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (Some(path), Some(tl)) = (app.export.path.clone(), app.timeline_id()) else {
        return;
    };
    let project = app.project().clone();
    let codec = app.export.codec;
    let cancel = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let opts = captaine_export::ExportOptions {
        codec,
        crf: app.export.crf,
        range,
        cancel: Some(cancel.clone()),
        ..Default::default()
    };
    let state: ExportProgress = Arc::new(Mutex::new((0, 0, None)));
    app.export.jobs.push(ExportJob {
        name: format!(
            "{} · {}",
            path.file_name()
                .map_or(String::new(), |n| n.to_string_lossy().into()),
            codec.label()
        ),
        path: path.clone(),
        codec,
        progress: state.clone(),
        cancel: cancel.clone(),
        started: started.clone(),
    });
    app.export.path = None;
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _turn = EXPORT_TURN.lock().unwrap_or_else(|e| e.into_inner());
        started.store(true, Ordering::Relaxed);
        let result = if cancel.load(Ordering::Relaxed) {
            Err(captaine_export::ExportError::Cancelled)
        } else {
            let s2 = state.clone();
            captaine_export::export(&project, tl, &path, &opts, &mut |d, t| {
                let mut g = s2.lock().expect("export");
                g.0 = d;
                g.1 = t;
            })
        };
        state.lock().expect("export").2 = Some(result.map_err(|e| e.to_string()));
        ctx.request_repaint();
    });
}

// ---- toasts -----------------------------------------------------------------------------------------

pub fn toasts(app: &mut App, ctx: &Context) {
    if app.toasts.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, 52.0))
        .order(egui::Order::Tooltip)
        .show(ctx, |ui| {
            for t in app.toasts.iter().rev().take(3) {
                egui::Frame::new()
                    .fill(if t.error {
                        Color32::from_rgb(0x5a, 0x24, 0x28)
                    } else {
                        theme::PANEL_DARK
                    })
                    .stroke(Stroke::new(
                        1.0,
                        if t.error {
                            Color32::from_rgb(0xa0, 0x40, 0x48)
                        } else {
                            theme::BORDER
                        },
                    ))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(12, 7))
                    .show(ui, |ui| {
                        ui.label(RichText::new(&t.text).size(12.0));
                    });
                ui.add_space(4.0);
            }
        });
}
