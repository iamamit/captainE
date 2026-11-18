//! Live test of CoCaptain on the Color tools: plain-English requests, one per tool, against a real project.
//!
//!     cargo run --release -p captaine-app --example cocaptain_color_eval -- <project.json> [--shot N] [--only NAME]
//!
//! Works on a new branch `test/cocaptain-color` (the open window follows the file). Shot N (default 2) on the first
//! video track is selected, the way the panel sends a selection. After each request the runner checks the clip's
//! stored values and the rendered picture, keeps the edit, and moves on. A Markdown and JSON report is written next
//! to the project, and every model exchange is logged.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use captaine_agent::{
    AgentConfig, AgentRequest, AnthropicTransport, ClaudeCli, Credentials, Event, Transport,
};
use captaine_model::*;
use captaine_ops::Author;
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

/// What the picture looks like (display values 0..1) and the pixels, for comparisons.
#[derive(Clone)]
struct Look {
    luma: f64,
    rgb: [f64; 3],
    sat: f64,
    px: Vec<u8>,
}

fn look(p: &Project, tl: Id, tick: i64) -> Look {
    let scale = (480.0 / p.canvas(tl).0.max(1) as f64).min(1.0);
    let q = preview_project(p, scale);
    let opts = RenderOptions {
        decode_scale: scale,
        hw_decode: true,
        ..RenderOptions::default()
    };
    let c = Renderer::new(&q, tl, opts).unwrap().render(tick).unwrap();
    let px = c.to_rgba8();
    let (mut rgb, mut sat, mut luma, mut n) = ([0.0f64; 3], 0.0f64, 0.0f64, 0.0f64);
    for p in px.chunks_exact(4) {
        let v = [
            p[0] as f64 / 255.0,
            p[1] as f64 / 255.0,
            p[2] as f64 / 255.0,
        ];
        let (mx, mn) = (v[0].max(v[1]).max(v[2]), v[0].min(v[1]).min(v[2]));
        if mx < 0.02 {
            continue; // letterbox bars
        }
        for k in 0..3 {
            rgb[k] += v[k];
        }
        sat += (mx - mn) / mx;
        luma += 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
        n += 1.0;
    }
    let n = n.max(1.0);
    Look {
        luma: luma / n,
        rgb: rgb.map(|v| v / n),
        sat: sat / n,
        px,
    }
}

fn change(a: &Look, b: &Look) -> f64 {
    let t: f64 =
        a.px.chunks_exact(4)
            .zip(b.px.chunks_exact(4))
            .map(|(x, y)| {
                (0..3)
                    .map(|k| (x[k] as f64 - y[k] as f64).abs())
                    .sum::<f64>()
                    / 3.0
            })
            .sum();
    t / (a.px.len() / 4).max(1) as f64 / 255.0
}

/// The video clips of the first video track, in order.
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

fn fx(c: &Clip, k: EffectKind) -> Option<&Effect> {
    c.effects.iter().find(|e| e.kind == k && e.enabled)
}

fn param(c: &Clip, k: EffectKind, name: &str) -> Option<f64> {
    let e = fx(c, k)?;
    let i = k
        .params()
        .iter()
        .position(|d| d.name.eq_ignore_ascii_case(name))?;
    Some(e.params[i])
}

/// A check: (before clip, after clip, before look, after look, project after, ids) -> (pass, what was seen)
type Check = Box<dyn Fn(&Clip, &Clip, &Look, &Look, &Project, &Ids) -> (bool, String)>;

struct Ids {
    prev: Option<Id>,
    next: Option<Id>,
    all: Vec<Id>,
}

struct Case {
    name: &'static str,
    ask: String,
    check: Check,
}

fn c(name: &'static str, ask: &str, check: Check) -> Case {
    Case {
        name,
        ask: ask.to_string(),
        check,
    }
}

