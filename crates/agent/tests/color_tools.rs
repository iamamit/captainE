//! Every Color tool, one by one, through the same tools CoCaptain uses: each test checks that the clip's stored
//! value changed and that the rendered picture moved the right way (brighter, warmer, less saturated...).
//! Footage is generated (FFmpeg's testsrc2), so the tests need FFmpeg and skip without it.

use std::path::{Path, PathBuf};
use std::process::Command;

use captaine_agent::ToolBox;
use captaine_media::media_ref;
use captaine_model::*;
use captaine_ops::*;
use captaine_render::{Frame8, RenderOptions, Renderer};
use captaine_time::{Rational, RationalTime};
use serde_json::{json, Value};

const RATE: Rational = Rational::FPS_24;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("captaine-color-tools-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Colourful moving test footage, toned down to the mid-tones of real camera footage (every hue, nothing pinned at
/// pure black, white or full saturation, so every tool has room to move it).
/// Made once per test run: the tests run in parallel and must not read the file while another one writes it.
fn footage(path: &Path) {
    static MADE: std::sync::Once = std::sync::Once::new();
    MADE.call_once(|| make_footage(path));
}

fn make_footage(path: &Path) {
    assert!(Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x180:r=24:d=2,eq=contrast=0.55:saturation=0.45"
        ])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv444p", "-crf", "6"])
        .arg(path)
        .status()
        .unwrap()
        .success());
}

/// Two shots on V1 (the same footage, a second apart) and the ids of the timeline and both clips.
struct Fx {
    project: Project,
    tl: Id,
    a: Id,
    b: Id,
}

fn fixture() -> Fx {
    let src = dir().join("colour-mid.mp4");
    footage(&src);
    fixture_of(&src)
}

/// Two shots of `src` back to back on V1.
fn fixture_of(src: &Path) -> Fx {
    let m = media_ref(src, Id::new()).unwrap();
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: RATE,
        width: 320,
        height: 180,
        sample_rate: 48_000,
    }));
    ed.apply(Author::User, Op::AddMedia { media: m.clone() })
        .unwrap();
    let (tl, v1) = (Id::new(), Id::new());
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "t", RATE),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: tl,
            index: 0,
            track: Track::new(v1, "V1", TrackKind::Video),
        },
    )
    .unwrap();
    let (a, b) = (Id::new(), Id::new());
    for (id, from) in [(a, 0), (b, 24)] {
        let c = Clip::new(
            id,
            "shot",
            m.id,
            RationalTime::new(from, RATE),
            RationalTime::new(24, RATE),
        );
        ed.apply(Author::User, edit::append(ed.project(), v1, c))
            .unwrap();
    }
    Fx {
        project: ed.project().clone(),
        tl,
        a,
        b,
    }
}

/// What a rendered frame looks like, in display values 0..1.
#[derive(Clone)]
struct Look {
    luma: f64,
    rgb: [f64; 3],
    sat: f64,
    low: f64,
    high: f64,
    /// the frame itself (RGBA), for per-pixel comparisons
    px: Vec<u8>,
}

impl std::fmt::Debug for Look {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "luma {:.4} rgb [{:.4} {:.4} {:.4}] sat {:.4} low {:.4} high {:.4}",
            self.luma, self.rgb[0], self.rgb[1], self.rgb[2], self.sat, self.low, self.high
        )
    }
}

/// Average per-pixel colour change between two frames (0..1).
fn change(a: &Look, b: &Look) -> f64 {
    let total: f64 =
        a.px.chunks_exact(4)
            .zip(b.px.chunks_exact(4))
            .map(|(x, y)| {
                (0..3)
                    .map(|k| (x[k] as f64 - y[k] as f64).abs())
                    .sum::<f64>()
                    / 3.0
            })
            .sum();
    total / (a.px.len() / 4) as f64 / 255.0
}

