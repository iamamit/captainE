//! Compare two versions of the project, and the merge screen built on it (docs/07).
//!
//! Two timelines stacked (A above B) with what changed colour-coded, two viewers on the same frame (side by side
//! or as a difference picture), and the list of changes. In merge mode every change has a checkbox and conflicts
//! need a decision before merging.

use std::collections::HashMap;
use std::sync::Arc;

use captaine_model::*;
use captaine_project::captaine_vcs::{diff, summarize, Change, ChangeKind, Choice, Status, Target};
use captaine_project::MergeSetup;
use egui::{
    pos2, vec2, Align2, Color32, Context, Rect, RichText, Sense, Shape, Stroke, TextureHandle, Ui,
};

use crate::app::App;
use crate::theme::{self, mono, ui_font};
use crate::ui::{chip, tab};
use crate::worker::RenderWorker;

pub const ADDED: Color32 = theme::rgb(0x3f, 0xb9, 0x7a);
pub const REMOVED: Color32 = theme::rgb(0xe0, 0x55, 0x55);
pub const CHANGED: Color32 = theme::rgb(0xe0, 0xa0, 0x30);
pub const CONFLICT: Color32 = theme::rgb(0xd0, 0x5a, 0xd0);

const HEADER_W: f32 = 56.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    SideBySide,
    Difference,
}

pub enum Mode {
    Browse { a_ref: String, b_ref: String },
    Merge { setup: Box<MergeSetup> },
}

/// One side's frames.
struct Side {
    worker: RenderWorker,
    texture: Option<TextureHandle>,
    image: Option<egui::ColorImage>,
    tick: i64,
    requested: Option<i64>,
}

impl Side {
    fn new(ctx: &Context, project: &Project) -> Side {
        let worker = RenderWorker::spawn(ctx.clone());
        if let Some(tl) = project.timelines.keys().next() {
            worker.set_project(Arc::new(project.clone()), *tl);
        }
        Side {
            worker,
            texture: None,
            image: None,
            tick: -1,
            requested: None,
        }
    }
}

pub struct CompareState {
    pub mode: Mode,
    pub a_label: String,
    pub b_label: String,
    a: Project,
    b: Project,
    /// What to list, in order
    changes: Vec<Change>,
    /// Per change: conflict reason if any (merge mode)
    conflicts: HashMap<String, String>,
    pub choices: HashMap<String, Choice>,
    colors_a: HashMap<Id, Color32>,
    colors_b: HashMap<Id, Color32>,
    by_item: HashMap<Id, String>,
    pub selected: Option<String>,
    tick: i64,
    playing: bool,
    play_origin: std::time::Instant,
    play_from: i64,
    pps: f32,
    scroll: f32,
    pub view: ViewMode,
    side_a: Side,
    side_b: Side,
    diff_tex: Option<TextureHandle>,
    summary: String,
    fps: f64,
}

fn kind_color(k: ChangeKind) -> Option<Color32> {
    match k {
        ChangeKind::Added => Some(ADDED),
        ChangeKind::Removed => Some(REMOVED),
        ChangeKind::Moved | ChangeKind::Trimmed | ChangeKind::Modified => Some(CHANGED),
        ChangeKind::Shifted => None,
    }
}