fn cases(lut: &Path) -> Vec<Case> {
    use EffectKind as K;
    let near = |a: f64, b: f64, tol: f64| (a - b).abs() <= tol;
    vec![
        c("exposure", "Brighten the selected shot by half a stop of exposure.", Box::new(move |b, a, lb, la, _, _| {
            let d = a.video.color.exposure - b.video.color.exposure;
            (near(d, 0.5, 0.15) && la.luma > lb.luma, format!("exposure {:+.2} -> {:+.2}; luma {:.3} -> {:.3}", b.video.color.exposure, a.video.color.exposure, lb.luma, la.luma))
        })),
        c("contrast", "Set the contrast of the selected shot to 1.3.", Box::new(move |b, a, _, _, _, _| {
            (near(a.video.color.contrast, 1.3, 0.02), format!("contrast {:.2} -> {:.2}", b.video.color.contrast, a.video.color.contrast))
        })),
        c("saturation", "Make the selected shot less colourful: saturation 50.", Box::new(move |b, a, lb, la, _, _| {
            (near(a.video.color.saturation, 50.0, 2.0) && la.sat < lb.sat, format!("saturation {:.0} -> {:.0}; picture sat {:.3} -> {:.3}", b.video.color.saturation, a.video.color.saturation, lb.sat, la.sat))
        })),
        c("temperature", "Warm up the selected shot: white balance 7800 K.", Box::new(move |b, a, lb, la, _, _| {
            let warmer = (la.rgb[0] - la.rgb[2]) > (lb.rgb[0] - lb.rgb[2]);
            (near(a.video.color.temperature, 7800.0, 50.0) && warmer, format!("temperature {:.0} -> {:.0} K; red-blue {:.3} -> {:.3}", b.video.color.temperature, a.video.color.temperature, lb.rgb[0] - lb.rgb[2], la.rgb[0] - la.rgb[2]))
        })),
        c("tint", "Add a little magenta tint (+25) to the selected shot.", Box::new(move |b, a, lb, la, _, _| {
            (near(a.video.color.tint, 25.0, 3.0) && la.rgb[1] < lb.rgb[1], format!("tint {:.0} -> {:.0}; green {:.3} -> {:.3}", b.video.color.tint, a.video.color.tint, lb.rgb[1], la.rgb[1]))
        })),
        c("highlights", "Recover the highlights of the selected shot (highlights -40).", Box::new(move |b, a, _, _, _, _| {
            (near(a.video.color.highlights, -40.0, 3.0), format!("highlights {:.0} -> {:.0}", b.video.color.highlights, a.video.color.highlights))
        })),
        c("shadows", "Open up the shadows of the selected shot (shadows +35).", Box::new(move |b, a, _, _, _, _| {
            (near(a.video.color.shadows, 35.0, 3.0), format!("shadows {:.0} -> {:.0}", b.video.color.shadows, a.video.color.shadows))
        })),
        c("vibrance", "Give the selected shot some vibrance, +30.", Box::new(move |b, a, _, _, _, _| {
            (near(a.video.color.vibrance, 30.0, 3.0), format!("vibrance {:.0} -> {:.0}", b.video.color.vibrance, a.video.color.vibrance))
        })),
        c("lift wheel", "Use the colour wheels: push the shadows (lift) of the selected shot slightly toward blue.", Box::new(move |_, a, _, _, _, _| {
            let (r, bl) = (param(a, K::ColorWheels, "Lift red").unwrap_or(0.0), param(a, K::ColorWheels, "Lift blue").unwrap_or(0.0));
            (bl > 0.0 && bl > r, format!("lift red {r:+.3}, lift blue {bl:+.3}"))
        })),
        c("gamma wheel", "Use the colour wheels: warm the mid-tones (gamma) of the selected shot toward orange.", Box::new(move |_, a, _, _, _, _| {
            let (r, bl) = (param(a, K::ColorWheels, "Gamma red").unwrap_or(0.0), param(a, K::ColorWheels, "Gamma blue").unwrap_or(0.0));
            (r > bl, format!("gamma red {r:+.3}, gamma blue {bl:+.3}"))
        })),
        c("gain wheel", "Use the colour wheels: bring the overall gain of the selected shot down a little.", Box::new(move |_, a, lb, la, _, _| {
            let g = param(a, K::ColorWheels, "Gain master").unwrap_or(0.0);
            (g < 0.0 && la.luma < lb.luma, format!("gain master {g:+.3}; luma {:.3} -> {:.3}", lb.luma, la.luma))
        })),
        c("master curve", "Add a gentle S-curve on the master curve of the selected shot for more contrast.", Box::new(move |_, a, _, _, _, _| {
            let (q1, q3) = (param(a, K::Curves, "Master 25%").unwrap_or(0.25), param(a, K::Curves, "Master 75%").unwrap_or(0.75));
            (q1 < 0.25 && q3 > 0.75, format!("master 25% {q1:.3}, 75% {q3:.3}"))
        })),
        c("red curve", "On the red curve of the selected shot, lift the mid-tones a bit.", Box::new(move |_, a, lb, la, _, _| {
            let m = param(a, K::Curves, "Red 50%").unwrap_or(0.5);
            (m > 0.5 && la.rgb[0] > lb.rgb[0], format!("red 50% {m:.3}; red {:.3} -> {:.3}", lb.rgb[0], la.rgb[0]))
        })),
        c("hsl saturation", "With HSL, make the greens of the selected shot less saturated.", Box::new(move |_, a, _, _, _, _| {
            let s = param(a, K::Hsl, "Green sat").unwrap_or(100.0);
            (s < 100.0, format!("green sat {s:.0}"))
        })),
        c("hsl hue", "With HSL, shift the blues of the selected shot a little toward cyan.", Box::new(move |_, a, lb, la, _, _| {
            let h = param(a, K::Hsl, "Blue hue").unwrap_or(0.0);
            (h != 0.0 && change(la, lb) > 0.0, format!("blue hue {h:+.0}; pixels changed {:.4}", change(la, lb)))
        })),
        c("hsl lightness", "With HSL, darken the reds of the selected shot.", Box::new(move |_, a, _, _, _, _| {
            let l = param(a, K::Hsl, "Red lum").unwrap_or(0.0);
            (l < 0.0, format!("red lum {l:+.0}"))
        })),
        c("lut", &format!("Apply the LUT file {} to the selected shot.", lut.display()), {
            let want = lut.to_string_lossy().to_string();
            Box::new(move |_, a, lb, la, _, _| {
                let on = fx(a, K::Lut).map(|e| e.text.clone()).unwrap_or_default();
                (on == want && change(la, lb) > 0.002, format!("lut {on:?}; pixels changed {:.4}", change(la, lb)))
            })
        }),
        c("keyframes", "Animate the exposure of the selected shot: -1 stop at its start rising to its current value by 1 second in.", Box::new(move |_, a, _, _, _, _| {
            let k = a.anim.keys("exposure").len();
            (k >= 2, format!("{k} exposure keyframes"))
        })),
        c("auto balance", "Auto balance the colour of the selected shot.", Box::new(move |b, a, _, _, _, _| {
            (a.video.color != b.video.color, format!("colour settings changed: {}", a.video.color != b.video.color))
        })),
        c("match previous", "Match the colour of the selected shot to the shot right before it.", Box::new(move |b, a, _, _, _, ids| {
            (ids.prev.is_some() && a.video.color != b.video.color, format!("colour settings changed: {}", a.video.color != b.video.color))
        })),
        c("copy grade", "Copy the selected shot's grade to the next shot.", Box::new(move |_, a, _, _, p, ids| {
            let same = ids.next.map(|n| Grade::of(&find(p, n)) == Grade::of(a));
            (same == Some(true), format!("next shot has the same grade: {same:?}"))
        })),
        c("grade all shots", "Give every shot in the timeline the same grade as the selected shot.", Box::new(move |_, a, _, _, p, ids| {
            let g = Grade::of(a);
            let n = ids.all.iter().filter(|id| Grade::of(&find(p, **id)) == g).count();
            (n == ids.all.len(), format!("{n} of {} shots have the grade", ids.all.len()))
        })),
        c("reset", "Reset the grade of the selected shot.", Box::new(move |_, a, _, _, _, _| {
            let g = Grade::of(a);
            (g.is_neutral(), format!("neutral: {}", g.is_neutral()))
        })),
    ]
}

