//! The timeline: toolbar, ruler, track headers, clips with waveforms, playhead and every editing gesture.

use captaine_audio::Peak;
use captaine_model::*;
use captaine_ops::{edit, Edge, Op};
use egui::{pos2, vec2, Align2, Color32, Context, Painter, Rect, RichText, Sense, Shape, Stroke};

use crate::app::*;
use crate::glyphs::{self, Glyph};
use crate::keymap::Action;
use crate::theme::{self, mono, ui_font};
use crate::ui::{self, chip, LayerDrag, MediaDrag, TitleDrag};

use crate::track_ui::{RowSize, HEADER_W};
const RULER_H: f32 = 26.0;
const EDGE_PX: f32 = 7.0;

/// Zoom so the whole timeline fits the visible width.
pub fn fit(app: &mut App) {
    let secs = (app.duration_ticks() as f64 / app.fps()).max(1.0);
    let width = (app.central_rect.width() + 640.0 - HEADER_W - 40.0).max(300.0);
    app.pps = (width as f64 / (secs * 1.05)) as f32;
    app.scroll = 0.0;
}

pub fn panel(app: &mut App, root: &mut egui::Ui) {
    let ctx = root.ctx().clone();
    let shown = egui::Panel::bottom("timeline")
        .resizable(true)
        .default_size(app.prefs.timeline_h)
        .min_size(180.0)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL_DARK)
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(root, |ui| {
            toolbar(app, ui);
            body(app, ui);
        });
    ui::note_panel(
        app,
        &ctx,
        shown.response.rect.height(),
        crate::keymap::PanelKind::Timeline,
    );
    drag_ghost(app, &ctx);
}

fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (t, g, l) in [
                    (Tool::Select, Glyph::Select, "Select"),
                    (Tool::Trim, Glyph::Trim, "Trim"),
                    (Tool::Roll, Glyph::Roll, "Roll"),
                    (Tool::Slip, Glyph::Slip, "Slip"),
                    (Tool::Slide, Glyph::Slide, "Slide"),
                    (Tool::Blade, Glyph::Blade, "Blade"),
                    (Tool::Hand, Glyph::Hand, "Hand"),
                    (Tool::Zoom, Glyph::Zoom, "Zoom"),
                ] {
                    let key = tool_action(t).map_or(String::new(), |a| app.keymap.label(a));
                    if glyphs::button(ui, g, app.tool == t, true, l, &key).clicked() {
                        app.tool = t;
                    }
                }
                separator(ui);
                if glyphs::button(
                    ui,
                    Glyph::Magnet,
                    app.snap,
                    true,
                    "Snap",
                    &app.keymap.label(Action::Snap),
                )
                .clicked()
                {
                    app.snap = !app.snap;
                }
                if glyphs::button(
                    ui,
                    Glyph::Link,
                    app.link_on,
                    true,
                    "Link",
                    "linked clips are edited together",
                )
                .clicked()
                {
                    app.link_on = !app.link_on;
                }
                if glyphs::button(
                    ui,
                    Glyph::Flag,
                    false,
                    true,
                    "Add marker",
                    &app.keymap.label(Action::Marker),
                )
                .clicked()
                {
                    app.add_marker(None, None);
                }
                if glyphs::button(
                    ui,
                    Glyph::List,
                    app.markers_open,
                    true,
                    "Markers",
                    "list the markers",
                )
                .clicked()
                {
                    app.markers_open = !app.markers_open;
                }
                separator(ui);
                let has_src = app.source_media().is_some();
                if glyphs::button(
                    ui,
                    Glyph::Insert,
                    false,
                    has_src,
                    "Insert",
                    "from the source monitor, at the playhead (,)",
                )
                .clicked()
                {
                    app.source_edit(true);
                }
                if glyphs::button(
                    ui,
                    Glyph::Overwrite,
                    false,
                    has_src,
                    "Overwrite",
                    "from the source monitor, at the playhead (.)",
                )
                .clicked()
                {
                    app.source_edit(false);
                }
                let rec = app.voiceover.is_some();
                if glyphs::button(
                    ui,
                    Glyph::Mic,
                    rec,
                    true,
                    "Voice-over",
                    "record the microphone while the timeline plays",
                )
                .clicked()
                {
                    app.voiceover_toggle();
                }
                if rec {
                    ui.ctx().request_repaint();
                }
                if let Some(mc) = app.multicam.as_ref().map(|m| m.angles.len()) {
                    ui.label(RichText::new("Multicam").size(12.0).color(theme::ACCENT));
                    for k in 0..mc {
                        if chip(ui, &format!("Cut to {}", k + 1), false, true).clicked() {
                            app.multicam_cut(k, None);
                        }
                    }
                    if chip(ui, "Done", false, true).clicked() {
                        app.end_multicam();
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if glyphs::button(ui, Glyph::Plus, false, true, "Zoom in", "=").clicked() {
                        app.pps = (app.pps * 1.25).min(600.0);
                    }
                    // zoom slider (logarithmic)
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(90.0, 20.0), Sense::click_and_drag());
                    let (lo, hi) = (1.0f32.ln(), 600.0f32.ln());
                    let t = ((app.pps.ln() - lo) / (hi - lo)).clamp(0.0, 1.0);
                    let (x0, x1, y) = (rect.left() + 4.0, rect.right() - 4.0, rect.center().y);
                    ui.painter()
                        .line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(3.0, theme::FIELD));
                    ui.painter().line_segment(
                        [pos2(x0, y), pos2(x0 + (x1 - x0) * t, y)],
                        Stroke::new(3.0, theme::MUTED),
                    );
                    ui.painter()
                        .circle_filled(pos2(x0 + (x1 - x0) * t, y), 4.5, Color32::WHITE);
                    if resp.dragged() || resp.clicked() {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            let nt = ((pos.x - x0) / (x1 - x0)).clamp(0.0, 1.0);
                            app.pps = (lo + (hi - lo) * nt).exp();
                        }
                    }
                    if glyphs::button(ui, Glyph::Minus, false, true, "Zoom out", "-").clicked() {
                        app.pps = (app.pps / 1.25).max(1.0);
                    }
                    if glyphs::button(
                        ui,
                        Glyph::Fit,
                        false,
                        true,
                        "Fit timeline",
                        &app.keymap.label(Action::FitTimeline),
                    )
                    .clicked()
                    {
                        crate::timeline::fit(app);
                    }
                    ui.add_space(8.0);
                    let name = app.timeline().map_or(String::new(), |t| t.name.clone());
                    let cur = app.timeline_id();
                    ui.menu_button(
                        RichText::new(format!("{name} ▾")).strong().size(12.0),
                        |ui| {
                            for (id, n) in app.sequences() {
                                if ui.selectable_label(Some(id) == cur, n).clicked() {
                                    app.set_active_timeline(id);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.button("New Sequence").clicked() {
                                app.new_sequence();
                                ui.close();
                            }
                            if ui.button("Duplicate Sequence").clicked() {
                                app.duplicate_sequence();
                                ui.close();
                            }
                            ui.menu_button("Auto Reframe To", |ui| {
                                for (label, w, h) in crate::vision_jobs::REFRAMES {
                                    if ui.button(label).clicked() {
                                        app.auto_reframe(label, (w, h));
                                        ui.close();
                                    }
                                }
                            });
                            if ui.button("Rename Sequence…").clicked() {
                                app.rename_sequence = cur.map(|id| (id, name.clone()));
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    app.sequences().len() > 1,
                                    egui::Button::new("Delete Sequence"),
                                )
                                .clicked()
                            {
                                if let Some(id) = cur {
                                    app.delete_sequence(id);
                                }
                                ui.close();
                            }
                        },
                    );
                    ui.add_space(8.0);
                    if glyphs::button(
                        ui,
                        Glyph::Extract,
                        false,
                        true,
                        "Extract",
                        "delete and close the gap (⇧⌫)",
                    )
                    .clicked()
                    {
                        ui::extract_selection(app);
                    }
                    if glyphs::button(
                        ui,
                        Glyph::Lift,
                        false,
                        true,
                        "Lift",
                        "delete and leave a gap (⌫)",
                    )
                    .clicked()
                    {
                        ui::lift_selection(app);
                    }
                });
            });
        });
}

