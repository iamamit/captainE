//! The source monitor: look through a media file, mark an in and out point, and put that stretch on the
//! timeline with Insert (ripple) or Overwrite. Three-point edits take the playhead as the missing point;
//! with marks on the timeline as well it is a four-point edit and the clip is re-timed to fit.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use captaine_model::{Clip, Id, Item, MediaRef, Retime, TrackKind};
use captaine_ops::edit;
use captaine_render::VideoReader;
use captaine_time::{Rational, RationalTime};
use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke};

use crate::app::App;
use crate::theme::{self, mono};
use crate::ui::chip;

struct Request {
    media: MediaRef,
    frame: i64,
}

pub struct Shown {
    pub media: Id,
    pub frame: i64,
    pub image: egui::ColorImage,
}

pub struct SourceMonitor {
    pub open: bool,
    pub media: Option<Id>,
    /// Frame number in the media, at the media's own frame rate
    pub frame: i64,
    pub mark_in: Option<i64>,
    pub mark_out: Option<i64>,
    pub tex: Option<egui::TextureHandle>,
    pub shown: Option<(Id, i64)>,
    asked: Option<(Id, i64)>,
    tx: Sender<Request>,
    rx: Receiver<Shown>,
}

impl SourceMonitor {
    pub fn new(ctx: egui::Context) -> SourceMonitor {
        let (tx, jobs) = channel::<Request>();
        let (done_tx, rx) = channel();
        std::thread::Builder::new()
            .name("source-monitor".into())
            .spawn(move || {
                // the decoder stays open between requests: stepping or playing through a file reads on
                // instead of starting a new decoder for every frame
                let mut open: Option<(Id, VideoReader)> = None;
                while let Ok(mut req) = jobs.recv() {
                    // only the latest request matters while scrubbing
                    while let Ok(newer) = jobs.try_recv() {
                        req = newer;
                    }
                    if let Some(image) = decode(&mut open, &req.media, req.frame) {
                        let _ = done_tx.send(Shown {
                            media: req.media.id,
                            frame: req.frame,
                            image,
                        });
                        ctx.request_repaint();
                    }
                }
            })
            .expect("spawn source monitor thread");
        SourceMonitor {
            open: false,
            media: None,
            frame: 0,
            mark_in: None,
            mark_out: None,
            tex: None,
            shown: None,
            asked: None,
            tx,
            rx,
        }
    }

    pub fn ask(&mut self, media: &MediaRef, frame: i64) {
        if self.asked != Some((media.id, frame)) {
            self.asked = Some((media.id, frame));
            let _ = self.tx.send(Request {
                media: media.clone(),
                frame,
            });
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(s) = self.rx.try_recv() {
            self.shown = Some((s.media, s.frame));
            match &mut self.tex {
                Some(t) => t.set(s.image, egui::TextureOptions::LINEAR),
                None => {
                    self.tex =
                        Some(ctx.load_texture("source", s.image, egui::TextureOptions::LINEAR))
                }
            }
        }
    }
}

/// How wide the source monitor's picture is decoded: about what the panel shows.
const SOURCE_WIDTH: u32 = 960;

fn decode(
    open: &mut Option<(Id, VideoReader)>,
    m: &MediaRef,
    frame: i64,
) -> Option<egui::ColorImage> {
    if !m.has_video {
        return None;
    }
    let len = m.duration?;
    let frame = frame.clamp(0, (len.value - 1).max(0));
    if !open.as_ref().is_some_and(|(id, _)| *id == m.id) {
        // decode at the size shown: from the proxy when there is one (small, every frame a keyframe, quickest
        // in software), else from the original on the video chip
        let w = m.width.clamp(2, SOURCE_WIDTH) & !1;
        let h = ((m.height as u64 * w as u64 / m.width.max(1) as u64) as u32).max(2) & !1;
        let (path, hw, space) = match &m.proxy {
            Some(p) if std::path::Path::new(p).exists() => {
                (p.clone(), false, captaine_model::InputSpace::Rec709)
            }
            _ => (m.path.clone(), true, captaine_media::detect_space(&m.path)),
        };
        let r = VideoReader::open_in(&path, w, h, len.rate, frame, hw, space)
            .or_else(|_| VideoReader::open_in(&path, w, h, len.rate, frame, false, space))
            .ok()?;
        *open = Some((m.id, r));
    }
    let (_, r) = open.as_mut()?;
    let f = match r.frame(frame) {
        Ok(f) => f,
        Err(_) => {
            *open = None;
            return None;
        }
    };
    let f = if f.width > SOURCE_WIDTH {
        Arc::new(f.thumbnail(SOURCE_WIDTH))
    } else {
        f
    };
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [f.width as usize, f.height as usize],
        &f.rgba,
    ))
}

