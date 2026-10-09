//! Application state and the per-frame update loop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use captaine_audio::Peak;
use captaine_model::*;
use captaine_ops::{edit, Op, OpError};
use captaine_project::Session;
use captaine_time::{Rational, RationalTime, Timecode};
use egui::{Context, TextureHandle};

use crate::audio::AudioEngine;
use crate::jobs::{Done, Jobs};
use crate::worker::RenderWorker;
use crate::{copilot, theme, timeline, ui, viewer};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Trim,
    Blade,
    Hand,
    Zoom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Media,
    Cut,
    Edit,
    Color,
    Effects,
    Deliver,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolTab {
    Media,
    Effects,
    Titles,
    Transitions,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspectorTab {
    Inspector,
    History,
    Metadata,
    Mixer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bin {
    All,
    Footage,
    Audio,
    Graphics,
    Stills,
}

/// What the pointer is doing in the timeline.
#[derive(Clone, Debug)]
pub enum Drag {
    None,
    Playhead,
    Pan {
        start_scroll: f32,
    },
    /// Moving a clip; nothing is applied until release.
    Clip {
        item: Id,
        grab_ticks: i64,
        target_track: Id,
        target_start: i64,
    },
    /// Dragging a clip edge; `delta` is the pending trim in ticks.
    Trim {
        item: Id,
        edge: captaine_ops::Edge,
        delta: i64,
    },
}

/// Screen geometry of the timeline.
#[derive(Clone, Debug)]
pub struct TimelineLayout {
    /// x of timeline time zero when scrolled to the start, and pixels per second
    pub origin_x: f32,
    pub scroll: f32,
    pub pps: f32,
    pub ruler: egui::Rect,
    /// `(track, top, height)` from the top of the screen down
    pub rows: Vec<(Id, f32, f32)>,
}

impl TimelineLayout {
    pub fn x(&self, secs: f64) -> f32 {
        self.origin_x + (secs as f32 - self.scroll) * self.pps
    }

    pub fn row_center(&self, track: Id) -> Option<f32> {
        self.rows
            .iter()
            .find(|r| r.0 == track)
            .map(|r| r.1 + r.2 / 2.0)
    }
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub born: Instant,
}

/// `(frames done, frames total, result once finished)` shared with the export thread.
pub type ExportProgress = Arc<std::sync::Mutex<(i64, i64, Option<Result<(), String>>)>>;

pub struct ExportDialog {
    pub open: bool,
    pub codec: captaine_export::Codec,
    pub crf: u32,
    pub path: Option<PathBuf>,
    pub progress: Option<ExportProgress>,
}

pub struct App {
    pub ctx: Context,
    pub session: Session,
    pub path: Option<PathBuf>,
    pub dirty_since: Option<Instant>,

    // view state
    pub page: Page,
    pub tool: Tool,
    pub pool_tab: PoolTab,
    pub inspector_tab: InspectorTab,
    pub bin: Bin,
    pub search: String,
    pub selected_media: Option<Id>,
    pub selected_clip: Option<Id>,
    /// Other items selected together with `selected_clip` (shift-click)
    pub extra_selected: Vec<Id>,
    pub clipboard: Vec<crate::clipboard::Copied>,
    pub snap: bool,
    pub pps: f32,
    pub scroll: f32,
    pub drag: Drag,
    pub mark_in: Option<i64>,
    pub mark_out: Option<i64>,
    pub show_copilot: bool,
    pub show_shortcuts: bool,
    pub new_project: Option<(String, u32, u32, String)>,
    pub export: ExportDialog,
    pub toasts: Vec<Toast>,
    pub central_rect: egui::Rect,
    /// Where the timeline was drawn last frame (screen coordinates); lets tests and tooling aim at things.
    pub timeline_layout: Option<TimelineLayout>,
    /// The Compare / Merge screen, when open
    pub compare: Option<crate::compare::CompareState>,
    pub branch_menu: bool,
    pub branch_draft: String,
    pub commit_draft: String,
    /// Where the branch pill is, so its menu can open under it
    pub branch_pill: egui::Rect,
    /// One-line descriptions of commits, computed once
    pub summaries: HashMap<captaine_project::captaine_vcs::CommitId, String>,
    pub preview_override: Option<Project>,

    // playback
    pub playhead: i64,
    pub playing: bool,
    pub shuttle: f64,
    play_origin: Instant,
    play_origin_secs: f64,
    pub last_requested: Option<i64>,
    audio: Option<AudioEngine>,
    audio_live: bool,

    // engine plumbing
    worker: RenderWorker,
    jobs: Jobs,
    sent_revision: Option<(u64, u64)>,
    pub program: Option<TextureHandle>,
    pub program_tick: i64,
    /// The scopes panel over the viewer, which kind it shows, and its picture
    pub scopes_open: bool,
    /// Smoothed peak levels: one per timeline track, then the master
    pub meters: Vec<f32>,
    pub scope_kind: crate::scopes::ScopeKind,
    pub scope_tex: Option<TextureHandle>,
    pub scope_src: Option<egui::ColorImage>,
    pub scope_dirty: bool,
    pub thumbs: HashMap<Id, TextureHandle>,
    pub waves: HashMap<Id, Arc<Vec<Peak>>>,
    queued_media: std::collections::HashSet<Id>,
    pub copilot: copilot::State,
    last_error: String,
}

impl App {
    pub fn new(ctx: &Context, session: Session, path: Option<PathBuf>) -> App {
        theme::apply(ctx);
        let mut app = App {
            ctx: ctx.clone(),
            session,
            path,
            dirty_since: None,
            page: Page::Edit,
            tool: Tool::Select,
            pool_tab: PoolTab::Media,
            inspector_tab: InspectorTab::Inspector,
            bin: Bin::All,
            search: String::new(),
            selected_media: None,
            selected_clip: None,
            extra_selected: Vec::new(),
            clipboard: Vec::new(),
            snap: true,
            pps: 40.0,
            scroll: 0.0,
            drag: Drag::None,
            mark_in: None,
            mark_out: None,
            show_copilot: false,
            show_shortcuts: false,
            new_project: None,
            export: ExportDialog {
                open: false,
                codec: captaine_export::Codec::H264,
                crf: 18,
                path: None,
                progress: None,
            },
            toasts: Vec::new(),
            central_rect: egui::Rect::from_min_size(
                egui::pos2(330.0, 40.0),
                egui::vec2(850.0, 500.0),
            ),
            preview_override: None,
            timeline_layout: None,
            compare: None,
            branch_menu: false,
            branch_draft: String::new(),
            commit_draft: String::new(),
            branch_pill: egui::Rect::NOTHING,
            summaries: HashMap::new(),
            playhead: 0,
            playing: false,
            shuttle: 1.0,
            play_origin: Instant::now(),
            play_origin_secs: 0.0,
            last_requested: None,
            audio: AudioEngine::new(),
            audio_live: false,
            worker: RenderWorker::spawn(ctx.clone()),
            jobs: Jobs::spawn(ctx.clone()),
            sent_revision: None,
            program: None,
            program_tick: 0,
            scopes_open: false,
            meters: Vec::new(),
            scope_kind: crate::scopes::ScopeKind::Waveform,
            scope_tex: None,
            scope_src: None,
            scope_dirty: true,
            thumbs: HashMap::new(),
            waves: HashMap::new(),
            queued_media: Default::default(),
            copilot: copilot::State::default(),
            last_error: String::new(),
        };
        app.queue_media_jobs();
        app
    }

    // ---- project access -------------------------------------------------------------------

    /// Show `project` in the viewer instead of the real one (Copilot previews); `None` goes back.
    pub fn set_preview(&mut self, project: Option<Project>) {
        self.preview_override = project;
        self.sent_revision = None;
    }

    pub fn do_op_as(&mut self, author: captaine_ops::Author, op: Op) -> bool {
        match self.session.apply_as(author, op) {
            Ok(()) => {
                self.dirty_since.get_or_insert_with(Instant::now);
                true
            }
            Err(e) => {
                self.toast(e, true);
                false
            }
        }
    }

    pub fn replace_session(&mut self, session: Session, path: Option<PathBuf>) {
        self.pause();
        self.session = session;
        self.path = path;
        self.selected_clip = None;
        self.selected_media = None;
        self.playhead = 0;
        self.mark_in = None;
        self.mark_out = None;
        self.preview_override = None;
        self.sent_revision = None;
        self.dirty_since = None;
        self.copilot.suggestion = None;
        self.queue_media_jobs();
        self.fit_on_start();
    }

    /// The whole project was swapped for another version (branch switch, merge): refresh everything that was derived from it.
    pub fn session_changed(&mut self) {
        self.sent_revision = None;
        self.last_requested = None;
        self.preview_override = None;
        self.copilot.suggestion = None;
        if self.selected_clip.is_some() && self.selected_span().is_none() {
            self.selected_clip = None;
        }
        self.playhead = self.playhead.clamp(0, self.duration_ticks().max(0));
        self.summaries.clear();
        self.drag = Drag::None;
    }

    pub fn fit_on_start(&mut self) {
        if self.duration_ticks() > 0 {
            timeline::fit(self);
        }
    }

    pub fn project(&self) -> &Project {
        self.session.editor.project()
    }

    pub fn timeline(&self) -> Option<&Timeline> {
        self.project().timelines.values().next().map(|t| t.as_ref())
    }

    pub fn timeline_id(&self) -> Option<Id> {
        self.timeline().map(|t| t.id)
    }

    pub fn rate(&self) -> Rational {
        self.timeline().map_or(Rational::FPS_30, |t| t.rate)
    }

    pub fn fps(&self) -> f64 {
        self.rate().as_f64()
    }

    pub fn duration_ticks(&self) -> i64 {
        self.timeline().map_or(0, |t| t.duration().value)
    }

    pub fn tc(&self, tick: i64) -> String {
        let r = self.rate();
        Timecode::from_frames(tick.max(0), r, r.is_ntsc())
            .map(|t| t.to_string())
            .unwrap_or_else(|_| format!("{tick}f"))
    }

    pub fn time(&self, tick: i64) -> RationalTime {
        RationalTime::new(tick, self.rate())
    }

    /// The selected clip with its track and start tick, if it still exists.
    pub fn selected(&self) -> Option<(Clip, Id, i64)> {
        let id = self.selected_clip?;
        let tl = self.timeline()?;
        tl.tracks.iter().find_map(|t| {
            t.items
                .iter()
                .zip(t.item_starts())
                .find_map(|(it, st)| match it {
                    Item::Clip(c) if c.id == id => Some((c.clone(), t.id, st)),
                    _ => None,
                })
        })
    }

    /// The selected title with its track and start tick, if the selection is a title.
    pub fn selected_title(&self) -> Option<(Title, Id, i64)> {
        let id = self.selected_clip?;
        let tl = self.timeline()?;
        tl.tracks.iter().find_map(|t| {
            t.items
                .iter()
                .zip(t.item_starts())
                .find_map(|(it, st)| match it {
                    Item::Title(x) if x.id == id => Some((x.clone(), t.id, st)),
                    _ => None,
                })
        })
    }

    /// `(item, track, start, length)` of the selection, whether it is a clip or a title.
    pub fn selected_span(&self) -> Option<(Id, Id, i64, i64)> {
        if let Some((c, track, start)) = self.selected() {
            return Some((c.id, track, start, c.duration.value));
        }
        self.selected_title()
            .map(|(t, track, start)| (t.id, track, start, t.duration.value))
    }

    /// Put a new title on the timeline: on `track` (default the top video track) at `at` (default the playhead).
    pub fn add_title(&mut self, template: TitleTemplate, at: Option<i64>, track: Option<Id>) {
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        let track = track.or_else(|| {
            tl.tracks
                .iter()
                .rev()
                .find(|t| t.kind == TrackKind::Video)
                .map(|t| t.id)
        });
        let Some(track) = track else {
            self.toast("There is no video track to put a title on", true);
            return;
        };
        if tl
            .tracks
            .iter()
            .find(|t| t.id == track)
            .is_some_and(|t| t.kind != TrackKind::Video)
        {
            self.toast("Titles go on video tracks", true);
            return;
        }
        let secs = 3.0;
        let dur = RationalTime::new(((secs * self.fps()).round() as i64).max(1), tl.rate);
        let title = Title::new(Id::new(), template, dur);
        let id = title.id;
        let at = self.time(at.unwrap_or(self.playhead).max(0));
        if self.do_plan(|p| edit::overwrite_item(p, track, at, Item::Title(title))) {
            self.selected_clip = Some(id);
            self.inspector_tab = InspectorTab::Inspector;
        }
    }

    // ---- editing ----------------------------------------------------------------------------

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast {
            text: text.into(),
            error,
            born: Instant::now(),
        });
    }

    pub fn do_op(&mut self, op: Op) -> bool {
        match self.session.apply(op) {
            Ok(()) => {
                self.dirty_since.get_or_insert_with(Instant::now);
                true
            }
            Err(e) => {
                self.toast(e, true);
                false
            }
        }
    }

    pub fn do_plan(&mut self, plan: impl FnOnce(&Project) -> Result<Op, OpError>) -> bool {
        match plan(self.project()) {
            Ok(op) => self.do_op(op),
            Err(e) => {
                self.toast(e.to_string(), true);
                false
            }
        }
    }

    /// For sliders: folds a drag into one undo step per `key`.
    pub fn do_merging(&mut self, op: Op, key: &str) {
        match self.session.apply_merging(op, key) {
            Ok(()) => {
                self.dirty_since.get_or_insert_with(Instant::now);
            }
            Err(e) => self.toast(e, true),
        }
    }

    pub fn end_merge(&mut self) {
        self.session.end_merge();
    }

    pub fn undo(&mut self) {
        if self.session.undo().is_some() {
            self.dirty_since.get_or_insert_with(Instant::now);
        }
    }

    pub fn redo(&mut self) {
        if self.session.redo().is_some() {
            self.dirty_since.get_or_insert_with(Instant::now);
        }
    }

    pub fn import(&mut self, files: Vec<PathBuf>) {
        for f in files {
            let canon = f.canonicalize().unwrap_or(f.clone());
            if self
                .project()
                .media
                .values()
                .any(|m| Path::new(&m.path) == canon)
            {
                self.toast(
                    format!(
                        "{} is already in the project",
                        f.file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                    ),
                    false,
                );
                continue;
            }
            match captaine_media::media_ref(&f, Id::new()) {
                Ok(m) => {
                    let name = m.name.clone();
                    if self.do_op(Op::AddMedia { media: m }) {
                        self.toast(format!("Imported {name}"), false);
                    }
                }
                Err(e) => self.toast(format!("{}: {e}", f.display()), true),
            }
        }
        self.queue_media_jobs();
    }

    fn queue_media_jobs(&mut self) {
        let todo: Vec<MediaRef> = self
            .project()
            .media
            .values()
            .filter(|m| !self.queued_media.contains(&m.id))
            .cloned()
            .collect();
        for m in todo {
            self.queued_media.insert(m.id);
            self.jobs.queue(m);
        }
    }

    /// Put a media item on the timeline, at the end of `track` or at `at` (overwriting).
    pub fn place_media(&mut self, media: Id, track: Option<Id>, at: Option<i64>) {
        let Some(m) = self.project().media.get(&media).cloned() else {
            return;
        };
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        let kind = if m.has_video {
            TrackKind::Video
        } else {
            TrackKind::Audio
        };
        let track = track.or_else(|| tl.tracks.iter().find(|t| t.kind == kind).map(|t| t.id));
        let Some(track) = track else {
            self.toast(
                "There is no suitable track; add one from the Timeline menu",
                true,
            );
            return;
        };
        let tr = tl
            .tracks
            .iter()
            .find(|t| t.id == track)
            .expect("track exists");
        if tr.kind == TrackKind::Video && !m.has_video {
            self.toast(
                format!("{} has no picture; drop it on an audio track", m.name),
                true,
            );
            return;
        }
        if tr.kind == TrackKind::Audio && !m.has_audio {
            self.toast(format!("{} has no audio", m.name), true);
            return;
        }
        let Some(len) = m.duration else { return };
        let frames = len
            .rescale(tl.rate, captaine_time::Rounding::Floor)
            .map_or(1, |t| t.value.max(1));
        let clip = Clip::new(
            Id::new(),
            m.name.clone(),
            m.id,
            RationalTime::new(0, len.rate),
            RationalTime::new(frames, tl.rate),
        );
        let id = clip.id;
        let ok = match at {
            None => self.do_op(edit::append(self.project(), track, clip)),
            Some(at) => {
                let at = self.time(at.max(0));
                self.do_plan(|p| edit::overwrite(p, track, at, clip))
            }
        };
        if ok {
            self.selected_clip = Some(id);
        }
    }

    // ---- playback ---------------------------------------------------------------------------

    fn secs_to_tick(&self, secs: f64) -> i64 {
        (secs * self.fps()).round() as i64
    }

    pub fn set_playhead(&mut self, tick: i64) {
        self.playhead = tick.clamp(0, self.duration_ticks().max(0));
        if self.playing {
            self.restart_clock();
        }
    }

    fn restart_clock(&mut self) {
        self.play_origin = Instant::now();
        self.play_origin_secs = self.playhead as f64 / self.fps();
        self.audio_live = false;
        if self.shuttle == 1.0 {
            let tl = self.timeline_id();
            if let (Some(a), Some(tl)) = (&mut self.audio, tl) {
                a.start(self.session.editor.project(), tl, self.play_origin_secs);
                self.audio_live = true;
            }
        } else if let Some(a) = &mut self.audio {
            a.stop();
        }
    }

    pub fn play(&mut self, shuttle: f64) {
        if self.playing && self.shuttle == shuttle {
            return;
        }
        if !self.playing && self.playhead >= self.duration_ticks() {
            self.playhead = 0;
        }
        self.playing = true;
        self.shuttle = shuttle;
        self.restart_clock();
    }

    pub fn pause(&mut self) {
        if self.playing {
            self.playing = false;
            self.audio_live = false;
            if let Some(a) = &mut self.audio {
                a.stop();
            }
        }
    }

    pub fn toggle_play(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.play(1.0);
        }
    }

    fn advance_playback(&mut self) {
        if !self.playing {
            return;
        }
        let secs = match (&self.audio, self.audio_live) {
            (Some(a), true) => {
                if !a.is_primed() {
                    return; // sound is still warming up: hold the picture
                }
                a.position_secs()
            }
            _ => self.play_origin_secs + self.play_origin.elapsed().as_secs_f64() * self.shuttle,
        };
        let end = self.duration_ticks();
        let tick = self.secs_to_tick(secs);
        if (self.shuttle > 0.0 && tick >= end) || (self.shuttle < 0.0 && tick <= 0) {
            self.playhead = tick.clamp(0, end);
            self.pause();
            return;
        }
        self.playhead = tick.clamp(0, end);
    }

    // ---- per-frame update --------------------------------------------------------------------

    fn sync_worker(&mut self, ctx: &Context) {
        let rev = (self.session.epoch(), self.session.editor.revision());
        if self.sent_revision != Some(rev) {
            if let Some(tl) = self.timeline_id() {
                let shown = self
                    .preview_override
                    .clone()
                    .unwrap_or_else(|| self.project().clone());
                self.worker.set_project(Arc::new(shown), tl);
            }
            self.sent_revision = Some(rev);
            self.last_requested = None;
            self.queue_media_jobs();
            if self.playing && self.shuttle == 1.0 {
                self.restart_clock();
            }
            if self.selected_clip.is_some() && self.selected_span().is_none() {
                self.selected_clip = None;
            }
            self.prune_selection();
        }
        if self.playhead != self.program_tick || self.last_requested != Some(self.playhead) {
            let busy = self.worker.in_flight >= 2;
            if self.last_requested != Some(self.playhead)
                && !(self.playing && busy)
                && self.timeline_id().is_some()
            {
                self.worker.request(self.playhead);
                self.last_requested = Some(self.playhead);
            }
        }
        while let Some(f) = self.worker.poll() {
            if f.tick < 0 {
                continue; // a skipped-frame placeholder
            }
            match f.image {
                Ok(img) => {
                    if self.scopes_open {
                        self.scope_src = Some(img.clone());
                        self.scope_dirty = true;
                    }
                    match &mut self.program {
                        Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                        None => {
                            self.program =
                                Some(ctx.load_texture("program", img, egui::TextureOptions::LINEAR))
                        }
                    }
                    self.program_tick = f.tick;
                    self.last_error.clear();
                }
                Err(e) => {
                    if e != self.last_error && !e.is_empty() {
                        self.toast(e.clone(), true);
                    }
                    self.last_error = e;
                }
            }
        }
        for done in self.jobs.poll() {
            match done {
                Done::Thumb(id, img) => {
                    self.thumbs.insert(
                        id,
                        ctx.load_texture(format!("thumb-{id}"), img, egui::TextureOptions::LINEAR),
                    );
                }
                Done::Wave(id, peaks) => {
                    self.waves.insert(id, Arc::new(peaks));
                }
            }
        }
    }

    fn autosave(&mut self, force: bool) {
        let Some(since) = self.dirty_since else {
            return;
        };
        if !force && since.elapsed() < Duration::from_millis(600) {
            return;
        }
        if let Some(p) = &self.path {
            match self.session.save(p) {
                Ok(()) => self.dirty_since = None,
                Err(e) => {
                    self.dirty_since = None;
                    self.toast(format!("could not save: {e}"), true);
                }
            }
        } else {
            self.dirty_since = None;
        }
    }

    pub fn project_title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| {
                s.to_string_lossy()
                    .trim_end_matches(".captaine")
                    .to_string()
            })
            .unwrap_or_else(|| "Untitled".to_string())
    }

    pub fn update(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        self.advance_playback();
        self.update_meters(ctx);
        self.sync_worker(ctx);
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            self.import(vec![f.path().to_path_buf()]);
        }
        if self.compare.is_none() {
            ui::handle_shortcuts(self, ctx);
        }
        ui::top_bar(self, root);
        if self.compare.is_some() {
            crate::compare::show(self, root);
        } else {
            timeline::panel(self, root);
            ui::media_pool(self, root);
            ui::inspector(self, root);
            viewer::central(self, root);
        }
        copilot::show(self, ctx);
        crate::branches_ui::popup(self, ctx, self.branch_pill);
        ui::dialogs(self, ctx);
        ui::toasts(self, ctx);
        self.toasts
            .retain(|t| t.born.elapsed() < Duration::from_secs(5));
        self.autosave(false);
        if self.playing || !self.toasts.is_empty() || self.dirty_since.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    pub fn on_exit(&mut self) {
        self.pause();
        self.autosave(true);
    }
}

