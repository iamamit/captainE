//! Each tool on its own, plus real audio analysis and the HTTP transport against a local fake server.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};

use captaine_agent::*;
use captaine_model::*;
use captaine_ops::apply;
use captaine_time::{Rational, RationalTime};
use serde_json::{json, Value};

const R: Rational = Rational::FPS_24;
fn t(v: i64) -> RationalTime {
    RationalTime::new(v, R)
}

fn project(media_path: &str) -> (Project, Id, Id) {
    let mut p = Project::new(ProjectSettings {
        frame_rate: R,
        width: 1280,
        height: 720,
        sample_rate: 48_000,
    });
    let m = Id::from_u128(0x1111_1111 << 96);
    let audio_only = Id::from_u128(0x2222_2222 << 96);
    p.media.insert(
        m,
        MediaRef {
            id: m,
            name: "talk.mp4".into(),
            path: media_path.into(),
            duration: Some(t(24 * 7)),
            has_audio: true,
            width: 1280,
            height: 720,
            has_video: true,
            proxy: None,
        },
    );
    p.media.insert(
        audio_only,
        MediaRef {
            id: audio_only,
            name: "music.wav".into(),
            path: "/m.wav".into(),
            duration: Some(RationalTime::new(
                48_000 * 10,
                Rational::new(48_000, 1).unwrap(),
            )),
            has_audio: true,
            width: 0,
            height: 0,
            has_video: false,
            proxy: None,
        },
    );
    let clip = Id::from_u128(0xc1c1c1c1 << 96);
    let mut v1 = Track::new(Id::from_u128(1), "V1", TrackKind::Video);
    v1.items
        .push(Item::Clip(Clip::new(clip, "talk", m, t(0), t(24 * 7))));
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
    (p, clip, m)
}

fn json_of(s: Result<String, String>) -> Value {
    serde_json::from_str(&s.expect("tool ok")).expect("tool returned JSON")
}

#[test]
fn read_tools_describe_the_project() {
    let (p, clip, _) = project("/nowhere.mp4");
    let mut tb = ToolBox::new(&p, &[], Some(clip), 2.5).unwrap();
    let tl = json_of(tb.call("get_timeline", &json!({})));
    assert_eq!(
        (
            tl["fps"].as_f64(),
            tl["duration_sec"].as_f64(),
            tl["playhead_sec"].as_f64()
        ),
        (Some(24.0), Some(7.0), Some(2.5))
    );
    assert_eq!(tl["selected_item"], clip.to_string());
    assert_eq!(tl["tracks"].as_array().unwrap().len(), 3);
    assert_eq!(tl["tracks"][0]["items"][0]["name"], "talk");
    assert!(
        tl["tracks"][0]["items"][0].get("opacity").is_none(),
        "defaults are left out to save tokens"
    );
    let media = json_of(tb.call("list_media", &json!({})));
    assert_eq!(media.as_array().unwrap().len(), 2);
    assert_eq!(media[0]["name"], "music.wav");
    assert!(tb.new_ops.is_empty());
    assert!(tb
        .call("nonsense", &json!({}))
        .unwrap_err()
        .contains("unknown tool"));
    assert_eq!(tool_definitions().as_array().unwrap().len(), 13);
    for d in tool_definitions().as_array().unwrap() {
        assert!(
            d["name"].is_string()
                && d["description"].as_str().unwrap().len() > 20
                && d["input_schema"]["type"] == "object"
        );
    }
}

