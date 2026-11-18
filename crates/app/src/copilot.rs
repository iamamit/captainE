//! The CoCaptain panel. This is the *local* CoCaptain: a handful of real skills that turn a request into ops
//! and apply them through the same command layer a person uses (author: `Agent("copilot")`).
//! The AI agent of docs/08 will plug in behind the same panel.

use std::sync::mpsc::{channel, Receiver};

use captaine_model::*;
use captaine_ops::{apply, Author, Op};
use egui::{pos2, vec2, Align2, Color32, Context, RichText, Stroke};

use crate::app::App;
use crate::theme::{self, mono, ui_font};
use crate::ui::chip;

/// Something the user attached to their next message: a picture (pasted or a file) or any other file by path.
pub struct Attachment {
    pub name: String,
    /// a picture, as PNG
    pub png: Option<Vec<u8>>,
    /// a video or other file, by path
    pub path: Option<std::path::PathBuf>,
    thumb: Option<egui::TextureHandle>,
}

/// Widest picture sent to Claude; bigger ones are scaled down (it sees no more detail past this).
const ATTACH_MAX_W: u32 = 1568;

#[derive(Default)]
pub struct Msg {
    pub user: bool,
    pub text: String,
    /// What CoCaptain did on the way (its tool calls), folded under the reply
    pub steps: Vec<String>,
    /// Pictures it looked at for this reply
    pub pictures: Vec<egui::TextureHandle>,
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
    /// a picture the agent looked at (PNG)
    Picture(Vec<u8>),
    /// the agent stopped because it wants to see the video and the user has not allowed that yet
    WantsFrames,
    /// things for the editor to do once the reply is in (new project, open, export)
    Actions(Vec<captaine_agent::AppAction>),
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
    /// Whether Claude Code is installed here (checked once)
    cli: Option<bool>,
    /// Whether Claude Code is signed in: unknown until checked, and checked again after a sign-in or when a
    /// request comes back signed out
    signed_in: Option<bool>,
    auth_check: Option<Receiver<Option<bool>>>,
    /// The sign-in started from the panel, while it waits for the browser
    login: Option<captaine_agent::ClaudeLogin>,
    /// The code the sign-in page may show, pasted here
    pub login_code: String,
    login_error: Option<String>,
    pub key_input: String,
    /// The branch CoCaptain last saved a suggestion on, kept so it can be opened for review
    pub last_branch: Option<String>,
    /// Supplies the model connection instead of the Anthropic API (tests use a scripted one).
    pub transport: Option<TransportFactory>,
    /// The panel was showing last frame: when it opens, the cursor goes straight into the request box
    was_open: bool,
    /// Pictures CoCaptain looked at during the run in flight
    live_pictures: Vec<egui::TextureHandle>,
    /// A request CoCaptain stopped because it wanted to see the video; asked again once the user allows it
    pub wants_frames: Option<String>,
    /// Pictures and files waiting to go with the next message
    pub attachments: Vec<Attachment>,
    /// Where the chat is on screen, so files dropped on it are attached instead of imported
    pub rect: Option<egui::Rect>,
}

impl State {
    /// A request is in flight (Claude or a local analysis).
    pub fn is_working(&self) -> bool {
        self.working.is_some()
    }
}

const GREETING_CLAUDE: &str = "Hi! What are we making? I can look at your timeline, change it and check my work. Every change comes back as a suggestion you can preview and apply.";

const SEE_HINT: &str = "When it needs to look, CoCaptain shows Claude small frames of your timeline and footage (and a \
reference you give it), so it can judge colour, framing and timing and check its own edits. Nothing is sent until you \
ask something.";

const GREETING: &str = "Hi! I run a few built-in skills for now (the AI agent isn't connected yet). Select a clip and try: remove silences, warm up the colour, add a 12-frame fade-in.";

