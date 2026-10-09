//! The agent loop against a scripted "model": no network, no key.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use captaine_agent::*;
use captaine_model::*;
use captaine_ops::{apply, Op};
use captaine_time::{Rational, RationalTime};
use serde_json::{json, Value};

const R: Rational = Rational::FPS_24;

fn t(v: i64) -> RationalTime {
    RationalTime::new(v, R)
}

struct Scripted {
    responses: Mutex<VecDeque<Result<Value, String>>>,
    requests: Mutex<Vec<Value>>,
}

impl Scripted {
    fn new(r: Vec<Result<Value, String>>) -> Scripted {
        Scripted {
            responses: Mutex::new(r.into()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl Transport for Scripted {
    fn send(&self, body: &Value) -> Result<Value, String> {
        self.requests.lock().unwrap().push(body.clone());
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("script exhausted".into()))
    }
}

fn tool(id: &str, name: &str, input: Value) -> Result<Value, String> {
    Ok(
        json!({"stop_reason": "tool_use", "content": [{"type": "text", "text": "working"}, {"type": "tool_use", "id": id, "name": name, "input": input}]}),
    )
}

fn end(text: &str) -> Result<Value, String> {
    Ok(json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": text}]}))
}

/// V1: clips a (0..4 s), b (4..8 s); V2 empty; A1 empty. Media files do not exist on disk.
fn project() -> (Project, Vec<Id>) {
    let mut p = Project::new(ProjectSettings {
        frame_rate: R,
        width: 1280,
        height: 720,
        sample_rate: 48_000,
    });
    let m = Id::from_u128(100);
    p.media.insert(
        m,
        MediaRef {
            id: m,
            name: "talk.mp4".into(),
            path: "/nowhere/talk.mp4".into(),
            duration: Some(t(24 * 20)),
            has_audio: true,
            width: 1280,
            height: 720,
            has_video: true,
            proxy: None,
        },
    );
    let a = Id::from_u128(0xa1a1a1a1 << 96);
    let b = Id::from_u128(0xb2b2b2b2 << 96);
    let mut v1 = Track::new(Id::from_u128(1), "V1", TrackKind::Video);
    v1.items
        .push(Item::Clip(Clip::new(a, "intro", m, t(0), t(96))));
    v1.items
        .push(Item::Clip(Clip::new(b, "main", m, t(96), t(96))));
    let mut tl = Timeline::new(Id::from_u128(2), "Timeline 1", R);
    tl.tracks.push(Arc::new(v1));
    tl.tracks.push(Arc::new(Track::new(
        Id::from_u128(3),
        "V2",
        TrackKind::Video,
    )));
    tl.tracks.push(Arc::new(Track::new(
        Id::from_u128(4),
        "A1",
        TrackKind::Audio,
    )));
    p.timelines.insert(tl.id, Arc::new(tl));
    (p, vec![a, b])
}

fn ask(
    transport: &Scripted,
    p: &Project,
    text: &str,
    sel: Option<Id>,
) -> Result<AgentResult, AgentError> {
    let req = AgentRequest {
        project: p,
        selection: sel,
        playhead_secs: 1.5,
        history: &[],
        text,
        base_ops: &[],
    };
    run(transport, &AgentConfig::default(), &req, &mut |_| {})
}

fn apply_all(p: &Project, ops: &[Op]) -> Project {
    ops.iter().fold(p.clone(), |p, op| apply(&p, op).unwrap().0)
}

fn v1(p: &Project) -> Vec<(i64, i64)> {
    let tr = &p.timelines.values().next().unwrap().tracks[0];
    tr.items
        .iter()
        .zip(tr.item_starts())
        .map(|(i, s)| (s, i.duration().value))
        .collect()
}

#[test]
fn looks_then_edits_and_returns_one_suggestion() {
    let (p, ids) = project();
    let model = Scripted::new(vec![
        tool("t1", "get_timeline", json!({})),
        tool("t2", "split", json!({"track": "V1", "at_sec": 2.0})),
        tool(
            "t3",
            "set_clip",
            json!({"item_id": ids[1].to_string(), "temperature_k": 7800, "volume_db": -3}),
        ),
        end("Cut the intro at 2 s and warmed the main clip."),
    ]);
    let mut events = Vec::new();
    let req = AgentRequest {
        project: &p,
        selection: Some(ids[1]),
        playhead_secs: 1.5,
        history: &[],
        text: "split the intro and warm up main",
        base_ops: &[],
    };
    let r = run(&model, &AgentConfig::default(), &req, &mut |Event::Step(
        s,
    )| {
        events.push(s)
    })
    .unwrap();
    assert_eq!(r.reply, "Cut the intro at 2 s and warmed the main clip.");
    assert_eq!(r.new_ops.len(), 2);
    assert_eq!(events.len(), 3, "{events:?}");
    assert!(
        events[1].starts_with("split") && events[1].contains("at_sec=2.0"),
        "{events:?}"
    );
    assert_eq!(r.steps, events);

    // the suggestion really does what was said
    let after = apply_all(&p, &r.new_ops);
    assert_eq!(v1(&after), [(0, 48), (48, 48), (96, 96)]);
    let main = after.timelines.values().next().unwrap().tracks[0].items[2]
        .as_clip()
        .unwrap()
        .clone();
    assert_eq!(
        (main.video.color.temperature, main.audio.gain_db),
        (7800.0, -3.0)
    );
    // ...and the real project was never touched
    assert_eq!(v1(&p), [(0, 96), (96, 96)]);

    // what was sent
    let reqs = model.requests.lock().unwrap();
    assert_eq!(reqs.len(), 4);
    let first = &reqs[0];
    assert_eq!(first["model"], "claude-sonnet-5-5");
    assert!(first["system"].as_str().unwrap().contains("Copilot"));
    assert_eq!(first["tools"].as_array().unwrap().len(), 13);
    let user0 = first["messages"][0]["content"].as_str().unwrap();
    assert!(
        user0.contains("[Current state]") && user0.contains("split the intro and warm up main"),
        "{user0}"
    );
    assert!(
        user0.contains(&ids[1].to_string()),
        "the selection is named"
    );
    // the second request carries the model's tool call and our result for it
    let m = reqs[1]["messages"].as_array().unwrap();
    assert_eq!(m[1]["role"], "assistant");
    assert_eq!(m[2]["role"], "user");
    let result = m[2]["content"][0]["content"].as_str().unwrap();
    assert_eq!(m[2]["content"][0]["tool_use_id"], "t1");
    assert!(
        result.contains("\"V1\"") && result.contains("intro") && result.contains("main"),
        "{result}"
    );
}

#[test]
fn tool_errors_are_reported_back_and_the_model_recovers() {
    let (p, ids) = project();
    let model = Scripted::new(vec![
        tool("t1", "delete_item", json!({"item_id": "deadbeef"})),
        tool(
            "t2",
            "delete_item",
            json!({"item_id": ids[0].to_string(), "ripple": true}),
        ),
        end("Removed the intro."),
    ]);
    let r = ask(&model, &p, "delete the intro", None).unwrap();
    assert_eq!(r.new_ops.len(), 1);
    assert_eq!(v1(&apply_all(&p, &r.new_ops)), [(0, 96)]);
    let reqs = model.requests.lock().unwrap();
    let err = &reqs[1]["messages"][2]["content"][0];
    assert_eq!(err["is_error"], true);
    assert!(
        err["content"]
            .as_str()
            .unwrap()
            .contains("no clip or title with id"),
        "{err}"
    );
}

#[test]
fn answering_without_editing_gives_no_suggestion() {
    let (p, _) = project();
    let model = Scripted::new(vec![
        tool("t1", "get_timeline", json!({})),
        end("The timeline is 8 seconds long with two clips."),
    ]);
    let r = ask(&model, &p, "how long is this?", None).unwrap();
    assert!(r.new_ops.is_empty());
    assert!(r.reply.contains("8 seconds"));
}

#[test]
fn earlier_turns_and_pending_edits_carry_over() {
    let (p, ids) = project();
    // turn 1 produced a suggestion that is still pending
    let m1 = Scripted::new(vec![
        tool("a", "split", json!({"track": "V1", "at_sec": 2.0})),
        end("Cut at 2 s."),
    ]);
    let r1 = ask(&m1, &p, "cut at 2 seconds", None).unwrap();
    // turn 2 builds on it: it can now see the cut
    let history = vec![ChatTurn {
        user: "cut at 2 seconds".into(),
        assistant: "Cut at 2 s.".into(),
    }];
    let m2 = Scripted::new(vec![
        tool("b", "get_timeline", json!({})),
        tool("c", "set_track", json!({"track": "V2", "enabled": false})),
        end("Also hid V2."),
    ]);
    let req = AgentRequest {
        project: &p,
        selection: None,
        playhead_secs: 0.0,
        history: &history,
        text: "and hide V2",
        base_ops: &r1.new_ops,
    };
    let r2 = run(&m2, &AgentConfig::default(), &req, &mut |_| {}).unwrap();
    assert_eq!(r2.new_ops.len(), 1, "only the new edit");
    let reqs = m2.requests.lock().unwrap();
    let msgs = reqs[0]["messages"].as_array().unwrap();
    assert_eq!(
        (msgs[0]["role"].as_str(), msgs[0]["content"].as_str()),
        (Some("user"), Some("cut at 2 seconds"))
    );
    assert_eq!(
        (msgs[1]["role"].as_str(), msgs[1]["content"].as_str()),
        (Some("assistant"), Some("Cut at 2 s."))
    );
    assert!(msgs[2]["content"].as_str().unwrap().contains("and hide V2"));
    let timeline_result = reqs[1]["messages"][4]["content"][0]["content"]
        .as_str()
        .unwrap();
    let tl: Value = serde_json::from_str(timeline_result).unwrap();
    assert_eq!(
        tl["tracks"][0]["items"].as_array().unwrap().len(),
        3,
        "the pending cut is visible: {tl}"
    );
    // together they apply to the real project
    let mut all = r1.new_ops.clone();
    all.extend(r2.new_ops);
    let after = apply_all(&p, &all);
    assert_eq!(v1(&after).len(), 3);
    assert!(!after.timelines.values().next().unwrap().tracks[1].enabled);
    let _ = ids;
}

#[test]
fn failures_are_reported() {
    let (p, _) = project();
    let e = ask(
        &Scripted::new(vec![Err("could not reach the Anthropic API".into())]),
        &p,
        "hi",
        None,
    )
    .unwrap_err();
    assert!(matches!(e, AgentError::Transport(_)) && e.to_string().contains("reach"));
    let e = ask(
        &Scripted::new(vec![Ok(json!({"nonsense": true}))]),
        &p,
        "hi",
        None,
    )
    .unwrap_err();
    assert!(matches!(e, AgentError::BadResponse(_)));
    // a stale pending suggestion that no longer applies is refused up front
    let bad = vec![Op::RemoveItem {
        item: Id::from_u128(999),
    }];
    let req = AgentRequest {
        project: &p,
        selection: None,
        playhead_secs: 0.0,
        history: &[],
        text: "x",
        base_ops: &bad,
    };
    assert!(matches!(
        run(
            &Scripted::new(vec![]),
            &AgentConfig::default(),
            &req,
            &mut |_| {}
        ),
        Err(AgentError::Setup(_))
    ));
}

#[test]
fn a_runaway_loop_is_stopped() {
    let (p, _) = project();
    let many: Vec<_> = (0..30)
        .map(|i| tool(&format!("t{i}"), "get_timeline", json!({})))
        .collect();
    let model = Scripted::new(many);
    let cfg = AgentConfig {
        max_steps: 3,
        ..AgentConfig::default()
    };
    let req = AgentRequest {
        project: &p,
        selection: None,
        playhead_secs: 0.0,
        history: &[],
        text: "loop",
        base_ops: &[],
    };
    let r = run(&model, &cfg, &req, &mut |_| {}).unwrap();
    assert!(r.reply.contains("stopped after 3 steps"), "{}", r.reply);
    assert!(
        model.requests.lock().unwrap().len() <= 8,
        "it gave up early"
    );
    // calls past the limit were answered with an error telling the model to wrap up
    let reqs = model.requests.lock().unwrap();
    let last = reqs.last().unwrap();
    let text = serde_json::to_string(last).unwrap();
    assert!(text.contains("step limit reached"));
}

#[test]
fn a_cut_off_reply_is_flagged() {
    let (p, _) = project();
    let model = Scripted::new(vec![Ok(
        json!({"stop_reason": "max_tokens", "content": [{"type": "text", "text": "I will"}]}),
    )]);
    let r = ask(&model, &p, "x", None).unwrap();
    assert!(r.reply.starts_with("I will") && r.reply.contains("ran out of room"));
}

/// Talks to the real API. Skipped by default; run with a key set:
/// `ANTHROPIC_API_KEY=... cargo test -p captaine-agent -- --ignored live`
#[test]
#[ignore = "needs an API key and network access"]
fn live_the_model_can_read_the_timeline_and_make_a_cut() {
    let Some(key) = Credentials::load() else {
        panic!("set ANTHROPIC_API_KEY first");
    };
    let (p, ids) = project();
    let transport = AnthropicTransport::new(key);
    let req = AgentRequest {
        project: &p,
        selection: Some(ids[0]),
        playhead_secs: 2.0,
        history: &[],
        text: "Cut the selected clip in two at the playhead.",
        base_ops: &[],
    };
    let r = run(
        &transport,
        &AgentConfig::default(),
        &req,
        &mut |Event::Step(s)| eprintln!("step: {s}"),
    )
    .unwrap();
    eprintln!("reply: {}", r.reply);
    let after = apply_all(&p, &r.new_ops);
    assert_eq!(
        v1(&after).len(),
        3,
        "the intro was split in two: {:?}",
        v1(&after)
    );
    assert_eq!(v1(&after)[0], (0, 48));
}