impl CompareState {
    #[allow(clippy::too_many_arguments)]
    fn build(
        ctx: &Context,
        mode: Mode,
        a_label: String,
        b_label: String,
        a: Project,
        b: Project,
        changes: Vec<Change>,
        conflicts: HashMap<String, String>,
    ) -> CompareState {
        let rate = a
            .timelines
            .values()
            .next()
            .or(b.timelines.values().next())
            .map_or(captaine_time::Rational::FPS_30, |t| t.rate);
        let summary = summarize(&changes, rate);
        let (mut colors_a, mut colors_b, mut by_item) =
            (HashMap::new(), HashMap::new(), HashMap::new());
        for c in changes.iter().filter(|c| !c.is_noise()) {
            let Target::Item(id) = c.target else { continue };
            by_item.insert(id, c.id.clone());
            let col = if conflicts.contains_key(&c.id) {
                Some(CONFLICT)
            } else {
                kind_color(c.kind)
            };
            let Some(col) = col else { continue };
            match c.kind {
                ChangeKind::Added => {
                    colors_b.insert(id, col);
                }
                ChangeKind::Removed => {
                    colors_a.insert(id, col);
                }
                _ => {
                    colors_a.insert(id, col);
                    colors_b.insert(id, col);
                }
            }
        }
        let (side_a, side_b) = (Side::new(ctx, &a), Side::new(ctx, &b));
        let longest = a
            .timelines
            .values()
            .chain(b.timelines.values())
            .map(|t| t.duration().value)
            .max()
            .unwrap_or(0);
        let fps = rate.as_f64();
        let pps = (900.0 / ((longest as f64 / fps).max(1.0) * 1.05)) as f32;
        CompareState {
            mode,
            a_label,
            b_label,
            a,
            b,
            changes,
            conflicts,
            choices: HashMap::new(),
            colors_a,
            colors_b,
            by_item,
            selected: None,
            tick: 0,
            playing: false,
            play_origin: std::time::Instant::now(),
            play_from: 0,
            pps,
            scroll: 0.0,
            view: ViewMode::SideBySide,
            side_a,
            side_b,
            diff_tex: None,
            summary,
            fps,
        }
    }

    /// Compare any two references (`working`, a branch, or a commit id).
    pub fn browse(
        app: &App,
        ctx: &Context,
        a_ref: &str,
        b_ref: &str,
    ) -> Result<CompareState, String> {
        let (a, b) = (app.session.resolve(a_ref)?, app.session.resolve(b_ref)?);
        let changes: Vec<Change> = diff(&a, &b);
        Ok(CompareState::build(
            ctx,
            Mode::Browse {
                a_ref: a_ref.into(),
                b_ref: b_ref.into(),
            },
            label(a_ref),
            label(b_ref),
            a,
            b,
            changes,
            HashMap::new(),
        ))
    }

    /// Review merging `from` into the current branch.
    pub fn merge(app: &mut App, ctx: &Context, from: &str) -> Result<CompareState, String> {
        let setup = app.session.prepare_merge(from)?;
        let changes: Vec<Change> = setup
            .plan
            .entries
            .iter()
            .map(|e| e.change.clone())
            .collect();
        let conflicts: HashMap<String, String> = setup
            .plan
            .entries
            .iter()
            .filter_map(|e| {
                if let Status::Conflict(m) = &e.status {
                    Some((e.change.id.clone(), m.clone()))
                } else {
                    None
                }
            })
            .collect();
        let (a, b) = (setup.ours.clone(), setup.theirs.clone());
        let mut s = CompareState::build(
            ctx,
            Mode::Merge {
                setup: Box::new(setup),
            },
            format!("{} (yours)", app.session.branch()),
            format!("{from} (theirs)"),
            a,
            b,
            changes,
            conflicts,
        );
        for c in &s.changes {
            s.choices.insert(
                c.id.clone(),
                if s.conflicts.contains_key(&c.id) {
                    Choice::Skip
                } else {
                    Choice::Take
                },
            );
        }
        Ok(s)
    }

    /// The changes listed on the right (bookkeeping like gaps and ripple shifts excluded).
    pub fn listed_changes(&self) -> Vec<&Change> {
        self.changes.iter().filter(|c| !c.is_noise()).collect()
    }

    pub fn is_merge(&self) -> bool {
        matches!(self.mode, Mode::Merge { .. })
    }

    pub fn conflict_count(&self) -> usize {
        self.conflicts.len()
    }

    /// Both sides have drawn a frame for the current playhead.
    pub fn frames_ready(&self) -> bool {
        self.side_a.tick == self.tick
            && self.side_b.tick == self.tick
            && self.side_a.texture.is_some()
            && self.side_b.texture.is_some()
    }

    pub fn summary_text(&self) -> &str {
        &self.summary
    }

    pub fn playhead(&self) -> i64 {
        self.tick
    }

    fn duration(&self) -> i64 {
        self.a
            .timelines
            .values()
            .chain(self.b.timelines.values())
            .map(|t| t.duration().value)
            .max()
            .unwrap_or(0)
    }