/// CoCaptain docked on the right, full height: the conversation fills it, the message box sits at the bottom,
/// and everything else (seeing the video, a new conversation, the Claude connection) is in the ••• menu.
pub fn dock(app: &mut App, root: &mut egui::Ui) {
    if !app.show_copilot {
        app.copilot.was_open = false;
        return;
    }
    let ctx = root.ctx().clone();
    let ctx = &ctx;
    crate::paste_key::install();
    let just_opened = !std::mem::replace(&mut app.copilot.was_open, true);
    if !app.copilot.greeted {
        app.copilot.greeted = true;
        let connected =
            api_key(app).is_some() || app.copilot.transport.is_some() || claude_cli_ready(app);
        app.copilot.messages.push(Msg {
            user: false,
            text: if connected { GREETING_CLAUDE } else { GREETING }.into(),
            ..Default::default()
        });
    }
    collect_work(app, ctx);
    let has_key =
        api_key(app).is_some() || app.copilot.transport.is_some() || claude_cli_ready(app);
    // through Claude Code (no API key): know whether it is signed in, and follow a sign-in started here
    let via_cli =
        api_key(app).is_none() && app.copilot.transport.is_none() && claude_cli_ready(app);
    if via_cli {
        poll_sign_in(app, ctx);
    }
    let signed_out = via_cli && app.copilot.signed_in == Some(false);
    let mut sign_in = false;
    let mut cancel_sign_in = false;
    let mut send_code: Option<String> = None;
    let mut connect: Option<String> = None;
    let mut disconnect = false;
    let mut close = false;
    let mut send: Option<String> = None;
    let mut apply_it = false;
    let mut branch_it = false;
    let mut review: Option<String> = None;
    let mut preview_it = false;
    let mut discard = false;
    let mut allow_and_go = false;
    let mut attach_picked = false;
    let mut paste_picture = false;
    let mut not_now = false;
    let mut new_chat = false;
    let avail = root.available_rect_before_wrap();
    let width = app.prefs.cocaptain_w;
    egui::Panel::right("cocaptain")
        .exact_size(width)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(root, |ui| {
            app.copilot.rect = Some(ui.max_rect());
            // ---- header
            egui::Panel::top("cocaptain_head")
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 10)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 11.0, theme::ACCENT);
                        ui.painter().text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            "cE",
                            ui_font(9.0),
                            Color32::WHITE,
                        );
                        ui.label(RichText::new("CoCaptain").strong().size(13.5));
                        let (status, colour) = if signed_out {
                            ("signed out", theme::PLAYHEAD)
                        } else if app.copilot.working.is_some() {
                            ("working", theme::ACCENT_HOVER)
                        } else if has_key {
                            ("ready", theme::OK)
                        } else {
                            ("local skills", theme::MUTED)
                        };
                        let (d, _) = ui.allocate_exact_size(vec2(8.0, 12.0), egui::Sense::hover());
                        ui.painter().circle_filled(d.center(), 3.0, colour);
                        ui.label(RichText::new(status).size(10.5).color(theme::MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("×").size(15.0).color(theme::MUTED),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text("Close CoCaptain")
                                .clicked()
                            {
                                close = true;
                            }
                            ui.menu_button(
                                RichText::new("•••").size(10.0).color(theme::MUTED),
                                |ui| {
                                    let mut see = app.prefs.cocaptain_frames;
                                    if ui
                                        .checkbox(&mut see, "Let CoCaptain see the video")
                                        .on_hover_text(SEE_HINT)
                                        .changed()
                                    {
                                        app.prefs.cocaptain_frames = see;
                                        let _ = app.prefs.save();
                                    }
                                    if ui.button("New conversation").clicked() {
                                        new_chat = true;
                                        ui.close();
                                    }
                                    if has_key
                                        && !via_cli
                                        && ui.button("Disconnect Claude").clicked()
                                    {
                                        disconnect = true;
                                        ui.close();
                                    }
                                },
                            );
                        });
                    });
                });
            // ---- message box (bottom)
            egui::Panel::bottom("cocaptain_input")
                .frame(egui::Frame::new().inner_margin(egui::Margin {
                    left: 12,
                    right: 12,
                    top: 8,
                    bottom: 12,
                }))
                .show(ui, |ui| {
                    if signed_out {
                        sign_in_box(
                            ui,
                            &mut app.copilot,
                            &mut sign_in,
                            &mut cancel_sign_in,
                            &mut send_code,
                        );
                        ui.add_space(8.0);
                    } else if !has_key {
                        connect_box(ui, &mut app.copilot.key_input, &mut connect);
                        ui.add_space(8.0);
                    }
                    egui::Frame::new()
                        .fill(theme::FIELD)
                        .stroke(Stroke::new(1.0, theme::BORDER))
                        .corner_radius(10.0)
                        .inner_margin(egui::Margin {
                            left: 10,
                            right: 6,
                            top: 6,
                            bottom: 6,
                        })
                        .show(ui, |ui| {
                            attachment_row(ui, &mut app.copilot.attachments);
                            ui.horizontal(|ui| {
                                if ui
                                    .add(egui::Button::new(RichText::new("+").size(16.0).color(theme::MUTED)).frame(false))
                                    .on_hover_text("Attach pictures or files (or paste a picture with ⌘V, or drop files here)")
                                    .clicked()
                                {
                                    attach_picked = true;
                                }
                                let w = ui.available_width() - 36.0;
                                // Enter sends; Shift+Enter starts a new line (the box grows to a few lines,
                                // then scrolls). Taken before the text box sees it, so it never adds a line.
                                let id = egui::Id::new("cocaptain_input");
                                let focused = ui.memory(|m| m.has_focus(id));
                                let enter = focused
                                    && ui.input_mut(|i| {
                                        !i.modifiers.shift
                                            && i.consume_key(
                                                egui::Modifiers::NONE,
                                                egui::Key::Enter,
                                            )
                                    });
                                let r = egui::ScrollArea::vertical()
                                    .id_salt("cocaptain_input_scroll")
                                    .max_height(160.0)
                                    .stick_to_bottom(true)
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::multiline(&mut app.copilot.input)
                                                .id(id)
                                                .frame(egui::Frame::NONE)
                                                .desired_rows(2)
                                                .desired_width(w)
                                                .font(egui::FontId::proportional(13.0))
                                                .hint_text("Ask CoCaptain anything…"),
                                        )
                                    })
                                    .inner;
                                // ⌘V with a picture on the clipboard attaches it
                                let key_v = crate::paste_key::take();
                                if focused
                                    && (key_v || ui.input(|i| {
                                        i.events.iter().any(|e| match e {
                                            egui::Event::Paste(_) => true,
                                            egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. } => modifiers.command,
                                            _ => false,
                                        })
                                    }))
                                {
                                    paste_picture = true;
                                }
                                let ready = !app.copilot.input.trim().is_empty() || !app.copilot.attachments.is_empty();
                                let go = ui
                                    .add_enabled(
                                        ready,
                                        egui::Button::new(
                                            RichText::new("↑")
                                                .strong()
                                                .size(14.0)
                                                .color(Color32::WHITE),
                                        )
                                        .fill(if ready { theme::ACCENT } else { theme::BORDER })
                                        .corner_radius(14.0)
                                        .min_size(vec2(28.0, 28.0)),
                                    )
                                    .on_hover_text("Send (Enter)")
                                    .clicked();
                                if (enter || go) && ready {
                                    send = Some(std::mem::take(&mut app.copilot.input));
                                    r.request_focus();
                                }
                                if just_opened {
                                    r.request_focus();
                                }
                            });
                        });
                });
            // ---- the conversation fills the rest
            egui::ScrollArea::vertical()
                .id_salt("cocaptain_chat")
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 14,
                            right: 14,
                            top: 10,
                            bottom: 10,
                        })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let only_greeting = app.copilot.messages.len() <= 1;
                            for m in &app.copilot.messages {
                                message(ui, m);
                            }
                            if only_greeting && app.copilot.working.is_none() {
                                starters(ui, has_key, &mut send);
                            }
                            if app.copilot.working.is_some() {
                                activity(ui, &app.copilot.steps, &app.copilot.live_pictures);
                            }
                            if app.copilot.wants_frames.is_some() && app.copilot.working.is_none() {
                                frames_card(ui, &mut allow_and_go, &mut not_now);
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
                            if let Some(b) = app.copilot.last_branch.clone() {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!("Review {b}"))
                                                .strong()
                                                .color(Color32::WHITE),
                                        )
                                        .fill(theme::ACCENT),
                                    )
                                    .clicked()
                                {
                                    review = Some(b);
                                }
                            }
                        });
                });
        });
    crate::ui::drag_edge(
        app,
        root,
        egui::Rect::from_min_max(pos2(avail.right() - width, avail.top()), avail.max),
        crate::keymap::PanelKind::CoCaptain,
    );
    if paste_picture {
        attach_clipboard_picture(app);
    }
    if attach_picked {
        if let Some(files) = rfd::FileDialog::new()
            .set_title("Attach to CoCaptain")
            .pick_files()
        {
            for f in files {
                attach_file(app, f);
            }
        }
    }
    if new_chat {
        app.copilot.messages.clear();
        app.copilot.history.clear();
        app.copilot.suggestion = None;
        app.copilot.wants_frames = None;
        app.copilot.greeted = false;
        app.set_preview(None);
    }
    if allow_and_go {
        app.prefs.cocaptain_frames = true;
        let _ = app.prefs.save();
        if let Some(text) = app.copilot.wants_frames.take() {
            // ask the same thing again, now able to look; the question is already in the chat
            if app
                .copilot
                .messages
                .last()
                .is_some_and(|m| m.user && m.text == text)
            {
                app.copilot.messages.pop();
            }
            send = Some(text);
        }
    }
    if not_now {
        app.copilot.wants_frames = None;
        app.copilot.messages.push(Msg {
            user: false,
            text: "OK, I won't look. Ask me something about the timeline instead, or turn on \"Let CoCaptain see the video\" in the ••• menu when you want me to.".into(),
            ..Default::default()
        });
    }
    if close {
        app.show_copilot = false;
    }
    if sign_in {
        match captaine_agent::ClaudeCli::find()
            .ok_or_else(|| "Claude Code is not installed on this Mac".to_string())
            .and_then(|c| c.start_login())
        {
            Ok(l) => {
                app.copilot.login = Some(l);
                app.copilot.login_error = None;
            }
            Err(e) => app.copilot.login_error = Some(e),
        }
    }
    if cancel_sign_in {
        app.copilot.login = None;
    }
    if let Some(code) = send_code {
        if let Some(l) = &mut app.copilot.login {
            if let Err(e) = l.send_code(&code) {
                app.copilot.login_error = Some(e);
            }
        }
    }
    if let Some(k) = connect {
        match captaine_agent::Credentials::save(&k) {
            Ok(()) => {
                app.copilot.key = None;
                app.toast("Connected: CoCaptain will now use Claude", false);
            }
            Err(e) => app.toast(format!("could not save the key: {e}"), true),
        }
    }
    if disconnect {
        captaine_agent::Credentials::forget();
        app.copilot.key = None;
        app.toast(
            "Claude disconnected; CoCaptain is using its local skills",
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
                    ..Default::default()
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
                    app.copilot.messages.push(Msg { user: false, text: format!("Saved on branch {name}. Your timeline is unchanged: review it, then merge what you like."), ..Default::default() });
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

/// Pictures and files waiting to go with the next message, each with a × to take it off.
fn attachment_row(ui: &mut egui::Ui, atts: &mut Vec<Attachment>) {
    if atts.is_empty() {
        return;
    }
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        for (i, a) in atts.iter().enumerate() {
            egui::Frame::new()
                .fill(theme::PANEL_DARK)
                .corner_radius(6.0)
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        match &a.thumb {
                            Some(t) => {
                                let [w, h] = t.size();
                                ui.add(
                                    egui::Image::new(t)
                                        .fit_to_exact_size(vec2(
                                            44.0 * w as f32 / h.max(1) as f32,
                                            44.0,
                                        ))
                                        .corner_radius(4.0),
                                );
                            }
                            None => {
                                ui.label(RichText::new(&a.name).size(11.0).color(theme::TEXT));
                            }
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("×").size(12.0).color(theme::MUTED),
                                )
                                .frame(false),
                            )
                            .on_hover_text("Remove")
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                });
        }
    });
    if let Some(i) = remove {
        atts.remove(i);
    }
    ui.add_space(4.0);
}

