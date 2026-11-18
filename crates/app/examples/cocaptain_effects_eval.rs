//! Live test of CoCaptain on the effects library: plain-English requests, one per effect (asked the way a person
//! would, not by the effect's internal name), plus editing, animating, removing and stacking effects and every
//! new transition, against a real project.
//!
//!     cargo run --release -p captaine-app --example cocaptain_effects_eval -- <project.json> [--shot N] [--only NAME]
//!
//! Works on a new branch `test/cocaptain-effects`. Before each request the selected shot (N, default 2) is set
//! up (its effects and transition cleared, or given a starting stack); afterwards the runner checks which effects
//! are on the shot, the settings that matter, that the rendered picture changed, and that nothing else was
//! touched. A Markdown and JSON report is written next to the project, and every model exchange is logged.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use captaine_agent::{
    AgentConfig, AgentRequest, AnthropicTransport, ClaudeCli, Credentials, Event, Transport,
};
use captaine_model::*;
use captaine_ops::{Author, Op};
use captaine_project::Session;
use captaine_render::{preview_project, RenderOptions, Renderer};
use serde_json::{json, Value};

struct Logged {
    inner: Box<dyn Transport + Sync>,
    log: Mutex<std::fs::File>,
}

impl Transport for Logged {
    fn send(&self, body: &Value) -> Result<Value, String> {
        let r = self.inner.send(body);
        if let Ok(mut f) = self.log.lock() {
            let line = json!({"messages": body["messages"], "reply": r.as_ref().map_or_else(|e| json!({"error": e}), |v| v.clone())});
            let _ = writeln!(f, "{line}");
        }
        r
    }
}

fn pixels(p: &Project, tl: Id, tick: i64) -> Vec<u8> {
    let scale = (480.0 / p.canvas(tl).0.max(1) as f64).min(1.0);
    let q = preview_project(p, scale);
    let opts = RenderOptions {
        decode_scale: scale,
        hw_decode: true,
        ..RenderOptions::default()
    };
    Renderer::new(&q, tl, opts)
        .unwrap()
        .render(tick)
        .unwrap()
        .to_rgba8()
}

/// Size of the pictures `pixels` returns.
fn canvas_px(p: &Project, tl: Id) -> (usize, usize) {
    let cw = p.canvas(tl).0;
    let scale = (480.0 / cw.max(1) as f64).min(1.0);
    let q = preview_project(p, scale);
    let (w, h) = q.canvas(tl);
    (w as usize, h as usize)
}

/// Width over height of the part of the picture that is not black bars (rows and columns through the middle).
fn visible_aspect(px: &[u8], w: usize, h: usize) -> f64 {
    let lit = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32 > 24
    };
    let cols = (0..w).filter(|&x| (0..h).any(|y| lit(x, y))).count();
    let rows = (0..h).filter(|&y| (0..w).any(|x| lit(x, y))).count();
    cols as f64 / rows.max(1) as f64
}

fn change(a: &[u8], b: &[u8]) -> f64 {
    let t: f64 = a
        .chunks_exact(4)
        .zip(b.chunks_exact(4))
        .map(|(x, y)| {
            (0..3)
                .map(|k| (x[k] as f64 - y[k] as f64).abs())
                .sum::<f64>()
                / 3.0
        })
        .sum();
    t / (a.len() / 4).max(1) as f64 / 255.0
}

fn shots(p: &Project) -> Vec<Clip> {
    let tl = p.timelines.values().next().unwrap();
    let v1 = tl
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Video)
        .unwrap();
    v1.items
        .iter()
        .filter_map(|i| i.as_clip().cloned())
        .collect()
}

fn find(p: &Project, id: Id) -> Clip {
    p.timelines
        .values()
        .flat_map(|t| t.tracks.iter())
        .flat_map(|t| t.items.iter())
        .find_map(|i| i.as_clip().filter(|c| c.id == id).cloned())
        .unwrap()
}

/// The effects of a title or layer added since `before` that carries `kind`, if there is one.
fn new_layer_with(before: &Project, after: &Project, kind: EffectKind) -> Option<Vec<Effect>> {
    let had: Vec<Id> = before
        .timelines
        .values()
        .flat_map(|t| t.tracks.iter())
        .flat_map(|t| t.items.iter())
        .map(|i| i.id())
        .collect();
    after
        .timelines
        .values()
        .flat_map(|t| t.tracks.iter())
        .flat_map(|t| t.items.iter())
        .filter(|i| !had.contains(&i.id()))
        .find_map(|i| match i {
            Item::Title(t) if t.effects.iter().any(|e| e.kind == kind) => Some(t.effects.clone()),
            _ => None,
        })
}