    fn seek(&mut self, tick: i64) {
        self.tick = tick.clamp(0, self.duration().max(0));
        self.play_from = self.tick;
        self.play_origin = std::time::Instant::now();
    }
}

fn label(r: &str) -> String {
    if r == "working" {
        "Working copy".into()
    } else {
        r.to_string()
    }
}

fn kind_name(k: ChangeKind) -> &'static str {
    match k {
        ChangeKind::Added => "Added",
        ChangeKind::Removed => "Removed",
        ChangeKind::Moved => "Moved",
        ChangeKind::Trimmed => "Trimmed",
        ChangeKind::Modified => "Changed",
        ChangeKind::Shifted => "Shifted",
    }
}

// ---- frames ------------------------------------------------------------------------------------------------

fn pump(side: &mut Side, ctx: &Context, name: &str, tick: i64) {
    if side.requested != Some(tick) && side.worker.in_flight < 2 {
        side.worker.request(tick, true);
        side.requested = Some(tick);
    }
    while let Some(f) = side.worker.poll() {
        if f.tick < 0 {
            continue;
        }
        if let Ok(img) = f.image {
            match &mut side.texture {
                Some(t) => t.set(img.clone(), egui::TextureOptions::LINEAR),
                None => {
                    side.texture =
                        Some(ctx.load_texture(name, img.clone(), egui::TextureOptions::LINEAR))
                }
            }
            side.image = Some(img);
            side.tick = f.tick;
        }
    }
}

/// Amplified absolute difference of two pictures; black where they agree.
fn difference(a: &egui::ColorImage, b: &egui::ColorImage) -> egui::ColorImage {
    let size = [a.size[0].min(b.size[0]), a.size[1].min(b.size[1])];
    let mut out = egui::ColorImage::filled(size, Color32::BLACK);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let (p, q) = (a.pixels[y * a.size[0] + x], b.pixels[y * b.size[0] + x]);
            let d = |u: u8, v: u8| ((u as i32 - v as i32).unsigned_abs() * 4).min(255) as u8;
            out.pixels[y * size[0] + x] =
                Color32::from_rgb(d(p.r(), q.r()), d(p.g(), q.g()), d(p.b(), q.b()));
        }
    }
    out
}

// ---- the screen -----------------------------------------------------------------------------------------------

/// What the screen asks the app to do after drawing.
enum Action {
    Close,
    Recompare(String, String),
    DoMerge,
    OpenMerge(String),
}

pub fn show(app: &mut App, root: &mut Ui) {
    let ctx = root.ctx().clone();
    let Some(mut cs) = app.compare.take() else {
        return;
    };
    let mut action: Option<Action> = None;
    // playback of both versions together (silent)
    if cs.playing {
        let t = cs.play_from + (cs.play_origin.elapsed().as_secs_f64() * cs.fps).round() as i64;
        if t >= cs.duration() {
            cs.tick = cs.duration();
            cs.playing = false;
        } else {
            cs.tick = t;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    let tick = cs.tick;
    pump(&mut cs.side_a, &ctx, "compare-a", tick);
    pump(&mut cs.side_b, &ctx, "compare-b", tick);
    if cs.view == ViewMode::Difference && cs.side_a.tick == tick && cs.side_b.tick == tick {
        if let (Some(a), Some(b)) = (&cs.side_a.image, &cs.side_b.image) {
            let img = difference(a, b);
            match &mut cs.diff_tex {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => {
                    cs.diff_tex =
                        Some(ctx.load_texture("compare-diff", img, egui::TextureOptions::LINEAR))
                }
            }
        }
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = Some(Action::Close);
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Space)) && !ctx.egui_wants_keyboard_input() {
        cs.playing = !cs.playing;
        cs.play_from = cs.tick;
        cs.play_origin = std::time::Instant::now();
    }

    // right: the change list
    egui::Panel::right("compare_changes")
        .default_size(340.0)
        .min_size(260.0)
        .resizable(true)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(root, |ui| change_list(app, &mut cs, ui, &mut action));
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::VIEWER_BG))
        .show(root, |ui| {
            header(app, &mut cs, ui, &mut action);
            let full = ui.available_rect_before_wrap();
            let viewers = Rect::from_min_max(
                full.min,
                pos2(full.right(), full.top() + (full.height() * 0.46).max(180.0)),
            );
            draw_viewers(&mut cs, ui, viewers);
            let timelines = Rect::from_min_max(pos2(full.left(), viewers.bottom()), full.max);
            draw_timelines(&mut cs, ui, timelines);
        });

    match action {
        Some(Action::Close) => {
            app.compare = None;
            return;
        }
        Some(Action::Recompare(a, b)) => match CompareState::browse(app, &ctx, &a, &b) {
            Ok(n) => cs = n,
            Err(e) => app.toast(e, true),
        },
        Some(Action::OpenMerge(from)) => match CompareState::merge(app, &ctx, &from) {
            Ok(n) => cs = n,
            Err(e) => app.toast(e, true),
        },
        Some(Action::DoMerge) => {
            if let Mode::Merge { setup } = &cs.mode {
                let from = setup.from.clone();
                let choices = cs.choices.clone();
                match app.session.merge(&from, &choices, None) {
                    Ok(report) => {
                        crate::branches_ui::announce_merge(app, &from, report);
                        app.compare = None;
                        return;
                    }
                    Err(e) => app.toast(format!("merge failed: {e}"), true),
                }
            }
        }
        None => {}
    }
    app.compare = Some(cs);
}