fn png_thumb(ctx: &Context, png: &[u8], n: usize) -> Option<egui::TextureHandle> {
    picture_texture(ctx, png, 10_000 + n)
}

/// A picture file becomes a picture (scaled to at most 1568 wide, as PNG); anything else is attached by path.
pub fn attach_file(app: &mut App, path: std::path::PathBuf) {
    let name = path
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let picture = [
        "png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "heic",
    ]
    .contains(&ext.as_str());
    let png = picture
        .then(|| {
            std::process::Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&path)
                .args(["-frames:v", "1", "-vf"])
                .arg(format!("scale='min({ATTACH_MAX_W},iw)':-2"))
                .args(["-f", "image2pipe", "-vcodec", "png", "-"])
                .output()
                .ok()
                .filter(|o| o.status.success() && !o.stdout.is_empty())
                .map(|o| o.stdout)
        })
        .flatten();
    let n = app.copilot.attachments.len();
    let thumb = png.as_deref().and_then(|p| png_thumb(&app.ctx, p, n));
    app.copilot.attachments.push(Attachment {
        name,
        path: png.is_none().then_some(path),
        png,
        thumb,
    });
}

/// The picture on the clipboard (a screenshot, a copied image), if there is one.
fn attach_clipboard_picture(app: &mut App) {
    let Ok(img) = arboard::Clipboard::new().and_then(|mut c| c.get_image()) else {
        return;
    };
    let frame = captaine_render::Frame8 {
        width: img.width as u32,
        height: img.height as u32,
        rgba: img.bytes.into_owned(),
    };
    let frame = if frame.width > ATTACH_MAX_W {
        frame.thumbnail(ATTACH_MAX_W)
    } else {
        frame
    };
    let rgb: Vec<u8> = frame
        .rgba
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let Ok(png) = captaine_render::encode_rgb_png(frame.width, frame.height, &rgb) else {
        return;
    };
    let n = app.copilot.attachments.len();
    let thumb = png_thumb(&app.ctx, &png, n);
    app.copilot.attachments.push(Attachment {
        name: format!("Pasted picture {}", n + 1),
        png: Some(png),
        path: None,
        thumb,
    });
}

