//! Renders the whole editor offscreen. With `CAPTAINE_SHOTS=<dir>` set it writes PNGs there for eyeballing;
//! either way it checks that the layout contains what the design promises.

use std::path::{Path, PathBuf};
use std::process::Command;

use captaine_app::app::*;
use captaine_model::*;
use captaine_ops::{edit, Op};
use captaine_time::{Rational, RationalTime};
use egui_kittest::Harness;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

fn work_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("captaine-app-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ffmpeg(args: &[&str], out: &Path) {
    assert!(Command::new("ffmpeg")
        .args(["-y", "-v", "error"])
        .args(args)
        .arg(out)
        .status()
        .unwrap()
        .success());
}

/// Five short scenes and a music bed, roughly the footage in the design mockup.
fn make_media(dir: &Path) -> Vec<PathBuf> {
    let scenes = [
        ("Shot_01_drone.mov", "mandelbrot=size=640x360:rate=24"),
        ("Shot_02_market.mov", "testsrc2=size=640x360:rate=24"),
        (
            "Shot_03_street.mov",
            "gradients=size=640x360:rate=24:speed=0.02",
        ),
        (
            "Shot_04_coast.mov",
            "life=size=640x360:rate=24:mold=10:life_color=#5ec0a0:death_color=#103040",
        ),
        (
            "Shot_05_sunset.mov",
            "gradients=size=640x360:rate=24:c0=#ff7a3d:c1=#3d2a6e:speed=0.03",
        ),
    ];
    let mut out = Vec::new();
    for (name, src) in scenes {
        let p = dir.join(name);
        // speech-like bursts so the waveform and silence detection have something to show
        let audio = "aevalsrc=0.5*sin(2*PI*180*t)*gt(sin(2*PI*0.6*t)\\,-0.2)*(0.6+0.4*sin(2*PI*5*t)):s=48000:d=8";
        ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                src,
                "-f",
                "lavfi",
                "-i",
                audio,
                "-t",
                "8",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
            ],
            &p,
        );
        out.push(p);
    }
    let music = dir.join("Score_theme.wav");
    ffmpeg(&["-f", "lavfi", "-i", "aevalsrc=0.3*sin(2*PI*110*t)*(0.7+0.3*sin(2*PI*0.5*t))+0.15*sin(2*PI*220*t):s=48000:d=40", "-c:a", "pcm_s16le"], &music);
    out.push(music);
    out
}

fn demo_app(ctx: &egui::Context, dir: &Path, files: &[PathBuf]) -> App {
    let session = captaine_app::blank_session(1920, 1080, Rational::FPS_24);
    std::env::set_var("CAPTAINE_NO_CLAUDE_CLI", "1");
    let mut app = App::new(ctx, session, None);
    app.import(files.to_vec());
    let tl = app.timeline_id().unwrap();
    let tracks: Vec<(Id, String)> = app
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .map(|t| (t.id, t.name.clone()))
        .collect();
    let track = |n: &str| tracks.iter().find(|t| t.1 == n).unwrap().0;
    let media: Vec<MediaRef> = {
        let mut m: Vec<_> = app.project().media.values().cloned().collect();
        m.sort_by(|a, b| a.name.cmp(&b.name));
        m
    };
    let t = |v: i64| RationalTime::new(v, Rational::FPS_24);
    // video clips back to back on V1
    for m in media.iter().filter(|m| m.has_video) {
        app.place_media(m.id, Some(track("V1")), None);
    }
    // music on A1 and A2
    let score = media.iter().find(|m| !m.has_video).unwrap();
    app.place_media(score.id, Some(track("A1")), Some(0));
    app.place_media(score.id, Some(track("A2")), Some(0));
    let _ = (tl, dir, t);
    // the mockup's lower third on V2
    let mut title = Title::new(Id::new(), TitleTemplate::LowerThird, t(72));
    title.text = "Coastline, Day 3".into();
    title.subtitle = "Lower third · 3 s".into();
    title.name = "Lower Third: Coastline".into();
    let at = app.time(24 * 14);
    let v2 = track("V2");
    app.do_plan(|p| edit::overwrite_item(p, v2, at, Item::Title(title)));
    app.selected_clip = None;
    app
}

