//! Keyboard shortcuts that can be changed, and the preferences file that remembers them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use egui::{Key, Modifiers};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    PlayPause,
    MarkIn,
    MarkOut,
    Split,
    Marker,
    FreezeFrame,
    Transition,
    Import,
    Export,
    CoCaptain,
    Duplicate,
    SelectAll,
    Snap,
    FitTimeline,
    ToolSelect,
    ToolTrim,
    ToolRoll,
    ToolSlip,
    ToolSlide,
    ToolBlade,
    ToolHand,
    ToolZoom,
}

/// A key with the modifiers held for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

const fn plain(key: Key) -> Chord {
    Chord {
        cmd: false,
        shift: false,
        alt: false,
        key,
    }
}

const fn cmd(key: Key) -> Chord {
    Chord {
        cmd: true,
        shift: false,
        alt: false,
        key,
    }
}

impl Chord {
    pub fn modifiers(&self) -> Modifiers {
        let mut m = Modifiers::NONE;
        if self.cmd {
            m |= Modifiers::COMMAND;
        }
        if self.shift {
            m |= Modifiers::SHIFT;
        }
        if self.alt {
            m |= Modifiers::ALT;
        }
        m
    }

    /// "⌘⇧B" style, for menus and the preferences list.
    pub fn label(&self) -> String {
        format!(
            "{}{}{}{}",
            if self.cmd { "⌘" } else { "" },
            if self.shift { "⇧" } else { "" },
            if self.alt { "⌥" } else { "" },
            match self.key {
                Key::Space => "Space".to_string(),
                Key::Delete => "⌦".to_string(),
                Key::Backspace => "⌫".to_string(),
                k => k.name().to_string(),
            }
        )
    }

    /// "Cmd+Shift+B", the form kept in the preferences file.
    pub fn to_text(&self) -> String {
        let mut parts = Vec::new();
        if self.cmd {
            parts.push("Cmd");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        parts.push(self.key.name());
        parts.join("+")
    }

    pub fn from_text(text: &str) -> Option<Chord> {
        let mut c = plain(Key::A);
        let mut key = None;
        for part in text.split('+') {
            match part {
                "Cmd" => c.cmd = true,
                "Shift" => c.shift = true,
                "Alt" => c.alt = true,
                name => key = Some(Key::from_name(name)?),
            }
        }
        c.key = key?;
        Some(c)
    }
}

pub const ACTIONS: [(Action, &str, Chord); 22] = [
    (Action::PlayPause, "Play / pause", plain(Key::Space)),
    (Action::MarkIn, "Mark in", plain(Key::I)),
    (Action::MarkOut, "Mark out", plain(Key::O)),
    (Action::Split, "Split at playhead", cmd(Key::B)),
    (Action::Marker, "Add marker", plain(Key::M)),
    (
        Action::FreezeFrame,
        "Freeze frame",
        Chord {
            shift: true,
            ..plain(Key::F)
        },
    ),
    (Action::Transition, "Add / remove transition", cmd(Key::T)),
    (Action::Import, "Import media", cmd(Key::I)),
    (Action::Export, "Export", cmd(Key::E)),
    (Action::CoCaptain, "CoCaptain", cmd(Key::J)),
    (Action::Duplicate, "Duplicate", cmd(Key::D)),
    (Action::SelectAll, "Select all", cmd(Key::A)),
    (Action::Snap, "Snapping on / off", plain(Key::S)),
    (
        Action::FitTimeline,
        "Fit timeline",
        Chord {
            shift: true,
            ..plain(Key::Z)
        },
    ),
    (Action::ToolSelect, "Select tool", plain(Key::V)),
    (Action::ToolTrim, "Trim tool", plain(Key::T)),
    (Action::ToolRoll, "Roll tool", plain(Key::N)),
    (Action::ToolSlip, "Slip tool", plain(Key::Y)),
    (Action::ToolSlide, "Slide tool", plain(Key::U)),
    (Action::ToolBlade, "Blade tool", plain(Key::B)),
    (Action::ToolHand, "Hand tool", plain(Key::H)),
    (Action::ToolZoom, "Zoom tool", plain(Key::Z)),
];

#[derive(Clone, Debug)]
pub struct Keymap {
    map: BTreeMap<Action, Chord>,
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap {
            map: ACTIONS.iter().map(|(a, _, c)| (*a, *c)).collect(),
        }
    }
}

/// Which panel's size to remember.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    Pool,
    Inspector,
    Timeline,
    CoCaptain,
}

