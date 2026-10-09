//! The Transcript tab: speech to text for the selected clip, edit by clicking words, captions and SRT.

use std::sync::mpsc::{channel, Receiver};

use captaine_model::Clip;
use captaine_ops::Op;
use captaine_speech::Segment;
use egui::{RichText, Ui};

use crate::app::App;
use crate::theme;
use crate::ui::chip;

/// What identifies a transcript: the stretch of media a clip plays, at its speed.
pub type TranscriptKey = (captaine_model::Id, i64, i64, u64);

pub fn key_of(c: &Clip) -> TranscriptKey {
    (
        c.media,
        c.source_start.value,
        c.duration.value,
        c.retime.speed.to_bits(),
    )
}

pub struct Transcribing {
    pub key: TranscriptKey,
    pub rx: Receiver<Result<Vec<Segment>, String>>,
}

impl App {
    /// Start transcribing the selected clip in the background.
    pub fn transcribe_selected(&mut self) {
        let Some((clip, ..)) = self.selected() else {
            self.toast("Select a clip with speech first", false);
            return;
        };
        let Some(media) = self.project().media.get(&clip.media).cloned() else {
            return;
        };
        if let Some(why) = captaine_speech::unavailable() {
            return self.toast(why, true);
        }
        let key = key_of(&clip);
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let r = captaine_agent::clip_transcript(&clip, &media, None);
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.transcribing = Some(Transcribing { key, rx });
        self.word_sel = None;
    }

