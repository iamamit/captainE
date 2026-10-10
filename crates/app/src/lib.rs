//! The captainE editor window (egui). All editing goes through `captaine_ops`; this crate is only the view.

pub mod app;
pub mod audio;
pub mod branches_ui;
pub mod clipboard;
pub mod compare;
pub mod copilot;
pub mod effect_widgets;
pub mod glyphs;
pub mod jobs;
pub mod keymap;
pub mod links;
pub mod loudness_ops;
pub mod markers_ui;
pub mod media_org;
pub mod multicam;
pub mod nesting;
pub mod presets_store;
pub mod scopes;
pub mod sequences;
pub mod source;
pub mod theme;
pub mod timeline;
pub mod titles_ui;
pub mod track_ui;
pub mod transcript_ui;
pub mod ui;
pub mod viewer;
pub mod vision_jobs;
pub mod voiceover;
pub mod worker;

use std::path::{Path, PathBuf};

use app::App;
use captaine_model::*;
use captaine_ops::Op;
use captaine_project::Session;
use captaine_time::Rational;

/// Replace the open project with the one in `path`.
pub fn open_project(app: &mut App, path: PathBuf) {
    match Session::load_recovering(&path) {
        Ok((s, note)) => {
            app.replace_session(s, Some(path.clone()));
            remember(&path);
            if let Some(n) = note {
                app.toast(n, true);
            }
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
    let (s, p, _) = startup_session_noted(arg);
    (s, p)
}

/// Like [`startup_session`], plus a note when a damaged project was replaced by its backup.
pub fn startup_session_noted(arg: Option<PathBuf>) -> (Session, PathBuf, Option<String>) {
    let candidates = arg.into_iter().chain(
        state_dir()
            .and_then(|d| std::fs::read_to_string(d.join("last_project")).ok())
            .map(|s| PathBuf::from(s.trim())),
    );
    for p in candidates {
        if p.exists() {
            if let Ok((s, note)) = Session::load_recovering(&p) {
                return (s, p, note);
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
    remember(&path);
    (s, path, None)
}

/// Mark this run as started; true when the previous run never marked itself finished (a crash or a kill).
pub fn mark_running(dir: &Path) -> bool {
    let flag = dir.join("running");
    let crashed = flag.exists();
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(&flag, std::process::id().to_string());
    crashed
}

/// This run ended normally.
pub fn mark_finished(dir: &Path) {
    let _ = std::fs::remove_file(dir.join("running"));
}

/// Write panics to `<dir>/crash-<time>.txt` (message, place, backtrace) before the usual report.
pub fn install_crash_log(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let text = format!(
            "captainE {} crashed\n{info}\n\n{}\n",
            env!("CARGO_PKG_VERSION"),
            std::backtrace::Backtrace::force_capture()
        );
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("crash-{when}.txt")), text);
        previous(info);
    }));
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
        if let Some(d) = state_dir() {
            mark_finished(&d);
        }
    }
}

/// PATH for the helper programs (ffmpeg, python). An app opened from Finder gets a bare PATH, so
/// put a bundled `Resources/bin` first and the usual install places last.
pub fn helper_search_path(current: &str, exe: &std::path::Path) -> String {
    let mut dirs: Vec<String> = Vec::new();
    if let Some(res) = exe
        .parent()
        .and_then(|m| m.parent())
        .map(|c| c.join("Resources"))
    {
        dirs.push(res.join("bin").to_string_lossy().into());
    }
    dirs.extend(
        current
            .split(':')
            .filter(|d| !d.is_empty())
            .map(String::from),
    );
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if !dirs.iter().any(|d| d == extra) {
            dirs.push(extra.into());
        }
    }
    dirs.join(":")
}

pub fn run(arg: Option<PathBuf>) -> eframe::Result {
    if let Ok(exe) = std::env::current_exe() {
        let path = helper_search_path(&std::env::var("PATH").unwrap_or_default(), &exe);
        std::env::set_var("PATH", path);
    }
    let crashed = state_dir().is_some_and(|d| {
        install_crash_log(d.clone());
        mark_running(&d)
    });
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
            let (session, path, note) = startup_session_noted(arg);
            let mut app = App::new(&cc.egui_ctx, session, Some(path));
            app.fit_on_start();
            if crashed {
                app.toast(
                    "captainE did not close properly last time. Your edits were saved as you made them; older versions are in the project's .backups folder.",
                    true,
                );
            }
            if let Some(n) = note {
                app.toast(n, true);
            }
            Ok(Box::new(Shell { app }))
        }),
    )
}