impl App {
    /// Show this media in the source monitor.
    pub fn open_source(&mut self, media: Id) {
        if self.source.media != Some(media) {
            self.source.media = Some(media);
            self.source.frame = 0;
            self.source.mark_in = None;
            self.source.mark_out = None;
            self.source.shown = None;
        }
        self.source.open = true;
    }

    pub fn source_media(&self) -> Option<MediaRef> {
        self.source
            .media
            .and_then(|id| self.project().media.get(&id).cloned())
    }

    pub fn source_step(&mut self, delta: i64) {
        if let Some(len) = self.source_media().and_then(|m| m.duration) {
            self.source.frame = (self.source.frame + delta).clamp(0, (len.value - 1).max(0));
        }
    }

    /// Put the marked stretch on the timeline: `insert` ripples, otherwise it overwrites.
    pub fn source_edit(&mut self, insert: bool) {
        let (Some(m), Some(tl)) = (self.source_media(), self.timeline().cloned()) else {
            self.toast("Open a media file in the source monitor first", false);
            return;
        };
        let Some(len) = m.duration else { return };
        let (s_in, s_out) = (
            self.source.mark_in.unwrap_or(0),
            self.source.mark_out.unwrap_or(len.value),
        );
        if s_out <= s_in {
            self.toast("The source out point must come after the in point", true);
            return;
        }
        let src_rate: Rational = len.rate;
        let src_secs = (s_out - s_in) as f64 / src_rate.as_f64();
        let timeline_marks = self
            .mark_in
            .zip(self.mark_out)
            .map(|(a, b)| (a.min(b), a.max(b)))
            .filter(|(a, b)| b > a);
        // four points: the timeline marks decide where and how long; otherwise the playhead is the start
        let (at, mut ticks, speed) = match timeline_marks {
            Some((a, b)) => (a, b - a, src_secs / ((b - a) as f64 / tl.rate.as_f64())),
            None => (
                self.playhead,
                ((src_secs * tl.rate.as_f64()).round() as i64).max(1),
                1.0,
            ),
        };
        let kind = if m.has_video {
            TrackKind::Video
        } else {
            TrackKind::Audio
        };
        let track = self
            .selected_span()
            .map(|(_, t, ..)| t)
            .filter(|t| tl.tracks.iter().any(|x| x.id == *t && x.kind == kind))
            .or_else(|| tl.tracks.iter().find(|t| t.kind == kind).map(|t| t.id));
        let Some(track) = track else {
            self.toast(
                "There is no suitable track; add one from the Timeline menu",
                true,
            );
            return;
        };
        let speed = speed.clamp(
            captaine_model::Retime::MIN_SPEED,
            captaine_model::Retime::MAX_SPEED,
        );
        let make = |ticks: i64| {
            let mut c = Clip::new(
                Id::new(),
                m.name.clone(),
                m.id,
                RationalTime::new(s_in, src_rate),
                RationalTime::new(ticks, tl.rate),
            );
            c.retime = Retime {
                speed,
                ..Retime::default()
            };
            c
        };
        // rounding may push the window a frame past the end of the file
        let mut clip = make(ticks);
        while s_in + clip.source_span() > len.value && ticks > 1 {
            ticks -= 1;
            clip = make(ticks);
        }
        let id = clip.id;
        let when = RationalTime::new(at, tl.rate);
        let plan = if insert {
            edit::insert_item(self.project(), track, when, Item::Clip(clip))
        } else {
            edit::overwrite(self.project(), track, when, clip)
        };
        match plan {
            Ok(op) => {
                if self.do_op(op) {
                    self.select_only(&[id]);
                }
            }
            Err(e) => self.toast(e.to_string(), true),
        }
    }
}

