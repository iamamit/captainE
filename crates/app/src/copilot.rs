//! The Copilot panel. This is the *local* Copilot: a handful of real skills that turn a request into ops
//! and apply them through the same command layer a person uses (author: `Agent("copilot")`).
//! The AI agent of docs/08 will plug in behind the same panel.

use std::sync::mpsc::{channel, Receiver};

use captaine_model::*;
use captaine_ops::{apply, Author, Op};
use egui::{pos2, vec2, Align2, Color32, Context, RichText, Stroke};

use crate::app::App;
use crate::theme::{self, mono, ui_font};
use crate::ui::chip;

pub struct Msg {
    pub user: bool,
    pub text: String,
}

#[derive(Clone)]
pub struct Suggestion {
    pub title: String,
    pub body: String,
    pub op: Op,
}

type Analysis = Result<(String, Option<Suggestion>), String>;

/// Builds a model connection for one request.
pub type TransportFactory =
    std::sync::Arc<dyn Fn() -> Box<dyn captaine_agent::Transport> + Send + Sync>;

/// Messages from a background analysis or agent run.
pub enum Work {
    /// a progress line (a tool the agent is calling)
    Step(String),
    Done(Box<Analysis>),
}

#[derive(Default)]
pub struct State {
    pub messages: Vec<Msg>,
    pub input: String,
    pub suggestion: Option<Suggestion>,
    working: Option<Receiver<Work>>,
    greeted: bool,
    /// Progress lines of the run in flight
    pub steps: Vec<String>,
    /// What was asked in the run in flight, kept for the conversation history
    pending_user: String,
    history: Vec<captaine_agent::ChatTurn>,
    /// Whether an API key is available (checked at start and when it changes)
    key: Option<Option<String>>,
    pub key_input: String,
    /// The branch Copilot last saved a suggestion on, kept so it can be opened for review
    pub last_branch: Option<String>,
    /// Supplies the model connection instead of the Anthropic API (tests use a scripted one).
    pub transport: Option<TransportFactory>,
}

impl State {
    /// A request is in flight (Claude or a local analysis).
    pub fn is_working(&self) -> bool {
        self.working.is_some()
    }
}

const GREETING_CLAUDE: &str = "Hi! Tell me what to do with the timeline: tighten a clip, add a title, fix the colour, rebalance the sound. I'll make the edits as a suggestion you can preview, apply or discard. Select a clip first if you mean a specific one.";

const GREETING: &str = "Hi! I run a few built-in skills for now (the AI agent isn't connected yet). Select a clip and try: remove silences, warm up the colour, add a 12-frame fade-in.";