fn look(p: &Project, tl: Id, tick: i64) -> Look {
    let c = Renderer::new(p, tl, RenderOptions::default())
        .unwrap()
        .render(tick)
        .unwrap();
    let f = Frame8 {
        width: c.width,
        height: c.height,
        rgba: c.to_rgba8(),
    };
    let mut rgb = [0.0; 3];
    let (mut sat, mut lum) = (0.0, Vec::new());
    for px in f.rgba.chunks_exact(4) {
        let v = [
            px[0] as f64 / 255.0,
            px[1] as f64 / 255.0,
            px[2] as f64 / 255.0,
        ];
        for k in 0..3 {
            rgb[k] += v[k];
        }
        let (mx, mn) = (v[0].max(v[1]).max(v[2]), v[0].min(v[1]).min(v[2]));
        sat += if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
        lum.push(0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]);
    }
    let n = lum.len() as f64;
    lum.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Look {
        px: f.rgba.clone(),
        luma: lum.iter().sum::<f64>() / n,
        rgb: rgb.map(|v| v / n),
        sat: sat / n,
        low: lum[(n * 0.05) as usize],
        high: lum[(n * 0.95) as usize],
    }
}

/// Run one tool on a fresh copy; returns the project after it and the clip.
fn run(fx: &Fx, tool: &str, input: Value) -> (Project, Clip) {
    let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
    tb.call(tool, &input)
        .unwrap_or_else(|e| panic!("{tool} failed: {e}"));
    assert!(!tb.new_ops.is_empty(), "{tool} changed nothing");
    let p = tb.project().clone();
    let clip = clip_of(&p, fx.a);
    (p, clip)
}

fn clip_of(p: &Project, id: Id) -> Clip {
    p.timelines
        .values()
        .flat_map(|t| t.tracks.iter())
        .flat_map(|t| t.items.iter())
        .find_map(|i| match i {
            Item::Clip(c) if c.id == id => Some(c.clone()),
            _ => None,
        })
        .unwrap()
}

/// The middle of shot A.
const MID: i64 = 12;

macro_rules! color_test {
    ($name:ident, $tool:expr, $input:expr, |$before:ident, $after:ident, $clip:ident| $check:expr) => {
        #[test]
        fn $name() {
            if !have_ffmpeg() {
                return;
            }
            let fx = fixture();
            let $before = look(&fx.project, fx.tl, MID);
            let mut input: Value = $input;
            input["item_id"] = json!(fx.a.to_string());
            let (p, $clip) = run(&fx, $tool, input);
            let $after = look(&p, fx.tl, MID);
            assert!($check, "before {:?}\nafter  {:?}", $before, $after);
        }
    };
}

// ---- primaries ------------------------------------------------------------------------------------------------

color_test!(
    exposure_brightens,
    "set_clip",
    json!({"exposure": 0.7}),
    |b, a, c| { c.video.color.exposure == 0.7 && a.luma > b.luma + 0.05 }
);
color_test!(
    contrast_spreads_the_tones,
    "set_clip",
    json!({"contrast": 1.6}),
    |b, a, c| { c.video.color.contrast == 1.6 && (a.high - a.low) > (b.high - b.low) + 0.03 }
);
color_test!(
    saturation_drains_colour,
    "set_clip",
    json!({"saturation": 30}),
    |b, a, c| { c.video.color.saturation == 30.0 && a.sat < b.sat - 0.1 }
);
color_test!(
    temperature_warms,
    "set_clip",
    json!({"temperature_k": 9500}),
    |b, a, c| {
        c.video.color.temperature == 9500.0 && (a.rgb[0] - a.rgb[2]) > (b.rgb[0] - b.rgb[2]) + 0.02
    }
);
color_test!(
    tint_adds_magenta,
    "set_clip",
    json!({"tint": 70}),
    |b, a, c| { c.video.color.tint == 70.0 && a.rgb[1] < b.rgb[1] - 0.01 }
);
color_test!(
    highlights_pull_down_the_brights,
    "set_clip",
    json!({"highlights": -90}),
    |b, a, c| { c.video.color.highlights == -90.0 && a.high < b.high - 0.02 }
);
color_test!(
    shadows_open_up_the_darks,
    "set_clip",
    json!({"shadows": 90}),
    |b, a, c| { c.video.color.shadows == 90.0 && a.low > b.low + 0.01 }
);
color_test!(
    vibrance_boosts_colour,
    "set_clip",
    json!({"vibrance": 90}),
    |b, a, c| { c.video.color.vibrance == 90.0 && a.sat > b.sat + 0.01 }
);

// ---- wheels -----------------------------------------------------------------------------------------------------