/// What a request may change, as `area:name` prefixes; everything else on the shot must stay as it was.
fn allowed(case: &str) -> &'static [&'static str] {
    match case {
        "exposure" => &["color:exposure"],
        "contrast" => &["color:contrast"],
        "saturation" => &["color:saturation"],
        "temperature" => &["color:temperature"],
        "tint" => &["color:tint"],
        "highlights" => &["color:highlights"],
        "shadows" => &["color:shadows"],
        "vibrance" => &["color:vibrance"],
        "lift wheel" => &["color_wheels:lift_"],
        "gamma wheel" => &["color_wheels:gamma_"],
        "gain wheel" => &["color_wheels:gain_"],
        "master curve" => &["curves:master_"],
        "red curve" => &["curves:red_"],
        "hsl saturation" => &["hsl:green_sat", "hsl:yellow_sat", "hsl:cyan_sat"],
        "hsl hue" => &["hsl:blue_hue", "hsl:cyan_hue", "hsl:magenta_hue"],
        "hsl lightness" => &["hsl:red_lum", "hsl:magenta_lum", "hsl:yellow_lum"],
        "lut" => &["lut:"],
        "keyframes" => &["anim:exposure"],
        "auto balance" | "match previous" => &["color:"],
        // these replace or remove a whole grade on purpose
        _ => &["*"],
    }
}