pub fn show(app: &mut App, ctx: &Context) {
    if !app.show_copilot {
        return;
    }
    if !app.copilot.greeted {
        app.copilot.greeted = true;
        let connected = api_key(app).is_some() || app.copilot.transport.is_some();
        app.copilot.messages.push(Msg {
            user: false,
            text: if connected { GREETING_CLAUDE } else { GREETING }.into(),
        });
    }
    // collect progress and results from the background run
    if let Some(rx) = &app.copilot.working {
        let events: Vec<Work> = rx.try_iter().collect();
        let mut finished = None;
        for e in events {
            match e {
                Work::Step(s) => app.copilot.steps.push(s),
                Work::Done(d) => finished = Some(*d),
            }
        }
        if let Some(done) = finished {
            app.copilot.working = None;
            app.copilot.steps.clear();
            let asked = std::mem::take(&mut app.copilot.pending_user);
            match done {
                Ok((text, sug)) => {
                    if !asked.is_empty() {
                        app.copilot.history.push(captaine_agent::ChatTurn {
                            user: asked,
                            assistant: text.clone(),
                        });
                    }
                    app.copilot.messages.push(Msg { user: false, text });
                    app.copilot.suggestion = sug;
                }
                Err(e) => app.copilot.messages.push(Msg {
                    user: false,
                    text: format!("That didn't work: {e}"),
                }),
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }
    }
    let has_key = api_key(app).is_some() || app.copilot.transport.is_some();
    let mut connect: Option<String> = None;
    let mut disconnect = false;
    let width = 300.0;
    let pos = pos2(
        app.central_rect.right() - width - 16.0,
        app.central_rect.top() + 46.0,
    );
    let mut close = false;
    let mut send: Option<String> = None;
    let mut apply_it = false;
    let mut branch_it = false;
    let mut review: Option<String> = None;
    let mut preview_it = false;
    let mut discard = false;
    egui::Area::new(egui::Id::new("copilot"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(10.0)
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(120),
                })
                .show(ui, |ui| {
                    ui.set_width(width);
                    // header
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (r, _) =
                                    ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover());
                                ui.painter().circle_filled(r.center(), 12.0, theme::ACCENT);
                                ui.painter().text(
                                    r.center(),
                                    Align2::CENTER_CENTER,
                                    "cE",
                                    ui_font(9.5),
                                    Color32::WHITE,
                                );
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label(RichText::new("Copilot").strong().size(13.0));
                                    let target = app
                                        .selected()
                                        .map_or("no clip selected".to_string(), |(c, ..)| c.name);
                                    ui.horizontal(|ui| {
                                        let (d, _) = ui.allocate_exact_size(
                                            vec2(8.0, 12.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(d.center(), 3.0, theme::OK);
                                        ui.label(
                                            RichText::new(format!(
                                                "{} · {} · {target}",
                                                if has_key { "Claude" } else { "Local skills" },
                                                if app.copilot.working.is_some() {
                                                    "Working…"
                                                } else {
                                                    "Ready"
                                                }
                                            ))
                                            .size(10.5)
                                            .color(theme::MUTED),
                                        );
                                    });
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Min),
                                    |ui| {
                                        let (r, resp) = ui.allocate_exact_size(
                                            vec2(20.0, 20.0),
                                            egui::Sense::click(),
                                        );
                                        let ink = if resp.hovered() {
                                            Color32::WHITE
                                        } else {
                                            theme::MUTED
                                        };
                                        let c = r.center();
                                        for (a, b) in
                                            [((-4.0, -4.0), (4.0, 4.0)), ((-4.0, 4.0), (4.0, -4.0))]
                                        {
                                            ui.painter().line_segment(
                                                [
                                                    pos2(c.x + a.0, c.y + a.1),
                                                    pos2(c.x + b.0, c.y + b.1),
                                                ],
                                                Stroke::new(1.4, ink),
                                            );
                                        }
                                        if resp.clicked() {
                                            close = true;
                                        }
                                    },
                                );
                            });
                        });
                    ui.painter().hline(
                        ui.min_rect().x_range(),
                        ui.cursor().top(),
                        Stroke::new(1.0, theme::BORDER),
                    );
                    egui::Frame::new()
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(300.0)
                                .stick_to_bottom(true)
                                .auto_shrink([true, true])
                                .show(ui, |ui| {
                                    for m in &app.copilot.messages {
                                        bubble(ui, m);
                                    }
                                    if app.copilot.working.is_some() {
                                        let n = app.copilot.steps.len();
                                        for s in app.copilot.steps.iter().skip(n.saturating_sub(4)) {
                                            ui.label(RichText::new(format!("· {s}")).font(mono(10.0)).color(theme::MUTED));
                                        }
                                        ui.label(RichText::new("Working…").size(11.0).color(theme::MUTED));
                                    }
                                    if let Some(s) = &app.copilot.suggestion {
                                        suggestion_card(
                                            ui,
                                            s,
                                            app.preview_override.is_some(),
                                            suggestion_is_big(s),
                                            &mut apply_it,
                                            &mut branch_it,
                                            &mut preview_it,
                                            &mut discard,
                                        );
                                    }
                                });
                            if let Some(b) = app.copilot.last_branch.clone() {
                                ui.add_space(4.0);
                                if ui.add(egui::Button::new(RichText::new(format!("Review {b}")).strong().color(Color32::WHITE)).fill(theme::ACCENT)).clicked() {
                                    review = Some(b);
                                }
                            }
                            ui.add_space(6.0);
                            ui.horizontal_wrapped(|ui| {
                                let chips: [(&str, &str); 3] = if has_key {
                                    [
                                        ("Remove silences", "Remove the silences from the selected clip"),
                                        ("Warm colours", "Warm up the colour of the selected clip a little"),
                                        ("Add a title", "Add a lower third at the playhead that says the name of the selected clip"),
                                    ]
                                } else {
                                    [
                                        ("Remove silences", "remove silences"),
                                        ("Warm colours", "warm up the colour"),
                                        ("Add fades", "add a 12-frame fade in and fade out"),
                                    ]
                                };
                                for (label, text) in chips {
                                    if chip(ui, label, false, app.copilot.working.is_none()).clicked() {
                                        send = Some(text.to_string());
                                    }
                                }
                            });
                            if !has_key {
                                ui.add_space(6.0);
                                egui::Frame::new()
                                    .fill(theme::PANEL_DARK)
                                    .corner_radius(6.0)
                                    .inner_margin(egui::Margin::same(8))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("Connect Claude").strong().size(11.5));
                                        ui.label(RichText::new("for free-form requests. Paste an Anthropic API key (kept in ~/.captaine, readable only by you) or set ANTHROPIC_API_KEY. Only project structure and analysis results are sent, never your footage.").size(10.0).color(theme::MUTED));
                                        ui.horizontal(|ui| {
                                            let w = ui.available_width() - 52.0;
                                            ui.add_sized([w, 24.0], egui::TextEdit::singleline(&mut app.copilot.key_input).password(true).hint_text("sk-ant-…").margin(egui::Margin::symmetric(6, 4)));
                                            if ui.add_enabled(!app.copilot.key_input.trim().is_empty(), egui::Button::new("Save").min_size(vec2(44.0, 24.0))).clicked() {
                                                connect = Some(std::mem::take(&mut app.copilot.key_input));
                                            }
                                        });
                                    });
                            } else if ui.add(egui::Button::new(RichText::new("Disconnect Claude").size(10.0).color(theme::FAINT)).frame(false)).clicked() {
                                disconnect = true;
                            }
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                let w = ui.available_width() - 62.0;
                                let r = ui.add_sized(
                                    [w, 28.0],
                                    egui::TextEdit::singleline(&mut app.copilot.input)
                                        .hint_text("Ask Copilot to edit…")
                                        .margin(egui::Margin::symmetric(8, 5)),
                                );
                                let enter =
                                    r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let go = ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("Send").color(Color32::WHITE),
                                        )
                                        .fill(theme::ACCENT)
                                        .min_size(vec2(54.0, 28.0)),
                                    )
                                    .clicked();
                                if (enter || go) && !app.copilot.input.trim().is_empty() {
                                    send = Some(std::mem::take(&mut app.copilot.input));
                                    r.request_focus();
                                }
                            });
                        });
                });
        });
    if close {
        app.show_copilot = false;
    }
    if let Some(k) = connect {
        match captaine_agent::Credentials::save(&k) {
            Ok(()) => {
                app.copilot.key = None;
                app.toast("Connected: Copilot will now use Claude", false);
            }
            Err(e) => app.toast(format!("could not save the key: {e}"), true),
        }
    }
    if disconnect {
        captaine_agent::Credentials::forget();
        app.copilot.key = None;
        app.toast(
            "Claude disconnected; Copilot is using its local skills",
            false,
        );
    }
    if let Some(b) = review {
        crate::branches_ui::start_merge(app, ctx, &b);
    }
    if let Some(text) = send {
        ask(app, text);
    }
    if preview_it {
        if let Some(s) = app.copilot.suggestion.clone() {
            match apply(app.project(), &s.op) {
                Ok((p, _)) => app.set_preview(Some(p)),
                Err(e) => app.toast(e.to_string(), true),
            }
        }
    }
    if apply_it {
        if let Some(s) = app.copilot.suggestion.take() {
            app.set_preview(None);
            if app.do_op_as(Author::Agent("copilot".into()), s.op) {
                app.copilot.messages.push(Msg {
                    user: false,
                    text: format!("Done: {}. Undo with ⌘Z.", s.title.to_lowercase()),
                });
            }
        }
    }
    if branch_it {
        if let Some(s) = app.copilot.suggestion.take() {
            app.set_preview(None);
            let ops = match s.op {
                Op::Transaction { ops, .. } => ops,
                other => vec![other],
            };
            let wanted = format!("agent/{}", captaine_project::slugify(&s.title, 4));
            match app.session.commit_to_new_branch(
                &wanted,
                Author::Agent("copilot".into()),
                &s.title,
                ops,
            ) {
                Ok(name) => {
                    app.dirty_since.get_or_insert_with(std::time::Instant::now);
                    app.summaries.clear();
                    app.copilot.messages.push(Msg { user: false, text: format!("Saved on branch {name}. Your timeline is unchanged: review it, then merge what you like.") });
                    app.copilot.last_branch = Some(name);
                }
                Err(e) => app.toast(e, true),
            }
        }
    }
    if discard {
        app.set_preview(None);
        app.copilot.suggestion = None;
    }
    if let Some(b) = app.copilot.last_branch.clone() {
        let exists = app.session.branch_infos().iter().any(|i| i.name == b);
        if !exists || app.session.branch() == b {
            app.copilot.last_branch = None;
        }
    }
}

