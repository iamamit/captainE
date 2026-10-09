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

#[derive(Default)]
pub struct State {
    pub messages: Vec<Msg>,
    pub input: String,
    pub suggestion: Option<Suggestion>,
    working: Option<Receiver<Analysis>>,
    greeted: bool,
}

const GREETING: &str = "Hi! I run a few built-in skills for now (the AI agent isn't connected yet). Select a clip and try: remove silences, warm up the colour, add a 12-frame fade-in.";

pub fn show(app: &mut App, ctx: &Context) {
    if !app.show_copilot {
        return;
    }
    if !app.copilot.greeted {
        app.copilot.greeted = true;
        app.copilot.messages.push(Msg {
            user: false,
            text: GREETING.into(),
        });
    }
    // collect a finished background analysis
    if let Some(rx) = &app.copilot.working {
        if let Ok(done) = rx.try_recv() {
            app.copilot.working = None;
            match done {
                Ok((text, sug)) => {
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
    let width = 300.0;
    let pos = pos2(
        app.central_rect.right() - width - 16.0,
        app.central_rect.top() + 46.0,
    );
    let mut close = false;
    let mut send: Option<String> = None;
    let mut apply_it = false;
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
                                                "{} · {target}",
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
                                    if let Some(s) = &app.copilot.suggestion {
                                        suggestion_card(
                                            ui,
                                            s,
                                            app.preview_override.is_some(),
                                            &mut apply_it,
                                            &mut preview_it,
                                            &mut discard,
                                        );
                                    }
                                });
                            ui.add_space(6.0);
                            ui.horizontal_wrapped(|ui| {
                                for (label, text) in [
                                    ("Remove silences", "remove silences"),
                                    ("Warm colours", "warm up the colour"),
                                    ("Add fades", "add a 12-frame fade in and fade out"),
                                ] {
                                    if chip(ui, label, false, app.copilot.working.is_none())
                                        .clicked()
                                    {
                                        send = Some(text.to_string());
                                    }
                                }
                            });
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
    if discard {
        app.set_preview(None);
        app.copilot.suggestion = None;
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

fn suggestion_card(
    ui: &mut egui::Ui,
    s: &Suggestion,
    previewing: bool,
    apply_it: &mut bool,
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
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Apply").strong().color(Color32::WHITE))
                            .fill(theme::ACCENT),
                    )
                    .clicked()
                {
                    *apply_it = true;
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

pub fn ask(app: &mut App, text: String) {
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
            let _ = tx.send(remove_silences(&project, &clip2, &m, fps));
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
    const PAD: f64 = 0.12;
    let src = clip.source_start.to_seconds_f64();
    let dur = clip.duration.value as f64 / fps;
    let found = captaine_audio::find_silences(&media.path, src, dur, -42.0, 0.45)
        .map_err(|e| e.to_string())?;
    let mut cuts: Vec<(i64, i64)> = found
        .iter()
        .filter(|(a, b)| b - a - 2.0 * PAD >= 0.15)
        .map(|(a, b)| {
            (
                ((a + PAD) * fps).round() as i64,
                ((b - PAD) * fps).round() as i64,
            )
        })
        .filter(|(a, b)| *a > 0 && *b < clip.duration.value && b > a)
        .collect();
    if cuts.is_empty() {
        return Ok((
            format!(
                "I didn't find any silences longer than half a second in {}.",
                clip.name
            ),
            None,
        ));
    }
    // work from the end so earlier cut points stay valid; each split leaves the original clip as the left piece
    cuts.reverse();
    let mut ops = Vec::new();
    for (a, b) in &cuts {
        let mid = Id::new();
        ops.push(Op::Split {
            item: clip.id,
            at: RationalTime::new(*b, clip.duration.rate),
            new_id: Id::new(),
        });
        ops.push(Op::Split {
            item: clip.id,
            at: RationalTime::new(*a, clip.duration.rate),
            new_id: mid,
        });
        ops.push(Op::RemoveItem { item: mid });
    }
    let op = Op::Transaction {
        description: "Copilot: remove silences".into(),
        ops,
    };
    // make sure it really applies before offering it
    apply(project, &op).map_err(|e| e.to_string())?;
    let removed: f64 = cuts.iter().map(|(a, b)| (b - a) as f64 / fps).sum();
    Ok((
        format!("I found {} silent stretch{} in {} ({removed:.1} s in total). Cutting them leaves a short pause either side.", cuts.len(), if cuts.len() == 1 { "" } else { "es" }, clip.name),
        Some(Suggestion {
            title: "Remove silences".into(),
            body: format!("Cut {} gap{} from {}, tightening it by {removed:.1} s.", cuts.len(), if cuts.len() == 1 { "" } else { "s" }, clip.name),
            op,
        }),
    ))
}

use captaine_time::RationalTime;