/// Progress and results from the run in flight: steps and pictures as they come, then the reply.
fn collect_work(app: &mut App, ctx: &Context) {
    let Some(rx) = &app.copilot.working else {
        return;
    };
    let events: Vec<Work> = rx.try_iter().collect();
    let mut finished = None;
    let mut actions = Vec::new();
    for e in events {
        match e {
            Work::Actions(a) => actions.extend(a),
            Work::Step(s) => app.copilot.steps.push(s),
            Work::Picture(png) => {
                if let Some(t) = picture_texture(ctx, &png, app.copilot.live_pictures.len()) {
                    app.copilot.live_pictures.push(t);
                }
            }
            Work::WantsFrames => app.copilot.wants_frames = Some(app.copilot.pending_user.clone()),
            Work::Done(d) => finished = Some(*d),
        }
    }
    let Some(done) = finished else {
        ctx.request_repaint_after(std::time::Duration::from_millis(80));
        return;
    };
    app.copilot.working = None;
    let steps = std::mem::take(&mut app.copilot.steps);
    let pictures = std::mem::take(&mut app.copilot.live_pictures);
    let asked = std::mem::take(&mut app.copilot.pending_user);
    match done {
        // it stopped to ask to see the video: the card asks, so no reply of its own
        Ok(_) if app.copilot.wants_frames.is_some() => {}
        Ok((text, sug)) => {
            if !asked.is_empty() {
                app.copilot.history.push(captaine_agent::ChatTurn {
                    user: asked,
                    assistant: text.clone(),
                });
            }
            app.copilot.messages.push(Msg {
                user: false,
                text,
                steps,
                pictures,
            });
            app.copilot.suggestion = sug;
        }
        Err(e) if e.starts_with(captaine_agent::CLAUDE_SIGNED_OUT) => {
            app.copilot.signed_in = Some(false);
            app.copilot.messages.push(Msg {
                user: false,
                text: "Claude Code is signed out, so I can't answer yet. Sign in below, then ask again."
                    .into(),
                ..Default::default()
            });
        }
        Err(e) => app.copilot.messages.push(Msg {
            user: false,
            text: format!("That didn't work: {e}"),
            steps,
            ..Default::default()
        }),
    }
    run_actions(app, ctx, actions);
}