/// Edits with more than a couple of steps are offered on a branch by default.
fn suggestion_is_big(s: &Suggestion) -> bool {
    match &s.op {
        Op::Transaction { ops, .. } => ops.len() >= 3,
        _ => false,
    }
}

fn bubble(ui: &mut egui::Ui, m: &Msg) {
    let (fill, align) = if m.user {
        (theme::ACCENT, egui::Align::Max)
    } else {
        (theme::FIELD, egui::Align::Min)
    };
    ui.with_layout(egui::Layout::top_down(align), |ui| {
        egui::Frame::new()
            .fill(fill)
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.set_max_width(250.0);
                ui.label(RichText::new(&m.text).size(12.0).color(if m.user {
                    Color32::WHITE
                } else {
                    theme::TEXT
                }));
            });
    });
    ui.add_space(6.0);
}

#[allow(clippy::too_many_arguments)]
fn suggestion_card(
    ui: &mut egui::Ui,
    s: &Suggestion,
    previewing: bool,
    big: bool,
    apply_it: &mut bool,
    branch_it: &mut bool,
    preview_it: &mut bool,
    discard: &mut bool,
) {
    egui::Frame::new()
        .fill(theme::PANEL_DARK)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("SUGGESTED EDIT")
                    .font(mono(9.5))
                    .color(theme::MUTED),
            );
            ui.add_space(2.0);
            ui.label(RichText::new(&s.title).strong().size(12.5));
            ui.label(RichText::new(&s.body).size(11.5).color(theme::MUTED));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                // a big edit is best reviewed on its own branch; a small one can just go in
                let primary = |label: &str| egui::Button::new(RichText::new(label).strong().color(Color32::WHITE)).fill(theme::ACCENT);
                let quiet = |label: &str| egui::Button::new(RichText::new(label).color(theme::TEXT)).fill(theme::FIELD);
                let (apply_b, branch_b) = if big { (quiet("Apply here"), primary("Review on a branch")) } else { (primary("Apply"), quiet("Review on a branch")) };
                if ui.add(apply_b).clicked() {
                    *apply_it = true;
                }
                if ui.add(branch_b).on_hover_text("Saves the edit on a new agent/… branch. Your timeline stays as it is; compare, then merge what you like.").clicked() {
                    *branch_it = true;
                }
                if chip(
                    ui,
                    if previewing { "Previewing" } else { "Preview" },
                    previewing,
                    true,
                )
                .clicked()
                {
                    *preview_it = true;
                }
                if ui
                    .add(
                        egui::Button::new(RichText::new("Discard").color(theme::MUTED))
                            .frame(false),
                    )
                    .clicked()
                {
                    *discard = true;
                }
            });
        });
    ui.add_space(6.0);
}