impl Prefs {
    /// Note the size a panel ended up at; true when it changed enough to be worth saving.
    pub fn note_panel(&mut self, which: PanelKind, size: f32) -> bool {
        let slot = match which {
            PanelKind::Pool => &mut self.pool_w,
            PanelKind::Inspector => &mut self.inspector_w,
            PanelKind::Timeline => &mut self.timeline_h,
            PanelKind::CoCaptain => &mut self.cocaptain_w,
        };
        if (*slot - size).abs() < 1.5 || !size.is_finite() {
            return false;
        }
        *slot = size;
        let kept = self.clone().sane();
        *self = kept;
        true
    }
}

impl Keymap {
    pub fn chord(&self, a: Action) -> Chord {
        self.map[&a]
    }

    pub fn label(&self, a: Action) -> String {
        self.chord(a).label()
    }

    pub fn default_chord(a: Action) -> Chord {
        ACTIONS.iter().find(|(x, ..)| *x == a).expect("listed").2
    }

    pub fn is_default(&self, a: Action) -> bool {
        self.chord(a) == Keymap::default_chord(a)
    }

    /// The other action that already uses `c`.
    pub fn taken_by(&self, c: Chord, except: Action) -> Option<Action> {
        self.map
            .iter()
            .find(|(a, x)| **a != except && **x == c)
            .map(|(a, _)| *a)
    }

    pub fn set(&mut self, a: Action, c: Chord) -> Result<(), String> {
        if let Some(other) = self.taken_by(c, a) {
            let name = ACTIONS
                .iter()
                .find(|(x, ..)| *x == other)
                .map_or("", |t| t.1);
            return Err(format!("{} is already used for \"{name}\"", c.label()));
        }
        self.map.insert(a, c);
        Ok(())
    }

    pub fn reset(&mut self, a: Action) {
        self.map.insert(a, Keymap::default_chord(a));
    }

    pub fn reset_all(&mut self) {
        *self = Keymap::default();
    }

    /// Was this action's key pressed this frame? (Consumes it.)
    pub fn pressed(&self, ctx: &egui::Context, a: Action) -> bool {
        let c = self.chord(a);
        ctx.input_mut(|i| i.consume_key(c.modifiers(), c.key))
    }

    /// Only the bindings that differ from the defaults, by action name.
    pub fn overrides(&self) -> BTreeMap<String, String> {
        ACTIONS
            .iter()
            .filter(|(a, ..)| !self.is_default(*a))
            .map(|(a, ..)| (format!("{a:?}"), self.chord(*a).to_text()))
            .collect()
    }

    /// Defaults with the saved changes on top; entries that make no sense are ignored.
    pub fn from_overrides(saved: &BTreeMap<String, String>) -> Keymap {
        let mut k = Keymap::default();
        for (a, ..) in ACTIONS.iter() {
            if let Some(c) = saved
                .get(&format!("{a:?}"))
                .and_then(|t| Chord::from_text(t))
            {
                let _ = k.set(*a, c);
            }
        }
        k
    }
}

/// Things the user can set once and keep.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prefs {
    /// Interface zoom, 0.8 to 1.6
    pub ui_scale: f32,
    /// Minutes between timed project snapshots; 0 turns them off
    pub snapshot_minutes: u32,
    /// Height of proxy files, in pixels
    pub proxy_height: u32,
    /// Make proxies on their own, in the background, for footage bigger than HD (4K camera files scrub and
    /// play far more smoothly from a proxy)
    #[serde(default)]
    pub auto_proxies: bool,
    /// How sharp the viewer is while playing (when stopped it always shows the original at full quality)
    #[serde(default)]
    pub playback_quality: PlaybackQuality,
    /// Play from proxy files instead of the originals (where proxies exist)
    #[serde(default)]
    pub use_proxies: bool,
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
    /// Panel sizes the user dragged them to, kept between runs
    #[serde(default = "default_pool_w")]
    pub pool_w: f32,
    #[serde(default = "default_inspector_w")]
    pub inspector_w: f32,
    #[serde(default = "default_timeline_h")]
    pub timeline_h: f32,
    /// Effects and transitions starred in the effects library, by key ("glow", "t:flash")
    #[serde(default)]
    pub favourites: Vec<String>,
    /// CoCaptain may show Claude small frames of the timeline and footage when it needs to look (off until the
    /// user turns it on in the panel)
    #[serde(default)]
    pub cocaptain_frames: bool,
    /// Width of the docked CoCaptain chat
    #[serde(default = "default_cocaptain_w")]
    pub cocaptain_w: f32,
}