    /// Collect a finished transcription.
    pub fn poll_transcript(&mut self) {
        let Some(t) = &self.transcribing else { return };
        match t.rx.try_recv() {
            Ok(Ok(segs)) => {
                let key = t.key;
                self.transcripts.insert(key, segs);
                self.transcribing = None;
            }
            Ok(Err(e)) => {
                self.transcribing = None;
                self.toast(e, true);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(_) => self.transcribing = None,
        }
    }

    fn cut_time_ranges(&mut self, clip: &Clip, ranges: &[(f64, f64)], what: &str) {
        let (ops, removed) = captaine_agent::cut_ranges(clip, self.fps(), ranges);
        if ops.is_empty() {
            return self.toast("Nothing to cut", false);
        }
        if self.do_op(Op::Transaction {
            description: what.to_string(),
            ops,
        }) {
            self.word_sel = None;
            self.toast(format!("Removed {removed:.1} s"), false);
        }
    }

    fn add_captions_from(&mut self, clip: &Clip, start: i64, segs: &[Segment]) {
        let lines = captaine_speech::caption_lines(segs, 42, 6.0);
        let Some(tl) = self.timeline_id() else { return };
        match captaine_interchange::srt::caption_ops(
            self.project(),
            tl,
            &lines,
            start as f64 / self.fps(),
        ) {
            Ok(ops) if !ops.is_empty() => {
                let n = lines.len();
                if self.do_op(Op::Transaction {
                    description: format!("Add {n} captions to {}", clip.name),
                    ops,
                }) {
                    self.toast(format!("Added {n} captions on the Captions track"), false);
                }
            }
            Ok(_) => self.toast("No speech to caption", false),
            Err(e) => self.toast(e, true),
        }
    }
}

pub fn tab(app: &mut App, ui: &mut Ui) {
    app.poll_transcript();
    // subtitles work on the whole timeline, with or without a transcript
    ui.label(RichText::new("Subtitles").strong().size(12.5));
    ui.horizontal_wrapped(|ui| {
        if chip(ui, "Export SRT…", false, true).clicked() {
            if let Some(tl) = app.timeline_id() {
                match captaine_interchange::srt::export(app.project(), tl) {
                    Ok(text) => {
                        if let Some(p) = rfd::FileDialog::new()
                            .set_file_name(format!("{}.srt", app.project_title()))
                            .add_filter("subtitles", &["srt"])
                            .save_file()
                        {
                            match std::fs::write(&p, text) {
                                Ok(()) => app.toast(format!("Wrote {}", p.display()), false),
                                Err(e) => app.toast(e.to_string(), true),
                            }
                        }
                    }
                    Err(e) => app.toast(e, true),
                }
            }
        }
        if chip(ui, "Import SRT…", false, true).clicked() {
            if let (Some(p), Some(tl)) = (
                rfd::FileDialog::new()
                    .add_filter("subtitles", &["srt"])
                    .pick_file(),
                app.timeline_id(),
            ) {
                let plan = std::fs::read_to_string(&p)
                    .map_err(|e| e.to_string())
                    .and_then(|t| captaine_interchange::srt::import(app.project(), tl, &t, 0.0));
                match plan {
                    Ok(ops) => {
                        let n = ops.len();
                        app.do_op(Op::Transaction {
                            description: "Import subtitles".into(),
                            ops,
                        });
                        app.toast(format!("Imported {n} changes from {}", p.display()), false);
                    }
                    Err(e) => app.toast(e, true),
                }
            }
        }
    });
    ui.add_space(8.0);
    ui.label(RichText::new("Transcript").strong().size(12.5));
    let Some((clip, _, start)) = app.selected() else {
        ui.label(
            RichText::new("Select a clip with speech to transcribe it.")
                .size(11.0)
                .color(theme::MUTED),
        );
        return;
    };
    let key = key_of(&clip);
    if let Some(why) = captaine_speech::unavailable() {
        ui.label(RichText::new(why).size(11.0).color(theme::MUTED));
        return;
    }
    let busy = app.transcribing.as_ref().is_some_and(|t| t.key == key);
    let segs = app.transcripts.get(&key).cloned();
    ui.horizontal_wrapped(|ui| {
        if chip(
            ui,
            if segs.is_some() {
                "Transcribe again"
            } else {
                "Transcribe"
            },
            false,
            !busy,
        )
        .clicked()
        {
            app.transcribe_selected();
        }
        if busy {
            ui.spinner();
            ui.label(RichText::new("Listening…").size(11.0).color(theme::MUTED));
        }
    });
    let Some(segs) = segs else {
        if !busy {
            ui.label(
                RichText::new("Runs on this Mac; nothing is uploaded.")
                    .size(10.5)
                    .color(theme::FAINT),
            );
        }
        return;
    };
    let words: Vec<(f64, f64, String)> = segs
        .iter()
        .flat_map(|s| s.words.iter())
        .map(|w| (w.start, w.end, w.text.clone()))
        .collect();
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        let sel = app.word_sel.map(|(a, b)| (a.min(b), a.max(b)));
        for (i, (ws, we, text)) in words.iter().enumerate() {
            let on = sel.is_some_and(|(a, b)| (a..=b).contains(&i));
            let filler = captaine_speech::is_filler(text, false);
            let r = chip(ui, text, on, true);
            let r = if filler && !on {
                r.on_hover_text("hesitation")
            } else {
                r.on_hover_text(format!("{:.2} s", ws))
            };
            if r.clicked() {
                let shift = ui.input(|i| i.modifiers.shift);
                app.word_sel = match (app.word_sel, shift) {
                    (Some((a, _)), true) => Some((a, i)),
                    _ => Some((i, i)),
                };
                app.set_playhead(start + (ws * app.fps()).round() as i64);
            }
            let _ = we;
        }
    });
    ui.add_space(8.0);
    let sel = app.word_sel.map(|(a, b)| (a.min(b), a.max(b)));
    ui.horizontal_wrapped(|ui| {
        if chip(ui, "Cut selected words", false, sel.is_some()).clicked() {
            if let Some((a, b)) = sel {
                let range = ((words[a].0 - 0.03).max(0.0), words[b].1 + 0.03);
                app.cut_time_ranges(&clip, &[range], "Cut words");
            }
        }
        if chip(ui, "Remove filler words", false, true).clicked() {
            let ranges = captaine_speech::filler_ranges(&segs, false, 0.04);
            if ranges.is_empty() {
                app.toast("No filler words found", false);
            } else {
                app.cut_time_ranges(&clip, &ranges, "Remove filler words");
            }
        }
        if chip(ui, "Add captions", false, true).clicked() {
            app.add_captions_from(&clip, start, &segs);
        }
    });
    ui.label(
        RichText::new(
            "Click a word to jump there; shift-click to select a range. Cutting closes the gap.",
        )
        .size(10.5)
        .color(theme::FAINT),
    );
}
