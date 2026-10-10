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
    // every test makes its own file: they run side by side and Windows will not share a half-written one
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = dir.join(format!(
        "talk-{}.mp4",
        N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
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
                    std::env::set_var("CAPTAINE_NO_CLAUDE_CLI", "1");
                    let mut app = App::new(
                        &ui.ctx().clone(),
                        captaine_app::blank_session(1280, 720, Rational::FPS_24),
                        None,
                    );
                    app.import(vec![path.clone()]);
                    let id = *app.project().media.keys().next().unwrap_or_else(|| {
                        let said: Vec<_> = app.toasts.iter().map(|t| t.text.clone()).collect();
                        panic!("the clip was not imported: {said:?}")
                    });
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
        copilot::ask_local(a, "please remove silences".into());
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
        copilot::ask_local(a, "warm up the colour".into());
        assert!(a.copilot.suggestion.is_none());
        assert!(a
            .copilot
            .messages
            .last()
            .unwrap()
            .text
            .contains("Select a clip"));
        a.selected_clip = first_clip(a);
        copilot::ask_local(a, "Warm up the colour and add a 12-frame fade in".into());
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
    copilot::ask_local(a, "make it feel nostalgic".into());
    assert!(a.copilot.messages.last().unwrap().text.contains("I can"));
}

// ---- the Claude path, with a scripted model ------------------------------------------------------

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use captaine_agent::Transport;
use serde_json::{json, Value};

struct Script {
    responses: Mutex<VecDeque<Result<Value, String>>>,
    seen: Arc<Mutex<Vec<Value>>>,
}

impl Transport for Script {
    fn send(&self, body: &Value) -> Result<Value, String> {
        self.seen.lock().unwrap().push(body.clone());
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("script exhausted".into()))
    }
}