fn separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.painter().vline(
        ui.cursor().left(),
        ui.max_rect().y_range(),
        Stroke::new(1.0, theme::BORDER),
    );
    ui.add_space(6.0);
}

fn tool_action(t: Tool) -> Option<Action> {
    Some(match t {
        Tool::Select => Action::ToolSelect,
        Tool::Trim => Action::ToolTrim,
        Tool::Roll => Action::ToolRoll,
        Tool::Slip => Action::ToolSlip,
        Tool::Slide => Action::ToolSlide,
        Tool::Blade => Action::ToolBlade,
        Tool::Hand => Action::ToolHand,
        Tool::Zoom => Action::ToolZoom,
    })
}

struct ClipRect {
    id: Id,
    track: Id,
    rect: Rect,
    start: i64,
    dur: i64,
    is_gap: bool,
}

struct Row {
    track: Id,
    kind: TrackKind,
    color: Option<[u8; 3]>,
    enabled: bool,
    top: f32,
    h: f32,
}

/// Peaks for display: `from..to` are indices into `peaks` (100 per second of source).
pub fn draw_wave(p: &Painter, rect: Rect, peaks: &[Peak], from: f32, to: f32, color: Color32) {
    if peaks.is_empty() || rect.width() < 1.0 {
        return;
    }
    let mid = rect.center().y;
    let half = rect.height() / 2.0;
    let cols = rect.width().ceil() as usize;
    let per_col = (to - from) / rect.width();
    for c in 0..cols {
        let a = (from + c as f32 * per_col).max(0.0) as usize;
        let b = ((from + (c as f32 + 1.0) * per_col).ceil() as usize)
            .max(a + 1)
            .min(peaks.len());
        if a >= peaks.len() {
            break;
        }
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        for pk in &peaks[a..b] {
            lo = lo.min(pk.min);
            hi = hi.max(pk.max);
        }
        let x = rect.left() + c as f32 + 0.5;
        let (y0, y1) = (
            mid - hi.clamp(0.0, 1.0) * half,
            mid - lo.clamp(-1.0, 0.0) * half,
        );
        p.line_segment(
            [pos2(x, y0), pos2(x, y1.max(y0 + 1.0))],
            Stroke::new(1.0, color),
        );
    }
}

struct View {
    left: f32, // x of timeline time 0 on screen when scroll = 0
    pps: f32,
    scroll: f32,
    fps: f64,
}

impl View {
    fn x(&self, tick: i64) -> f32 {
        self.left + ((tick as f64 / self.fps) as f32 - self.scroll) * self.pps
    }
    fn tick(&self, x: f32) -> i64 {
        (((x - self.left) / self.pps + self.scroll) as f64 * self.fps).round() as i64
    }
}

