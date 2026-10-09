//! Background jobs that decorate the UI: thumbnails for the media pool, waveforms for audio clips.

use std::sync::mpsc::{channel, Receiver, Sender};

use captaine_audio::{media_peaks, Peak};
use captaine_model::{Id, MediaRef};
use captaine_render::VideoReader;

pub enum Done {
    Thumb(Id, egui::ColorImage),
    Wave(Id, Vec<Peak>),
}

pub struct Jobs {
    tx: Sender<MediaRef>,
    rx: Receiver<Done>,
}

/// Waveform resolution: peaks per second of media.
pub const PEAKS_PER_SEC: u32 = 100;

impl Jobs {
    pub fn spawn(ctx: egui::Context) -> Jobs {
        let (tx, jobs) = channel::<MediaRef>();
        let (done_tx, rx) = channel();
        std::thread::Builder::new()
            .name("media-jobs".into())
            .spawn(move || {
                for m in jobs {
                    if m.has_video {
                        if let Some(img) = thumbnail(&m) {
                            let _ = done_tx.send(Done::Thumb(m.id, img));
                            ctx.request_repaint();
                        }
                    }
                    if m.has_audio {
                        let secs = m.duration.map_or(0.0, |d| d.to_seconds_f64());
                        if let Ok(p) = media_peaks(&m.path, secs, PEAKS_PER_SEC) {
                            let _ = done_tx.send(Done::Wave(m.id, p));
                            ctx.request_repaint();
                        }
                    }
                }
            })
            .expect("spawn jobs thread");
        Jobs { tx, rx }
    }

    pub fn queue(&self, media: MediaRef) {
        let _ = self.tx.send(media);
    }

    pub fn poll(&self) -> Vec<Done> {
        self.rx.try_iter().collect()
    }
}

fn thumbnail(m: &MediaRef) -> Option<egui::ColorImage> {
    let len = m.duration?;
    let frame = (len.value / 10).clamp(0, (len.value - 1).max(0));
    let mut r = VideoReader::open(&m.path, m.width, m.height, len.rate, frame).ok()?;
    let f = r.frame(frame).ok()?.thumbnail(320);
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [f.width as usize, f.height as usize],
        &f.rgba,
    ))
}