fn tool(id: &str, name: &str, input: Value) -> Result<Value, String> {
    Ok(
        json!({"stop_reason": "tool_use", "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}),
    )
}

fn say(text: &str) -> Result<Value, String> {
    Ok(json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": text}]}))
}

/// Make the Copilot talk to `responses` instead of the Anthropic API.
fn script(h: &mut H, responses: Vec<Result<Value, String>>) -> Arc<Mutex<Vec<Value>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (s2, queue) = (
        seen.clone(),
        Arc::new(Mutex::new(VecDeque::from(responses))),
    );
    let a = h.state_mut().as_mut().unwrap();
    a.copilot.transport = Some(Arc::new(move || -> Box<dyn Transport> {
        Box::new(Script {
            responses: Mutex::new(std::mem::take(&mut *queue.lock().unwrap())),
            seen: s2.clone(),
        })
    }));
    seen
}

fn settled(a: &App) -> bool {
    !a.copilot.messages.is_empty() && a.copilot.suggestion.is_some()
}

#[test]
fn claude_edits_show_up_as_a_suggestion_you_can_preview_and_apply() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let clip = first_clip(app(&h)).unwrap();
    let seen = script(
        &mut h,
        vec![
            tool("a", "get_timeline", json!({})),
            tool(
                "b",
                "add_title",
                json!({"text": "Talking head", "subtitle": "Take 1", "at_sec": 0.5, "duration_sec": 2}),
            ),
            tool(
                "c",
                "set_clip",
                json!({"item_id": clip.to_string(), "temperature_k": 7600}),
            ),
            say("Added a lower third at 0.5 s and warmed the clip."),
        ],
    );
    {
        let a = h.state_mut().as_mut().unwrap();
        a.selected_clip = Some(clip);
        copilot::ask(a, "put a title on this and warm it up".into());
        assert!(a.copilot.messages.last().unwrap().user);
    }
    run_until(&mut h, settled);
    let a = app(&h);
    assert_eq!(
        a.copilot.messages.last().unwrap().text,
        "Added a lower third at 0.5 s and warmed the clip."
    );
    let sug = a.copilot.suggestion.clone().unwrap();
    assert!(sug.body.starts_with("2 changes"), "{}", sug.body);
    // nothing has been applied yet
    assert!(a
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .all(|t| t.items.iter().all(|i| i.as_title().is_none())));
    assert_eq!(a.selected().unwrap().0.video.color.temperature, 6500.0);
    // the model was told about the selection and the request
    let first = seen.lock().unwrap()[0].clone();
    let user = first["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        user.contains("put a title on this and warm it up") && user.contains(&clip.to_string()),
        "{user}"
    );

    // preview in the viewer, then apply as the agent
    {
        let a = h.state_mut().as_mut().unwrap();
        let (p, _) = captaine_ops::apply(a.project(), &sug.op).unwrap();
        a.set_preview(Some(p));
    }
    h.step();
    let a = h.state_mut().as_mut().unwrap();
    a.set_preview(None);
    assert!(a.do_op_as(captaine_ops::Author::Agent("copilot".into()), sug.op));
    assert!(a
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .any(|t| t.items.iter().any(|i| i.as_title().is_some())));
    assert_eq!(a.selected().unwrap().0.video.color.temperature, 7600.0);
    a.undo();
    assert_eq!(a.selected().unwrap().0.video.color.temperature, 6500.0);
}

#[test]
fn a_follow_up_builds_on_the_pending_suggestion_and_remembers_the_conversation() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let clip = first_clip(app(&h)).unwrap();
    // turn one
    script(
        &mut h,
        vec![
            tool(
                "a",
                "set_clip",
                json!({"item_id": clip.to_string(), "opacity": 0.5}),
            ),
            say("Made it half transparent."),
        ],
    );
    {
        let a = h.state_mut().as_mut().unwrap();
        a.selected_clip = Some(clip);
        copilot::ask(a, "make it half transparent".into());
    }
    run_until(&mut h, settled);
    // turn two, with a fresh script: it must see the first change as already done
    let seen = script(
        &mut h,
        vec![
            tool("b", "get_timeline", json!({})),
            tool(
                "c",
                "set_clip",
                json!({"item_id": clip.to_string(), "saturation": 60}),
            ),
            say("Also muted the colour."),
        ],
    );
    {
        let a = h.state_mut().as_mut().unwrap();
        copilot::ask(a, "and desaturate it".into());
    }
    run_until(&mut h, |a| {
        !a.copilot.is_working()
            && a.copilot
                .messages
                .last()
                .is_some_and(|m| m.text == "Also muted the colour.")
    });
    let reqs = seen.lock().unwrap().clone();
    let msgs = reqs[0]["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["content"], "make it half transparent");
    assert_eq!(msgs[1]["content"], "Made it half transparent.");
    let tl = reqs[1]["messages"][4]["content"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        tl.contains("\"opacity\":0.5"),
        "the model sees the pending change: {tl}"
    );
    // one combined suggestion covering both turns
    let sug = app(&h).copilot.suggestion.clone().unwrap();
    assert!(sug.body.starts_with("2 changes"), "{}", sug.body);
    let a = h.state_mut().as_mut().unwrap();
    assert!(a.do_op_as(captaine_ops::Author::Agent("copilot".into()), sug.op));
    let c = a.selected().unwrap().0;
    assert_eq!((c.video.opacity, c.video.color.saturation), (0.5, 60.0));
    // and one undo undoes both
    a.undo();
    let c = a.selected().unwrap().0;
    assert_eq!((c.video.opacity, c.video.color.saturation), (1.0, 100.0));
}

#[test]
fn model_failures_are_explained_in_the_chat() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    script(
        &mut h,
        vec![Err("the API key was rejected (HTTP 401)".into())],
    );
    {
        let a = h.state_mut().as_mut().unwrap();
        copilot::ask(a, "do something".into());
    }
    run_until(&mut h, |a| {
        !a.copilot.is_working()
            && a.copilot
                .messages
                .iter()
                .any(|m| m.text.contains("rejected"))
    });
    let a = app(&h);
    assert!(a.copilot.suggestion.is_none());
    assert!(a
        .copilot
        .messages
        .last()
        .unwrap()
        .text
        .starts_with("That didn't work"));
}

#[test]
fn with_no_key_the_local_skills_answer_and_nothing_goes_to_the_network() {
    if !have_ffmpeg() {
        return;
    }
    // point HOME somewhere empty and clear the env key so this holds on any machine
    let home = std::env::temp_dir().join(format!("captaine-nokey-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
    let mut h = editor();
    let a = h.state_mut().as_mut().unwrap();
    a.selected_clip = first_clip(a);
    copilot::ask(a, "warm up the colour".into());
    assert!(
        a.copilot.suggestion.is_some(),
        "the local skill answered immediately"
    );
    assert!(!a.copilot.is_working());
}