fn body(app: &mut App, ui: &mut egui::Ui) {
    let area = ui.available_rect_before_wrap();
    ui.take_available_space(); // a resizable panel keeps its height only if its content fills it
    let ruler = Rect::from_min_size(
        pos2(area.left() + HEADER_W, area.top()),
        vec2(area.width() - HEADER_W, RULER_H),
    );
    let tracks_top = ruler.bottom();
    let tl = match app.timeline() {
        Some(t) => t.clone(),
        None => return,
    };
    // rows: video tracks top-down (highest first), then audio tracks
    let mut order: Vec<&std::sync::Arc<Track>> = tl
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Video)
        .collect();
    order.reverse();
    order.extend(tl.tracks.iter().filter(|t| t.kind == TrackKind::Audio));
    let n = order.len().max(1) as f32;
    let row_h = ((area.bottom() - tracks_top) / n).clamp(34.0, 56.0);
    let mut top = tracks_top;
    let rows: Vec<Row> = order
        .iter()
        .map(|t| {
            let h = match app.row_size(t.id) {
                RowSize::Small => 28.0,
                RowSize::Medium => row_h,
                RowSize::Large => 88.0,
            };
            let r = Row {
                track: t.id,
                kind: t.kind,
                color: t.color,
                enabled: t.enabled,
                top,
                h,
            };
            top += h;
            r
        })
        .collect();
    let body_rect = Rect::from_min_max(
        pos2(area.left() + HEADER_W, tracks_top),
        pos2(area.right(), area.bottom()),
    );
    let view = View {
        left: ruler.left(),
        pps: app.pps,
        scroll: app.scroll,
        fps: app.fps(),
    };

    app.timeline_layout = Some(TimelineLayout {
        origin_x: ruler.left(),
        scroll: app.scroll,
        pps: app.pps,
        ruler,
        rows: rows.iter().map(|r| (r.track, r.top, r.h)).collect(),
    });

    // gather clip rectangles
    let mut clips: Vec<ClipRect> = Vec::new();
    for row in &rows {
        let track = tl
            .tracks
            .iter()
            .find(|t| t.id == row.track)
            .expect("row track");
        for (item, start) in track.items.iter().zip(track.item_starts()) {
            let dur = item.duration().value;
            let r = Rect::from_min_max(
                pos2(view.x(start), row.top + 3.0),
                pos2(view.x(start + dur), row.top + row.h - 3.0),
            );
            clips.push(ClipRect {
                id: item.id(),
                track: row.track,
                rect: r,
                start,
                dur,
                is_gap: !item.is_content(),
            });
        }
    }

    let resp = ui.interact(
        Rect::from_min_max(pos2(area.left() + HEADER_W, area.top()), area.max),
        egui::Id::new("timeline_area"),
        Sense::click_and_drag(),
    );
    let pointer = resp.hover_pos();

    // wheel: pinch or ⌘-scroll zooms around the pointer, plain scroll pans
    if let Some(pos) = pointer {
        let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
        if zoom != 1.0 {
            let anchor = (pos.x - view.left) / app.pps + app.scroll;
            app.pps = (app.pps * zoom).clamp(1.0, 600.0);
            app.scroll = anchor - (pos.x - view.left) / app.pps;
        } else if scroll != egui::Vec2::ZERO {
            let d = if scroll.x.abs() > scroll.y.abs() {
                scroll.x
            } else {
                scroll.y
            };
            app.scroll -= d / app.pps;
        }
        app.scroll = app.scroll.max(-2.0);
    }
    let view = View {
        left: ruler.left(),
        pps: app.pps,
        scroll: app.scroll,
        fps: app.fps(),
    };
    let end_tick = app.duration_ticks();

    // ---- interaction ---------------------------------------------------------------------------
    let hit = |pos: egui::Pos2| clips.iter().find(|c| !c.is_gap && c.rect.contains(pos));
    let snap_threshold = (8.0 / view.pps as f64 * view.fps).round().max(1.0) as i64;
    let mut snapped: Option<i64> = None;
    let mut do_snap = |app: &App, tick: i64, exclude: Option<Id>, out: &mut Option<i64>| -> i64 {
        if !app.snap {
            return tick;
        }
        let mut best: Option<(i64, i64)> = None;
        let mut consider = |c: i64| {
            let d = (c - tick).abs();
            if d <= snap_threshold && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, c));
            }
        };
        consider(0);
        consider(app.playhead);
        for m in [app.mark_in, app.mark_out].into_iter().flatten() {
            consider(m);
        }
        for c in clips.iter().filter(|c| Some(c.id) != exclude) {
            consider(c.start);
            consider(c.start + c.dur);
        }
        *out = best.map(|(_, c)| c);
        best.map_or(tick, |(_, c)| c)
    };

    let press = ui.input(|i| i.pointer.press_origin());
    if resp.drag_started() {
        if let Some(origin) = press {
            if ruler.contains(origin) {
                app.drag = Drag::Playhead;
                let t = view.tick(origin.x).clamp(0, end_tick.max(0));
                app.set_playhead(t);
            } else if app.tool == Tool::Hand {
                app.drag = Drag::Pan {
                    start_scroll: app.scroll,
                };
            } else if matches!(app.tool, Tool::Roll | Tool::Slip | Tool::Slide) {
                let under = clips.iter().find(|c| c.rect.contains(origin));
                if let Some(c) = under {
                    match app.tool {
                        Tool::Roll => {
                            // the cut nearest to the press: the clip's own start or end
                            let near_start = origin.x - c.rect.left() < c.rect.right() - origin.x;
                            let other = clips.iter().find(|o| {
                                o.track == c.track
                                    && if near_start {
                                        o.start + o.dur == c.start
                                    } else {
                                        o.start == c.start + c.dur
                                    }
                            });
                            if let Some(o) = other {
                                let (left, right) = if near_start { (o, c) } else { (c, o) };
                                app.drag = Drag::Roll {
                                    left: left.id,
                                    right: right.id,
                                    delta: 0,
                                };
                            } else {
                                app.toast("There is nothing on the other side of that cut", false);
                            }
                        }
                        Tool::Slip if !c.is_gap => {
                            app.select_with_links(c.id);
                            app.drag = Drag::Slip {
                                item: c.id,
                                delta: 0,
                            };
                        }
                        Tool::Slide if !c.is_gap => {
                            app.select_with_links(c.id);
                            app.drag = Drag::Slide {
                                item: c.id,
                                delta: 0,
                            };
                        }
                        _ => {}
                    }
                }
            } else if matches!(app.tool, Tool::Select | Tool::Trim) {
                if let Some(c) = hit(origin) {
                    if ui.input(|i| i.modifiers.shift) {
                        app.toggle_selected(c.id);
                    } else if !app.is_selected(c.id) {
                        app.select_with_links(c.id);
                    }
                    let edge_px = if app.tool == Tool::Trim {
                        EDGE_PX + 4.0
                    } else {
                        EDGE_PX
                    };
                    let wide = c.rect.width() > edge_px * 3.0;
                    if wide && origin.x - c.rect.left() <= edge_px {
                        app.drag = Drag::Trim {
                            item: c.id,
                            edge: Edge::In,
                            delta: 0,
                        };
                    } else if wide && c.rect.right() - origin.x <= edge_px {
                        app.drag = Drag::Trim {
                            item: c.id,
                            edge: Edge::Out,
                            delta: 0,
                        };
                    } else {
                        app.drag = Drag::Clip {
                            item: c.id,
                            grab_ticks: view.tick(origin.x) - c.start,
                            target_track: c.track,
                            target_start: c.start,
                        };
                    }
                }
            }
        }
    }
    if resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            match app.drag.clone() {
                Drag::Playhead => {
                    let t = do_snap(app, view.tick(pos.x), None, &mut snapped)
                        .clamp(0, end_tick.max(0));
                    app.set_playhead(t);
                }
                Drag::Pan { .. } => app.scroll -= resp.drag_delta().x / app.pps,
                Drag::Clip {
                    item,
                    grab_ticks,
                    target_track,
                    ..
                } => {
                    let origin = clips.iter().find(|c| c.id == item);
                    let (dur, from_track) = origin.map_or((1, target_track), |c| (c.dur, c.track));
                    let mut start = view.tick(pos.x) - grab_ticks;
                    // snap the clip's start or its end, whichever is closer to something
                    let mut s1 = None;
                    let mut s2 = None;
                    let a = do_snap(app, start, Some(item), &mut s1);
                    let b = do_snap(app, start + dur, Some(item), &mut s2) - dur;
                    if s1.is_some() && (s2.is_none() || (a - start).abs() <= (b - start).abs()) {
                        start = a;
                        snapped = s1;
                    } else if s2.is_some() {
                        start = b;
                        snapped = s2;
                    }
                    let start = start.max(0);
                    // same kind of track only
                    let kind = rows.iter().find(|r| r.track == from_track).map(|r| r.kind);
                    let track = rows
                        .iter()
                        .find(|r| pos.y >= r.top && pos.y < r.top + r.h && Some(r.kind) == kind)
                        .map_or(target_track, |r| r.track);
                    app.drag = Drag::Clip {
                        item,
                        grab_ticks,
                        target_track: track,
                        target_start: start,
                    };
                }
                Drag::Roll { left, right, .. } => {
                    if let (Some(l), Some(r)) = (
                        clips.iter().find(|c| c.id == left),
                        clips.iter().find(|c| c.id == right),
                    ) {
                        let t = do_snap(app, view.tick(pos.x), None, &mut snapped);
                        let delta = (t - r.start).clamp(-(l.dur - 1), r.dur - 1);
                        app.drag = Drag::Roll { left, right, delta };
                    }
                }
                Drag::Slip { item, .. } => {
                    let press_x = ui
                        .input(|i| i.pointer.press_origin())
                        .map_or(pos.x, |p| p.x);
                    let delta = view.tick(pos.x) - view.tick(press_x);
                    app.drag = Drag::Slip { item, delta };
                }
                Drag::Slide { item, .. } => {
                    if let Some(c) = clips.iter().find(|c| c.id == item) {
                        let prev = clips
                            .iter()
                            .find(|o| o.track == c.track && o.start + o.dur == c.start);
                        let next = clips
                            .iter()
                            .find(|o| o.track == c.track && o.start == c.start + c.dur);
                        let press_x = ui
                            .input(|i| i.pointer.press_origin())
                            .map_or(pos.x, |p| p.x);
                        let raw = view.tick(pos.x) - view.tick(press_x);
                        let lo = -prev.map_or(0, |p| p.dur - 1);
                        let hi = next.map_or(0, |n| n.dur - 1);
                        app.drag = Drag::Slide {
                            item,
                            delta: raw.clamp(lo, hi),
                        };
                    }
                }
                Drag::Trim { item, edge, .. } => {
                    if let Some(c) = clips.iter().find(|c| c.id == item) {
                        let raw = view.tick(pos.x);
                        let t = do_snap(app, raw, Some(item), &mut snapped);
                        let mut delta = match edge {
                            Edge::In => t - c.start,
                            Edge::Out => t - (c.start + c.dur),
                        };
                        // keep at least one frame
                        delta = match edge {
                            Edge::In => delta.min(c.dur - 1),
                            Edge::Out => delta.max(-(c.dur - 1)),
                        };
                        app.drag = Drag::Trim { item, edge, delta };
                    }
                }
                Drag::None => {}
            }
        }
    }
    if resp.drag_stopped() {
        match std::mem::replace(&mut app.drag, Drag::None) {
            Drag::Clip {
                item,
                target_track,
                target_start,
                ..
            } => {
                let orig = clips
                    .iter()
                    .find(|c| c.id == item)
                    .map(|c| (c.track, c.start));
                if orig != Some((target_track, target_start)) {
                    let delta = orig.map_or(0, |(_, s)| target_start - s);
                    let others: Vec<(Id, Id, i64)> = app
                        .companions_of(item)
                        .into_iter()
                        .filter_map(|id| {
                            clips
                                .iter()
                                .find(|c| c.id == id)
                                .map(|c| (id, c.track, c.start))
                        })
                        .collect();
                    if others.is_empty() {
                        let at = app.time(target_start);
                        app.do_plan(|p| edit::move_item(p, item, target_track, at));
                    } else {
                        // the whole selection moves by the same amount in time; only the dragged clip changes track
                        let mut p = app.project().clone();
                        let mut ops = Vec::new();
                        let mut failed = None;
                        match edit::move_item(&p, item, target_track, app.time(target_start)) {
                            Ok(op) => match captaine_ops::apply(&p, &op) {
                                Ok((n, _)) => {
                                    p = n;
                                    ops.push(op);
                                }
                                Err(e) => failed = Some(e.to_string()),
                            },
                            Err(e) => failed = Some(e.to_string()),
                        }
                        for (id, tr, st) in &others {
                            if failed.is_some() {
                                break;
                            }
                            let at = app.time((st + delta).max(0));
                            match edit::move_item(&p, *id, *tr, at).and_then(|op| {
                                let (n, _) = captaine_ops::apply(&p, &op)?;
                                p = n;
                                Ok(op)
                            }) {
                                Ok(op) => ops.push(op),
                                Err(e) => failed = Some(e.to_string()),
                            }
                        }
                        match failed {
                            Some(e) => app.toast(e, true),
                            None => {
                                app.do_op(Op::Transaction {
                                    description: format!("Move {} clips", ops.len()),
                                    ops,
                                });
                            }
                        }
                    }
                }
            }
            Drag::Roll { left, right, delta } if delta != 0 => {
                let d = app.time(delta);
                app.do_plan(|p| edit::roll(p, left, right, d));
            }
            Drag::Slip { item, delta } if delta != 0 => {
                // dragging right shows earlier footage, as in other editors
                let d = app.time(-delta);
                let mut ops = vec![Op::Slip { item, delta: d }];
                if app.link_on {
                    ops.extend(
                        app.link_partners(item)
                            .into_iter()
                            .filter(|p| app.is_selected(*p))
                            .map(|p| Op::Slip { item: p, delta: d }),
                    );
                }
                if ops.len() == 1 {
                    app.do_op(ops.remove(0));
                } else {
                    app.do_op(Op::Transaction {
                        description: "Slip linked clips".into(),
                        ops,
                    });
                }
            }
            Drag::Slide { item, delta } if delta != 0 => {
                let d = app.time(delta);
                app.do_plan(|p| edit::slide(p, item, d));
            }
            Drag::Trim { item, edge, delta } if delta != 0 => {
                let d = app.time(delta);
                // clips linked to this one are trimmed the same way
                let mut ops = vec![Op::Trim {
                    item,
                    edge,
                    delta: d,
                }];
                if app.link_on {
                    ops.extend(
                        app.link_partners(item)
                            .into_iter()
                            .filter(|p| app.is_selected(*p))
                            .map(|p| Op::Trim {
                                item: p,
                                edge,
                                delta: d,
                            }),
                    );
                }
                if ops.len() == 1 {
                    app.do_op(ops.remove(0));
                } else {
                    app.do_op(Op::Transaction {
                        description: "Trim linked clips".into(),
                        ops,
                    });
                }
            }
            _ => {}
        }
    }
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if ruler.contains(pos) {
                let ms = app.markers();
                match crate::markers_ui::marker_at(&ms, &|t| view.x(t), pos.x) {
                    Some(id) => {
                        let m = ms.iter().find(|m| m.id == id).expect("hit marker exists");
                        app.set_playhead(m.tick);
                        if ui.input(|i| i.modifiers.alt) {
                            app.marker_edit = Some(m.clone());
                        }
                    }
                    None => app.set_playhead(view.tick(pos.x).clamp(0, end_tick.max(0))),
                }
            } else {
                match app.tool {
                    Tool::Blade => {
                        if let Some(c) = clips.iter().find(|c| c.rect.contains(pos)) {
                            let mut s = None;
                            let t = do_snap(app, view.tick(pos.x), None, &mut s);
                            let (track, at) = (c.track, app.time(t));
                            app.do_plan(|p| edit::split_at(p, track, at));
                        }
                    }
                    Tool::Zoom => {
                        let zoom_out = ui.input(|i| i.modifiers.alt);
                        let anchor = (pos.x - view.left) / app.pps + app.scroll;
                        app.pps = (app.pps * if zoom_out { 0.7 } else { 1.43 }).clamp(1.0, 600.0);
                        app.scroll = anchor - (pos.x - view.left) / app.pps;
                    }
                    _ => {
                        let shift = ui.input(|i| i.modifiers.shift);
                        match (hit(pos), shift) {
                            (Some(c), true) => app.toggle_selected(c.id),
                            (Some(c), false) => app.select_with_links(c.id),
                            (None, _) => app.select_only(&[]),
                        }
                        if app.selected_clip.is_some() {
                            app.inspector_tab = InspectorTab::Inspector;
                        }
                    }
                }
            }
        }
    }
    if resp.double_clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if let Some(c) = hit(pos) {
                let nested = app
                    .clip_by_id(c.id)
                    .and_then(|(clip, ..)| app.nested_sequence_of(&clip));
                if let Some(t) = nested {
                    app.set_active_timeline(t);
                }
            }
        }
    }
    if resp.secondary_clicked() {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            app.ctx_tick = view.tick(pos.x).max(0);
            app.ctx_marker = if ruler.contains(pos) {
                crate::markers_ui::marker_at(&app.markers(), &|t| view.x(t), pos.x)
            } else {
                None
            };
            if let Some(c) = hit(pos) {
                app.selected_clip = Some(c.id);
            }
        }
    }
    resp.context_menu(|ui| clip_menu(app, ui));

    // media dragged in from the pool
    let dropped = media_drop(
        app,
        ui.ctx(),
        &rows,
        &view,
        body_rect,
        &mut do_snap,
        &mut snapped,
    );

    // ---- painting ------------------------------------------------------------------------------
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, theme::PANEL_DARK);
    // ruler
    p.rect_filled(ruler, 0.0, theme::PANEL);
    draw_ruler(app, &p, ruler, &view);
    draw_markers(app, &p, ruler, &view);
    // rows
    for row in &rows {
        let r = Rect::from_min_max(
            pos2(area.left(), row.top),
            pos2(area.right(), row.top + row.h),
        );
        p.hline(r.x_range(), r.bottom(), Stroke::new(1.0, theme::BORDER));
        let head = Rect::from_min_max(r.min, pos2(r.left() + HEADER_W, r.bottom()));
        crate::track_ui::header(app, ui, &p, head, row.track);
    }
    let body_painter = ui.painter_at(body_rect);
    // marked range
    if let (Some(a), Some(b)) = (app.mark_in, app.mark_out) {
        let r = Rect::from_min_max(
            pos2(view.x(a.min(b)), area.top()),
            pos2(view.x(a.max(b)), area.bottom()),
        );
        body_painter.rect_filled(
            r,
            0.0,
            Color32::from_rgba_unmultiplied(0x5b, 0x4f, 0xd6, 40),
        );
    }
    for (m, label) in [(app.mark_in, "I"), (app.mark_out, "O")] {
        if let Some(m) = m {
            let x = view.x(m);
            body_painter.vline(x, area.y_range(), Stroke::new(1.0, theme::ACCENT_HOVER));
            body_painter.text(
                pos2(x + 3.0, ruler.bottom() - 6.0),
                Align2::LEFT_CENTER,
                label,
                mono(9.0),
                theme::ACCENT_HOVER,
            );
        }
    }
    let moving = match &app.drag {
        Drag::Clip { item, .. } => Some(*item),
        _ => None,
    };
    for c in clips.iter().filter(|c| !c.is_gap) {
        let row = rows.iter().find(|r| r.track == c.track).expect("row");
        let item = tl
            .tracks
            .iter()
            .find(|t| t.id == c.track)
            .and_then(|t| t.items.iter().find(|i| i.id() == c.id))
            .cloned();
        let Some(item) = item else { continue };
        let mut rect = c.rect;
        let mut dim = moving == Some(c.id);
        if let Drag::Trim { item, edge, delta } = &app.drag {
            if *item == c.id {
                match edge {
                    Edge::In => rect.min.x = view.x(c.start + delta),
                    Edge::Out => rect.max.x = view.x(c.start + c.dur + delta),
                }
                dim = false;
            }
        }
        match &app.drag {
            Drag::Roll { left, right, delta } if *left == c.id => {
                rect.max.x = view.x(c.start + c.dur + delta);
            }
            Drag::Roll { left, right, delta } if *right == c.id => {
                rect.min.x = view.x(c.start + delta);
            }
            Drag::Slide { item, delta } if *item == c.id => {
                rect = rect.translate(vec2(view.x(c.start + delta) - view.x(c.start), 0.0));
            }
            Drag::Slide { item, delta } => {
                // the neighbours give and take the time
                if let Some(s) = clips.iter().find(|o| o.id == *item) {
                    if s.track == c.track && c.start + c.dur == s.start {
                        rect.max.x = view.x(c.start + c.dur + delta);
                    } else if s.track == c.track && c.start == s.start + s.dur {
                        rect.min.x = view.x(c.start + delta);
                    }
                }
            }
            _ => {}
        }
        let selected = app.is_selected(c.id);
        match &item {
            Item::Clip(clip) => draw_clip(app, &body_painter, clip, rect, row, selected, dim),
            Item::Title(t) => draw_title(&body_painter, t, rect, selected, dim),
            Item::Gap(_) => {}
        }
    }
    // the pending change of a roll, slip or slide, in frames
    let pending = match &app.drag {
        Drag::Roll { left, delta, .. } => Some(("Roll", *left, *delta)),
        Drag::Slip { item, delta } => Some(("Slip", *item, -*delta)),
        Drag::Slide { item, delta } => Some(("Slide", *item, *delta)),
        _ => None,
    };
    if let Some((what, id, d)) = pending {
        if let Some(c) = clips.iter().find(|c| c.id == id) {
            body_painter.text(
                pos2(c.rect.center().x, c.rect.top() - 2.0),
                Align2::CENTER_BOTTOM,
                format!("{what} {d:+} f"),
                mono(11.0),
                Color32::WHITE,
            );
        }
    }
    // ghosts
    if let Drag::Clip {
        item,
        target_track,
        target_start,
        ..
    } = &app.drag
    {
        if let (Some(c), Some(row)) = (
            clips.iter().find(|c| c.id == *item),
            rows.iter().find(|r| r.track == *target_track),
        ) {
            let r = Rect::from_min_max(
                pos2(view.x(*target_start), row.top + 3.0),
                pos2(view.x(*target_start + c.dur), row.top + row.h - 3.0),
            );
            body_painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(255, 255, 255, 50));
            body_painter.rect_stroke(
                r,
                4.0,
                Stroke::new(1.5, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }
    }
    if let Some((row_track, start, dur)) = dropped {
        if let Some(row) = rows.iter().find(|r| r.track == row_track) {
            let r = Rect::from_min_max(
                pos2(view.x(start), row.top + 3.0),
                pos2(view.x(start + dur), row.top + row.h - 3.0),
            );
            body_painter.rect_filled(
                r,
                4.0,
                Color32::from_rgba_unmultiplied(0x5b, 0x4f, 0xd6, 110),
            );
            body_painter.rect_stroke(
                r,
                4.0,
                Stroke::new(1.5, theme::ACCENT_HOVER),
                egui::StrokeKind::Inside,
            );
        }
    }
    if app.tool == Tool::Blade {
        if let Some(pos) = pointer.filter(|p| body_rect.contains(*p)) {
            body_painter.vline(pos.x, body_rect.y_range(), Stroke::new(1.0, Color32::WHITE));
        }
    }
    if let Some(s) = snapped {
        body_painter.vline(
            view.x(s),
            area.y_range(),
            Stroke::new(1.0, theme::ACCENT_HOVER),
        );
    }
    if clips.iter().all(|c| c.is_gap) && !app.project().media.is_empty() {
        body_painter.text(
            body_rect.center() + vec2(0.0, 6.0),
            Align2::CENTER_CENTER,
            "Double-click a clip in the media pool, or drag it here",
            ui_font(12.0),
            theme::FAINT,
        );
    }
    // playhead
    let px = view.x(app.playhead);
    if px >= area.left() + HEADER_W - 1.0 && px <= area.right() {
        let p = ui.painter_at(Rect::from_min_max(
            pos2(area.left() + HEADER_W - 8.0, area.top()),
            area.max,
        ));
        p.vline(
            px,
            pos2(0.0, ruler.top()).y..=area.bottom(),
            Stroke::new(1.5, theme::PLAYHEAD),
        );
        p.add(Shape::convex_polygon(
            vec![
                pos2(px - 6.0, ruler.top() + 2.0),
                pos2(px + 6.0, ruler.top() + 2.0),
                pos2(px + 6.0, ruler.top() + 12.0),
                pos2(px, ruler.top() + 19.0),
                pos2(px - 6.0, ruler.top() + 12.0),
            ],
            theme::PLAYHEAD,
            Stroke::NONE,
        ));
        // keep the playhead in view while playing
        if app.playing && px > area.right() - 30.0 {
            app.scroll += (area.right() - area.left() - HEADER_W - 60.0) / app.pps;
        }
    }
    // cursor
    if let Some(pos) = pointer {
        if matches!(app.tool, Tool::Select | Tool::Trim) && matches!(app.drag, Drag::None) {
            if let Some(c) = hit(pos) {
                let wide = c.rect.width() > EDGE_PX * 3.0;
                if wide && (pos.x - c.rect.left() <= EDGE_PX || c.rect.right() - pos.x <= EDGE_PX) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                } else {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
            }
        }
        match (&app.drag, app.tool) {
            (Drag::Trim { .. } | Drag::Roll { .. } | Drag::Slip { .. } | Drag::Slide { .. }, _) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal)
            }
            (_, Tool::Roll | Tool::Slip | Tool::Slide) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn)
            }
            (Drag::Clip { .. }, _) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing),
            (_, Tool::Hand) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
            (_, Tool::Blade) => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
            (_, Tool::Zoom) => ui.ctx().set_cursor_icon(egui::CursorIcon::ZoomIn),
            _ => {}
        }
    }
}