// ---- skills ---------------------------------------------------------------------------------------

fn frames_before(text: &str, word: &str) -> Option<i64> {
    // "12-frame fade in" / "12 frame fade-in"
    let idx = text.find(word)?;
    let before = &text[..idx];
    let digits: String = before
        .trim_end()
        .trim_end_matches("frames")
        .trim_end_matches("frame")
        .trim_end_matches(['-', ' '])
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}

/// The key for Claude, if there is one (looked up once and again after it changes).
fn api_key(app: &mut App) -> Option<String> {
    if app.copilot.key.is_none() {
        app.copilot.key = Some(captaine_agent::Credentials::load());
    }
    app.copilot.key.clone().flatten()
}

/// Handle a request: Claude when a key is set, the built-in skills otherwise.
pub fn ask(app: &mut App, text: String) {
    if app.copilot.transport.is_some() {
        return ask_claude(app, text, String::new());
    }
    match api_key(app) {
        Some(key) => ask_claude(app, text, key),
        None => ask_local(app, text),
    }
}

/// The built-in rule-based skills (no network).
pub fn ask_local(app: &mut App, text: String) {
    let t = text.to_lowercase();
    app.copilot.messages.push(Msg {
        user: true,
        text: text.clone(),
    });
    app.set_preview(None);
    app.copilot.suggestion = None;
    let Some((clip, _, start)) = app.selected() else {
        app.copilot.messages.push(Msg {
            user: false,
            text: "Select a clip in the timeline first, then ask again.".into(),
        });
        return;
    };
    let fps = app.fps();
    let mut ops: Vec<Op> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut video = clip.video;
    let mut audio = clip.audio;
    let (mut vchanged, mut achanged) = (false, false);
    if t.contains("warm") {
        video.color.temperature = (video.color.temperature + 1500.0).min(10000.0);
        video.color.saturation = (video.color.saturation + 5.0).min(200.0);
        notes.push("warm the colour (+1500 K, slightly richer)".into());
        vchanged = true;
    }
    if t.contains("cool") || t.contains("cold") {
        video.color.temperature = (video.color.temperature - 1500.0).max(2500.0);
        notes.push("cool the colour (−1500 K)".into());
        vchanged = true;
    }
    if t.contains("bright") {
        video.color.exposure = (video.color.exposure + 0.4).min(3.0);
        notes.push("brighten by 0.4 stops".into());
        vchanged = true;
    }
    if t.contains("dark") {
        video.color.exposure = (video.color.exposure - 0.4).max(-3.0);
        notes.push("darken by 0.4 stops".into());
        vchanged = true;
    }
    if t.contains("black and white") || t.contains("b&w") || t.contains("desaturate") {
        video.color.saturation = 0.0;
        notes.push("make it black and white".into());
        vchanged = true;
    }
    if t.contains("fade") {
        let n_in = frames_before(&t, "fade in")
            .or_else(|| frames_before(&t, "fade-in"))
            .unwrap_or(12);
        let n_out = frames_before(&t, "fade out")
            .or_else(|| frames_before(&t, "fade-out"))
            .unwrap_or(12);
        let dur = clip.duration.value;
        if t.contains("fade in") || t.contains("fade-in") {
            audio.fade_in = n_in.clamp(0, dur);
            notes.push(format!("{n_in}-frame audio fade-in"));
            achanged = true;
        }
        if t.contains("fade out") || t.contains("fade-out") {
            audio.fade_out = n_out.clamp(0, dur - audio.fade_in);
            notes.push(format!("{n_out}-frame audio fade-out"));
            achanged = true;
        }
    }
    if vchanged {
        ops.push(Op::SetVideo {
            item: clip.id,
            video,
        });
    }
    if achanged {
        ops.push(Op::SetAudio {
            item: clip.id,
            audio,
        });
    }
    if t.contains("silence") {
        let Some(m) = app
            .project()
            .media
            .get(&clip.media)
            .cloned()
            .filter(|m| m.has_audio)
        else {
            app.copilot.messages.push(Msg {
                user: false,
                text: format!("{} has no audio to analyse.", clip.name),
            });
            return;
        };
        let (tx, rx) = channel();
        app.copilot.working = Some(rx);
        let (project, clip2, ctx) = (app.project().clone(), clip.clone(), ());
        let _ = (ctx, start);
        std::thread::spawn(move || {
            let _ = tx.send(Work::Done(Box::new(remove_silences(
                &project, &clip2, &m, fps,
            ))));
        });
        return;
    }
    if ops.is_empty() {
        app.copilot.messages.push(Msg {
            user: false,
            text: "I can: remove silences, warm or cool the colour, brighten or darken, go black and white, and add fade-ins or fade-outs to the selected clip. Beat-sync and colour matching are not built yet.".into(),
        });
        return;
    }
    let title = format!("{}{}", notes[0][..1].to_uppercase(), &notes[0][1..]);
    let title = if notes.len() > 1 {
        format!("{title} and more")
    } else {
        title
    };
    let body = format!("On {}: {}.", clip.name, notes.join(", "));
    let op = if ops.len() == 1 {
        ops.remove(0)
    } else {
        Op::Transaction {
            description: format!("Copilot: {}", notes.join(", ")),
            ops,
        }
    };
    app.copilot.messages.push(Msg {
        user: false,
        text: format!(
            "Here is what I'd do to {}. Apply it or preview it first.",
            clip.name
        ),
    });
    app.copilot.suggestion = Some(Suggestion { title, body, op });
}