fn effect(c: &Clip, k: EffectKind) -> &Effect {
    c.effects
        .iter()
        .find(|e| e.kind == k)
        .expect("effect on the clip")
}

color_test!(
    lift_wheel_pushes_shadows_blue,
    "set_effects",
    json!({"effects": [{"kind": "color_wheels", "params": {"lift_blue": 0.4}}]}),
    |b, a, c| effect(&c, EffectKind::ColorWheels).params[3] == 0.4 && a.rgb[2] > b.rgb[2] + 0.02
);
color_test!(
    gamma_wheel_pushes_mids_red,
    "set_effects",
    json!({"effects": [{"kind": "color_wheels", "params": {"gamma_red": 0.5}}]}),
    |b, a, c| effect(&c, EffectKind::ColorWheels).params[5] == 0.5 && a.rgb[0] > b.rgb[0] + 0.02
);
color_test!(
    gain_master_darkens,
    "set_effects",
    json!({"effects": [{"kind": "color_wheels", "params": {"gain_master": -0.4}}]}),
    |b, a, c| effect(&c, EffectKind::ColorWheels).params[8] == -0.4 && a.luma < b.luma - 0.05
);

// ---- curves -----------------------------------------------------------------------------------------------------

color_test!(
    master_s_curve_adds_contrast,
    "set_effects",
    json!({"effects": [{"kind": "curves", "params": {"master_25%": 0.12, "master_75%": 0.88}}]}),
    |b, a, c| {
        let e = effect(&c, EffectKind::Curves);
        e.params[1] == 0.12 && e.params[3] == 0.88 && (a.high - a.low) > (b.high - b.low) + 0.02
    }
);
color_test!(
    red_curve_lifts_red,
    "set_effects",
    json!({"effects": [{"kind": "curves", "params": {"red_50%": 0.75}}]}),
    |b, a, c| effect(&c, EffectKind::Curves).params[7] == 0.75 && a.rgb[0] > b.rgb[0] + 0.03
);
color_test!(
    green_curve_lowers_green,
    "set_effects",
    json!({"effects": [{"kind": "curves", "params": {"green_50%": 0.25}}]}),
    // green goes down, red and blue are untouched: the curve works on its own channel only
    |b, a, c| effect(&c, EffectKind::Curves).params[12] == 0.25
        && a.rgb[1] < b.rgb[1] - 0.015
        && a.rgb[0] == b.rgb[0]
        && a.rgb[2] == b.rgb[2]
);
color_test!(
    blue_curve_lifts_blue,
    "set_effects",
    json!({"effects": [{"kind": "curves", "params": {"blue_50%": 0.75}}]}),
    |b, a, c| effect(&c, EffectKind::Curves).params[17] == 0.75 && a.rgb[2] > b.rgb[2] + 0.03
);

// ---- HSL ----------------------------------------------------------------------------------------------------------

color_test!(
    hsl_saturation_drains_every_band,
    "set_effects",
    json!({"effects": [{"kind": "hsl", "params": {"red_sat": 0, "yellow_sat": 0, "green_sat": 0, "cyan_sat": 0, "blue_sat": 0, "magenta_sat": 0}}]}),
    |b, a, c| effect(&c, EffectKind::Hsl).params[1] == 0.0 && a.sat < b.sat - 0.1
);
color_test!(
    hsl_lightness_darkens_every_band,
    "set_effects",
    json!({"effects": [{"kind": "hsl", "params": {"red_lum": -50, "yellow_lum": -50, "green_lum": -50, "cyan_lum": -50, "blue_lum": -50, "magenta_lum": -50}}]}),
    |b, a, c| effect(&c, EffectKind::Hsl).params[2] == -50.0 && a.luma < b.luma - 0.03
);
color_test!(
    hsl_hue_shifts_colours,
    "set_effects",
    json!({"effects": [{"kind": "hsl", "params": {"red_hue": 50, "yellow_hue": 50, "green_hue": 50, "cyan_hue": 50, "blue_hue": 50, "magenta_hue": 50}}]}),
    // turning every hue changes the colours pixel by pixel (the frame's average barely moves: it all rotates)
    |b, a, c| effect(&c, EffectKind::Hsl).params[0] == 50.0 && change(&a, &b) > 0.03
);

// ---- LUT ------------------------------------------------------------------------------------------------------------