fn draw_title(p: &Painter, t: &Title, rect: Rect, selected: bool, dim: bool) {
    if rect.right() < p.clip_rect().left() || rect.left() > p.clip_rect().right() {
        return;
    }
    let mut fill = if t.enabled {
        theme::OVERLAY_CLIP
    } else {
        Color32::from_gray(0x44)
    };
    if dim {
        fill = fill.gamma_multiply(0.45);
    }
    p.rect_filled(rect, 4.0, fill);
    let inner = p.with_clip_rect(rect.intersect(p.clip_rect()));
    // fade ramps
    let fps_w = rect.width() / t.duration.value.max(1) as f32;
    for (len, from_left) in [(t.fade_in, true), (t.fade_out, false)] {
        if len > 0 {
            let w = len as f32 * fps_w;
            let (x0, x1) = if from_left {
                (rect.left(), rect.left() + w)
            } else {
                (rect.right() - w, rect.right())
            };
            let pts = if from_left {
                vec![
                    pos2(x0, rect.bottom()),
                    pos2(x1, rect.top()),
                    pos2(x1, rect.bottom()),
                ]
            } else {
                vec![
                    pos2(x0, rect.top()),
                    pos2(x1, rect.bottom()),
                    pos2(x0, rect.bottom()),
                ]
            };
            inner.add(Shape::convex_polygon(
                pts,
                Color32::from_black_alpha(60),
                Stroke::NONE,
            ));
        }
    }
    let label_color = if t.enabled {
        Color32::WHITE
    } else {
        theme::MUTED
    };
    inner.text(
        rect.left_top() + vec2(7.0, 4.0),
        Align2::LEFT_TOP,
        &t.name,
        ui_font(11.0),
        label_color,
    );
    if rect.height() > 40.0 && !t.subtitle.is_empty() {
        inner.text(
            rect.left_top() + vec2(7.0, 20.0),
            Align2::LEFT_TOP,
            &t.subtitle,
            ui_font(10.0),
            Color32::from_white_alpha(150),
        );
    }
    if selected {
        p.rect_stroke(
            rect,
            4.0,
            Stroke::new(2.0, theme::ACCENT_HOVER),
            egui::StrokeKind::Inside,
        );
        p.rect_stroke(
            rect.shrink(2.0),
            3.0,
            Stroke::new(1.0, Color32::from_white_alpha(130)),
            egui::StrokeKind::Inside,
        );
    } else {
        p.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, Color32::from_black_alpha(90)),
            egui::StrokeKind::Inside,
        );
    }
}