/// Do what CoCaptain asked of the editor itself, after its reply.
fn run_actions(app: &mut App, ctx: &Context, actions: Vec<captaine_agent::AppAction>) {
    use captaine_agent::AppAction;
    for a in actions {
        let note = match a {
            AppAction::NewProject {
                name,
                width,
                height,
                fps,
            } => {
                let dir = std::env::var_os("HOME")
                    .map(|h| std::path::PathBuf::from(h).join("Movies/captainE"))
                    .unwrap_or_else(std::env::temp_dir);
                let _ = std::fs::create_dir_all(&dir);
                // a free name: "Untitled", "Untitled 2", ...
                let free = (1..1000)
                    .map(|i| {
                        if i == 1 {
                            name.clone()
                        } else {
                            format!("{name} {i}")
                        }
                    })
                    .find(|n| !dir.join(format!("{n}.captaine.json")).exists())
                    .unwrap_or(name);
                crate::new_project(app, &dir, &free, width, height, &fps);
                format!(
                    "Opened the new project {free} ({width}x{height}, {fps} fps), saved in {}.",
                    dir.display()
                )
            }
            AppAction::OpenProject { path } => {
                crate::open_project(app, std::path::PathBuf::from(&path));
                format!("Opened {path}.")
            }
            AppAction::Export { path } => {
                app.export.path = Some(std::path::PathBuf::from(&path));
                app.export.codec = if path.ends_with(".mov") {
                    captaine_export::Codec::ProRes
                } else {
                    captaine_export::Codec::H264
                };
                crate::ui::queue_export(app, ctx, None);
                format!("Exporting to {path}; progress is on the Deliver page.")
            }
        };
        app.copilot.messages.push(Msg {
            user: false,
            text: note,
            ..Default::default()
        });
    }
}

/// A PNG CoCaptain looked at, as a texture for the chat.
fn picture_texture(ctx: &Context, png: &[u8], n: usize) -> Option<egui::TextureHandle> {
    let dec = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let img = match info.color_type {
        png::ColorType::Rgb => egui::ColorImage::from_rgb([w, h], &buf[..w * h * 3]),
        png::ColorType::Rgba => egui::ColorImage::from_rgba_unmultiplied([w, h], &buf[..w * h * 4]),
        _ => return None,
    };
    Some(ctx.load_texture(
        format!("cocaptain_seen_{n}_{}", png.len()),
        img,
        egui::TextureOptions::LINEAR,
    ))
}

/// The tool calls in words a person reads at a glance: "look" -> "Looked at the timeline".
fn plain_step(s: &str) -> String {
    let name = s.split_whitespace().next().unwrap_or(s);
    match name {
        "get_timeline" => "Read the timeline".into(),
        "list_media" => "Listed the media".into(),
        "look" => "Looked at the timeline".into(),
        "look_media" => "Looked at footage".into(),
        other => other.replace('_', " "),
    }
}