/// A 2x2x2 3D LUT that keeps red and green and halves blue.
fn half_blue_lut() -> PathBuf {
    let p = dir().join("half_blue.cube");
    let mut s = String::from("TITLE \"half blue (test)\"\nLUT_3D_SIZE 2\n");
    for b in [0.0, 1.0] {
        for g in [0.0, 1.0] {
            for r in [0.0, 1.0] {
                s += &format!("{r:.1} {g:.1} {:.1}\n", b * 0.5);
            }
        }
    }
    std::fs::write(&p, s).unwrap();
    p
}

#[test]
fn lut_changes_the_picture() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    let before = look(&fx.project, fx.tl, MID);
    let lut = half_blue_lut();
    let (p, c) = run(
        &fx,
        "set_effects",
        json!({"item_id": fx.a.to_string(), "effects": [{"kind": "lut", "path": lut.to_string_lossy(), "params": {"strength": 100}}]}),
    );
    let after = look(&p, fx.tl, MID);
    assert_eq!(effect(&c, EffectKind::Lut).text, lut.to_string_lossy());
    assert!(
        after.rgb[2] < before.rgb[2] - 0.05,
        "before {before:?}\nafter  {after:?}"
    );
}

/// The live CoCaptain run sent the LUT file inside params; that must work too.
#[test]
fn lut_path_inside_params_is_accepted() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    let lut = half_blue_lut();
    let (_, c) = run(
        &fx,
        "edit_effect",
        json!({"item_id": fx.a.to_string(), "kind": "lut", "params": {"path": lut.to_string_lossy(), "strength": 100}}),
    );
    assert_eq!(effect(&c, EffectKind::Lut).text, lut.to_string_lossy());
}

// ---- animated colour, auto balance --------------------------------------------------------------------------------

#[test]
fn exposure_keyframes_brighten_over_the_shot() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    let (p, c) = run(
        &fx,
        "set_keyframes",
        json!({"item_id": fx.a.to_string(), "property": "exposure", "keys": [{"at_sec": 0.0, "value": -1.0}, {"at_sec": 0.9, "value": 1.0}]}),
    );
    assert!(c.anim.has("exposure"));
    let (early, late) = (look(&p, fx.tl, 1), look(&p, fx.tl, 22));
    assert!(
        late.luma > early.luma + 0.1,
        "early {early:?}\nlate {late:?}"
    );
}

/// Found by the live CoCaptain run: exposure keyframes left from an earlier edit silently overrode a new fixed
/// exposure. CoCaptain now sees keyframes in get_timeline, set_clip warns, and reset_grade removes them.
#[test]
fn colour_keyframes_are_visible_warned_about_and_reset() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    let untouched = look(&fx.project, fx.tl, MID);
    let id = fx.a.to_string();
    let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
    tb.call(
        "set_keyframes",
        &json!({"item_id": id, "property": "exposure", "keys": [{"at_sec": 0.0, "value": -1.0}, {"at_sec": 0.5, "value": 1.0}]}),
    )
    .unwrap();
    let timeline = tb.call("get_timeline", &json!({})).unwrap();
    assert!(
        timeline.contains(
            r#""keyframes":{"exposure":[{"at_sec":0.0,"value":-1.0},{"at_sec":0.5,"value":1.0}]}"#
        ),
        "{timeline}"
    );
    let out = tb
        .call("set_clip", &json!({"item_id": id, "exposure": 0.5}))
        .unwrap();
    assert!(out.contains("exposure is keyframed"), "{out}");
    tb.call("reset_grade", &json!({"item_id": id})).unwrap();
    let c = clip_of(tb.project(), fx.a);
    assert!(!c.anim.has("exposure"), "reset removes colour keyframes");
    assert!(change(&untouched, &look(tb.project(), fx.tl, MID)) < 1e-6);
}

#[test]
fn auto_color_sets_a_balanced_grade() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    // make the shot dark and blue first, then ask for a balance
    let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
    tb.call(
        "set_clip",
        &json!({"item_id": fx.a.to_string(), "exposure": -1.5, "temperature_k": 3500}),
    )
    .unwrap();
    let dark = look(tb.project(), fx.tl, MID);
    tb.call("auto_color", &json!({"item_id": fx.a.to_string()}))
        .unwrap();
    let c = clip_of(tb.project(), fx.a);
    assert_ne!(
        c.video.color,
        ColorProps {
            exposure: -1.5,
            temperature: 3500.0,
            ..ColorProps::default()
        }
    );
    let fixed = look(tb.project(), fx.tl, MID);
    assert!(fixed.luma > dark.luma, "dark {dark:?}\nfixed {fixed:?}");
}

