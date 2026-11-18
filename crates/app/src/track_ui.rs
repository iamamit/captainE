//! Track headers: lock, show/mute, solo, the name and colour, and the right-click menu with everything
//! that can be done to a track.

use captaine_model::{Id, Item, Track, TrackKind};
use captaine_ops::Op;
use egui::{pos2, vec2, Align2, Color32, Painter, Rect, Sense, Stroke, Ui};

use crate::app::App;
use crate::glyphs::{self, Glyph};
use crate::theme::{self, ui_font};

pub const HEADER_W: f32 = 152.0;

/// How tall a track's row is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowSize {
    Small,
    Medium,
    Large,
}

pub const PALETTE: [(&str, [u8; 3]); 8] = [
    ("Red", [0xe5, 0x48, 0x4d]),
    ("Orange", [0xf0, 0x8c, 0x3a]),
    ("Yellow", [0xe8, 0xc5, 0x3a]),
    ("Green", [0x3d, 0xc2, 0x7a]),
    ("Teal", [0x30, 0xb8, 0xc4]),
    ("Blue", [0x4f, 0x8c, 0xf5]),
    ("Purple", [0x8b, 0x6c, 0xf0]),
    ("Pink", [0xe0, 0x6c, 0xb0]),
];

impl App {
    /// Look for plugins in the background the first time they are wanted.
    pub fn ensure_plugin_scan(&mut self, force: bool) {
        if force {
            self.plugin_list = None;
        }
        if self.plugin_list.is_some() || self.plugin_scan.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(captaine_plugins::scan(&[]));
            ctx.request_repaint();
        });
        self.plugin_scan = Some(rx);
    }

    /// Pick up the result of a finished look.
    pub fn poll_plugin_scan(&mut self) {
        if let Some(rx) = &self.plugin_scan {
            if let Ok(list) = rx.try_recv() {
                self.plugin_list = Some(list);
                self.plugin_scan = None;
            }
        }
    }

    /// The controls of a plugin, asked of it once.
    pub fn plugin_controls(
        &mut self,
        plugin: &captaine_plugins::PluginRef,
    ) -> Vec<captaine_plugins::PluginParam> {
        let key = format!("{}@{}", plugin.id, plugin.path);
        if !self.plugin_params.contains_key(&key) {
            let found = captaine_plugins::params(plugin).unwrap_or_default();
            self.plugin_params.insert(key.clone(), found);
        }
        self.plugin_params[&key].clone()
    }

    fn track(&self, id: Id) -> Option<Track> {
        self.timeline()?
            .tracks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.as_ref().clone())
    }

    /// Add `count` tracks of `kind` after the last one of that kind (video: above it).
    pub fn add_tracks(&mut self, kind: TrackKind, count: u32) {
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        if let Ok(op) = captaine_ops::edit::add_tracks(&tl, kind, count as usize, None) {
            self.do_op(op);
        }
    }

    pub fn add_track_beside(&mut self, near: Id) {
        let (Some(tl), Some(t)) = (self.timeline().cloned(), self.track(near)) else {
            return;
        };
        let at = tl.tracks.iter().position(|x| x.id == near).unwrap_or(0);
        let n = tl.tracks.iter().filter(|x| x.kind == t.kind).count() + 1;
        let name = format!("{}{n}", if t.kind == TrackKind::Video { "V" } else { "A" });
        let index = at + 1;
        self.do_op(Op::AddTrack {
            timeline: tl.id,
            index,
            track: Track::new(Id::new(), name, t.kind),
        });
    }

    /// One step up or down in the order the timeline shows (video: higher is on top; audio: first is on top).
    pub fn move_track_step(&mut self, id: Id, up: bool) {
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        let Some(me) = tl.tracks.iter().position(|t| t.id == id) else {
            return;
        };
        let kind = tl.tracks[me].kind;
        let same: Vec<usize> = (0..tl.tracks.len())
            .filter(|&i| tl.tracks[i].kind == kind)
            .collect();
        let at = same.iter().position(|&i| i == me).unwrap_or(0);
        // storage order is bottom-to-top for video, top-to-bottom for audio
        let toward_end = (kind == TrackKind::Video) == up;
        let other = if toward_end {
            same.get(at + 1)
        } else {
            at.checked_sub(1).and_then(|a| same.get(a))
        };
        if let Some(&to) = other {
            self.do_op(Op::MoveTrack {
                track: id,
                index: to,
            });
        }
    }

    pub fn can_move_track(&self, id: Id, up: bool) -> bool {
        let Some(tl) = self.timeline() else {
            return false;
        };
        let Some(me) = tl.tracks.iter().position(|t| t.id == id) else {
            return false;
        };
        let kind = tl.tracks[me].kind;
        let same: Vec<usize> = (0..tl.tracks.len())
            .filter(|&i| tl.tracks[i].kind == kind)
            .collect();
        let at = same.iter().position(|&i| i == me).unwrap_or(0);
        let toward_end = (kind == TrackKind::Video) == up;
        if toward_end {
            at + 1 < same.len()
        } else {
            at > 0
        }
    }

    /// The track captions are written to (a video track named "Captions"), made if it is not there.
    pub fn add_subtitle_track(&mut self) {
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        if tl
            .tracks
            .iter()
            .any(|t| t.name == "Captions" && t.kind == TrackKind::Video)
        {
            return self.toast("There is a Captions track already", false);
        }
        let index = tl
            .tracks
            .iter()
            .rposition(|t| t.kind == TrackKind::Video)
            .map_or(0, |i| i + 1);
        let mut t = Track::new(Id::new(), "Captions", TrackKind::Video);
        t.color = Some([0xe8, 0xc5, 0x3a]);
        self.do_op(Op::AddTrack {
            timeline: tl.id,
            index,
            track: t,
        });
    }

    pub fn delete_track(&mut self, id: Id) {
        let Some(tl) = self.timeline() else { return };
        if tl.tracks.len() <= 1 {
            return self.toast("A sequence needs at least one track", true);
        }
        self.do_op(Op::RemoveTrack { track: id });
    }

    pub fn delete_empty_tracks(&mut self) {
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        let mut left = tl.tracks.len();
        let mut ops = Vec::new();
        for t in tl.tracks.iter() {
            if left > 1 && !t.locked && t.items.iter().all(|i| matches!(i, Item::Gap(_))) {
                ops.push(Op::RemoveTrack { track: t.id });
                left -= 1;
            }
        }
        if ops.is_empty() {
            return self.toast("There are no empty tracks", false);
        }
        let n = ops.len();
        self.do_op(Op::Transaction {
            description: format!("Delete {n} empty track(s)"),
            ops,
        });
    }

    pub fn set_track_style(
        &mut self,
        id: Id,
        name: Option<String>,
        color: Option<Option<[u8; 3]>>,
    ) {
        let Some(t) = self.track(id) else { return };
        let name = name.unwrap_or(t.name.clone());
        let color = color.unwrap_or(t.color);
        if name != t.name || color != t.color {
            self.do_op(Op::SetTrackStyle {
                track: id,
                name,
                color,
            });
        }
    }

    pub fn toggle_track_lock(&mut self, id: Id) {
        if let Some(t) = self.track(id) {
            self.do_op(Op::SetTrackMix {
                track: id,
                pan: t.pan,
                solo: t.solo,
                locked: !t.locked,
            });
        }
    }

    pub fn toggle_track_solo(&mut self, id: Id) {
        if let Some(t) = self.track(id) {
            self.do_op(Op::SetTrackMix {
                track: id,
                pan: t.pan,
                solo: !t.solo,
                locked: t.locked,
            });
        }
    }

    pub fn toggle_track_enabled(&mut self, id: Id) {
        if let Some(t) = self.track(id) {
            self.do_op(Op::SetTrack {
                track: id,
                enabled: !t.enabled,
                gain_db: t.gain_db,
            });
        }
    }

    pub fn row_size(&self, id: Id) -> RowSize {
        self.row_sizes.get(&id).copied().unwrap_or(RowSize::Medium)
    }

    /// Select every clip on a track.
    pub fn select_track_clips(&mut self, id: Id) {
        let Some(t) = self.track(id) else { return };
        let ids: Vec<Id> = t
            .items
            .iter()
            .filter(|i| i.is_content())
            .map(|i| i.id())
            .collect();
        if let Some((first, rest)) = ids.split_first() {
            self.selected_clip = Some(*first);
            self.extra_selected = rest.to_vec();
        }
    }
}