fn draw_clip(
    app: &App,
    p: &Painter,
    clip: &Clip,
    rect: Rect,
    row: &Row,
    selected: bool,
    dim: bool,
) {
    if rect.right() < p.clip_rect().left() || rect.left() > p.clip_rect().right() {
        return;
    }
    let base = match (row.color, row.kind) {
        (Some(c), _) => Color32::from_rgb(c[0], c[1], c[2]).gamma_multiply(0.75),
        (None, TrackKind::Video) => theme::VIDEO_CLIP,
        (None, TrackKind::Audio) => theme::AUDIO_CLIP,
    };
    let mut fill = if clip.enabled && row.enabled {
        base
    } else {
        Color32::from_gray(0x44)
    };
    if dim {
        fill = fill.gamma_multiply(0.45);
    }
    p.rect_filled(rect, 4.0, fill);
    let inner = p.with_clip_rect(rect.intersect(p.clip_rect()));
    if let Some(peaks) = app.waves.get(&clip.media) {
        let fps = app.fps();
        let src = clip.source_start.to_seconds_f64();
        let from = (src * crate::jobs::PEAKS_PER_SEC as f64) as f32;
        let to =
            from + (clip.duration.value as f64 / fps * crate::jobs::PEAKS_PER_SEC as f64) as f32;
        if !clip.audio.muted {
            match row.kind {
                TrackKind::Audio => {
                    let g = clip.audio.gain_db as f32;
                    let amp = 10f32.powf(g / 20.0);
                    let wave_rect = Rect::from_center_size(
                        rect.center() + vec2(0.0, 5.0),
                        vec2(rect.width(), (rect.height() - 20.0) * amp.clamp(0.1, 1.5)),
                    );
                    draw_wave(
                        &inner,
                        wave_rect,
                        peaks,
                        from,
                        to,
                        theme::AUDIO_WAVE.gamma_multiply(if dim { 0.4 } else { 0.9 }),
                    );
                }
                TrackKind::Video => {
                    let strip = Rect::from_min_max(
                        pos2(rect.left(), rect.bottom() - rect.height() * 0.38),
                        pos2(rect.right(), rect.bottom() - 2.0),
                    );
                    draw_wave(
                        &inner,
                        strip,
                        peaks,
                        from,
                        to,
                        Color32::from_white_alpha(70),
                    );
                }
            }
        }
    }
    let label_color = if clip.enabled {
        Color32::WHITE
    } else {
        theme::MUTED
    };
    inner.text(
        rect.left_top() + vec2(7.0, 4.0),
        Align2::LEFT_TOP,
        &clip.name,
        ui_font(11.0),
        label_color,
    );
    if let Some(t) = clip.transition {
        // the overlap sits on the cut: half before this clip's start, half after
        let ppt = rect.width() / clip.duration.value.max(1) as f32;
        let (a, b) = t.split();
        let r = Rect::from_min_max(
            pos2(rect.left() - a as f32 * ppt, rect.top()),
            pos2(rect.left() + b as f32 * ppt, rect.bottom()),
        );
        let glass = Color32::from_white_alpha(70);
        p.rect_filled(r, 3.0, glass);
        let line = Stroke::new(1.2, Color32::from_white_alpha(190));
        p.line_segment([r.left_bottom(), r.right_top()], line);
        p.line_segment([r.left_top(), r.right_bottom()], line);
    }
    if clip.link.is_some() {
        // two interlocked rings
        let c = rect.right_top() + vec2(-12.0, 9.0);
        let col = Color32::from_white_alpha(170);
        inner.circle_stroke(c + vec2(-2.5, 0.0), 3.2, Stroke::new(1.2, col));
        inner.circle_stroke(c + vec2(2.5, 0.0), 3.2, Stroke::new(1.2, col));
    }
    if !clip.retime.is_identity() {
        let rt = clip.retime;
        let badge = if rt.freeze {
            "❄ freeze".to_string()
        } else {
            format!(
                "{}{:.0}%",
                if rt.reverse { "◀ " } else { "" },
                rt.speed * 100.0
            )
        };
        inner.text(
            rect.left_bottom() + vec2(7.0, -4.0),
            Align2::LEFT_BOTTOM,
            badge,
            mono(9.5),
            Color32::from_white_alpha(200),
        );
    }
    if clip.video.opacity < 1.0 && row.kind == TrackKind::Video {
        inner.text(
            rect.right_top() + vec2(-6.0, 4.0),
            Align2::RIGHT_TOP,
            format!("{:.0}%", clip.video.opacity * 100.0),
            mono(9.5),
            Color32::from_white_alpha(170),
        );
    }
    if selected {
        p.rect_stroke(
            rect,
            4.0,
            Stroke::new(2.0, theme::ACCENT_HOVER),
            egui::StrokeKind::Inside,
        );
        p.rect_stroke(
            rect.shrink(2.0),
            3.0,
            Stroke::new(1.0, Color32::from_white_alpha(130)),
            egui::StrokeKind::Inside,
        );
    } else {
        p.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, Color32::from_black_alpha(90)),
            egui::StrokeKind::Inside,
        );
    }
}