// ---- copy, paste to all, reset --------------------------------------------------------------------------------------

#[test]
fn copy_grade_gives_another_shot_the_same_look_and_keeps_its_other_effects() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
    tb.call(
        "set_clip",
        &json!({"item_id": fx.a.to_string(), "exposure": 0.5, "saturation": 60}),
    )
    .unwrap();
    tb.call("set_effects", &json!({"item_id": fx.a.to_string(), "effects": [{"kind": "curves", "params": {"master_75%": 0.9}}]}))
        .unwrap();
    tb.call("set_effects", &json!({"item_id": fx.b.to_string(), "effects": [{"kind": "blur", "params": {"radius": 3}}]})).unwrap();
    tb.call(
        "copy_grade",
        &json!({"from_item_id": fx.a.to_string(), "to_item_ids": [fx.b.to_string()]}),
    )
    .unwrap();
    let (a, b) = (clip_of(tb.project(), fx.a), clip_of(tb.project(), fx.b));
    assert_eq!(Grade::of(&a), Grade::of(&b));
    assert!(
        b.effects.iter().any(|e| e.kind == EffectKind::Blur),
        "the blur stays on shot B"
    );
}

#[test]
fn copy_grade_to_all_shots() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
    tb.call(
        "set_clip",
        &json!({"item_id": fx.b.to_string(), "temperature_k": 8000}),
    )
    .unwrap();
    let out = tb
        .call(
            "copy_grade",
            &json!({"from_item_id": fx.b.to_string(), "all_shots": true}),
        )
        .unwrap();
    assert!(out.contains("\"changed\":1"), "{out}");
    assert_eq!(clip_of(tb.project(), fx.a).video.color.temperature, 8000.0);
}

#[test]
fn reset_grade_brings_back_the_untouched_picture() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    let untouched = look(&fx.project, fx.tl, MID);
    let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
    tb.call(
        "set_clip",
        &json!({"item_id": fx.a.to_string(), "exposure": 1.0, "tint": 40}),
    )
    .unwrap();
    tb.call("set_effects", &json!({"item_id": fx.a.to_string(), "effects": [{"kind": "hsl", "params": {"red_sat": 0}}, {"kind": "blur", "params": {"radius": 2}}]}))
        .unwrap();
    tb.call("reset_grade", &json!({"item_id": fx.a.to_string()}))
        .unwrap();
    let c = clip_of(tb.project(), fx.a);
    assert!(Grade::of(&c).is_neutral());
    assert!(
        c.effects.iter().any(|e| e.kind == EffectKind::Blur),
        "non-colour effects stay"
    );
    // without the blur the picture is exactly the untouched one again
    tb.call(
        "set_effects",
        &json!({"item_id": fx.a.to_string(), "effects": []}),
    )
    .unwrap();
    let back = look(tb.project(), fx.tl, MID);
    assert!((back.luma - untouched.luma).abs() < 1e-9 && (back.sat - untouched.sat).abs() < 1e-9);
}

#[test]
fn edit_effect_changes_one_effect_and_keeps_the_rest() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    let a = fx.a.to_string();
    let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
    tb.call(
        "edit_effect",
        &json!({"item_id": a, "kind": "blur", "params": {"radius": 2}}),
    )
    .unwrap();
    tb.call(
        "edit_effect",
        &json!({"item_id": a, "kind": "color_wheels", "params": {"lift_blue": 0.2}}),
    )
    .unwrap();
    tb.call(
        "edit_effect",
        &json!({"item_id": a, "kind": "color_wheels", "params": {"gamma_red": 0.3}}),
    )
    .unwrap();
    tb.call(
        "edit_effect",
        &json!({"item_id": a, "kind": "curves", "params": {"master_75%": 0.85}}),
    )
    .unwrap();
    let c = clip_of(tb.project(), fx.a);
    let kinds: Vec<EffectKind> = c.effects.iter().map(|e| e.kind).collect();
    // colour effects in front, in the order added; the blur stays at the end
    assert_eq!(
        kinds,
        [
            EffectKind::ColorWheels,
            EffectKind::Curves,
            EffectKind::Blur
        ]
    );
    let w = effect(&c, EffectKind::ColorWheels);
    assert_eq!(
        (w.params[3], w.params[5]),
        (0.2, 0.3),
        "the second wheel change kept the first"
    );
    tb.call(
        "edit_effect",
        &json!({"item_id": a, "kind": "curves", "remove": true}),
    )
    .unwrap();
    let c = clip_of(tb.project(), fx.a);
    assert!(c.effects.iter().all(|e| e.kind != EffectKind::Curves));
    assert!(tb
        .call(
            "edit_effect",
            &json!({"item_id": a, "kind": "curves", "remove": true})
        )
        .is_err());
}