#[test]
fn edit_tools_make_the_edits_they_describe() {
    let (p, clip, m) = project("/nowhere.mp4");
    let mut tb = ToolBox::new(&p, &[], None, 0.0).unwrap();
    let id = clip.to_string();
    // trim 1 s off the front (positive delta on 'in' shortens), then split, move, retitle
    json_of(tb.call(
        "trim_item",
        &json!({"item_id": id, "edge": "in", "delta_sec": 1.0}),
    ));
    let c = json_of(tb.call("set_clip", &json!({"item_id": id, "opacity": 0.5, "scale_percent": 80, "position_x": 40, "exposure": 0.3, "fade_in_sec": 0.5, "pan": -0.25, "muted": false})));
    assert_eq!(
        (c["opacity"].as_f64(), c["scale_percent"].as_f64()),
        (Some(0.5), Some(80.0))
    );
    assert_eq!(c["color"]["exposure"], 0.3);
    assert_eq!(c["fade_in_sec"], 0.5);
    assert_eq!(c["duration_sec"], 6.0);
    let tt = json_of(tb.call("add_title", &json!({"text": "Day 3", "subtitle": "Coast", "template": "lower_third", "at_sec": 1, "duration_sec": 2})));
    assert_eq!(
        (
            tt["type"].as_str(),
            tt["template"].as_str(),
            tt["duration_sec"].as_f64()
        ),
        (Some("title"), Some("lower_third"), Some(2.0))
    );
    let title_id = tt["id"].as_str().unwrap().to_string();
    let tt = json_of(tb.call(
        "edit_title",
        &json!({"item_id": title_id, "text": "Day 4", "template": "centered", "fade_in_sec": 0.25}),
    ));
    assert_eq!(
        (
            tt["text"].as_str(),
            tt["template"].as_str(),
            tt["name"].as_str()
        ),
        (Some("Day 4"), Some("centered"), Some("Title: Day 4"))
    );
    json_of(tb.call("move_item", &json!({"item_id": title_id, "at_sec": 4.0})));
    json_of(tb.call(
        "set_track",
        &json!({"track": "A1", "volume_db": -6, "enabled": true}),
    ));
    let added = json_of(tb.call(
        "add_clip",
        &json!({"media_id": "music.wav", "track": "A1", "at_sec": 0, "duration_sec": 5}),
    ));
    assert_eq!(added["duration_sec"], 5.0);
    let video_clip = json_of(tb.call(
        "add_clip",
        &json!({"media_id": m.to_string(), "from_sec": 2.0, "duration_sec": 1.5}),
    ));
    assert_eq!(video_clip["source_start_sec"], 2.0);
    // splitting reports the pieces
    let cut = json_of(tb.call("split", &json!({"track": "V1", "at_sec": 3.0})));
    assert_eq!(cut["items_around_the_cut"].as_array().unwrap().len(), 2);
    json_of(tb.call(
        "delete_item",
        &json!({"item_id": added["id"], "ripple": false}),
    ));

    // everything it did replays onto the real project
    let mut after = p.clone();
    for op in &tb.new_ops {
        after = apply(&after, op).unwrap().0;
    }
    assert_eq!(&after, tb.project());
    after.validate().unwrap();
    assert_ne!(after, p);
}

#[test]
fn bad_arguments_give_useful_errors() {
    let (p, clip, _) = project("/nowhere.mp4");
    let mut tb = ToolBox::new(&p, &[], None, 0.0).unwrap();
    let e = |tb: &mut ToolBox, name: &str, v: Value| tb.call(name, &v).unwrap_err();
    assert!(
        e(&mut tb, "split", json!({"track": "V9", "at_sec": 1})).contains("tracks are: V1, V2, A1")
    );
    assert!(e(&mut tb, "split", json!({"track": "V1"})).contains("missing number 'at_sec'"));
    assert!(e(&mut tb, "split", json!({"track": "V1", "at_sec": 99})).contains("no item"));
    assert!(e(
        &mut tb,
        "trim_item",
        json!({"item_id": clip.to_string(), "edge": "middle", "delta_sec": 1})
    )
    .contains("'in' or 'out'"));
    assert!(e(
        &mut tb,
        "trim_item",
        json!({"item_id": clip.to_string(), "edge": "in", "delta_sec": 100})
    )
    .contains("duration"));
    assert!(
        e(&mut tb, "set_clip", json!({"item_id": clip.to_string()})).contains("nothing to change")
    );
    assert!(e(
        &mut tb,
        "set_clip",
        json!({"item_id": clip.to_string(), "contrast": -2})
    )
    .contains("color values out of range"));
    assert!(e(&mut tb, "add_clip", json!({"media_id": "nope"})).contains("no media"));
    assert!(e(
        &mut tb,
        "add_clip",
        json!({"media_id": "music.wav", "track": "V1"})
    )
    .contains("cannot go on track"));
    assert!(e(
        &mut tb,
        "add_title",
        json!({"text": "x", "at_sec": 0, "template": "fancy"})
    )
    .contains("template must be"));
    assert!(e(
        &mut tb,
        "add_title",
        json!({"text": "x", "at_sec": 0, "track": "A1"})
    )
    .contains("video tracks"));
    assert!(e(
        &mut tb,
        "edit_title",
        json!({"item_id": clip.to_string(), "text": "x"})
    )
    .contains("not a title"));
    assert!(e(&mut tb, "set_track", json!({"track": "V1"})).contains("nothing to change"));
    assert!(
        e(&mut tb, "find_silences", json!({"item_id": "zzzzzzzz"})).contains("no clip or title")
    );
    assert!(tb.new_ops.is_empty(), "failed calls change nothing");
}

/// 1 s tone, 2 s silence, 1.5 s tone, 1.5 s silence, 1 s tone: 7 s.
fn talk_file() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("captaine-agent-test-{}", std::process::id()));
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
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "ffmpeg is needed for this test");
    path
}