/// The source monitor picture, scrub bar and edit buttons. `area` is where the picture goes.
pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect, transport: Rect) {
    app.source.poll(ui.ctx());
    let Some(m) = app.source_media() else {
        ui.painter().text(
            area.center(),
            Align2::CENTER_CENTER,
            "Double-click a media file in the pool to look through it here",
            crate::theme::ui_font(13.0),
            theme::MUTED,
        );
        return;
    };
    let Some(len) = m.duration else { return };
    let frame = app.source.frame.clamp(0, (len.value - 1).max(0));
    app.source.ask(&m, frame);
    let (w, h) = if m.has_video {
        (m.width.max(1) as f32, m.height.max(1) as f32)
    } else {
        (16.0, 9.0)
    };
    let scale = (area.width() / w).min(area.height() / h).max(0.01);
    let pic = Rect::from_center_size(area.center(), vec2(w * scale, h * scale));
    ui.painter().rect_filled(pic, 0.0, Color32::BLACK);
    if let (Some(t), Some(_)) = (
        &app.source.tex,
        app.source.shown.filter(|(id, _)| *id == m.id),
    ) {
        ui.painter().image(
            t.id(),
            pic,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else if !m.has_video {
        ui.painter().text(
            pic.center(),
            Align2::CENTER_CENTER,
            "Audio only",
            crate::theme::ui_font(13.0),
            theme::MUTED,
        );
    }
    let fps = len.rate.as_f64();
    let tc = |f: i64| {
        format!(
            "{:02}:{:05.2}",
            (f as f64 / fps / 60.0) as i64,
            (f as f64 / fps) % 60.0
        )
    };
    ui.painter().text(
        transport.left_top() + vec2(16.0, 12.0),
        Align2::LEFT_TOP,
        format!("{} / {}", tc(frame), tc(len.value)),
        mono(16.0),
        Color32::WHITE,
    );
    // scrub bar with the marked stretch
    let bar = Rect::from_min_size(
        transport.left_top() + vec2(16.0, 44.0),
        vec2(transport.width() - 32.0, 14.0),
    );
    ui.painter().rect_filled(bar, 3.0, theme::FIELD);
    let x_of = |f: i64| bar.left() + bar.width() * (f as f32 / len.value.max(1) as f32);
    let (mi, mo) = (
        app.source.mark_in.unwrap_or(0),
        app.source.mark_out.unwrap_or(len.value),
    );
    if app.source.mark_in.is_some() || app.source.mark_out.is_some() {
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(x_of(mi), bar.top()), pos2(x_of(mo), bar.bottom())),
            3.0,
            theme::ACCENT.gamma_multiply(0.55),
        );
    }
    ui.painter()
        .vline(x_of(frame), bar.y_range(), Stroke::new(2.0, Color32::WHITE));
    let resp = ui.interact(bar, egui::Id::new("source-scrub"), Sense::click_and_drag());
    if let (true, Some(p)) = (
        resp.dragged() || resp.clicked(),
        resp.interact_pointer_pos(),
    ) {
        let f = (((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0) * len.value as f32) as i64;
        app.source.frame = f.clamp(0, (len.value - 1).max(0));
    }
    // buttons
    let row = Rect::from_min_size(
        transport.left_top() + vec2(16.0, 6.0),
        vec2(transport.width() - 32.0, 26.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 6.0;
    if chip(&mut child, "Overwrite  .", false, true).clicked() {
        app.source_edit(false);
    }
    if chip(&mut child, "Insert  ,", false, true).clicked() {
        app.source_edit(true);
    }
    if chip(
        &mut child,
        "Mark Out  O",
        app.source.mark_out.is_some(),
        true,
    )
    .clicked()
    {
        app.source.mark_out = Some(frame);
    }
    if chip(&mut child, "Mark In  I", app.source.mark_in.is_some(), true).clicked() {
        app.source.mark_in = Some(frame);
    }
    if chip(&mut child, "Clear", false, true).clicked() {
        app.source.mark_in = None;
        app.source.mark_out = None;
    }
    let _ = RichText::new("");
}
