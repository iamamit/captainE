//! Branch switcher, history tab and the actions behind them (docs/07).

use captaine_ops::Author;
use captaine_project::{BranchInfo, MergeReport};
use egui::{pos2, vec2, Align2, Color32, Context, Rect, RichText, Sense, Stroke, Ui};

use crate::app::App;
use crate::compare::CompareState;
use crate::theme::{self, mono, ui_font};
use crate::ui::chip;

pub use captaine_project::{author_label, time_ago};

// ---- actions ----------------------------------------------------------------------------------------------------

/// Everything on screen is stale after the whole project was swapped (switch, merge).
pub fn session_replaced(app: &mut App) {
    app.session_changed();
}

pub fn commit_now(app: &mut App, message: &str) {
    let msg = message.trim();
    match app
        .session
        .commit(if msg.is_empty() { None } else { Some(msg) })
    {
        Ok(Some(id)) => {
            app.commit_draft.clear();
            app.dirty_since.get_or_insert_with(std::time::Instant::now);
            app.toast(format!("Committed {}", id), false);
        }
        Ok(None) => app.toast("Nothing to commit", false),
        Err(e) => app.toast(e, true),
    }
}

pub fn switch_branch(app: &mut App, name: &str) {
    app.pause();
    match app.session.checkout(name) {
        Ok(()) => {
            session_replaced(app);
            app.dirty_since.get_or_insert_with(std::time::Instant::now);
            app.toast(format!("Switched to {name}"), false);
        }
        Err(e) => app.toast(e, true),
    }
}

pub fn new_branch(app: &mut App, name: &str) {
    match app.session.create_branch(name.trim(), true) {
        Ok(()) => {
            app.branch_draft.clear();
            app.dirty_since.get_or_insert_with(std::time::Instant::now);
            app.toast(
                format!("New branch {}: your edits here stay on it", name.trim()),
                false,
            );
        }
        Err(e) => app.toast(e, true),
    }
}

pub fn announce_merge(app: &mut App, from: &str, report: MergeReport) {
    session_replaced(app);
    app.dirty_since.get_or_insert_with(std::time::Instant::now);
    let into = app.session.branch().to_string();
    match report {
        MergeReport::UpToDate => {
            app.toast(format!("{into} already has everything from {from}"), false)
        }
        MergeReport::FastForward => app.toast(format!("Merged {from} into {into}"), false),
        MergeReport::Merged {
            applied, skipped, ..
        } => {
            let extra = if skipped.is_empty() {
                String::new()
            } else {
                format!(" ({} could not be brought in)", skipped.len())
            };
            app.toast(
                format!(
                    "Merged {from} into {into}: {applied} change{} brought in{extra}",
                    if applied == 1 { "" } else { "s" }
                ),
                false,
            );
        }
    }
}

pub fn start_merge(app: &mut App, ctx: &Context, from: &str) {
    app.pause();
    match CompareState::merge(app, ctx, from) {
        Ok(c) => {
            app.compare = Some(c);
            app.branch_menu = false;
            // preparing a merge commits work in progress
            app.dirty_since.get_or_insert_with(std::time::Instant::now);
        }
        Err(e) => app.toast(e, true),
    }
}

pub fn start_compare(app: &mut App, ctx: &Context, a: &str, b: &str) {
    app.pause();
    match CompareState::browse(app, ctx, a, b) {
        Ok(c) => {
            app.compare = Some(c);
            app.branch_menu = false;
        }
        Err(e) => app.toast(e, true),
    }
}

// ---- the branch pill ---------------------------------------------------------------------------------------------

