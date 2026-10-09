//! Markers: pins on the ruler that mark moments and chapters, a list window and the commands around them.

use captaine_model::{Id, Marker};
use captaine_ops::Op;
use egui::{vec2, Color32, RichText};

use crate::app::App;
use crate::theme;
use crate::ui::chip;

impl App {
    pub fn markers(&self) -> Vec<Marker> {
        self.timeline()
            .map(|t| t.markers.clone())
            .unwrap_or_default()
    }

    /// Pin a marker at `tick` (the playhead by default), named "Marker N" unless given a name.
    pub fn add_marker(&mut self, tick: Option<i64>, name: Option<String>) -> Option<Id> {
        let tl = self.timeline_id()?;
        let tick = tick
            .unwrap_or(self.playhead)
            .clamp(0, self.duration_ticks().max(self.playhead));
        let n = self.markers().len() + 1;
        let m = Marker::new(
            Id::new(),
            tick,
            name.unwrap_or_else(|| format!("Marker {n}")),
        );
        let id = m.id;
        self.do_op(Op::AddMarker {
            timeline: tl,
            marker: m,
        })
        .then_some(id)
    }

    pub fn update_marker(&mut self, m: Marker) {
        if let Some(tl) = self.timeline_id() {
            self.do_op(Op::SetMarker {
                timeline: tl,
                marker: m,
            });
        }
    }

    pub fn remove_marker(&mut self, id: Id) {
        if let Some(tl) = self.timeline_id() {
            self.do_op(Op::RemoveMarker {
                timeline: tl,
                marker: id,
            });
        }
    }

    /// Move the playhead to the next (`dir` > 0) or previous marker.
    pub fn jump_marker(&mut self, dir: i32) {
        let ms = self.markers();
        let target = if dir > 0 {
            ms.iter().map(|m| m.tick).find(|t| *t > self.playhead)
        } else {
            ms.iter().rev().map(|m| m.tick).find(|t| *t < self.playhead)
        };
        match target {
            Some(t) => self.set_playhead(t),
            None => self.toast(
                if dir > 0 {
                    "No later marker"
                } else {
                    "No earlier marker"
                },
                false,
            ),
        }
    }

    /// The markers as chapter lines, `0:00 Name`, the way video sites read them.
    pub fn chapters_text(&self) -> String {
        let fps = self.fps();
        let mut lines: Vec<String> = Vec::new();
        for m in self.markers() {
            let s = (m.tick as f64 / fps).round() as i64;
            let t = if s >= 3600 {
                format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
            } else {
                format!("{}:{:02}", s / 60, s % 60)
            };
            lines.push(format!("{t} {}", m.name));
        }
        // sites want the first chapter at 0:00
        if lines.first().is_some_and(|l| !l.starts_with("0:00")) {
            lines.insert(0, "0:00 Start".into());
        }
        lines.join("\n")
    }
}

/// The marker under screen position `x` on the ruler, if one is within a few pixels.
pub fn marker_at(markers: &[Marker], x_of: &dyn Fn(i64) -> f32, x: f32) -> Option<Id> {
    markers
        .iter()
        .map(|m| (m.id, (x_of(m.tick) - x).abs()))
        .filter(|(_, d)| *d <= 7.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

pub fn color_of(m: &Marker) -> Color32 {
    let c = Marker::COLORS[(m.color as usize).min(Marker::COLORS.len() - 1)].1;
    Color32::from_rgb(c[0], c[1], c[2])
}

/// The list window (toolbar "Markers") and the edit window for one marker.
pub fn windows(app: &mut App, ctx: &egui::Context) {
    if app.markers_open {
        let mut open = true;
        egui::Window::new("Markers")
            .open(&mut open)
            .collapsible(false)
            .default_width(360.0)
            .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-16.0, -200.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if chip(ui, "Add at playhead  M", false, true).clicked() {
                        app.add_marker(None, None);
                    }
                    if chip(ui, "Copy chapters", false, !app.markers().is_empty()).clicked() {
                        let text = app.chapters_text();
                        ui.ctx().copy_text(text);
                        app.toast("Chapters copied", false);
                    }
                });
                ui.add_space(6.0);
                let ms = app.markers();
                if ms.is_empty() {
                    ui.label(
                        RichText::new("No markers yet. Press M to pin one at the playhead.")
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                }
                let mut jump = None;
                let mut edit = None;
                let mut remove = None;
                for m in &ms {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 5.0, color_of(m));
                        ui.label(
                            RichText::new(app.tc(m.tick))
                                .font(crate::theme::mono(11.0))
                                .color(theme::MUTED),
                        );
                        if ui.link(&m.name).clicked() {
                            jump = Some(m.tick);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if chip(ui, "Delete", false, true).clicked() {
                                remove = Some(m.id);
                            }
                            if chip(ui, "Edit", false, true).clicked() {
                                edit = Some(m.clone());
                            }
                        });
                    });
                }
                if let Some(t) = jump {
                    app.set_playhead(t);
                }
                if let Some(m) = edit {
                    app.marker_edit = Some(m);
                }
                if let Some(id) = remove {
                    app.remove_marker(id);
                }
            });
        app.markers_open = open;
    }
    if let Some(mut m) = app.marker_edit.take() {
        let (mut keep, mut save, mut delete) = (true, false, false);
        egui::Window::new("Marker")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut m.name);
                ui.label("Note");
                ui.text_edit_multiline(&mut m.note);
                ui.horizontal(|ui| {
                    for (i, (name, rgb)) in Marker::COLORS.iter().enumerate() {
                        let on = m.color as usize == i;
                        let (r, resp) =
                            ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
                        resp.widget_info(|| {
                            egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, *name)
                        });
                        ui.painter().circle_filled(
                            r.center(),
                            8.0,
                            Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                        );
                        if on {
                            ui.painter().circle_stroke(
                                r.center(),
                                10.0,
                                egui::Stroke::new(1.5, Color32::WHITE),
                            );
                        }
                        if resp.clicked() {
                            m.color = i as u8;
                        }
                    }
                });
                let fps = app.fps();
                let mut secs = m.duration as f64 / fps;
                ui.horizontal(|ui| {
                    ui.label("Length (s, 0 = a point)");
                    if ui
                        .add(
                            egui::DragValue::new(&mut secs)
                                .range(0.0..=3600.0)
                                .speed(0.1),
                        )
                        .changed()
                    {
                        m.duration = (secs * fps).round() as i64;
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command)
                    {
                        save = true;
                    }
                    if ui.button("Delete").clicked() {
                        delete = true;
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        keep = false;
                    }
                });
            });
        if save {
            app.update_marker(m);
        } else if delete {
            app.remove_marker(m.id);
        } else if keep {
            app.marker_edit = Some(m);
        }
    }
}