/// The header of one track: colour strip, name, and the lock, eye or speaker and solo buttons.
/// `buttons`: false for a row only partly scrolled into view; its buttons would sit outside the timeline, and
/// placing widgets there would make the panel grow.
pub fn header(app: &mut App, ui: &mut Ui, p: &Painter, head: Rect, track: Id, buttons: bool) {
    let Some(t) = app.track(track) else { return };
    p.rect_filled(head, 0.0, theme::PANEL);
    let accent = t.color.map_or(
        if t.kind == TrackKind::Video {
            theme::ACCENT
        } else {
            theme::AUDIO_WAVE
        },
        |c| Color32::from_rgb(c[0], c[1], c[2]),
    );
    p.rect_filled(
        Rect::from_min_max(head.min, pos2(head.left() + 4.0, head.bottom())),
        0.0,
        accent,
    );
    p.vline(
        head.right(),
        head.y_range(),
        Stroke::new(1.0, theme::BORDER),
    );
    let on = t.enabled;
    p.text(
        pos2(head.left() + 12.0, head.top() + 13.0),
        Align2::LEFT_CENTER,
        &t.name,
        ui_font(12.0),
        if on { Color32::WHITE } else { theme::FAINT },
    );
    let clips = t.items.iter().filter(|i| i.is_content()).count();
    if head.height() >= 40.0 {
        p.text(
            pos2(head.left() + 12.0, head.top() + 29.0),
            Align2::LEFT_CENTER,
            if clips == 1 {
                "1 clip".to_string()
            } else {
                format!("{clips} clips")
            },
            ui_font(10.0),
            theme::FAINT,
        );
    }
    // the buttons, right-aligned on the first line (or centred when the row is short)
    let y = if head.height() >= 40.0 {
        head.top() + 13.0
    } else {
        head.center().y
    };
    let mut x = head.right() - 12.0;
    let mut button = |ui: &mut Ui, g: Glyph, lit: bool, label: &str, tip: &str| {
        let r = Rect::from_center_size(pos2(x, y), vec2(18.0, 18.0));
        x -= 20.0;
        if !buttons {
            return false;
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
            glyphs::small(ui, g, lit, label, tip).clicked()
        })
        .inner
    };
    let name = t.name.clone();
    if button(
        ui,
        if t.locked { Glyph::Lock } else { Glyph::Unlock },
        t.locked,
        &format!("Lock {name}"),
        if t.locked {
            "Unlock this track"
        } else {
            "Lock this track"
        },
    ) {
        app.toggle_track_lock(track);
    }
    let (g_on, g_off, tip_on, tip_off) = if t.kind == TrackKind::Video {
        (
            Glyph::Eye,
            Glyph::EyeOff,
            "Hide this track",
            "Show this track",
        )
    } else {
        (
            Glyph::Speaker,
            Glyph::SpeakerOff,
            "Mute this track",
            "Unmute this track",
        )
    };
    if button(
        ui,
        if on { g_on } else { g_off },
        on,
        &format!(
            "{} {name}",
            if t.kind == TrackKind::Video {
                "Show"
            } else {
                "Mute"
            }
        ),
        if on { tip_on } else { tip_off },
    ) {
        app.toggle_track_enabled(track);
    }
    let targeted = !app.untargeted.contains(&track);
    let tip = if targeted {
        "Edits at the playhead reach this track (click to leave it alone)"
    } else {
        "This track is left alone by splits and range edits (click to include it)"
    };
    if button(
        ui,
        Glyph::Target,
        targeted,
        &format!("Auto-select {name}"),
        tip,
    ) && !app.untargeted.remove(&track)
    {
        app.untargeted.insert(track);
    }
    if t.kind == TrackKind::Audio && buttons {
        let r = Rect::from_center_size(pos2(x, y), vec2(18.0, 18.0));
        let resp = ui
            .scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                if t.solo {
                    ui.painter()
                        .rect_filled(rect, 3.0, Color32::from_rgb(0xe8, 0xc5, 0x3a));
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 3.0, theme::FIELD);
                }
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "S",
                    ui_font(10.5),
                    if t.solo { Color32::BLACK } else { theme::FAINT },
                );
                let n = format!("Solo {name}");
                resp.widget_info(move || {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, n.clone())
                });
                resp.on_hover_text("Hear only soloed tracks")
            })
            .inner;
        if resp.clicked() {
            app.toggle_track_solo(track);
        }
    }
    // the whole header answers a right click
    let hr = ui.interact(
        Rect::from_min_max(head.min, pos2(head.right() - 92.0, head.bottom())),
        egui::Id::new(("track_head", track)),
        Sense::click(),
    );
    if hr.double_clicked() {
        app.track_rename = Some((track, t.name.clone()));
    }
    hr.context_menu(|ui| menu(app, ui, track));
}