fn header(app: &App, cs: &mut CompareState, ui: &mut Ui, action: &mut Option<Action>) {
    egui::Frame::new()
        .fill(theme::PANEL_DARK)
        .inner_margin(egui::Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let merging = matches!(cs.mode, Mode::Merge { .. });
                ui.label(
                    RichText::new(if merging { "Merge" } else { "Compare" })
                        .strong()
                        .size(14.0),
                );
                ui.add_space(8.0);
                if let Mode::Browse { a_ref, b_ref } = &cs.mode {
                    let (mut a, mut b) = (a_ref.clone(), b_ref.clone());
                    let opts: Vec<String> = std::iter::once("working".to_string())
                        .chain(app.session.branch_infos().into_iter().map(|b| b.name))
                        .collect();
                    for (id, sel, tag) in [("cmp_a", &mut a, "A"), ("cmp_b", &mut b, "B")] {
                        ui.label(RichText::new(tag).font(mono(11.0)).color(theme::MUTED));
                        egui::ComboBox::from_id_salt(id)
                            .selected_text(label(sel))
                            .width(160.0)
                            .show_ui(ui, |ui| {
                                for o in &opts {
                                    ui.selectable_value(sel, o.clone(), label(o));
                                }
                            });
                    }
                    if (&a, &b) != (a_ref, b_ref) {
                        *action = Some(Action::Recompare(a, b));
                    }
                    if b_ref != "working" && *b_ref != app.session.branch() && opts.contains(b_ref)
                    {
                        ui.add_space(6.0);
                        if chip(
                            ui,
                            &format!("Merge {b_ref} into {}…", app.session.branch()),
                            false,
                            true,
                        )
                        .clicked()
                        {
                            *action = Some(Action::OpenMerge(b_ref.clone()));
                        }
                    }
                } else if let Mode::Merge { setup } = &cs.mode {
                    ui.label(
                        RichText::new(format!("{} into {}", setup.from, setup.into)).size(13.0),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if chip(ui, "Close", false, true)
                        .on_hover_text("Esc")
                        .clicked()
                    {
                        *action = Some(Action::Close);
                    }
                    ui.add_space(8.0);
                    for (v, l) in [
                        (ViewMode::Difference, "Difference"),
                        (ViewMode::SideBySide, "Side by side"),
                    ] {
                        if chip(ui, l, cs.view == v, true).clicked() {
                            cs.view = v;
                        }
                    }
                    ui.add_space(8.0);
                    if chip(
                        ui,
                        if cs.playing { "Pause" } else { "Play both" },
                        cs.playing,
                        true,
                    )
                    .on_hover_text("Space")
                    .clicked()
                    {
                        cs.playing = !cs.playing;
                        cs.play_from = cs.tick;
                        cs.play_origin = std::time::Instant::now();
                    }
                });
            });
            ui.label(
                RichText::new(if cs.summary == "no differences" {
                    "These two versions are the same.".to_string()
                } else {
                    format!("{} → {}: {}", cs.a_label, cs.b_label, cs.summary)
                })
                .size(11.5)
                .color(theme::MUTED),
            );
        });
}