/// A branch name with a small branch icon; returns its response.
pub fn pill(app: &App, ui: &mut Ui) -> egui::Response {
    let name = app.session.branch().to_string();
    let shown: String = if name.chars().count() > 22 {
        format!("{}…", name.chars().take(21).collect::<String>())
    } else {
        name
    };
    let galley = ui
        .painter()
        .layout_no_wrap(shown, ui_font(11.5), theme::TEXT);
    let size = vec2(
        galley.size().x
            + 36.0
            + if app.session.has_uncommitted() {
                12.0
            } else {
                0.0
            },
        26.0,
    );
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let open = app.branch_menu;
    let p = ui.painter();
    p.rect_filled(
        rect,
        6.0,
        if open || resp.hovered() {
            theme::rgb(0x36, 0x36, 0x3e)
        } else {
            theme::FIELD
        },
    );
    if open {
        p.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    // branch icon: a stem with two nodes and a side node
    let c = pos2(rect.left() + 13.0, rect.center().y);
    let ink = theme::MUTED;
    p.line_segment(
        [c + vec2(-3.0, -5.0), c + vec2(-3.0, 5.0)],
        Stroke::new(1.3, ink),
    );
    p.line_segment(
        [c + vec2(-3.0, 1.0), c + vec2(4.0, -3.0)],
        Stroke::new(1.3, ink),
    );
    for d in [vec2(-3.0, -5.0), vec2(-3.0, 5.0), vec2(4.0, -3.0)] {
        p.circle_filled(c + d, 2.2, theme::PANEL);
        p.circle_stroke(c + d, 2.2, Stroke::new(1.3, ink));
    }
    p.galley(
        pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::TEXT,
    );
    if app.session.has_uncommitted() {
        p.circle_filled(
            pos2(rect.right() - 11.0, rect.center().y),
            3.0,
            crate::compare::CHANGED,
        );
    }
    resp.on_hover_text(if app.session.has_uncommitted() {
        "Branches (you have uncommitted edits)"
    } else {
        "Branches"
    })
}

fn branch_row(app: &mut App, ctx: &Context, ui: &mut Ui, b: &BranchInfo, close: &mut bool) {
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
        if b.current {
            ui.painter().circle_filled(dot.center(), 4.0, theme::ACCENT);
        } else {
            ui.painter()
                .circle_stroke(dot.center(), 4.0, Stroke::new(1.2, theme::FAINT));
        }
        let r = ui.add(
            egui::Label::new(
                RichText::new(&b.name)
                    .strong()
                    .size(12.5)
                    .color(if b.current {
                        Color32::WHITE
                    } else {
                        theme::TEXT
                    }),
            )
            .sense(Sense::click()),
        );
        if r.clicked() && !b.current {
            switch_branch(app, &b.name);
            *close = true;
        }
        r.on_hover_text(format!(
            "{} · {} · {}",
            b.last_message,
            author_label(&b.last_author),
            time_ago(b.last_time)
        ));
        let tag = match (b.ahead, b.behind) {
            _ if b.current => "current".to_string(),
            (0, 0) => "same".to_string(),
            (a, 0) => format!("{a} ahead"),
            (0, bh) => format!("{bh} behind"),
            (a, bh) => format!("{a} ahead, {bh} behind"),
        };
        ui.label(RichText::new(tag).font(mono(10.0)).color(theme::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !b.current {
                if ui
                    .add(egui::Button::new(RichText::new("Merge").size(11.0)).frame(false))
                    .on_hover_text("Review and merge this branch into the current one")
                    .clicked()
                {
                    start_merge(app, ctx, &b.name);
                    *close = true;
                }
                if ui
                    .add(egui::Button::new(RichText::new("Compare").size(11.0)).frame(false))
                    .clicked()
                {
                    let cur = app.session.branch().to_string();
                    start_compare(app, ctx, &cur, &b.name);
                    *close = true;
                }
            }
        });
    });
}

/// The branch picker, shown under the pill while `app.branch_menu` is on.
pub fn popup(app: &mut App, ctx: &Context, anchor: Rect) {
    if !app.branch_menu {
        return;
    }
    let mut close = false;
    let pos = pos2((anchor.right() - 360.0).max(8.0), anchor.bottom() + 6.0);
    let area = egui::Area::new(egui::Id::new("branch_popup")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::new()
            .fill(theme::PANEL)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(10.0)
            .inner_margin(egui::Margin::same(12))
            .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(120) })
            .show(ui, |ui| {
                ui.set_width(336.0);
                ui.label(RichText::new("Branches").strong().size(13.0));
                ui.label(RichText::new("Try something on a branch; compare it with this one; merge what you like.").size(10.5).color(theme::MUTED));
                ui.add_space(6.0);
                let mut infos = app.session.branch_infos();
                infos.sort_by(|a, b| b.current.cmp(&a.current).then(b.last_time.cmp(&a.last_time)));
                for b in &infos {
                    branch_row(app, ctx, ui, b, &mut close);
                    ui.add_space(2.0);
                }
                ui.add_space(6.0);
                ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, theme::BORDER));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let w = ui.available_width() - 70.0;
                    let r = ui.add_sized([w, 26.0], egui::TextEdit::singleline(&mut app.branch_draft).hint_text("New branch name").margin(egui::Margin::symmetric(8, 5)));
                    let go = ui.add_enabled(!app.branch_draft.trim().is_empty(), egui::Button::new("Create").min_size(vec2(62.0, 26.0))).clicked();
                    if go || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !app.branch_draft.trim().is_empty()) {
                        let n = app.branch_draft.clone();
                        new_branch(app, &n);
                        close = true;
                    }
                });
                if app.session.has_uncommitted() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("{} uncommitted edit{}", app.session.uncommitted_count(), if app.session.uncommitted_count() == 1 { "" } else { "s" })).size(11.5).color(crate::compare::CHANGED));
                    ui.horizontal(|ui| {
                        let w = ui.available_width() - 70.0;
                        ui.add_sized([w, 26.0], egui::TextEdit::singleline(&mut app.commit_draft).hint_text("Describe this version").margin(egui::Margin::symmetric(8, 5)));
                        if ui.add(egui::Button::new(RichText::new("Commit").color(Color32::WHITE)).fill(theme::ACCENT).min_size(vec2(62.0, 26.0))).clicked() {
                            let m = app.commit_draft.clone();
                            commit_now(app, &m);
                        }
                    });
                }
            });
    });
    let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
        && ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| !area.response.rect.contains(p) && !anchor.contains(p));
    if close || clicked_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.branch_menu = false;
    }
}