fn draw_markers(app: &App, p: &Painter, ruler: Rect, view: &View) {
    let painter = p.with_clip_rect(ruler);
    for m in app.markers() {
        let x = view.x(m.tick);
        if x < ruler.left() - 80.0 || x > ruler.right() + 8.0 {
            continue;
        }
        let col = crate::markers_ui::color_of(&m);
        if m.duration > 0 {
            let x2 = view.x(m.tick + m.duration);
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(x, ruler.bottom() - 5.0),
                    pos2(x2.max(x + 2.0), ruler.bottom()),
                ),
                0.0,
                col.gamma_multiply(0.7),
            );
        }
        let top = ruler.bottom() - 14.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(x - 4.5, top),
                pos2(x + 4.5, top),
                pos2(x + 4.5, top + 7.0),
                pos2(x, top + 12.0),
                pos2(x - 4.5, top + 7.0),
            ],
            col,
            Stroke::new(1.0, Color32::from_black_alpha(120)),
        ));
        painter.text(
            pos2(x + 8.0, top + 4.0),
            Align2::LEFT_CENTER,
            &m.name,
            ui_font(10.0),
            Color32::from_white_alpha(210),
        );
    }
}

fn draw_ruler(app: &App, p: &Painter, ruler: Rect, view: &View) {
    p.hline(
        ruler.x_range(),
        ruler.bottom(),
        Stroke::new(1.0, theme::BORDER),
    );
    let steps = [
        0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0,
    ];
    let step = steps
        .iter()
        .copied()
        .find(|s| s * view.pps as f64 >= 92.0)
        .unwrap_or(3600.0);
    let first = (view.scroll as f64 / step).floor() as i64;
    let last = ((view.scroll + ruler.width() / view.pps) as f64 / step).ceil() as i64;
    let painter = p.with_clip_rect(ruler);
    for i in first..=last {
        let secs = i as f64 * step;
        if secs < 0.0 {
            continue;
        }
        let x = view.left + (secs as f32 - view.scroll) * view.pps;
        painter.vline(
            x,
            ruler.bottom() - 7.0..=ruler.bottom(),
            Stroke::new(1.0, theme::FAINT),
        );
        let label = if step < 1.0 {
            app.tc((secs * view.fps).round() as i64)
        } else {
            let s = secs.round() as i64;
            format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
        };
        painter.text(
            pos2(x + 4.0, ruler.top() + 6.0),
            Align2::LEFT_CENTER,
            label,
            mono(9.5),
            theme::MUTED,
        );
        // minor ticks
        for k in 1..5 {
            let mx = x + (step as f32 * view.pps) * k as f32 / 5.0;
            painter.vline(
                mx,
                ruler.bottom() - 3.0..=ruler.bottom(),
                Stroke::new(1.0, theme::BORDER),
            );
        }
    }
}