/// One message: the user's in a bubble on the right, CoCaptain's as plain text, with what it looked at and what
/// it did folded underneath.
fn message(ui: &mut egui::Ui, m: &Msg) {
    if m.user {
        ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
            if !m.pictures.is_empty() {
                ui.horizontal(|ui| {
                    for t in &m.pictures {
                        let [w, h] = t.size();
                        ui.add(
                            egui::Image::new(t)
                                .fit_to_exact_size(vec2(72.0 * w as f32 / h.max(1) as f32, 72.0))
                                .corner_radius(6.0),
                        );
                    }
                });
                ui.add_space(4.0);
            }
            egui::Frame::new()
                .fill(theme::FIELD)
                .corner_radius(12.0)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width() * 0.85);
                    ui.label(RichText::new(&m.text).size(13.0).color(theme::TEXT));
                });
        });
        ui.add_space(10.0);
        return;
    }
    if !m.steps.is_empty() {
        let id = ui.make_persistent_id(("cocaptain_steps", m.text.len(), m.steps.len()));
        egui::CollapsingHeader::new(
            RichText::new(format!(
                "{} step{}",
                m.steps.len(),
                if m.steps.len() == 1 { "" } else { "s" }
            ))
            .size(11.0)
            .color(theme::MUTED),
        )
        .id_salt(id)
        .default_open(false)
        .show(ui, |ui| {
            for s in &m.steps {
                ui.label(
                    RichText::new(format!("· {}", plain_step(s)))
                        .size(11.0)
                        .color(theme::MUTED),
                )
                .on_hover_text(s);
            }
        });
    }
    pictures(ui, &m.pictures);
    for (i, para) in m.text.split("\n\n").enumerate() {
        if i > 0 {
            ui.add_space(6.0);
        }
        for line in para.lines() {
            let (bullet, body) = match line.trim_start().strip_prefix("- ") {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if bullet {
                    ui.label(RichText::new("•  ").size(13.0).color(theme::MUTED));
                }
                // **bold** spans
                for (k, part) in body.split("**").enumerate() {
                    if part.is_empty() {
                        continue;
                    }
                    let t = RichText::new(part).size(13.0).color(theme::TEXT);
                    ui.label(if k % 2 == 1 { t.strong() } else { t });
                }
            });
        }
    }
    ui.add_space(12.0);
}

/// Thumbnails of what CoCaptain looked at; hover one to see it bigger.
fn pictures(ui: &mut egui::Ui, pics: &[egui::TextureHandle]) {
    if pics.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        let w = ui.available_width();
        for t in pics {
            let [pw, ph] = t.size();
            let tw = if pics.len() == 1 { w } else { (w - 6.0) / 2.0 };
            let size = vec2(tw, tw * ph as f32 / pw.max(1) as f32);
            ui.add(
                egui::Image::new(t)
                    .fit_to_exact_size(size)
                    .corner_radius(6.0),
            )
            .on_hover_ui(|ui| {
                let big = 720.0f32.min(pw as f32);
                ui.add(
                    egui::Image::new(t)
                        .fit_to_exact_size(vec2(big, big * ph as f32 / pw.max(1) as f32)),
                );
            });
        }
    });
    ui.add_space(6.0);
}

/// While it works: what it is doing now, and what it has looked at so far.
fn activity(ui: &mut egui::Ui, steps: &[String], pics: &[egui::TextureHandle]) {
    pictures(ui, pics);
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(12.0).color(theme::ACCENT_HOVER));
        let now = steps
            .last()
            .map_or("Thinking…".to_string(), |s| format!("{}…", plain_step(s)));
        ui.label(RichText::new(now).size(12.0).color(theme::MUTED));
    });
    if steps.len() > 1 {
        ui.label(
            RichText::new(format!("{} steps so far", steps.len()))
                .size(10.5)
                .color(theme::FAINT),
        );
    }
    ui.add_space(10.0);
}

/// First thing in an empty conversation: a few things to try.
fn starters(ui: &mut egui::Ui, has_key: bool, send: &mut Option<String>) {
    let items: [(&str, &str); 3] = if has_key {
        [
            (
                "Does the colour look right?",
                "Look at every shot: does the colour grading look right? Fix what is off.",
            ),
            (
                "Tighten the edit",
                "Watch the timeline and tighten the edit: trim slow starts and ends.",
            ),
            (
                "Add a title",
                "Add a title that fits the video at the start.",
            ),
        ]
    } else {
        [
            ("Remove silences", "remove silences"),
            ("Warm colours", "warm up the colour"),
            ("Add fades", "add a 12-frame fade in and fade out"),
        ]
    };
    ui.add_space(4.0);
    for (label, text) in items {
        if chip(ui, label, false, true).clicked() {
            *send = Some(text.to_string());
        }
        ui.add_space(4.0);
    }
}

/// CoCaptain stopped because it wants to see the video: allow it once and it carries on.
fn frames_card(ui: &mut egui::Ui, allow: &mut bool, not_now: &mut bool) {
    egui::Frame::new()
        .fill(theme::PANEL_DARK)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("CoCaptain wants to see your video")
                    .strong()
                    .size(12.5),
            );
            ui.label(RichText::new(SEE_HINT).size(11.0).color(theme::MUTED));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Allow and continue")
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(theme::ACCENT),
                    )
                    .clicked()
                {
                    *allow = true;
                }
                if ui
                    .add(
                        egui::Button::new(RichText::new("Not now").color(theme::MUTED))
                            .frame(false),
                    )
                    .clicked()
                {
                    *not_now = true;
                }
            });
        });
    ui.add_space(10.0);
}