fn draw_viewers(cs: &mut CompareState, ui: &mut Ui, area: Rect) {
    let p = ui.painter_at(area);
    let pad = 12.0;
    let panes: Vec<(Rect, &str)> = match cs.view {
        ViewMode::SideBySide => {
            let w = (area.width() - pad * 3.0) / 2.0;
            vec![
                (
                    Rect::from_min_size(
                        area.min + vec2(pad, pad),
                        vec2(w, area.height() - pad * 2.0 - 18.0),
                    ),
                    "A",
                ),
                (
                    Rect::from_min_size(
                        area.min + vec2(pad * 2.0 + w, pad),
                        vec2(w, area.height() - pad * 2.0 - 18.0),
                    ),
                    "B",
                ),
            ]
        }
        ViewMode::Difference => vec![(
            Rect::from_min_size(
                area.min + vec2(pad, pad),
                vec2(area.width() - pad * 2.0, area.height() - pad * 2.0 - 18.0),
            ),
            "A − B",
        )],
    };
    for (rect, tag) in panes {
        let (tex, name) = match (cs.view, tag) {
            (ViewMode::Difference, _) => (
                cs.diff_tex.as_ref(),
                format!("{} − {}", cs.a_label, cs.b_label),
            ),
            (_, "A") => (cs.side_a.texture.as_ref(), cs.a_label.clone()),
            _ => (cs.side_b.texture.as_ref(), cs.b_label.clone()),
        };
        let (cw, ch) = {
            let s = &cs.a.settings;
            (s.width as f32, s.height as f32)
        };
        let scale = (rect.width() / cw).min(rect.height() / ch).max(0.01);
        let pic = Rect::from_center_size(rect.center(), vec2(cw * scale, ch * scale));
        p.rect_filled(pic, 0.0, Color32::BLACK);
        if let Some(t) = tex {
            p.image(
                t.id(),
                pic,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        p.text(
            pos2(pic.left(), pic.bottom() + 4.0),
            Align2::LEFT_TOP,
            name,
            ui_font(11.5),
            theme::MUTED,
        );
    }
}

fn draw_timelines(cs: &mut CompareState, ui: &mut Ui, area: Rect) {
    let ruler = Rect::from_min_size(
        pos2(area.left() + HEADER_W, area.top()),
        vec2(area.width() - HEADER_W, 24.0),
    );
    let body = Rect::from_min_max(pos2(area.left(), ruler.bottom()), area.max);
    let resp = ui.interact(area, egui::Id::new("compare_tl"), Sense::click_and_drag());
    // zoom and scroll
    if let Some(pos) = resp.hover_pos() {
        let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
        if zoom != 1.0 {
            let anchor = (pos.x - ruler.left()) / cs.pps + cs.scroll;
            cs.pps = (cs.pps * zoom).clamp(1.0, 600.0);
            cs.scroll = anchor - (pos.x - ruler.left()) / cs.pps;
        } else if scroll != egui::Vec2::ZERO {
            cs.scroll -= (if scroll.x.abs() > scroll.y.abs() {
                scroll.x
            } else {
                scroll.y
            }) / cs.pps;
        }
        cs.scroll = cs.scroll.max(-1.0);
    }
    let (pps, scroll, fps, left) = (cs.pps, cs.scroll, cs.fps, ruler.left());
    let x_of = move |tick: i64| left + ((tick as f64 / fps) as f32 - scroll) * pps;
    let tick_of = move |x: f32| (((x - left) / pps + scroll) as f64 * fps).round() as i64;
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, theme::PANEL_DARK);
    p.rect_filled(ruler, 0.0, theme::PANEL);
    // ruler labels every ~100 px
    let steps = [
        0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0,
    ];
    let step = steps
        .iter()
        .copied()
        .find(|s| s * pps as f64 >= 90.0)
        .unwrap_or(600.0);
    let first = (scroll as f64 / step).floor() as i64;
    let last = ((scroll + ruler.width() / pps) as f64 / step).ceil() as i64;
    for i in first.max(0)..=last {
        let x = left + ((i as f64 * step) as f32 - scroll) * pps;
        p.vline(
            x,
            ruler.bottom() - 6.0..=ruler.bottom(),
            Stroke::new(1.0, theme::FAINT),
        );
        let s = (i as f64 * step).round() as i64;
        p.text(
            pos2(x + 4.0, ruler.top() + 5.0),
            Align2::LEFT_CENTER,
            format!("{:02}:{:02}", s / 60, s % 60),
            mono(9.5),
            theme::MUTED,
        );
    }
    // two stacked timelines
    let half = body.height() / 2.0;
    let mut hit: Option<String> = None;
    let click = resp
        .clicked()
        .then(|| resp.interact_pointer_pos())
        .flatten();
    for (i, (project, colors, name)) in [
        (&cs.a, &cs.colors_a, &cs.a_label),
        (&cs.b, &cs.colors_b, &cs.b_label),
    ]
    .into_iter()
    .enumerate()
    {
        let pane = Rect::from_min_size(
            pos2(body.left(), body.top() + i as f32 * half),
            vec2(body.width(), half),
        );
        p.hline(pane.x_range(), pane.top(), Stroke::new(1.0, theme::BORDER));
        p.text(
            pane.left_top() + vec2(6.0, 4.0),
            Align2::LEFT_TOP,
            if i == 0 { "A" } else { "B" },
            mono(11.0),
            theme::ACCENT_HOVER,
        );
        p.text(
            pane.left_top() + vec2(6.0, 18.0),
            Align2::LEFT_TOP,
            name,
            ui_font(9.5),
            theme::MUTED,
        );
        let Some(tl) = project.timelines.values().next() else {
            continue;
        };
        let mut tracks: Vec<&Arc<Track>> = tl
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Video)
            .collect();
        tracks.reverse();
        tracks.extend(tl.tracks.iter().filter(|t| t.kind == TrackKind::Audio));
        let row_h = (pane.height() / tracks.len().max(1) as f32).clamp(14.0, 40.0);
        let clip_area = Rect::from_min_max(pos2(pane.left() + HEADER_W, pane.top()), pane.max);
        let pc = p.with_clip_rect(clip_area);
        for (ri, tr) in tracks.iter().enumerate() {
            let top = pane.top() + ri as f32 * row_h;
            p.text(
                pos2(pane.left() + 6.0, top + row_h / 2.0),
                Align2::LEFT_CENTER,
                "",
                ui_font(9.0),
                theme::MUTED,
            );
            if ri > 0 {
                p.hline(
                    pane.x_range(),
                    top,
                    Stroke::new(1.0, Color32::from_black_alpha(60)),
                );
            }
            for (it, st) in tr.items.iter().zip(tr.item_starts()) {
                if !it.is_content() {
                    continue;
                }
                let r = Rect::from_min_max(
                    pos2(x_of(st), top + 2.0),
                    pos2(x_of(st + it.duration().value), top + row_h - 2.0),
                );
                let base = match (tr.kind, it) {
                    (_, Item::Title(_)) => theme::OVERLAY_CLIP,
                    (TrackKind::Video, _) => theme::VIDEO_CLIP,
                    _ => theme::AUDIO_CLIP,
                };
                let mark = colors.get(&it.id()).copied();
                pc.rect_filled(
                    r,
                    3.0,
                    if mark.is_some() {
                        base.gamma_multiply(0.55)
                    } else {
                        base.gamma_multiply(0.8)
                    },
                );
                if let Some(c) = mark {
                    pc.rect_stroke(r, 3.0, Stroke::new(2.0, c), egui::StrokeKind::Inside);
                    pc.rect_filled(
                        Rect::from_min_size(r.left_top(), vec2(4.0, r.height())),
                        0.0,
                        c,
                    );
                }
                let sel = cs
                    .by_item
                    .get(&it.id())
                    .is_some_and(|k| cs.selected.as_ref() == Some(k));
                if sel {
                    pc.rect_stroke(
                        r.expand(1.0),
                        3.0,
                        Stroke::new(1.5, Color32::WHITE),
                        egui::StrokeKind::Outside,
                    );
                }
                if r.width() > 30.0 && row_h > 18.0 {
                    let nm = match it {
                        Item::Clip(c) => c.name.as_str(),
                        Item::Title(t) => t.name.as_str(),
                        Item::Gap(_) => "",
                    };
                    pc.with_clip_rect(r).text(
                        r.left_top() + vec2(7.0, 3.0),
                        Align2::LEFT_TOP,
                        nm,
                        ui_font(9.5),
                        Color32::WHITE,
                    );
                }
                if let Some(pos) = click {
                    if r.contains(pos) {
                        hit = cs.by_item.get(&it.id()).cloned();
                    }
                }
            }
        }
    }
    // playhead
    let px = x_of(cs.tick);
    if px >= area.left() + HEADER_W && px <= area.right() {
        p.vline(px, area.y_range(), Stroke::new(1.5, theme::PLAYHEAD));
        p.add(Shape::convex_polygon(
            vec![
                pos2(px - 5.0, ruler.top() + 2.0),
                pos2(px + 5.0, ruler.top() + 2.0),
                pos2(px, ruler.top() + 12.0),
            ],
            theme::PLAYHEAD,
            Stroke::NONE,
        ));
    }
    // seeking and picking
    if let Some(pos) = resp.interact_pointer_pos() {
        if (resp.dragged() || resp.clicked())
            && (ruler.contains(pos) || resp.dragged())
            && pos.x >= ruler.left()
        {
            cs.seek(tick_of(pos.x));
        }
    }
    if let Some(k) = hit {
        select(cs, &k);
    }
}