/// Find the silent stretches of a clip and plan cutting them out (keeping a little breathing room).
fn remove_silences(
    project: &Project,
    clip: &Clip,
    media: &MediaRef,
    fps: f64,
) -> Result<(String, Option<Suggestion>), String> {
    let (ops, removed) = captaine_agent::remove_silences(clip, media, fps, -42.0, 0.45, 0.12)?;
    if ops.is_empty() {
        return Ok((
            format!(
                "I didn't find any silences longer than half a second in {}.",
                clip.name
            ),
            None,
        ));
    }
    let op = Op::Transaction {
        description: "Copilot: remove silences".into(),
        ops,
    };
    // make sure it really applies before offering it
    apply(project, &op).map_err(|e| e.to_string())?;
    let total: f64 = removed.iter().map(|(a, b)| b - a).sum();
    let n = removed.len();
    Ok((
        format!("I found {n} silent stretch{} in {} ({total:.1} s in total). Cutting them leaves a short pause either side.", if n == 1 { "" } else { "es" }, clip.name),
        Some(Suggestion {
            title: "Remove silences".into(),
            body: format!("Cut {n} gap{} from {}, tightening it by {total:.1} s.", if n == 1 { "" } else { "s" }, clip.name),
            op,
        }),
    ))
}

/// Ask Claude. Runs on a background thread; progress and the result arrive through `copilot.working`.
fn ask_claude(app: &mut App, text: String, key: String) {
    app.copilot.messages.push(Msg {
        user: true,
        text: text.clone(),
    });
    // a pending suggestion is kept: Claude builds on top of it, and the result replaces it
    let base_ops: Vec<Op> = match app.copilot.suggestion.take() {
        Some(Suggestion {
            op: Op::Transaction { ops, .. },
            ..
        }) => ops,
        Some(s) => vec![s.op],
        None => Vec::new(),
    };
    app.set_preview(None);
    let (tx, rx) = channel();
    app.copilot.working = Some(rx);
    app.copilot.steps.clear();
    app.copilot.pending_user = text.clone();
    let project = app.project().clone();
    let (selection, playhead) = (app.selected_clip, app.playhead as f64 / app.fps());
    let history = app.copilot.history.clone();
    let ctx = app.ctx.clone();
    let factory = app.copilot.transport.clone();
    std::thread::spawn(move || {
        let transport: Box<dyn captaine_agent::Transport> = match factory {
            Some(f) => f(),
            None => Box::new(captaine_agent::AnthropicTransport::new(key)),
        };
        let cfg = captaine_agent::AgentConfig::default();
        let req = captaine_agent::AgentRequest {
            project: &project,
            selection,
            playhead_secs: playhead,
            history: &history,
            text: &text,
            base_ops: &base_ops,
        };
        let result = captaine_agent::run(
            transport.as_ref(),
            &cfg,
            &req,
            &mut |captaine_agent::Event::Step(s)| {
                let _ = tx.send(Work::Step(s));
                ctx.request_repaint();
            },
        );
        let done = result.map_err(|e| e.to_string()).map(|r| {
            let reply = if r.reply.is_empty() {
                "Done.".to_string()
            } else {
                r.reply.clone()
            };
            (reply, suggestion_from(&text, &base_ops, &r))
        });
        let _ = tx.send(Work::Done(Box::new(done)));
        ctx.request_repaint();
    });
}

/// One suggestion out of the pending ops plus what the agent just did (none if nothing changed at all).
fn suggestion_from(
    request: &str,
    base: &[Op],
    r: &captaine_agent::AgentResult,
) -> Option<Suggestion> {
    let mut ops: Vec<Op> = base.to_vec();
    ops.extend(r.new_ops.iter().cloned());
    if ops.is_empty() {
        return None;
    }
    let short: String = request.chars().take(48).collect();
    let title = if base.is_empty() {
        short
    } else {
        "Copilot edit".to_string()
    };
    let n = ops.len();
    Some(Suggestion {
        title,
        body: format!(
            "{} change{}: {}",
            n,
            if n == 1 { "" } else { "s" },
            if r.reply.is_empty() {
                "see the timeline preview"
            } else {
                &r.reply
            }
        ),
        op: Op::Transaction {
            description: format!("Copilot: {request}"),
            ops,
        },
    })
}