/// No Claude yet: paste an API key.
fn connect_box(ui: &mut egui::Ui, key_input: &mut String, connect: &mut Option<String>) {
    egui::Frame::new()
        .fill(theme::PANEL_DARK)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.label(RichText::new("Connect Claude").strong().size(11.5));
            ui.label(RichText::new("for free-form requests. Sign in to Claude Code on this Mac, or paste an Anthropic API key (kept in ~/.captaine, readable only by you). Only project structure and analysis results are sent, plus small frames of your video if you let CoCaptain see it.").size(10.0).color(theme::MUTED));
            ui.horizontal(|ui| {
                let w = ui.available_width() - 52.0;
                ui.add_sized([w, 24.0], egui::TextEdit::singleline(key_input).password(true).hint_text("sk-ant-…").margin(egui::Margin::symmetric(6, 4)));
                if ui.add_enabled(!key_input.trim().is_empty(), egui::Button::new("Save").min_size(vec2(44.0, 24.0))).clicked() {
                    *connect = Some(std::mem::take(key_input));
                }
            });
        });
}

/// Edits with more than a couple of steps are offered on a branch by default.
fn suggestion_is_big(s: &Suggestion) -> bool {
    match &s.op {
        Op::Transaction { ops, .. } => ops.len() >= 3,
        _ => false,
    }
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

/// Signed out of Claude Code: a button that starts its sign-in, then a place for the code the page may show.
fn sign_in_box(
    ui: &mut egui::Ui,
    st: &mut State,
    sign_in: &mut bool,
    cancel: &mut bool,
    send_code: &mut Option<String>,
) {
    egui::Frame::new()
        .fill(theme::PANEL_DARK)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            if st.login.is_none() {
                ui.label(RichText::new("Sign in to Claude").strong().size(11.5));
                ui.label(
                    RichText::new(
                        "CoCaptain uses your Claude account through Claude Code, and that sign-in has run out. \
                         Your browser opens to sign in again.",
                    )
                    .size(10.0)
                    .color(theme::MUTED),
                );
                ui.add_space(4.0);
                let button = egui::Button::new(
                    RichText::new("Sign in to Claude")
                        .strong()
                        .color(Color32::WHITE),
                )
                .fill(theme::ACCENT)
                .min_size(vec2(0.0, 26.0));
                if ui.add(button).clicked() {
                    *sign_in = true;
                }
            } else {
                ui.label(
                    RichText::new("Finish signing in in your browser")
                        .strong()
                        .size(11.5),
                );
                ui.label(
                    RichText::new("Approve there. If the page shows a code, paste it here.")
                        .size(10.0)
                        .color(theme::MUTED),
                );
                ui.horizontal(|ui| {
                    let w = ui.available_width() - 52.0;
                    ui.add_sized(
                        [w, 24.0],
                        egui::TextEdit::singleline(&mut st.login_code)
                            .hint_text("code from the page")
                            .margin(egui::Margin::symmetric(6, 4)),
                    );
                    let ready = !st.login_code.trim().is_empty();
                    if ui
                        .add_enabled(ready, egui::Button::new("Send").min_size(vec2(44.0, 24.0)))
                        .clicked()
                    {
                        *send_code = Some(std::mem::take(&mut st.login_code));
                    }
                });
                let cancel_button =
                    egui::Button::new(RichText::new("Cancel").size(10.0).color(theme::FAINT))
                        .frame(false);
                if ui.add(cancel_button).clicked() {
                    *cancel = true;
                }
            }
            if let Some(e) = &st.login_error {
                ui.label(RichText::new(e).size(10.0).color(theme::PLAYHEAD));
            }
        });
}

/// Know whether Claude Code is signed in (asked once in the background, and again after a sign-in) and follow
/// a sign-in started from the panel until the browser finishes it.
fn poll_sign_in(app: &mut App, ctx: &Context) {
    let st = &mut app.copilot;
    if let Some(rx) = &st.auth_check {
        if let Ok(v) = rx.try_recv() {
            // when it cannot tell, requests go ahead: a signed-out answer still brings the sign-in up
            st.signed_in = Some(v.unwrap_or(true));
            st.auth_check = None;
        }
    } else if st.signed_in.is_none() && st.login.is_none() {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let v = captaine_agent::ClaudeCli::find().and_then(|c| c.signed_in());
            let _ = tx.send(v);
            ctx.request_repaint();
        });
        st.auth_check = Some(rx);
    }
    let done = st.login.as_mut().map(|l| (l.finished(), l.output()));
    match done {
        Some((Some(true), _)) => {
            st.login = None;
            st.login_error = None;
            st.signed_in = None;
            st.messages.push(Msg {
                user: false,
                text: "Signed in to Claude. Ask me again.".into(),
                ..Default::default()
            });
        }
        Some((Some(false), out)) => {
            st.login = None;
            let last = out
                .lines()
                .map(str::trim)
                .rfind(|l| !l.is_empty() && !l.starts_with("If the browser"))
                .unwrap_or("")
                .to_string();
            st.login_error = Some(if last.is_empty() {
                "The sign-in did not finish. Try again.".into()
            } else {
                format!("The sign-in did not finish: {last}")
            });
        }
        Some((None, _)) => ctx.request_repaint_after(std::time::Duration::from_millis(400)),
        None => {}
    }
}