/// Everything about the shot's look that changed, as `area:name` entries.
fn differences(b: &Clip, a: &Clip) -> Vec<String> {
    let mut out = Vec::new();
    let (cb, ca) = (b.video.color, a.video.color);
    for (n, x, y) in [
        ("exposure", cb.exposure, ca.exposure),
        ("contrast", cb.contrast, ca.contrast),
        ("saturation", cb.saturation, ca.saturation),
        ("temperature", cb.temperature, ca.temperature),
        ("tint", cb.tint, ca.tint),
        ("highlights", cb.highlights, ca.highlights),
        ("shadows", cb.shadows, ca.shadows),
        ("vibrance", cb.vibrance, ca.vibrance),
    ] {
        if (x - y).abs() > 1e-9 {
            out.push(format!("color:{n}"));
        }
    }
    for k in EffectKind::ALL.into_iter().filter(|k| !k.is_audio()) {
        let get = |c: &Clip| c.effects.iter().find(|e| e.kind == k).cloned();
        let (eb, ea) = (get(b), get(a));
        if eb.is_none() && ea.is_none() {
            continue;
        }
        let defaults = Effect::new(k);
        let (pb, pa) = (
            eb.clone().unwrap_or(defaults.clone()),
            ea.clone().unwrap_or(defaults),
        );
        for (i, d) in k.params().iter().enumerate() {
            if (pb.params[i] - pa.params[i]).abs() > 1e-9 {
                out.push(format!(
                    "{}:{}",
                    k.key(),
                    d.name.to_lowercase().replace(' ', "_")
                ));
            }
        }
        if pb.text != pa.text || pb.enabled != pa.enabled {
            out.push(format!("{}:", k.key()));
        }
    }
    for prop in [
        "exposure",
        "contrast",
        "saturation",
        "temperature",
        "opacity",
        "scale",
    ] {
        if b.anim.keys(prop) != a.anim.keys(prop) {
            out.push(format!("anim:{prop}"));
        }
    }
    out
}