fn kinds(c: &Clip) -> Vec<EffectKind> {
    c.effects.iter().map(|e| e.kind).collect()
}

fn param(c: &Clip, k: EffectKind, key: &str) -> Option<f64> {
    let e = c.effects.iter().find(|e| e.kind == k)?;
    let i = k.params().iter().position(|d| param_key(d.name) == key)?;
    Some(e.params[i])
}

/// What to look for after a request.
enum Expect {
    /// exactly this effect added (on top of the set-up stack); the settings check; does the picture have to change
    Add(EffectKind, Option<Settings>, bool),
    /// a free-form check on the clip after
    Check(ClipCheck),
    /// this transition on the cut before the shot
    Transition(TransitionKind),
    /// this effect (on the shot or a new adjustment layer over it) and the visible picture of this width/height
    Visible(EffectKind, f64),
    /// a look asked for in plain words: this effect must be there and the picture change, and CoCaptain may add
    /// what usually goes with it (grain with old film, saturation with a miniature)
    Look(EffectKind),
}

type Settings = Box<dyn Fn(&Clip) -> (bool, String)>;
type ClipCheck = Box<dyn Fn(&Clip, &Clip) -> (bool, String)>;

struct Case {
    name: String,
    setup: Vec<Effect>,
    ask: String,
    expect: Expect,
}

fn add(kind: EffectKind, ask: &str) -> Case {
    Case {
        name: kind.key().to_string(),
        setup: Vec::new(),
        ask: ask.to_string(),
        expect: Expect::Add(kind, None, true),
    }
}

fn add_with(kind: EffectKind, ask: &str, visible: bool, s: Settings) -> Case {
    Case {
        name: kind.key().to_string(),
        setup: Vec::new(),
        ask: ask.to_string(),
        expect: Expect::Add(kind, Some(s), visible),
    }
}

fn look(kind: EffectKind, ask: &str) -> Case {
    Case {
        name: kind.key().to_string(),
        setup: Vec::new(),
        ask: ask.to_string(),
        expect: Expect::Look(kind),
    }
}

