//! Preview rendering on its own thread, so the UI never waits for a decode.
//!
//! The UI sends "show this project" and "show frame N"; the worker always renders only the newest request,
//! so scrubbing stays responsive however slow a frame is.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use captaine_model::{Id, Project};
use captaine_render::{preview_project, RenderOptions, Renderer};

enum Req {
    Project { project: Arc<Project>, timeline: Id },
    Frame { tick: i64, seq: u64, still: bool },
    Quality(Quality),
}

/// How the viewer's pictures are made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    /// Width of pictures while playing, in pixels
    pub play_width: u32,
    /// Width of the picture shown when stopped: always made from the original footage
    pub still_width: u32,
    /// Play from proxy files where they exist
    pub proxies: bool,
}

impl Default for Quality {
    fn default() -> Quality {
        Quality {
            play_width: 960,
            still_width: 960,
            proxies: false,
        }
    }
}

pub struct Frame {
    pub seq: u64,
    pub tick: i64,
    pub image: Result<egui::ColorImage, String>,
}

pub struct RenderWorker {
    tx: Sender<Req>,
    rx: Receiver<Frame>,
    next_seq: u64,
    /// requests sent minus frames received (lets playback skip frames when the worker is behind)
    pub in_flight: u64,
}

impl RenderWorker {
    pub fn spawn(ctx: egui::Context) -> RenderWorker {
        let (tx, req_rx) = channel::<Req>();
        let (frame_tx, rx) = channel::<Frame>();
        std::thread::Builder::new()
            .name("preview-render".into())
            .spawn(move || run(req_rx, frame_tx, ctx))
            .expect("spawn render thread");
        RenderWorker {
            tx,
            rx,
            next_seq: 0,
            in_flight: 0,
        }
    }

    pub fn set_project(&self, project: Arc<Project>, timeline: Id) {
        let _ = self.tx.send(Req::Project { project, timeline });
    }

    /// Ask for the picture at `tick`; `still` when nothing is playing, so it is made at full quality.
    pub fn request(&mut self, tick: i64, still: bool) -> u64 {
        self.next_seq += 1;
        self.in_flight += 1;
        let _ = self.tx.send(Req::Frame {
            tick,
            seq: self.next_seq,
            still,
        });
        self.next_seq
    }

    pub fn set_quality(&self, q: Quality) {
        let _ = self.tx.send(Req::Quality(q));
    }

    /// The newest finished frame, if any.
    pub fn poll(&mut self) -> Option<Frame> {
        let mut latest = None;
        while let Ok(f) = self.rx.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            latest = Some(f);
        }
        latest
    }
}

/// One renderer per way of rendering: playing (fast) and stopped (sharp). When both ask for the same thing they
/// share one, so the decoders are not doubled.
struct Renderers {
    play: Option<Renderer>,
    still: Option<Renderer>,
}

fn make(p: &Project, tl: Id, width: u32, proxies: bool) -> Option<Renderer> {
    let scale = (width as f64 / p.canvas(tl).0.max(1) as f64).min(1.0);
    let scaled = preview_project(p, scale);
    // pictures are decoded at the size they will be shown, camera originals on the video chip
    let opts = RenderOptions {
        use_proxy: proxies,
        decode_scale: scale,
        hw_decode: true,
        ..RenderOptions::default()
    };
    Renderer::new(&scaled, tl, opts).ok()
}

fn run(rx: Receiver<Req>, tx: Sender<Frame>, ctx: egui::Context) {
    let mut r = Renderers {
        play: None,
        still: None,
    };
    let mut quality = Quality::default();
    let mut current: Option<(Arc<Project>, Id)> = None;
    while let Ok(first) = rx.recv() {
        // fold everything queued into "newest project", "newest quality" and "newest frame"
        let mut project = None;
        let mut frame = None;
        let mut new_quality = None;
        let mut skipped = 0u64;
        for req in std::iter::once(first).chain(rx.try_iter()) {
            match req {
                Req::Project {
                    project: p,
                    timeline,
                } => project = Some((p, timeline)),
                Req::Frame { tick, seq, still } => {
                    if frame.is_some() {
                        skipped += 1;
                    }
                    frame = Some((tick, seq, still));
                }
                Req::Quality(q) => new_quality = Some(q),
            }
        }
        let quality_changed = new_quality.is_some_and(|q| q != quality);
        if let Some(q) = new_quality {
            quality = q;
        }
        if let Some((p, tl)) = project
            .clone()
            .or_else(|| quality_changed.then(|| current.clone()).flatten())
        {
            let same_timeline = current.as_ref().is_some_and(|(_, t)| *t == tl);
            let shared = quality.play_width == quality.still_width && !quality.proxies;
            // an edit keeps decoders and cached frames; a new timeline or new quality starts afresh
            let refresh = |slot: &mut Option<Renderer>, width: u32, proxies: bool| {
                let scale = (width as f64 / p.canvas(tl).0.max(1) as f64).min(1.0);
                let kept = match slot {
                    Some(r) if same_timeline && !quality_changed => {
                        r.set_project(&preview_project(&p, scale)).is_ok()
                    }
                    _ => false,
                };
                if !kept {
                    *slot = make(&p, tl, width, proxies);
                }
            };
            refresh(&mut r.play, quality.play_width, quality.proxies);
            if shared {
                r.still = None;
            } else {
                refresh(&mut r.still, quality.still_width, false);
            }
            current = Some((p, tl));
        }
        // frames skipped over still count as answered so the UI's in-flight counter stays right
        for _ in 0..skipped {
            let _ = tx.send(Frame {
                seq: 0,
                tick: -1,
                image: Err(String::new()),
            });
        }
        if let Some((tick, seq, still)) = frame {
            let renderer = if still {
                r.still.as_mut().or(r.play.as_mut())
            } else {
                r.play.as_mut()
            };
            let image = match renderer {
                Some(r) => r.render(tick).map_err(|e| e.to_string()).map(|c| {
                    let img = egui::ColorImage::from_rgba_unmultiplied(
                        [c.width as usize, c.height as usize],
                        &c.to_rgba8(),
                    );
                    r.recycle(c);
                    img
                }),
                None => Err("no timeline".to_string()),
            };
            let _ = tx.send(Frame { seq, tick, image });
            ctx.request_repaint();
        }
    }
}