/// Everything that can be done to a track.
pub fn menu(app: &mut App, ui: &mut Ui, track: Id) {
    let Some(t) = app.track(track) else { return };
    if ui.button("Add Track").clicked() {
        app.add_track_beside(track);
        ui.close();
    }
    if ui.button("Add Tracks…").clicked() {
        app.add_tracks_dialog = Some((t.kind, 2));
        ui.close();
    }
    if ui.button("Add Subtitle Track").clicked() {
        app.add_subtitle_track();
        ui.close();
    }
    ui.separator();
    if ui
        .add_enabled(
            app.can_move_track(track, true),
            egui::Button::new("Move Track Up"),
        )
        .clicked()
    {
        app.move_track_step(track, true);
        ui.close();
    }
    if ui
        .add_enabled(
            app.can_move_track(track, false),
            egui::Button::new("Move Track Down"),
        )
        .clicked()
    {
        app.move_track_step(track, false);
        ui.close();
    }
    ui.separator();
    if ui.button("Rename Track…").clicked() {
        app.track_rename = Some((track, t.name.clone()));
        ui.close();
    }
    ui.menu_button("Change Track Color", |ui| {
        for (name, c) in PALETTE {
            let (rect, _) = ui.allocate_exact_size(vec2(0.0, 0.0), Sense::hover());
            let _ = rect;
            if ui
                .button(
                    egui::RichText::new(format!("●  {name}"))
                        .color(Color32::from_rgb(c[0], c[1], c[2])),
                )
                .clicked()
            {
                app.set_track_style(track, None, Some(Some(c)));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Default").clicked() {
            app.set_track_style(track, None, Some(None));
            ui.close();
        }
    });
    ui.menu_button("Track Height", |ui| {
        for (label, size) in [
            ("Small", RowSize::Small),
            ("Medium", RowSize::Medium),
            ("Large", RowSize::Large),
        ] {
            if ui
                .selectable_label(app.row_size(track) == size, label)
                .clicked()
            {
                app.row_sizes.insert(track, size);
                ui.close();
            }
        }
    });
    ui.separator();
    if ui
        .button(if t.locked {
            "Unlock Track"
        } else {
            "Lock Track"
        })
        .clicked()
    {
        app.toggle_track_lock(track);
        ui.close();
    }
    let (on_label, off_label) = if t.kind == TrackKind::Video {
        ("Hide Track", "Show Track")
    } else {
        ("Mute Track", "Unmute Track")
    };
    if ui
        .button(if t.enabled { on_label } else { off_label })
        .clicked()
    {
        app.toggle_track_enabled(track);
        ui.close();
    }
    if t.kind == TrackKind::Audio
        && ui
            .button(if t.solo { "Unsolo Track" } else { "Solo Track" })
            .clicked()
    {
        app.toggle_track_solo(track);
        ui.close();
    }
    let targeted = !app.untargeted.contains(&track);
    if ui
        .button(if targeted {
            "Leave Track Out of Auto-select"
        } else {
            "Include Track in Auto-select"
        })
        .clicked()
    {
        if !app.untargeted.remove(&track) {
            app.untargeted.insert(track);
        }
        ui.close();
    }
    if ui.button("Select All Clips on Track").clicked() {
        app.select_track_clips(track);
        ui.close();
    }
    ui.separator();
    if ui.button("Delete Track").clicked() {
        app.delete_track(track);
        ui.close();
    }
    if ui.button("Delete Empty Tracks").clicked() {
        app.delete_empty_tracks();
        ui.close();
    }
}

/// The small windows: renaming a track and adding several at once.
pub fn dialogs(app: &mut App, ctx: &egui::Context) {
    if let Some((id, mut name)) = app.track_rename.take() {
        let (mut keep, mut done) = (true, false);
        egui::Window::new("Rename track")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let r = ui.text_edit_singleline(&mut name);
                r.request_focus();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        done = true;
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        keep = false;
                    }
                });
            });
        if done {
            app.set_track_style(id, Some(name.trim().to_string()), None);
        } else if keep {
            app.track_rename = Some((id, name));
        }
    }
    if let Some((mut kind, mut n)) = app.add_tracks_dialog.take() {
        let (mut keep, mut done) = (true, false);
        egui::Window::new("Add tracks")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (k, label) in [(TrackKind::Video, "Video"), (TrackKind::Audio, "Audio")] {
                        if crate::ui::chip(ui, label, kind == k, true).clicked() {
                            kind = k;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("How many");
                    ui.add(egui::DragValue::new(&mut n).range(1..=16));
                });
                ui.horizontal(|ui| {
                    if ui.button("Add").clicked() {
                        done = true;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        if done {
            app.add_tracks(kind, n);
        } else if keep {
            app.add_tracks_dialog = Some((kind, n));
        }
    }
}
