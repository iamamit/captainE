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
    Frame { tick: i64, seq: u64 },
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

/// Pictures are rendered at most this wide: plenty for the viewer, much cheaper than full size.
const PREVIEW_WIDTH: f64 = 960.0;

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

    pub fn request(&mut self, tick: i64) -> u64 {
        self.next_seq += 1;
        self.in_flight += 1;
        let _ = self.tx.send(Req::Frame {
            tick,
            seq: self.next_seq,
        });
        self.next_seq
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

fn run(rx: Receiver<Req>, tx: Sender<Frame>, ctx: egui::Context) {
    let mut renderer: Option<Renderer> = None;
    let mut current: Option<(Arc<Project>, Id)> = None;
    while let Ok(first) = rx.recv() {
        // fold everything queued into "newest project" and "newest frame"
        let mut project = None;
        let mut frame = None;
        let mut skipped = 0u64;
        for req in std::iter::once(first).chain(rx.try_iter()) {
            match req {
                Req::Project {
                    project: p,
                    timeline,
                } => project = Some((p, timeline)),
                Req::Frame { tick, seq } => {
                    if frame.is_some() {
                        skipped += 1;
                    }
                    frame = Some((tick, seq));
                }
            }
        }
        if let Some((p, tl)) = project {
            let scaled = preview_project(&p, (PREVIEW_WIDTH / p.settings.width as f64).min(1.0));
            let same_timeline = current.as_ref().is_some_and(|(_, t)| *t == tl);
            let reused = match (&mut renderer, same_timeline) {
                (Some(r), true) => r.set_project(&scaled).is_ok(),
                _ => false,
            };
            if !reused {
                let opts = RenderOptions {
                    use_proxy: true,
                    ..RenderOptions::default()
                };
                renderer = Renderer::new(&scaled, tl, opts).ok();
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
        if let Some((tick, seq)) = frame {
            let image = match &mut renderer {
                Some(r) => r.render(tick).map_err(|e| e.to_string()).map(|c| {
                    egui::ColorImage::from_rgba_unmultiplied(
                        [c.width as usize, c.height as usize],
                        &c.to_rgba8(),
                    )
                }),
                None => Err("no timeline".to_string()),
            };
            let _ = tx.send(Frame { seq, tick, image });
            ctx.request_repaint();
        }
    }
}