impl App {
    /// Redraw the scope picture from the latest frame, if the panel is open.
    pub fn refresh_scope(&mut self, ctx: &egui::Context) {
        let Some(src) = &self.scope_src else { return };
        let img = crate::scopes::render(self.scope_kind, src);
        match &mut self.scope_tex {
            Some(t) => t.set(img, egui::TextureOptions::NEAREST),
            None => {
                self.scope_tex = Some(ctx.load_texture("scope", img, egui::TextureOptions::NEAREST))
            }
        }
        self.scope_dirty = false;
    }
}

impl App {
    /// Meters rise at once and fall away smoothly.
    fn update_meters(&mut self, ctx: &Context) {
        let fresh = if self.playing && self.audio_live {
            self.audio.as_ref().and_then(|a| a.levels())
        } else {
            None
        };
        if fresh.is_none() && self.meters.iter().all(|m| *m < 0.001) {
            self.meters.iter_mut().for_each(|m| *m = 0.0);
            return;
        }
        let decay = if self.playing { 0.82 } else { 0.7 };
        match fresh {
            Some(v) => {
                self.meters.resize(v.len(), 0.0);
                for (m, n) in self.meters.iter_mut().zip(v) {
                    *m = n.max(*m * decay);
                }
            }
            None => self.meters.iter_mut().for_each(|m| *m *= decay),
        }
        ctx.request_repaint();
    }
}