/// A small LUT made here (warm: blue a little lower), so the test uses nothing taken from elsewhere.
fn make_lut(dir: &Path) -> PathBuf {
    let p = dir.join("cocaptain_test_warm.cube");
    let mut s = String::from("TITLE \"captainE test warm\"\nLUT_3D_SIZE 2\n");
    for b in [0.0, 1.0] {
        for g in [0.0, 1.0] {
            for r in [0.0, 1.0] {
                s += &format!(
                    "{:.3} {:.3} {:.3}\n",
                    (r * 1.0f64).min(1.0),
                    g * 0.97,
                    b * 0.8
                );
            }
        }
    }
    std::fs::write(&p, s).unwrap();
    p
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let proj = PathBuf::from(
        args.get(1)
            .ok_or("usage: cocaptain_color_eval <project.json> [--shot N] [--only NAME]")?,
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
    let lut = make_lut(&dir);

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
            std::fs::File::create(dir.join(format!("color_eval-{stamp}.jsonl")))
                .map_err(|e| e.to_string())?,
        ),
    };

    let mut session = Session::load(&proj)?;
    session.commit(Some("Save before: CoCaptain colour test"))?;
    let branch = session.unique_branch_name("test/cocaptain-color");
    session.create_branch(&branch, true)?;
    session.save(&proj)?;
    println!("branch {branch}");

    let all = shots(session.editor.project());
    let target = all.get(shot_no - 1).ok_or("no such shot")?.id;
    let ids = Ids {
        prev: shot_no
            .checked_sub(2)
            .and_then(|i| all.get(i))
            .map(|c| c.id),
        next: all.get(shot_no).map(|c| c.id),
        all: all.iter().map(|c| c.id).collect(),
    };
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
    for case in cases(&lut) {
        if only.as_deref().is_some_and(|o| o != case.name) {
            continue;
        }
        let before_p = session.editor.project().clone();
        let before = find(&before_p, target);
        let mid = start_of(&before_p) + before.duration.value / 2;
        let lb = look(&before_p, tl, mid);
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
                let la = look(&after_p, tl, mid);
                let (mut ok, mut seen) = (case.check)(&before, &after, &lb, &la, &after_p, &ids);
                // nothing else on the shot may change
                let allow = allowed(case.name);
                if !allow.contains(&"*") {
                    let stray: Vec<String> = differences(&before, &after)
                        .into_iter()
                        .filter(|d| !allow.iter().any(|a| d.starts_with(a)))
                        .collect();
                    if !stray.is_empty() {
                        ok = false;
                        seen += &format!("; ALSO CHANGED (not asked): {}", stray.join(", "));
                    }
                }
                (ok, seen, r.reply, r.new_ops.len())
            }
            Err(e) => (false, format!("CoCaptain error: {e:?}"), String::new(), 0),
        };
        println!(
            "{} {:<16} {:>5.1}s  {}  | {}",
            if pass { "PASS" } else { "FAIL" },
            case.name,
            secs,
            seen,
            steps.join("; ")
        );
        rows.push(json!({"tool": case.name, "asked": case.ask, "pass": pass, "seen": seen, "steps": steps, "edits": n_ops, "seconds": (secs * 10.0).round() / 10.0, "reply": reply}));
    }
    let passed = rows.iter().filter(|r| r["pass"] == true).count();
    let mut md = format!(
        "# CoCaptain colour test\n\nProject `{}`, branch `{branch}`, shot {shot_no}. **{passed} of {} passed.**\n\n| # | Tool | Asked | Result | What was measured | CoCaptain's steps |\n|---|---|---|---|---|---|\n",
        proj.display(),
        rows.len()
    );
    for (i, r) in rows.iter().enumerate() {
        md += &format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            i + 1,
            r["tool"].as_str().unwrap_or(""),
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
    let md_path = dir.join(format!("color_eval-{stamp}.md"));
    std::fs::write(&md_path, md).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join(format!("color_eval-{stamp}.json")),
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