#[test]
fn the_editor_lays_out_like_the_design() {
    if !have_ffmpeg() {
        return;
    }
    let dir = work_dir();
    let files = make_media(&dir);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1480.0, 940.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(
            |ui, state: &mut Option<App>| {
                let app = state.get_or_insert_with(|| {
                    demo_app(&ui.ctx().clone(), &std::env::temp_dir(), &files)
                });
                app.update(ui);
            },
            None,
        );
    for _ in 0..30 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(60)); // let thumbnails, waveforms and the first frame arrive
    }
    // select a clip, move the playhead into it, open CoCaptain
    {
        let app = harness.state_mut().as_mut().unwrap();
        let (id, start) = {
            let tl = app.timeline().unwrap();
            let v1 = tl.tracks.iter().find(|t| t.name == "V1").unwrap();
            (v1.items[3].id(), v1.item_starts()[3])
        };
        app.selected_clip = Some(id);
        app.set_playhead(start + 12);
        app.show_copilot = true;
    }
    for _ in 0..30 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    if let Ok(out) = std::env::var("CAPTAINE_SHOTS") {
        let img = harness.render().expect("render");
        std::fs::create_dir_all(&out).unwrap();
        img.save(Path::new(&out).join("editor.png")).unwrap();
        // second view: the lower third selected, Titles tab open, CoCaptain closed
        {
            let app = harness.state_mut().as_mut().unwrap();
            let (id, start) = {
                let tl = app.timeline().unwrap();
                let v2 = tl.tracks.iter().find(|t| t.name == "V2").unwrap();
                let i = v2
                    .items
                    .iter()
                    .position(|i| i.as_title().is_some())
                    .unwrap();
                (v2.items[i].id(), v2.item_starts()[i])
            };
            app.selected_clip = Some(id);
            app.set_playhead(start + 30);
            app.show_copilot = false;
            app.pool_tab = PoolTab::Titles;
        }
        for _ in 0..20 {
            harness.step();
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        harness
            .render()
            .expect("render")
            .save(Path::new(&out).join("titles.png"))
            .unwrap();
    }
    // the Color page: a shot selected and graded, the scopes reading the frame
    {
        let app = harness.state_mut().as_mut().unwrap();
        let (id, start) = {
            let tl = app.timeline().unwrap();
            let v1 = tl.tracks.iter().find(|t| t.name == "V1").unwrap();
            (v1.items[1].id(), v1.item_starts()[1])
        };
        app.selected_clip = Some(id);
        app.set_playhead(start + 12);
        app.show_copilot = false;
        app.set_page(Page::Color);
        let clip = app.selected().unwrap().0;
        let mut video = clip.video;
        video.color.exposure = 0.4;
        video.color.saturation = 130.0;
        assert!(app.do_op(Op::SetVideo { item: id, video }));
    }
    for _ in 0..30 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    if let Ok(out) = std::env::var("CAPTAINE_SHOTS") {
        harness
            .render()
            .expect("render")
            .save(Path::new(&out).join("color.png"))
            .unwrap();
    }
    {
        let app = harness.state().as_ref().unwrap();
        assert_eq!(app.page, Page::Color);
        assert!(
            app.scope_src.is_some(),
            "the Color page's scopes got a frame"
        );
        assert_eq!(app.selected().unwrap().0.video.color.exposure, 0.4);
    }
    // the Effects page: the library, a clip with two effects (one setting keyframed) and a transition
    {
        let app = harness.state_mut().as_mut().unwrap();
        let id = app.selected_clip.unwrap();
        app.set_page(Page::Effects);
        let mut glow = Effect::new(EffectKind::Glow);
        glow.params[0] = 55.0;
        let mut leak = Effect::new(EffectKind::LightLeak);
        leak.params[0] = 45.0;
        assert!(app.do_op(Op::SetEffects {
            item: id,
            effects: vec![glow, leak],
        }));
        assert!(app.set_transition_kind(id, TransitionKind::Flash).is_ok());
        assert!(app.do_op(Op::SetAnimTrack {
            item: id,
            prop: "fx.glow.intensity".into(),
            keys: vec![Keyframe::new(0, 20.0), Keyframe::new(30, 150.0)],
        }));
    }
    for _ in 0..30 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    if let Ok(out) = std::env::var("CAPTAINE_SHOTS") {
        harness
            .render()
            .expect("render")
            .save(Path::new(&out).join("effects.png"))
            .unwrap();
    }
    assert_eq!(harness.state().as_ref().unwrap().page, Page::Effects);
    // the Deliver page: settings, viewer and timeline, the render queue
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .set_page(Page::Deliver);
    for _ in 0..10 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    if let Ok(out) = std::env::var("CAPTAINE_SHOTS") {
        harness
            .render()
            .expect("render")
            .save(Path::new(&out).join("deliver.png"))
            .unwrap();
    }
    let app = harness.state().as_ref().unwrap();
    assert_eq!(app.project().media.len(), 6);
    assert!(app.program.is_some(), "the viewer received a frame");
    assert!(
        app.thumbs.len() >= 5,
        "thumbnails arrived: {}",
        app.thumbs.len()
    );
    assert!(!app.waves.is_empty(), "waveforms arrived");
    let _ = (
        edit::lift as fn(&Project, Id) -> _,
        Op::SetName {
            item: Id::new(),
            name: String::new(),
        },
    );
}
