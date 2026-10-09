//! The local Copilot skills: real analysis, real ops, undoable.

use std::process::Command;

use captaine_app::app::*;
use captaine_app::copilot;
use captaine_model::*;
use captaine_ops::Op;
use captaine_time::Rational;
use egui_kittest::Harness;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

type H = Harness<'static, Option<App>>;

/// One 7 s clip: 1 s of tone, 2 s of silence, 1.5 s of tone, 1.5 s of silence, 1 s of tone.
fn editor() -> H {
    let dir = std::env::temp_dir().join(format!("captaine-copilot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("talk.mp4");
    let audio =
        "aevalsrc=0.5*sin(2*PI*200*t)*(lt(t\\,1)+between(t\\,3\\,4.5)+gt(t\\,6)):s=48000:d=7";
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x90:rate=24:duration=7",
            "-f",
            "lavfi",
            "-i",
            audio,
        ])
        .args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1480.0, 940.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(
            move |ui, state: &mut Option<App>| {
                let app = state.get_or_insert_with(|| {
                    let mut app = App::new(
                        &ui.ctx().clone(),
                        captaine_app::blank_session(1280, 720, Rational::FPS_24),
                        None,
                    );
                    app.import(vec![path.clone()]);
                    let id = *app.project().media.keys().next().unwrap();
                    app.place_media(id, None, None);
                    app.show_copilot = true;
                    app
                });
                app.update(ui);
            },
            None,
        );
    h.step();
    h.step();
    h
}

fn app(h: &H) -> &App {
    h.state().as_ref().unwrap()
}

fn first_clip(a: &App) -> Option<Id> {
    a.timeline()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == "V1")
        .and_then(|t| t.items.first())
        .map(|i| i.id())
}

fn run_until(h: &mut H, cond: impl Fn(&App) -> bool) {
    for _ in 0..200 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(25));
        if cond(app(h)) {
            return;
        }
    }
    panic!("timed out waiting for Copilot");
}

#[test]
fn removes_silences_through_the_command_layer() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    assert_eq!(app(&h).duration_ticks(), 168);
    let before = app(&h).project().clone();
    {
        let a = h.state_mut().as_mut().unwrap();
        a.selected_clip = first_clip(a);
        copilot::ask(a, "please remove silences".into());
    }
    run_until(&mut h, |a| a.copilot.suggestion.is_some());
    let sug = app(&h).copilot.suggestion.clone().unwrap();
    assert!(sug.body.contains("2 gaps"), "{}", sug.body);
    // apply as the agent would
    {
        let a = h.state_mut().as_mut().unwrap();
        let s = a.copilot.suggestion.take().unwrap();
        assert!(a.do_op_as(captaine_ops::Author::Agent("copilot".into()), s.op));
    }
    h.step();
    let a = app(&h);
    // two gaps of 2 s and 1.5 s, each trimmed by 0.12 s of padding on both sides
    let removed = 168 - a.duration_ticks();
    assert!(
        (removed - ((2.0 - 0.24 + 1.5 - 0.24) * 24.0) as i64).abs() <= 6,
        "removed {removed} frames"
    );
    a.project().validate().unwrap();
    assert_eq!(
        a.session.editor.history().last().unwrap().author,
        captaine_ops::Author::Agent("copilot".into())
    );
    // one undo gets everything back
    let a = h.state_mut().as_mut().unwrap();
    a.undo();
    assert_eq!(a.project().timelines, before.timelines);
}

#[test]
fn colour_and_fade_requests_become_one_undoable_edit() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let sug;
    {
        let a = h.state_mut().as_mut().unwrap();
        // no selection: it asks for one instead of guessing
        a.selected_clip = None;
        copilot::ask(a, "warm up the colour".into());
        assert!(a.copilot.suggestion.is_none());
        assert!(a
            .copilot
            .messages
            .last()
            .unwrap()
            .text
            .contains("Select a clip"));
        a.selected_clip = first_clip(a);
        copilot::ask(a, "Warm up the colour and add a 12-frame fade in".into());
        sug = a.copilot.suggestion.clone().expect("a suggestion");
    }
    let Op::Transaction { ops, .. } = &sug.op else {
        panic!("{:?}", sug.op)
    };
    assert!(matches!(&ops[0], Op::SetVideo { video, .. } if video.color.temperature > 7000.0));
    assert!(
        matches!(&ops[1], Op::SetAudio { audio, .. } if audio.fade_in == 12 && audio.fade_out == 0)
    );
    // previewing shows the change in the viewer without touching the project
    {
        let a = h.state_mut().as_mut().unwrap();
        let (p, _) = captaine_ops::apply(a.project(), &sug.op).unwrap();
        a.set_preview(Some(p));
    }
    h.step();
    assert!(app(&h).preview_override.is_some());
    assert_eq!(
        app(&h).selected().unwrap().0.video.color.temperature,
        6500.0
    );
    let a = h.state_mut().as_mut().unwrap();
    a.set_preview(None);
    assert!(a.do_op_as(captaine_ops::Author::Agent("copilot".into()), sug.op));
    assert_eq!(a.selected().unwrap().0.audio.fade_in, 12);
    a.undo();
    assert_eq!(a.selected().unwrap().0.audio.fade_in, 0);
    assert_eq!(a.selected().unwrap().0.video.color.temperature, 6500.0);
    // unknown requests explain what is possible
    copilot::ask(a, "make it feel nostalgic".into());
    assert!(a.copilot.messages.last().unwrap().text.contains("I can"));
}