/// Viewer resolution while playing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackQuality {
    /// As many pixels as the viewer shows (up to 1920 wide): sharp on screen and smooth
    #[default]
    Auto,
    /// The timeline's own size (4K for a 4K timeline)
    Full,
    Half,
    Quarter,
}

impl PlaybackQuality {
    pub const ALL: [PlaybackQuality; 4] = [
        PlaybackQuality::Auto,
        PlaybackQuality::Full,
        PlaybackQuality::Half,
        PlaybackQuality::Quarter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlaybackQuality::Auto => "Auto",
            PlaybackQuality::Full => "Full",
            PlaybackQuality::Half => "1/2",
            PlaybackQuality::Quarter => "1/4",
        }
    }

    /// Picture width while playing, for a viewer `viewer_px` physical pixels wide and a canvas `canvas` wide.
    /// Half and quarter are of what Auto shows, so they are always lighter than Auto (a half of a 4K timeline would
    /// still be more pixels than a small viewer can show, and look and play no differently).
    pub fn width(self, viewer_px: u32, canvas: u32) -> u32 {
        let canvas = canvas.max(1);
        let auto = viewer_px.min(1920).min(canvas);
        let w = match self {
            PlaybackQuality::Auto => auto,
            PlaybackQuality::Full => canvas,
            PlaybackQuality::Half => auto / 2,
            PlaybackQuality::Quarter => auto / 4,
        };
        w.clamp(160.min(canvas), canvas)
    }
}

fn default_pool_w() -> f32 {
    340.0
}
fn default_inspector_w() -> f32 {
    340.0
}
fn default_cocaptain_w() -> f32 {
    400.0
}
fn default_timeline_h() -> f32 {
    300.0
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            ui_scale: 1.0,
            snapshot_minutes: 5,
            proxy_height: 540,
            auto_proxies: false,
            playback_quality: PlaybackQuality::Auto,
            use_proxies: false,
            keys: BTreeMap::new(),
            pool_w: default_pool_w(),
            favourites: Vec::new(),
            inspector_w: default_inspector_w(),
            timeline_h: default_timeline_h(),
            cocaptain_frames: false,
            cocaptain_w: default_cocaptain_w(),
        }
    }
}

impl Prefs {
    pub fn path() -> Option<PathBuf> {
        std::env::var_os("CAPTAINE_PREFS")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".captaine/prefs.json"))
            })
    }

    pub fn load() -> Prefs {
        Prefs::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Prefs>(&t).ok())
            .map(Prefs::sane)
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        match Prefs::path() {
            Some(p) => self.save_to(&p),
            None => Ok(()),
        }
    }

    pub fn save_to(&self, p: &std::path::Path) -> Result<(), String> {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(p, text).map_err(|e| format!("could not save preferences: {e}"))
    }

    /// Keep values in the range the interface supports.
    pub fn sane(mut self) -> Prefs {
        self.ui_scale = if self.ui_scale.is_finite() {
            self.ui_scale.clamp(0.8, 1.6)
        } else {
            1.0
        };
        self.snapshot_minutes = self.snapshot_minutes.min(120);
        self.proxy_height = self.proxy_height.clamp(120, 1080);
        // narrower than the tab rows would clip them
        self.pool_w = self.pool_w.clamp(340.0, 640.0);
        self.inspector_w = self.inspector_w.clamp(340.0, 600.0);
        self.timeline_h = self.timeline_h.clamp(180.0, 900.0);
        self.cocaptain_w = if self.cocaptain_w.is_finite() {
            self.cocaptain_w.clamp(320.0, 720.0)
        } else {
            default_cocaptain_w()
        };
        self
    }
}

#[cfg(test)]
mod quality_tests {
    use super::PlaybackQuality as Q;

    #[test]
    fn lower_settings_are_always_fewer_pixels() {
        // a 2160-wide vertical reel in a small viewer, a 4K timeline in a big one, an HD timeline
        for (viewer, canvas) in [(640, 2160), (1600, 3840), (1300, 1920), (2400, 1280)] {
            let w = |q: Q| q.width(viewer, canvas);
            assert!(w(Q::Full) >= w(Q::Auto), "{viewer} {canvas}");
            assert!(w(Q::Auto) > w(Q::Half), "{viewer} {canvas}");
            assert!(w(Q::Half) > w(Q::Quarter), "{viewer} {canvas}");
            assert_eq!(w(Q::Full), canvas);
        }
    }
}
