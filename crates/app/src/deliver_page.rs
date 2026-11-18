//! The Deliver page: render settings on the left, the viewer and timeline in the middle (to check the picture and
//! set In and Out marks), and the render queue on the right. Renders run in the background, one after another,
//! while you keep working on any page.

use egui::{pos2, vec2, Rect, Stroke, Ui};

use crate::app::App;
use crate::theme;
use crate::ui::tab;

pub fn show(app: &mut App, root: &mut Ui) {
    crate::timeline::panel(app, root);
    settings_panel(app, root);
    queue_panel(app, root);
    crate::viewer::central(app, root);
}

fn header(ui: &mut Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(8.0);
        tab(ui, title, true);
    });
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        Stroke::new(1.0, theme::BORDER),
    );
}

fn settings_panel(app: &mut App, root: &mut Ui) {
    let avail = root.available_rect_before_wrap();
    egui::Panel::left("deliver_settings")
        .exact_size(app.prefs.pool_w)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(root, |ui| {
            header(ui, "Render Settings");
            // the button stays at the bottom, in view; the settings scroll above it
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| crate::ui::render_button(app, ui));
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Frame::new()
                                .inner_margin(egui::Margin::same(12))
                                .show(ui, |ui| crate::ui::export_settings(app, ui));
                        });
                });
            });
        });
    let w = app.prefs.pool_w;
    crate::ui::drag_edge(
        app,
        root,
        Rect::from_min_size(avail.min, vec2(w, avail.height())),
        crate::keymap::PanelKind::Pool,
    );
}

fn queue_panel(app: &mut App, root: &mut Ui) {
    let avail = root.available_rect_before_wrap();
    egui::Panel::right("deliver_queue")
        .exact_size(app.prefs.inspector_w)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(root, |ui| {
            header(ui, "Render Queue");
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| crate::ui::render_queue(app, ui));
                });
        });
    let w = app.prefs.inspector_w;
    crate::ui::drag_edge(
        app,
        root,
        Rect::from_min_size(
            pos2(avail.right() - w, avail.top()),
            vec2(w, avail.height()),
        ),
        crate::keymap::PanelKind::Inspector,
    );
}