// ---- the effects library ---------------------------------------------------------------------------------------

/// Every picture effect in the library, added through edit_effect the way CoCaptain adds it, changes the rendered
/// clip. Effects that start neutral (like any adjustment) are given a setting; drop shadow needs a smaller layer
/// to show around, and low frame rate shows on a frame between two held ones.
#[test]
fn every_library_effect_changes_the_rendered_clip() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    let id = fx.a.to_string();
    let small = {
        let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
        tb.call("set_clip", &json!({"item_id": id, "scale_percent": 60}))
            .unwrap();
        tb.project().clone()
    };
    let mut weak = Vec::new();
    for kind in EffectKind::ALL.into_iter().filter(|k| !k.is_audio()) {
        if matches!(kind, EffectKind::Matte | EffectKind::Lut) {
            continue; // need a matte video or a .cube file: covered by their own tests
        }
        let params = match kind {
            EffectKind::BrightnessContrast => json!({"brightness": 30, "contrast": 20}),
            EffectKind::Levels => json!({"input_black": 40}),
            EffectKind::ChannelMixer => json!({"red_from_green": 50}),
            EffectKind::CornerPin => json!({"top_left_x": 20}),
            EffectKind::Offset => json!({"x": 25}),
            EffectKind::Crop => json!({"left": 10, "roundness": 20}),
            EffectKind::ColorWheels => json!({"gain_master": 0.2}),
            EffectKind::Curves => json!({"master_50%": 0.7}),
            EffectKind::Hsl => json!({"red_sat": 0}),
            // the footage has no deep shadows, and the canvas is black
            EffectKind::LumaKey => json!({"threshold": 50}),
            EffectKind::DropShadow => {
                json!({"red": 255, "green": 255, "blue": 255, "distance": 60})
            }
            _ => json!({}),
        };
        let base = if kind == EffectKind::DropShadow {
            small.clone()
        } else {
            fx.project.clone()
        };
        let tick = if kind == EffectKind::LowFrameRate {
            MID + 1
        } else {
            MID
        };
        let before = look(&base, fx.tl, tick);
        let mut tb = ToolBox::new(&base, &[], None, 0.0).unwrap();
        tb.call(
            "edit_effect",
            &json!({"item_id": id, "kind": kind.key(), "params": params}),
        )
        .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        let after = look(tb.project(), fx.tl, tick);
        let c = change(&before, &after);
        if c < 0.002 {
            weak.push(format!("{kind:?} {c:.5}"));
        }
    }
    assert!(weak.is_empty(), "no visible change: {weak:?}");
}

#[test]
fn effect_settings_can_be_keyframed_through_the_tools() {
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    let id = fx.a.to_string();
    let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
    // animating an effect that is not there says what to do
    let err = tb
        .call(
            "set_keyframes",
            &json!({"item_id": id, "property": "fx.zoom_blur.amount", "keys": [{"at_sec": 0, "value": 0}]}),
        )
        .unwrap_err();
    assert!(format!("{err:?}").contains("add it first"), "{err:?}");
    tb.call("edit_effect", &json!({"item_id": id, "kind": "zoom_blur"}))
        .unwrap();
    let err = tb
        .call(
            "set_keyframes",
            &json!({"item_id": id, "property": "fx.zoom_blur.amount", "keys": [{"at_sec": 0, "value": 500}]}),
        )
        .unwrap_err();
    assert!(format!("{err:?}").contains("outside 0..100"), "{err:?}");
    tb.call(
        "set_keyframes",
        &json!({"item_id": id, "property": "fx.zoom_blur.amount", "keys": [{"at_sec": 0, "value": 0}, {"at_sec": 0.9, "value": 80}]}),
    )
    .unwrap();
    let timeline = tb.call("get_timeline", &json!({})).unwrap();
    assert!(timeline.contains("fx.zoom_blur.amount"), "{timeline}");
    // no blur at the start, a strong one near the end
    let none = {
        let mut t = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
        t.call("get_timeline", &json!({})).unwrap();
        look(t.project(), fx.tl, 0)
    };
    let (early, late) = (look(tb.project(), fx.tl, 0), look(tb.project(), fx.tl, 22));
    let late_plain = look(&fx.project, fx.tl, 22);
    assert!(change(&none, &early) < 0.003, "no blur at the start");
    assert!(change(&late_plain, &late) > 0.01, "strong blur at the end");
}