fn clip_menu(app: &mut App, ui: &mut egui::Ui) {
    if let Some(id) = app.ctx_marker {
        if ui.button("Edit Marker…").clicked() {
            app.marker_edit = app.markers().into_iter().find(|m| m.id == id);
            ui.close();
        }
        if ui.button("Delete Marker").clicked() {
            app.remove_marker(id);
            app.ctx_marker = None;
            ui.close();
        }
        ui.separator();
    }
    let has_clip = app.selected().is_some();
    if ui
        .add_enabled(has_clip, egui::Button::new("Separate Audio"))
        .clicked()
    {
        app.separate_audio();
        ui.close();
    }
    if ui
        .add_enabled(
            app.selection_ids().len() > 1,
            egui::Button::new("Link Clips"),
        )
        .clicked()
    {
        app.link_selected();
        ui.close();
    }
    if ui
        .add_enabled(has_clip, egui::Button::new("Unlink Clips"))
        .clicked()
    {
        app.unlink_selected();
        ui.close();
    }
    ui.separator();
    if ui.button("Add Marker Here   M").clicked() {
        let t = app.ctx_tick;
        app.add_marker(Some(t), None);
        ui.close();
    }
    ui.separator();
    let has = app.selected().is_some() || app.selected_title().is_some();
    if ui
        .add_enabled(has, egui::Button::new("Copy   ⌘C"))
        .clicked()
    {
        app.copy_selection();
        ui.close();
    }
    if ui
        .add_enabled(
            !app.clipboard.is_empty(),
            egui::Button::new("Paste at Playhead   ⌘V"),
        )
        .clicked()
    {
        app.paste_at(app.playhead);
        ui.close();
    }
    if ui
        .add_enabled(has, egui::Button::new("Duplicate   ⌘D"))
        .clicked()
    {
        app.duplicate_selection();
        ui.close();
    }
    ui.separator();
    if ui.add_enabled(has, egui::Button::new("Lift   ⌫")).clicked() {
        ui::lift_selection(app);
        ui.close();
    }
    if ui
        .add_enabled(has, egui::Button::new("Extract   ⇧⌫"))
        .clicked()
    {
        ui::extract_selection(app);
        ui.close();
    }
    ui.separator();
    if ui.button("Split at Playhead   ⌘B").clicked() {
        ui::split_at_playhead(app);
        ui.close();
    }
    if ui
        .add_enabled(
            app.selected().is_some(),
            egui::Button::new("Split at Scene Changes"),
        )
        .clicked()
    {
        ui::split_at_scenes(app);
        ui.close();
    }
    if ui
        .add_enabled(has, egui::Button::new("Trim Start to Playhead"))
        .clicked()
    {
        ui::trim_to_playhead(app, Edge::In);
        ui.close();
    }
    if ui
        .add_enabled(has, egui::Button::new("Trim End to Playhead"))
        .clicked()
    {
        ui::trim_to_playhead(app, Edge::Out);
        ui.close();
    }
    ui.separator();
    if ui
        .add_enabled(has, egui::Button::new("Enable / Disable"))
        .clicked()
    {
        ui::toggle_enabled(app);
        ui.close();
    }
    if ui.button("Clean Up Gaps").clicked() {
        ui::tidy_gaps(app);
        ui.close();
    }
}