/// Select a change and bring the playhead to it.
fn select(cs: &mut CompareState, key: &str) {
    cs.selected = Some(key.to_string());
    if let Some(r) = cs
        .changes
        .iter()
        .find(|c| c.id == key)
        .and_then(|c| c.range)
    {
        cs.seek(r.0);
        let t = cs.tick as f64 / cs.fps;
        if (t as f32) < cs.scroll || (t as f32) > cs.scroll + 800.0 / cs.pps {
            cs.scroll = (t as f32 - 2.0).max(0.0);
        }
    }
}

fn change_list(app: &mut App, cs: &mut CompareState, ui: &mut Ui, action: &mut Option<Action>) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        tab(
            ui,
            if matches!(cs.mode, Mode::Merge { .. }) {
                "Changes to merge"
            } else {
                "Differences"
            },
            true,
        );
    });
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        Stroke::new(1.0, theme::BORDER),
    );
    let merging = matches!(cs.mode, Mode::Merge { .. });
    let rows: Vec<Change> = cs
        .changes
        .iter()
        .filter(|c| !c.is_noise())
        .cloned()
        .collect();
    // legend
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (c, t) in [
                    (ADDED, "added"),
                    (REMOVED, "removed"),
                    (CHANGED, "changed"),
                    (CONFLICT, "conflict"),
                ] {
                    let (r, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, c);
                    ui.label(RichText::new(t).size(10.5).color(theme::MUTED));
                }
            });
            if let Mode::Merge { setup } = &cs.mode {
                if setup.fast_forward {
                    ui.label(
                        RichText::new(format!(
                            "{} has nothing of its own: merging just moves {} forward.",
                            setup.into, setup.into
                        ))
                        .size(11.0)
                        .color(theme::OK),
                    );
                }
                if setup.up_to_date {
                    ui.label(
                        RichText::new(
                            "Already up to date: everything in this branch is already here.",
                        )
                        .size(11.0)
                        .color(theme::OK),
                    );
                }
            }
        });
    let footer_h = if merging { 92.0 } else { 0.0 };
    let list_h = (ui.available_height() - footer_h).max(60.0);
    egui::ScrollArea::vertical()
        .max_height(list_h)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if rows.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No differences").color(theme::MUTED))
                });
            }
            let mut pick: Option<String> = None;
            for c in &rows {
                let conflict = cs.conflicts.get(&c.id).cloned();
                let col = if conflict.is_some() {
                    CONFLICT
                } else {
                    kind_color(c.kind).unwrap_or(theme::MUTED)
                };
                let selected = cs.selected.as_ref() == Some(&c.id);
                let frame = egui::Frame::new()
                    .fill(if selected {
                        theme::ACCENT_SOFT
                    } else {
                        Color32::TRANSPARENT
                    })
                    .inner_margin(egui::Margin::symmetric(12, 6));
                let r = frame
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_top(|ui| {
                            if merging {
                                let mut take = cs.choices.get(&c.id).copied() == Some(Choice::Take);
                                if ui.checkbox(&mut take, "").changed() {
                                    cs.choices.insert(
                                        c.id.clone(),
                                        if take { Choice::Take } else { Choice::Skip },
                                    );
                                }
                            }
                            let (dot, _) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
                            ui.painter().rect_filled(
                                Rect::from_center_size(dot.center(), vec2(8.0, 8.0)),
                                2.0,
                                col,
                            );
                            ui.vertical(|ui| {
                                if ui
                                    .add(
                                        egui::Label::new(RichText::new(&c.summary).size(12.0))
                                            .sense(Sense::click()),
                                    )
                                    .clicked()
                                {
                                    pick = Some(c.id.clone());
                                }
                                let mut meta = kind_name(c.kind).to_string();
                                if let Some((s, _)) = c.range {
                                    meta = format!("{meta} · at {}", app.tc(s));
                                }
                                ui.label(RichText::new(meta).font(mono(10.0)).color(theme::MUTED));
                                if let Some(m) = &conflict {
                                    ui.label(RichText::new(m).size(11.0).color(CONFLICT));
                                    ui.horizontal(|ui| {
                                        let theirs =
                                            cs.choices.get(&c.id).copied() == Some(Choice::Take);
                                        if chip(ui, "Keep mine", !theirs, true).clicked() {
                                            cs.choices.insert(c.id.clone(), Choice::Skip);
                                        }
                                        if chip(ui, "Take theirs", theirs, true).clicked() {
                                            cs.choices.insert(c.id.clone(), Choice::Take);
                                        }
                                    });
                                }
                            });
                        });
                    })
                    .response;
                let _ = r;
                if let Some(k) = pick.take() {
                    select(cs, &k);
                }
                ui.painter().hline(
                    ui.min_rect().x_range(),
                    ui.min_rect().bottom(),
                    Stroke::new(1.0, Color32::from_black_alpha(40)),
                );
            }
        });
    if let Mode::Merge { setup } = &cs.mode {
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.cursor().top(),
            Stroke::new(1.0, theme::BORDER),
        );
        egui::Frame::new().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            let taken = rows.iter().filter(|c| cs.choices.get(&c.id).copied() == Some(Choice::Take)).count();
            let conflicts = rows.iter().filter(|c| cs.conflicts.contains_key(&c.id)).count();
            let unresolved_theirs = rows.iter().filter(|c| cs.conflicts.contains_key(&c.id) && cs.choices.get(&c.id).copied() == Some(Choice::Take)).count();
            ui.label(RichText::new(format!("Bring in {taken} of {} changes", rows.len())).strong().size(12.5));
            if conflicts > 0 {
                ui.label(RichText::new(format!("{conflicts} conflict{}: {unresolved_theirs} will take theirs, {} keep yours", if conflicts == 1 { "" } else { "s" }, conflicts - unresolved_theirs)).size(11.0).color(CONFLICT));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let nothing = setup.up_to_date;
                if ui.add_enabled(!nothing, egui::Button::new(RichText::new(format!("Merge into {}", setup.into)).strong().color(Color32::WHITE)).fill(theme::ACCENT)).clicked() {
                    *action = Some(Action::DoMerge);
                }
                if chip(ui, "Cancel", false, true).clicked() {
                    *action = Some(Action::Close);
                }
            });
        });
    }
}