#[test]
fn silence_tools_analyse_real_audio() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let file = talk_file();
    let (p, clip, _) = project(file.to_str().unwrap());
    let mut tb = ToolBox::new(&p, &[], None, 0.0).unwrap();
    let found = json_of(tb.call("find_silences", &json!({"item_id": clip.to_string()})));
    let s = found["silences"].as_array().unwrap();
    assert_eq!(s.len(), 2, "{found}");
    assert!(
        (s[0]["start_sec"].as_f64().unwrap() - 1.0).abs() < 0.1
            && (s[0]["end_sec"].as_f64().unwrap() - 3.0).abs() < 0.1,
        "{found}"
    );
    assert!(
        (s[1]["length_sec"].as_f64().unwrap() - 1.5).abs() < 0.1,
        "{found}"
    );
    assert!(tb.new_ops.is_empty(), "analysis is read-only");
    let out = json_of(tb.call("remove_silences", &json!({"item_id": clip.to_string()})));
    assert_eq!(out["cuts"], 2);
    let removed = out["removed_sec"].as_f64().unwrap();
    assert!((removed - (2.0 - 0.24 + 1.5 - 0.24)).abs() < 0.3, "{out}");
    assert!(
        (out["timeline_duration_sec"].as_f64().unwrap() - (7.0 - removed)).abs() < 0.2,
        "{out}"
    );
    // a stricter threshold finds nothing and edits nothing
    let mut tb2 = ToolBox::new(&p, &[], None, 0.0).unwrap();
    let none = json_of(tb2.call(
        "remove_silences",
        &json!({"item_id": clip.to_string(), "min_seconds": 5.0}),
    ));
    assert_eq!(none["cuts"], 0);
    assert!(tb2.new_ops.is_empty());
}

/// One-shot HTTP server: records the request, answers with `status` and `body`.
fn fake_server(status: u16, body: &str) -> (String, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/messages", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(String::new()));
    let (seen2, body) = (seen.clone(), body.to_string());
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = s.read(&mut chunk).unwrap();
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(i) = text.find("\r\n\r\n") {
                let len = text
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if buf.len() >= i + 4 + len {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        *seen2.lock().unwrap() = String::from_utf8_lossy(&buf).to_string();
        let resp = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
        s.write_all(resp.as_bytes()).unwrap();
    });
    (url, seen)
}

#[test]
fn the_http_transport_sends_the_right_request_and_maps_errors() {
    let (url, seen) = fake_server(
        200,
        r#"{"stop_reason":"end_turn","content":[{"type":"text","text":"hi"}]}"#,
    );
    let tr = AnthropicTransport::with_endpoint("sk-test-123".into(), &url);
    let resp = tr.send(&json!({"model": "m", "messages": []})).unwrap();
    assert_eq!(resp["content"][0]["text"], "hi");
    let raw = seen.lock().unwrap().to_lowercase();
    assert!(raw.starts_with("post /v1/messages"), "{raw}");
    assert!(
        raw.contains("x-api-key: sk-test-123")
            && raw.contains("anthropic-version: 2023-06-01")
            && raw.contains("content-type: application/json"),
        "{raw}"
    );
    assert!(raw.contains(r#"{"messages":[],"model":"m"}"#), "{raw}");

    for (status, body, needle) in [
        (
            401,
            r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
            "rejected",
        ),
        (
            429,
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#,
            "rate limited",
        ),
        (
            500,
            r#"{"type":"error","error":{"message":"boom"}}"#,
            "boom",
        ),
        (200, "not json at all", "unexpected response"),
    ] {
        let (url, _) = fake_server(status, body);
        let err = AnthropicTransport::with_endpoint("k".into(), &url)
            .send(&json!({}))
            .unwrap_err();
        assert!(err.contains(needle), "{status}: {err}");
    }
    let err = AnthropicTransport::with_endpoint("k".into(), "http://127.0.0.1:1/v1/messages")
        .send(&json!({}))
        .unwrap_err();
    assert!(err.contains("could not reach"), "{err}");
}

#[test]
fn credentials_round_trip_through_a_private_file() {
    // point HOME at a scratch directory so the real key file is never touched
    let home = std::env::temp_dir().join(format!("captaine-home-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: tests in this file that read HOME are this one only
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
    assert_eq!(Credentials::load(), None);
    Credentials::save("  sk-abc \n").unwrap();
    assert_eq!(Credentials::load().as_deref(), Some("sk-abc"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(Credentials::key_file().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "only the owner can read it");
    }
    unsafe { std::env::set_var("ANTHROPIC_API_KEY", "from-env") };
    assert_eq!(
        Credentials::load().as_deref(),
        Some("from-env"),
        "the environment wins"
    );
    unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
    Credentials::forget();
    assert_eq!(Credentials::load(), None);
}