// ---- the History tab ----------------------------------------------------------------------------------------------

pub fn history_tab(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let branch = app.session.branch().to_string();
    ui.horizontal(|ui| {
        ui.label(RichText::new(&branch).strong().size(13.0));
        ui.label(RichText::new("history").size(11.5).color(theme::MUTED));
    });
    ui.add_space(6.0);
    if app.session.has_uncommitted() {
        egui::Frame::new()
            .fill(theme::PANEL_DARK)
            .corner_radius(8.0)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} uncommitted edit{}",
                        app.session.uncommitted_count(),
                        if app.session.uncommitted_count() == 1 {
                            ""
                        } else {
                            "s"
                        }
                    ))
                    .strong()
                    .size(12.0)
                    .color(crate::compare::CHANGED),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut app.commit_draft)
                        .hint_text("Describe this version (optional)")
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 5)),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Commit").color(Color32::WHITE))
                                .fill(theme::ACCENT),
                        )
                        .clicked()
                    {
                        let m = app.commit_draft.clone();
                        commit_now(app, &m);
                    }
                    if chip(ui, "What changed", false, true).clicked() {
                        let head = app.session.branch().to_string();
                        start_compare(app, &ctx, &head, "working");
                    }
                });
            });
        ui.add_space(8.0);
    } else {
        ui.label(
            RichText::new("Everything is committed.")
                .size(11.5)
                .color(theme::MUTED),
        );
        ui.add_space(6.0);
    }
    let log = app.session.log(&branch).unwrap_or_default();
    let mut compare_to: Option<String> = None;
    let mut branch_from: Option<String> = None;
    for c in &log {
        let summary = app
            .summaries
            .entry(c.id.clone())
            .or_insert_with(|| app.session.commit_summary(&c.id))
            .clone();
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(0, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(12.0, 16.0), Sense::hover());
                    let col = if c.is_merge {
                        crate::compare::CONFLICT
                    } else if matches!(c.author, Author::Agent(_)) {
                        theme::ACCENT_HOVER
                    } else {
                        theme::MUTED
                    };
                    ui.painter().circle_filled(dot.center(), 4.0, col);
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&c.message).size(12.0).strong())
                                .truncate(),
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}",
                                    author_label(&c.author),
                                    time_ago(c.time)
                                ))
                                .font(mono(10.0))
                                .color(theme::MUTED),
                            );
                            for b in &c.branches {
                                ui.label(
                                    RichText::new(b).font(mono(10.0)).color(theme::ACCENT_HOVER),
                                );
                            }
                        });
                        if !summary.is_empty() && summary != "no differences" {
                            ui.label(RichText::new(&summary).size(11.0).color(theme::MUTED));
                        }
                    });
                });
            })
            .response
            .context_menu(|ui| {
                if ui.button("Compare with what I have now").clicked() {
                    compare_to = Some(c.id.to_string());
                    ui.close();
                }
                if ui.button("New branch from here…").clicked() {
                    branch_from = Some(c.id.to_string());
                    ui.close();
                }
            });
        ui.painter().hline(
            ui.min_rect().x_range(),
            ui.min_rect().bottom(),
            Stroke::new(1.0, Color32::from_black_alpha(40)),
        );
    }
    if let Some(id) = compare_to {
        start_compare(app, &ctx, &id, "working");
    }
    if let Some(id) = branch_from {
        let name = app
            .session
            .unique_branch_name(&format!("from-{}", &id[..6]));
        match app.session.create_branch_at(&name, &id) {
            Ok(()) => {
                app.dirty_since.get_or_insert_with(std::time::Instant::now);
                app.toast(
                    format!("Created branch {name}; switch to it from the branch menu"),
                    false,
                );
            }
            Err(e) => app.toast(e, true),
        }
    }
    let _ = (Align2::LEFT_TOP, ui_font(1.0));
}