fn cases() -> Vec<Case> {
    use EffectKind as K;
    let mut v = vec![
        add(K::Blur, "Blur the selected shot softly."),
        add(K::Sharpen, "Sharpen the selected shot."),
        add(K::Vignette, "Darken the corners of the selected shot with a vignette."),
        add(K::Mirror, "Mirror the selected shot so one half reflects onto the other."),
        add_with(K::Crop, "Crop about 10% off the left and right of the selected shot and round its corners.", true, Box::new(|c| {
            let (l, r, round) = (param(c, K::Crop, "left").unwrap_or(0.0), param(c, K::Crop, "right").unwrap_or(0.0), param(c, K::Crop, "roundness").unwrap_or(0.0));
            (l > 5.0 && r > 5.0 && round > 0.0, format!("left {l:.0} right {r:.0} roundness {round:.0}"))
        })),
        // (this gym's artificial turf reads as yellow-green to the camera)
        add(K::ChromaKey, "Key out the yellow-green turf floor in the selected shot."),
        add(K::Pixelate, "Pixelate the selected shot like a mosaic."),
        add(K::Mask, "Show only an ellipse in the middle of the selected shot."),
        add(K::Glow, "Make the bright parts of the selected shot glow."),
        add(K::Grain, "Add film grain to the selected shot."),
        add(K::Aberration, "Give the selected shot an RGB split."),
        add(K::Duotone, "Give the selected shot a two-colour duotone look."),
        add(K::Invert, "Invert the colours of the selected shot, like a negative."),
        add(K::Posterize, "Posterize the selected shot."),
        // (the reel already has 2.39 cinema bars on an adjustment layer, so ask for a shape those do not hide)
        // judged on the picture: the visible part has to be square, whatever bars are already there
        Case {
            name: "letterbox".into(),
            setup: Vec::new(),
            ask: "Put bars on the sides of the selected shot so only a square (1:1) picture shows.".into(),
            expect: Expect::Visible(K::Letterbox, 1.0),
        },
        add_with(K::DirectionalBlur, "Add a horizontal motion-blur streak to the selected shot.", true, Box::new(|c| {
            let a = param(c, K::DirectionalBlur, "angle").unwrap_or(99.0);
            let horizontal = a.abs() < 20.0 || (a.abs() - 180.0).abs() < 20.0;
            (horizontal, format!("angle {a:.0}"))
        })),
        add(K::RadialBlur, "Add a spin blur to the selected shot."),
        add(K::ZoomBlur, "Add a zoom blur bursting out from the centre of the selected shot."),
        look(K::TiltShift, "Make the selected shot look like a miniature model."),
        add(K::Denoise, "Clean up the video noise in the selected shot."),
        add_with(K::BrightnessContrast, "Brighten the selected shot by about 20 with a brightness and contrast effect.", true, Box::new(|c| {
            let b = param(c, K::BrightnessContrast, "brightness").unwrap_or(0.0);
            (b > 5.0, format!("brightness {b:.0}"))
        })),
        add_with(K::Levels, "Use levels to crush the blacks of the selected shot a little.", true, Box::new(|c| {
            let b = param(c, K::Levels, "input_black").unwrap_or(0.0);
            (b > 0.0, format!("input black {b:.0}"))
        })),
        add(K::BlackWhite, "Turn the selected shot black and white."),
        add_with(K::ColorIsolate, "Keep only the reds in the selected shot and make everything else grey.", true, Box::new(|c| {
            let h = param(c, K::ColorIsolate, "hue").unwrap_or(-1.0);
            let red = !(30.0..=330.0).contains(&h);
            (red, format!("hue {h:.0}"))
        })),
        add_with(K::ChannelMixer, "Swap the red and blue channels of the selected shot.", true, Box::new(|c| {
            let rb = param(c, K::ChannelMixer, "red_from_blue").unwrap_or(0.0);
            let br = param(c, K::ChannelMixer, "blue_from_red").unwrap_or(0.0);
            (rb > 50.0 && br > 50.0, format!("red from blue {rb:.0}, blue from red {br:.0}"))
        })),
        add(K::Threshold, "Make the selected shot pure black and white, no greys."),
        add(K::LensDistortion, "Give the selected shot a fisheye-like barrel distortion."),
        add(K::Bulge, "Make the middle of the selected shot bulge out."),
        add(K::Twirl, "Twirl the centre of the selected shot."),
        add(K::Wave, "Add a wave distortion that makes the selected shot sway from side to side."),
        add(K::Turbulence, "Add a heat-haze wobble to the selected shot."),
        add_with(K::CornerPin, "Corner-pin the selected shot so its top edge is narrower than its bottom, like a trapezoid.", true, Box::new(|c| {
            let (tl, tr) = (param(c, K::CornerPin, "top_left_x").unwrap_or(0.0), param(c, K::CornerPin, "top_right_x").unwrap_or(100.0));
            (tl > 0.0 && tr < 100.0, format!("top left x {tl:.0}, top right x {tr:.0}"))
        })),
        add(K::Offset, "Slide the selected shot a quarter of the frame sideways, wrapping it around the edge."),
        add_with(K::Tile, "Repeat the selected shot in a 3 by 3 grid.", true, Box::new(|c| {
            let n = param(c, K::Tile, "count").unwrap_or(0.0);
            (n.round() == 3.0, format!("count {n}"))
        })),
        add(K::Gradient, "Lay a four-colour gradient over the selected shot."),
        add(K::Grid, "Draw a grid over the selected shot."),
        add_with(K::LensFlare, "Add a lens flare in the top left of the selected shot.", true, Box::new(|c| {
            let (x, y) = (param(c, K::LensFlare, "center_x").unwrap_or(99.0), param(c, K::LensFlare, "center_y").unwrap_or(99.0));
            (x < 50.0 && y < 50.0, format!("at {x:.0}%, {y:.0}%"))
        })),
        add(K::LightRays, "Add light rays streaming from the top of the selected shot."),
        add(K::LightLeak, "Add a warm light leak to the selected shot."),
        add(K::LumaKey, "Make the dark parts of the selected shot transparent."),
        look(K::FilmDamage, "Make the selected shot look like old, damaged film."),
        look(K::Analog, "Give the selected shot a worn VHS tape look."),
        add_with(K::DropShadow, "Add a drop shadow to the selected shot.", false, Box::new(|c| {
            let o = param(c, K::DropShadow, "opacity").unwrap_or(0.0);
            (o > 0.0, format!("opacity {o:.0}"))
        })),
        add(K::Perspective, "Turn the selected shot in 3D, swivelled a little to one side."),
        add(K::FindEdges, "Show only the outlines in the selected shot."),
        add(K::Emboss, "Emboss the selected shot."),
        add(K::Halftone, "Give the selected shot a comic-book halftone dot look."),
        add(K::Scanlines, "Add old-TV scanlines to the selected shot."),
        add(K::Cartoon, "Make the selected shot look like a cartoon."),
        add(K::Sketch, "Make the selected shot look like a pencil drawing."),
        add(K::Paint, "Make the selected shot look like an oil painting."),
        add(K::Glitch, "Add a digital glitch to the selected shot."),
        add_with(K::Strobe, "Make the selected shot flash with a strobe.", false, Box::new(|c| {
            let r = param(c, K::Strobe, "rate").unwrap_or(0.0);
            (r > 0.0, format!("rate {r:.1} Hz"))
        })),
        add_with(K::LowFrameRate, "Make the selected shot choppy, like stop motion at 8 frames a second.", false, Box::new(|c| {
            let f = param(c, K::LowFrameRate, "frame_rate").unwrap_or(0.0);
            ((f - 8.0).abs() < 0.5, format!("frame rate {f:.0}"))
        })),
        add(K::CameraShake, "Add a continuous hand-held camera shake to the selected shot."),
    ];
    // working with effects already there
    v.push(Case {
        name: "edit: stronger glow".into(),
        setup: vec![Effect::new(K::Glow)],
        ask: "Make the glow on the selected shot much stronger.".into(),
        expect: Expect::Check(Box::new(|_, a| {
            let i = param(a, K::Glow, "intensity").unwrap_or(0.0);
            (
                kinds(a) == vec![K::Glow] && i > 80.0,
                format!("effects {:?}, intensity {i:.0}", kinds(a)),
            )
        })),
    });
    v.push(Case {
        name: "animate an effect".into(),
        setup: vec![{
            let mut e = Effect::new(K::ZoomBlur);
            e.params[0] = 0.0;
            e
        }],
        ask: "Animate the zoom blur on the selected shot: none at its start, strong (about 60) one second in.".into(),
        expect: Expect::Check(Box::new(|_, a| {
            let keys = a.anim.keys("fx.zoom_blur.amount");
            let ok = keys.len() >= 2 && keys.first().is_some_and(|k| k.v < 10.0) && keys.iter().any(|k| k.v > 40.0);
            (ok && kinds(a) == vec![K::ZoomBlur], format!("{} keys {:?}", keys.len(), keys.iter().map(|k| k.v).collect::<Vec<_>>()))
        })),
    });
    v.push(Case {
        name: "remove one".into(),
        setup: vec![Effect::new(K::Glow), Effect::new(K::CameraShake)],
        ask: "Take the camera shake off the selected shot, keep everything else.".into(),
        expect: Expect::Check(Box::new(|_, a| {
            (kinds(a) == vec![K::Glow], format!("effects {:?}", kinds(a)))
        })),
    });
    v.push(Case {
        name: "stack a look".into(),
        setup: Vec::new(),
        ask: "Give the selected shot a vintage look: old film damage, grain and a vignette.".into(),
        expect: Expect::Check(Box::new(|_, a| {
            let k = kinds(a);
            let n = [K::FilmDamage, K::Grain, K::Vignette]
                .iter()
                .filter(|x| k.contains(x))
                .count();
            (n >= 2, format!("effects {k:?}"))
        })),
    });
    for (t, ask) in [
        (TransitionKind::Flash, "a flash"),
        (TransitionKind::DipToWhite, "a dip to white"),
        (TransitionKind::BlurDissolve, "a blur dissolve"),
        (TransitionKind::CrossZoom, "a cross zoom"),
        (TransitionKind::Spin, "a spin"),
        (TransitionKind::ClockWipe, "a clock wipe"),
        (
            TransitionKind::BarnDoors,
            "a barn-doors (opening from the middle)",
        ),
        (TransitionKind::Glitch, "a glitch"),
        (TransitionKind::LightLeak, "a light-leak"),
        (TransitionKind::LumaFade, "a luma fade"),
    ] {
        v.push(Case {
            name: format!("transition {}", t.key()),
            setup: Vec::new(),
            ask: format!("Put {ask} transition on the cut before the selected shot."),
            expect: Expect::Transition(t),
        });
    }
    v
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let proj = PathBuf::from(
        args.get(1)
            .ok_or("usage: cocaptain_effects_eval <project.json> [--shot N] [--only NAME]")?,
    );
    let opt = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let shot_no: usize = opt("--shot").and_then(|v| v.parse().ok()).unwrap_or(2);
    let only = opt("--only");
    let dir = proj
        .parent()
        .unwrap_or(Path::new("."))
        .join("cocaptain_logs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    let inner: Box<dyn Transport + Sync> = match Credentials::load().filter(|k| !k.is_empty()) {
        Some(key) => Box::new(AnthropicTransport::new(key)),
        None => Box::new(
            ClaudeCli::find()
                .ok_or("CoCaptain cannot reach Claude: no key and no `claude` command")?,
        ),
    };
    let transport = Logged {
        inner,
        log: Mutex::new(
            std::fs::File::create(dir.join(format!("effects_eval-{stamp}.jsonl")))
                .map_err(|e| e.to_string())?,
        ),
    };

    let mut session = Session::load(&proj)?;
    session.commit(Some("Save before: CoCaptain effects test"))?;
    let branch = session.unique_branch_name("test/cocaptain-effects");
    session.create_branch(&branch, true)?;
    session.save(&proj)?;
    println!("branch {branch}");

    let all = shots(session.editor.project());
    let target = all.get(shot_no - 1).ok_or("no such shot")?.id;
    let tl = *session.editor.project().timelines.keys().next().unwrap();
    let start_of = |p: &Project| -> i64 {
        let t = p.timeline(tl).unwrap();
        let v = t
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Video)
            .unwrap();
        v.items
            .iter()
            .zip(v.item_starts())
            .find(|(i, _)| i.id() == target)
            .map(|(_, s)| s)
            .unwrap()
    };

    let mut rows = Vec::new();
    for case in cases() {
        if only.as_deref().is_some_and(|o| o != case.name) {
            continue;
        }
        // set the shot up: the case's starting effects, no effect keyframes, no transition
        let shot = find(session.editor.project(), target);
        let mut setup = vec![Op::SetEffects {
            item: target,
            effects: case.setup.clone(),
        }];
        for prop in shot.anim.0.keys().filter(|k| k.starts_with("fx.")) {
            setup.push(Op::SetAnimTrack {
                item: target,
                prop: prop.clone(),
                keys: Vec::new(),
            });
        }
        if shot.transition.is_some() {
            setup.push(Op::SetTransition {
                item: target,
                transition: None,
            });
        }
        session.apply_as(
            Author::User,
            Op::Transaction {
                description: format!("Test set-up: {}", case.name),
                ops: setup,
            },
        )?;
        let before_p = session.editor.project().clone();
        let before = find(&before_p, target);
        let mid = start_of(&before_p) + before.duration.value / 2;
        let pb = pixels(&before_p, tl, mid);
        let req = AgentRequest {
            project: &before_p,
            selection: Some(target),
            playhead_secs: mid as f64 * tl_secs(&before_p, tl),
            history: &[],
            text: &case.ask,
            base_ops: &[],
            images: &[],
            files: &[],
        };
        let t = std::time::Instant::now();
        let mut steps = Vec::new();
        let res = captaine_agent::run(
            &transport,
            &AgentConfig::default(),
            &req,
            &mut |e| match e {
                Event::Step(s) => steps.push(s),
                Event::Picture(_) => {}
            },
        );
        let secs = t.elapsed().as_secs_f64();
        let (pass, seen, reply, n_ops) = match res {
            Ok(r) => {
                for op in &r.new_ops {
                    session.apply_as(Author::Agent("copilot".into()), op.clone())?;
                }
                session.commit(Some(&format!("CoCaptain test: {}", case.name)))?;
                session.save(&proj)?;
                let after_p = session.editor.project().clone();
                let after = find(&after_p, target);
                let pa = pixels(&after_p, tl, mid);
                let moved = change(&pb, &pa);
                let (mut ok, mut seen) = match &case.expect {
                    Expect::Add(kind, settings, visible) => {
                        let mut want = kinds(&before);
                        want.push(*kind);
                        let mut got = kinds(&after);
                        let mut seen = String::new();
                        // bars and other frame-wide looks may go on a new adjustment layer over the shot instead
                        if got != want && got == kinds(&before) {
                            if let Some(layer) = new_layer_with(&before_p, &after_p, *kind) {
                                seen += "on a new adjustment layer over the shot; ";
                                got = want.clone();
                                if let Some(s) = settings {
                                    let mut stand_in = after.clone();
                                    stand_in.effects = layer;
                                    let (o, txt) = s(&stand_in);
                                    seen += &format!("{txt}; ");
                                    if !o {
                                        got.clear();
                                    }
                                }
                            }
                        }
                        let mut ok = got == want;
                        seen += &format!("effects {got:?}");
                        if let Some(s) = settings {
                            let (o, txt) = s(&after);
                            ok &= o;
                            seen += &format!("; {txt}");
                        }
                        if *visible {
                            ok &= moved > 0.002;
                            seen += &format!("; picture changed {moved:.4}");
                        }
                        (ok, seen)
                    }
                    Expect::Check(f) => f(&before, &after),
                    Expect::Visible(kind, want) => {
                        let on = kinds(&after).contains(kind)
                            || new_layer_with(&before_p, &after_p, *kind).is_some();
                        let (w, h) = canvas_px(&after_p, tl);
                        let a = visible_aspect(&pa, w, h);
                        (
                            on && (a - want).abs() < 0.06,
                            format!("{} on; visible picture {a:.3} wide per 1 high", kind.key()),
                        )
                    }
                    Expect::Look(kind) => {
                        let got = kinds(&after);
                        (
                            got.contains(kind) && moved > 0.002,
                            format!("effects {got:?}; picture changed {moved:.4}"),
                        )
                    }
                    Expect::Transition(k) => {
                        let got = after.transition.map(|t| t.kind);
                        (got == Some(*k), format!("transition {got:?}"))
                    }
                };
                // the shot's other properties stay as they were
                if after.video != before.video && !matches!(case.expect, Expect::Look(_)) {
                    ok = false;
                    seen += "; ALSO CHANGED: the shot's picture settings";
                }
                if !matches!(case.expect, Expect::Transition(_))
                    && after.transition != before.transition
                {
                    ok = false;
                    seen += "; ALSO CHANGED: the transition";
                }
                (ok, seen, r.reply, r.new_ops.len())
            }
            Err(e) => (false, format!("CoCaptain error: {e:?}"), String::new(), 0),
        };
        println!(
            "{} {:<24} {:>5.1}s  {}  | {}",
            if pass { "PASS" } else { "FAIL" },
            case.name,
            secs,
            seen,
            steps.join("; ")
        );
        rows.push(json!({"case": case.name, "asked": case.ask, "pass": pass, "seen": seen, "steps": steps, "edits": n_ops, "seconds": (secs * 10.0).round() / 10.0, "reply": reply}));
    }
    let passed = rows.iter().filter(|r| r["pass"] == true).count();
    let mut md = format!(
        "# CoCaptain effects test\n\nProject `{}`, branch `{branch}`, shot {shot_no}. **{passed} of {} passed.**\n\n| # | Case | Asked | Result | What was measured | CoCaptain's steps |\n|---|---|---|---|---|---|\n",
        proj.display(),
        rows.len()
    );
    for (i, r) in rows.iter().enumerate() {
        md += &format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            i + 1,
            r["case"].as_str().unwrap_or(""),
            r["asked"].as_str().unwrap_or("").replace('|', "/"),
            if r["pass"] == true {
                "PASS"
            } else {
                "**FAIL**"
            },
            r["seen"].as_str().unwrap_or("").replace('|', "/"),
            r["steps"]
                .as_array()
                .map(|s| s
                    .iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join("; "))
                .unwrap_or_default()
                .replace('|', "/"),
        );
    }
    let md_path = dir.join(format!("effects_eval-{stamp}.md"));
    std::fs::write(&md_path, md).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join(format!("effects_eval-{stamp}.json")),
        serde_json::to_string_pretty(&rows).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "\n{passed} of {} passed; report {}",
        rows.len(),
        md_path.display()
    );
    Ok(())
}

fn tl_secs(p: &Project, tl: Id) -> f64 {
    let r = p.timeline(tl).unwrap().rate;
    r.den() as f64 / r.num() as f64
}