/// The key for Claude, if there is one (looked up once and again after it changes).
/// Claude Code on this Mac, signed in already: used when there is no API key.
fn claude_cli_ready(app: &mut App) -> bool {
    if app.copilot.cli.is_none() {
        app.copilot.cli = Some(captaine_agent::ClaudeCli::find().is_some());
    }
    app.copilot.cli == Some(true)
}

fn api_key(app: &mut App) -> Option<String> {
    if app.copilot.key.is_none() {
        app.copilot.key = Some(captaine_agent::Credentials::load());
    }
    app.copilot.key.clone().flatten()
}

/// Handle a request: Claude when a key is set, the built-in skills otherwise.
pub fn ask(app: &mut App, text: String) {
    if app.copilot.transport.is_none() && api_key(app).is_none() && !claude_cli_ready(app) {
        // the built-in skills cannot use pictures or files
        app.copilot.attachments.clear();
    }
    if app.copilot.transport.is_some() {
        return ask_claude(app, text, String::new());
    }
    match api_key(app) {
        Some(key) => ask_claude(app, text, key),
        None if claude_cli_ready(app) => ask_claude(app, text, String::new()),
        None => ask_local(app, text),
    }
}

/// The built-in rule-based skills (no network).
pub fn ask_local(app: &mut App, text: String) {
    let t = text.to_lowercase();
    app.copilot.messages.push(Msg {
        user: true,
        text: text.clone(),
        ..Default::default()
    });
    app.set_preview(None);
    app.copilot.suggestion = None;
    let Some((clip, _, start)) = app.selected() else {
        app.copilot.messages.push(Msg {
            user: false,
            text: "Select a clip in the timeline first, then ask again.".into(),
            ..Default::default()
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
                ..Default::default()
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
            text: "I can: remove silences, warm or cool the colour, brighten or darken, go black and white, and add fade-ins or fade-outs to the selected clip. Beat-sync and colour matching are not built yet.".into(), ..Default::default() });
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
            description: format!("CoCaptain: {}", notes.join(", ")),
            ops,
        }
    };
    app.copilot.messages.push(Msg {
        user: false,
        text: format!(
            "Here is what I'd do to {}. Apply it or preview it first.",
            clip.name
        ),
        ..Default::default()
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
        description: "CoCaptain: remove silences".into(),
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
    let attached = std::mem::take(&mut app.copilot.attachments);
    let images: Vec<Vec<u8>> = attached.iter().filter_map(|a| a.png.clone()).collect();
    let files: Vec<String> = attached
        .iter()
        .filter_map(|a| a.path.as_ref().map(|p| p.to_string_lossy().into_owned()))
        .collect();
    let shown = if files.is_empty() {
        text.clone()
    } else {
        let names: Vec<&str> = attached
            .iter()
            .filter(|a| a.path.is_some())
            .map(|a| a.name.as_str())
            .collect();
        format!("{text}\n\nAttached: {}", names.join(", "))
            .trim()
            .to_string()
    };
    app.copilot.messages.push(Msg {
        user: true,
        text: shown,
        pictures: attached.iter().filter_map(|a| a.thumb.clone()).collect(),
        ..Default::default()
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
    // the assistant sees the sequence being edited; the edits it suggests apply to the real project by id
    let mut project = app.project().clone();
    if let Some(active) = app.timeline_id() {
        project.timelines.retain(|id, _| *id == active);
    }
    let (selection, playhead) = (app.selected_clip, app.playhead as f64 / app.fps());
    let history = app.copilot.history.clone();
    let ctx = app.ctx.clone();
    let factory = app.copilot.transport.clone();
    let allow_frames = app.prefs.cocaptain_frames;
    std::thread::spawn(move || {
        let transport: Box<dyn captaine_agent::Transport> = match factory {
            Some(f) => f(),
            None if key.is_empty() => match captaine_agent::ClaudeCli::find() {
                Some(c) => Box::new(c),
                None => Box::new(captaine_agent::AnthropicTransport::new(key)),
            },
            None => Box::new(captaine_agent::AnthropicTransport::new(key)),
        };
        let cfg = captaine_agent::AgentConfig {
            allow_frames,
            ..captaine_agent::AgentConfig::default()
        };
        let req = captaine_agent::AgentRequest {
            project: &project,
            selection,
            playhead_secs: playhead,
            history: &history,
            text: &text,
            base_ops: &base_ops,
            images: &images,
            files: &files,
        };
        let result = captaine_agent::run(transport.as_ref(), &cfg, &req, &mut |e| {
            let _ = tx.send(match e {
                captaine_agent::Event::Step(s) => Work::Step(s),
                captaine_agent::Event::Picture(png) => Work::Picture(png),
            });
            ctx.request_repaint();
        });
        if result.as_ref().is_ok_and(|r| r.wanted_frames) {
            let _ = tx.send(Work::WantsFrames);
        }
        if let Ok(r) = &result {
            if !r.app_actions.is_empty() {
                let _ = tx.send(Work::Actions(r.app_actions.clone()));
            }
        }
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
        "CoCaptain edit".to_string()
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
            description: format!("CoCaptain: {request}"),
            ops,
        },
    })
}
