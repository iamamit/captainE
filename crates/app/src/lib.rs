//! The captainE editor window (egui). All editing goes through `captaine_ops`; this crate is only the view.

pub mod app;
pub mod audio;
pub mod branches_ui;
pub mod compare;
pub mod copilot;
pub mod jobs;
pub mod theme;
pub mod timeline;
pub mod titles_ui;
pub mod ui;
pub mod viewer;
pub mod worker;

use std::path::{Path, PathBuf};

use app::App;
use captaine_model::*;
use captaine_ops::Op;
use captaine_project::Session;
use captaine_time::Rational;

/// Replace the open project with the one in `path`.
pub fn open_project(app: &mut App, path: PathBuf) {
    match Session::load(&path) {
        Ok(s) => {
            app.replace_session(s, Some(path.clone()));
            remember(&path);
        }
        Err(e) => app.toast(e, true),
    }
}

pub fn parse_fps(s: &str) -> Rational {
    match s {
        "23.976" => Rational::FPS_23_976,
        "29.97" => Rational::FPS_29_97,
        "59.94" => Rational::FPS_59_94,
        other => Rational::new(other.parse().unwrap_or(30), 1).unwrap_or(Rational::FPS_30),
    }
}

/// A new empty project: one timeline with tracks V2, V1 (video) and A1, A2 (audio).
pub fn blank_session(w: u32, h: u32, rate: Rational) -> Session {
    let mut s = Session::new(ProjectSettings {
        frame_rate: rate,
        width: w,
        height: h,
        sample_rate: 48_000,
    });
    let tl = Id::new();
    s.apply(Op::AddTimeline {
        timeline: Timeline::new(tl, "Timeline 1", rate),
    })
    .expect("fresh timeline");
    for (i, (name, kind)) in [
        ("V1", TrackKind::Video),
        ("V2", TrackKind::Video),
        ("A1", TrackKind::Audio),
        ("A2", TrackKind::Audio),
    ]
    .into_iter()
    .enumerate()
    {
        s.apply(Op::AddTrack {
            timeline: tl,
            index: i,
            track: Track::new(Id::new(), name, kind),
        })
        .expect("fresh track");
    }
    s
}

pub fn new_project(app: &mut App, dir: &Path, name: &str, w: u32, h: u32, fps: &str) {
    let path = dir.join(format!("{}.captaine.json", name.trim().replace('/', "-")));
    if path.exists() {
        app.toast(format!("{} already exists", path.display()), true);
        return;
    }
    let s = blank_session(w, h, parse_fps(fps));
    match s.save(&path) {
        Ok(()) => {
            app.replace_session(s, Some(path.clone()));
            remember(&path);
        }
        Err(e) => app.toast(e, true),
    }
}

fn state_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".captaine"))
}

fn remember(path: &Path) {
    if let Some(d) = state_dir() {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join("last_project"), path.to_string_lossy().as_bytes());
    }
}

/// The project to open at start-up: the path given, else the last one used, else a fresh one in ~/Movies/captainE.
pub fn startup_session(arg: Option<PathBuf>) -> (Session, PathBuf) {
    let candidates = arg.into_iter().chain(
        state_dir()
            .and_then(|d| std::fs::read_to_string(d.join("last_project")).ok())
            .map(|s| PathBuf::from(s.trim())),
    );
    for p in candidates {
        if p.exists() {
            if let Ok(s) = Session::load(&p) {
                return (s, p);
            }
        }
    }
    let dir = std::env::var_os("HOME").map_or_else(
        || PathBuf::from("."),
        |h| PathBuf::from(h).join("Movies/captainE"),
    );
    let _ = std::fs::create_dir_all(&dir);
    let mut path = dir.join("Untitled.captaine.json");
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("Untitled {n}.captaine.json"));
    }
    let s = blank_session(1920, 1080, Rational::FPS_30);
    let _ = s.save(&path);
    (s, path)
}

struct Shell {
    app: App,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.update(ui);
    }

    fn on_exit(&mut self) {
        self.app.on_exit();
    }
}

pub fn run(arg: Option<PathBuf>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1480.0, 940.0])
            .with_min_inner_size([1100.0, 700.0])
            .with_title("captainE")
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "captainE",
        options,
        Box::new(move |cc| {
            let (session, path) = startup_session(arg);
            let mut app = App::new(&cc.egui_ctx, session, Some(path));
            app.fit_on_start();
            Ok(Box::new(Shell { app }))
        }),
    )
}