/// Handles a media item being dragged over the tracks. Returns the ghost to draw `(track, start, length)`.
#[allow(clippy::type_complexity)]
fn media_drop(
    app: &mut App,
    ctx: &Context,
    rows: &[Row],
    view: &View,
    body: Rect,
    snap: &mut dyn FnMut(&App, i64, Option<Id>, &mut Option<i64>) -> i64,
    snapped: &mut Option<i64>,
) -> Option<(Id, i64, i64)> {
    if let Some(tp) = egui::DragAndDrop::payload::<TitleDrag>(ctx) {
        let pos = ctx.input(|i| i.pointer.hover_pos())?;
        if !body.contains(pos) {
            return None;
        }
        let row = rows
            .iter()
            .find(|r| pos.y >= r.top && pos.y < r.top + r.h && r.kind == TrackKind::Video)?;
        let tick = snap(app, view.tick(pos.x).max(0), None, snapped);
        let len = (3.0 * app.fps()).round() as i64;
        if ctx.input(|i| i.pointer.any_released()) {
            app.add_title(tp.0, Some(tick), Some(row.track));
            egui::DragAndDrop::clear_payload(ctx);
            return None;
        }
        return Some((row.track, tick, len));
    }
    if let Some(lp) = egui::DragAndDrop::payload::<LayerDrag>(ctx) {
        let pos = ctx.input(|i| i.pointer.hover_pos())?;
        if !body.contains(pos) {
            return None;
        }
        let row = rows
            .iter()
            .find(|r| pos.y >= r.top && pos.y < r.top + r.h && r.kind == TrackKind::Video)?;
        let tick = snap(app, view.tick(pos.x).max(0), None, snapped);
        let len = (5.0 * app.fps()).round() as i64;
        if ctx.input(|i| i.pointer.any_released()) {
            app.add_layer(lp.0, Some(tick), Some(row.track));
            egui::DragAndDrop::clear_payload(ctx);
            return None;
        }
        return Some((row.track, tick, len));
    }
    let payload = egui::DragAndDrop::payload::<MediaDrag>(ctx)?;
    let MediaDrag(media) = *payload;
    let pos = ctx.input(|i| i.pointer.hover_pos())?;
    if !body.contains(pos) {
        return None;
    }
    let row = rows
        .iter()
        .find(|r| pos.y >= r.top && pos.y < r.top + r.h)?;
    let m = app.project().media.get(&media)?.clone();
    let tick = snap(app, view.tick(pos.x).max(0), None, snapped);
    let len = m
        .duration?
        .rescale(app.rate(), captaine_time::Rounding::Floor)?
        .value
        .max(1);
    if ctx.input(|i| i.pointer.any_released()) {
        app.place_media(media, Some(row.track), Some(tick));
        egui::DragAndDrop::clear_payload(ctx);
        return None;
    }
    Some((row.track, tick, len))
}

/// A little label that follows the pointer while media is being dragged.
fn drag_ghost(app: &App, ctx: &Context) {
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let label = if let Some(t) = egui::DragAndDrop::payload::<TitleDrag>(ctx) {
        t.0.label().to_string()
    } else if let Some(l) = egui::DragAndDrop::payload::<LayerDrag>(ctx) {
        l.0.map_or("Adjustment Layer", |k| k.label()).to_string()
    } else if let Some(m) =
        egui::DragAndDrop::payload::<MediaDrag>(ctx).and_then(|p| app.project().media.get(&p.0))
    {
        m.name.clone()
    } else {
        return;
    };
    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("media_ghost"));
    let painter = ctx.layer_painter(layer);
    let galley = painter.layout_no_wrap(label, ui_font(11.5), Color32::WHITE);
    let r = Rect::from_min_size(pos + vec2(14.0, 10.0), galley.size() + vec2(14.0, 8.0));
    painter.rect_filled(r, 4.0, theme::ACCENT);
    painter.galley(r.min + vec2(7.0, 4.0), galley, Color32::WHITE);
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
}