#[test]
fn every_transition_can_be_set_and_the_catalogue_lists_everything() {
    let defs = captaine_agent::tool_definitions().to_string();
    for kind in EffectKind::ALL {
        assert!(
            defs.contains(kind.key()),
            "{} missing from the tools",
            kind.key()
        );
    }
    for kind in TransitionKind::ALL {
        assert!(
            defs.contains(kind.key()),
            "{} missing from the tools",
            kind.key()
        );
    }
    if !have_ffmpeg() {
        return;
    }
    let fx = fixture();
    for kind in TransitionKind::ALL {
        let (p, _) = {
            let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
            tb.call(
                "set_transition",
                &json!({"item_id": fx.b.to_string(), "kind": kind.key(), "duration_sec": 0.5}),
            )
            .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            (tb.project().clone(), ())
        };
        assert_eq!(clip_of(&p, fx.b).transition.map(|t| t.kind), Some(kind));
    }
}

/// A chroma key asked for without a colour picks the backdrop from the shot: here a dull, turf-like green that
/// pure green would miss.
#[test]
fn a_chroma_key_without_a_colour_picks_the_backdrop_from_the_shot() {
    if !have_ffmpeg() {
        return;
    }
    let src = dir().join("dull-green.mp4");
    assert!(Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("color=c=0x2e8a3c:s=320x180:r=24:d=2,drawbox=x=110:y=50:w=100:h=80:color=0xc8c8c8:t=fill")
        .args(["-c:v", "libx264", "-pix_fmt", "yuv444p", "-crf", "4"])
        .arg(&src)
        .status()
        .unwrap()
        .success());
    let fx = fixture_of(&src);
    let mut tb = ToolBox::new(&fx.project, &[], None, 0.0).unwrap();
    let out = tb
        .call(
            "edit_effect",
            &json!({"item_id": fx.a.to_string(), "kind": "chroma_key"}),
        )
        .unwrap();
    assert!(out.contains("picked from the shot"), "{out}");
    let k = effect(&clip_of(tb.project(), fx.a), EffectKind::ChromaKey).params[..3].to_vec();
    assert!(
        (k[0] - 46.0).abs() < 12.0 && (k[1] - 138.0).abs() < 12.0 && (k[2] - 60.0).abs() < 12.0,
        "{k:?}"
    );
    // the green is gone (black canvas shows), the grey box stays
    let after = look(tb.project(), fx.tl, MID);
    let px = |x: usize, y: usize| &after.px[(y * 320 + x) * 4..(y * 320 + x) * 4 + 3];
    assert!(
        px(20, 20).iter().all(|v| *v < 30),
        "backdrop keyed: {:?}",
        px(20, 20)
    );
    assert!(
        px(160, 90).iter().all(|v| *v > 150),
        "subject kept: {:?}",
        px(160, 90)
    );
    // removing an effect takes its keyframes with it
    tb.call(
        "set_keyframes",
        &json!({"item_id": fx.a.to_string(), "property": "fx.chroma_key.tolerance", "keys": [{"at_sec": 0, "value": 20}, {"at_sec": 0.5, "value": 50}]}),
    )
    .unwrap();
    tb.call(
        "edit_effect",
        &json!({"item_id": fx.a.to_string(), "kind": "chroma_key", "remove": true}),
    )
    .unwrap();
    assert!(clip_of(tb.project(), fx.a).anim.is_empty());
}
