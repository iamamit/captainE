//! CoCaptain's eyes: the look tools render real pictures (with pending edits), respect the user's choice and the
//! picture budget, and old pictures are dropped from long conversations. Skipped when FFmpeg is missing.

use std::path::PathBuf;
use std::process::Command;

use captaine_agent::{prune_pictures, ToolBox, IMAGE_BUDGET};
use captaine_media::media_ref;
use captaine_model::*;
use captaine_ops::{edit, Author, Editor, Op};
use captaine_time::{Rational, RationalTime};
use serde_json::{json, Value};

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

/// 2 s of FFmpeg's test pattern, 320x180 at 30 fps.
fn pattern_video() -> PathBuf {
    let p = std::env::temp_dir().join(format!("captaine-look-{}.mp4", std::process::id()));
    if !p.exists() {
        let ok = Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=320x180:r=30:d=2",
            ])
            .args(["-pix_fmt", "yuv420p"])
            .arg(&p)
            .status()
            .unwrap()
            .success();
        assert!(ok);
    }
    p
}

/// A 1280x720 project with the pattern on V1 for its whole 2 s; returns it and the clip id.
fn project() -> (Project, Id) {
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: Rational::FPS_30,
        width: 1280,
        height: 720,
        sample_rate: 48_000,
    }));
    let (tl, v1) = (Id::new(), Id::new());
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "Main", Rational::FPS_30),
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
    let m = media_ref(&pattern_video(), Id::new()).unwrap();
    ed.apply(Author::User, Op::AddMedia { media: m.clone() })
        .unwrap();
    let clip = Clip::new(
        Id::new(),
        m.name.clone(),
        m.id,
        RationalTime::new(0, Rational::FPS_30),
        RationalTime::new(60, Rational::FPS_30),
    );
    let id = clip.id;
    ed.apply(Author::User, edit::append(ed.project(), v1, clip))
        .unwrap();
    (ed.project().clone(), id)
}

/// Width and height from a PNG's header.
fn png_size(png: &[u8]) -> (u32, u32) {
    assert_eq!(&png[1..4], b"PNG");
    let be = |i: usize| u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]);
    (be(16), be(20))
}

#[test]
fn look_shows_the_timeline_with_the_edits_being_made() {
    if !have_ffmpeg() {
        return;
    }
    let (p, clip) = project();
    let mut tb = ToolBox::new(&p, &[], None, 0.0).unwrap();
    // nothing is shown until the user allows it
    let e = tb.call("look", &json!({"times": [0.5]})).unwrap_err();
    assert!(e.contains("see the video"), "{e}");
    assert!(tb.take_images().is_empty());

    tb.allow_frames = true;
    let out: Value =
        serde_json::from_str(&tb.call("look", &json!({"times": [0.5, 1.5]})).unwrap()).unwrap();
    assert_eq!(out["pictures"], 1);
    assert_eq!(
        out["frames"][0]["layers_bottom_to_top"][0],
        format!("V1 {}", p.media.values().next().unwrap().name)
    );
    let sheet = tb.take_images();
    assert_eq!(sheet.len(), 1);
    // two 480-wide frames side by side on one sheet
    assert_eq!(png_size(&sheet[0]), (2 * 480 + 4, 270));

    // one big picture, then the same moment after the clip is deleted: the picture changes
    tb.call(
        "look",
        &json!({"times": [1.0], "sheet": false, "width": 640}),
    )
    .unwrap();
    let before = tb.take_images();
    assert_eq!(png_size(&before[0]), (640, 360));
    tb.call(
        "delete_item",
        &json!({"item_id": clip.to_string(), "ripple": false}),
    )
    .unwrap();
    tb.call(
        "look",
        &json!({"times": [1.0], "sheet": false, "width": 640}),
    )
    .unwrap();
    let after = tb.take_images();
    assert_ne!(
        before, after,
        "look renders the scratch copy, edits included"
    );
}

#[test]
fn look_media_reads_any_file_and_the_budget_holds() {
    if !have_ffmpeg() {
        return;
    }
    let (p, _) = project();
    let mut tb = ToolBox::new(&p, &[], None, 0.0).unwrap();
    tb.allow_frames = true;
    let path = pattern_video();
    let out: Value = serde_json::from_str(
        &tb.call("look_media", &json!({"path": path, "count": 4}))
            .unwrap(),
    )
    .unwrap();
    assert!((out["duration_sec"].as_f64().unwrap() - 2.0).abs() < 0.05);
    assert_eq!(out["times_sec"].as_array().unwrap().len(), 4);
    let sheet = tb.take_images();
    // four frames at 320 (the file is smaller than the asked 480) on a 4-wide sheet
    assert_eq!(png_size(&sheet[0]), (4 * 320 + 3 * 4, 180));

    for _ in 1..IMAGE_BUDGET {
        tb.call("look", &json!({"times": [0.2], "width": 64}))
            .unwrap();
    }
    let e = tb.call("look", &json!({"times": [0.2]})).unwrap_err();
    assert!(e.contains("budget"), "{e}");
}

#[test]
fn only_the_latest_pictures_stay_in_the_conversation() {
    let img = json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "x"}});
    let result = |id: &str| json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": [{"type": "text", "text": "look"}, img.clone()]}]});
    let mut messages = vec![
        json!({"role": "user", "content": "hi"}),
        result("a"),
        json!({"role": "assistant", "content": "…"}),
        result("b"),
        result("c"),
    ];
    prune_pictures(&mut messages, 2);
    let images = |m: &Value| {
        m["content"][0]["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["type"] == "image")
            .count()
    };
    assert_eq!(images(&messages[1]), 0, "the oldest pictures are dropped");
    assert_eq!(
        messages[1]["content"][0]["content"][1]["text"],
        "[picture shown earlier]"
    );
    assert_eq!(images(&messages[3]), 1);
    assert_eq!(images(&messages[4]), 1);
}

/// The editor itself: files come into the pool as part of the edit; a new project, opening one and exporting are
/// handed to the editor for after the reply.
#[test]
fn import_and_app_actions() {
    if !have_ffmpeg() {
        return;
    }
    let (p, _) = project();
    let mut tb = ToolBox::new(&p, &[], None, 0.0).unwrap();
    let path = pattern_video();
    // already in the pool: nothing new
    let out: Value =
        serde_json::from_str(&tb.call("import_media", &json!({"paths": [path]})).unwrap()).unwrap();
    assert_eq!(out["imported"][0]["note"], "already in the pool");
    assert!(tb.new_ops.is_empty());
    // a new file becomes an AddMedia op in the suggestion
    let copy = std::env::temp_dir().join(format!("captaine-look-copy-{}.mp4", std::process::id()));
    std::fs::copy(&path, &copy).unwrap();
    let out: Value =
        serde_json::from_str(&tb.call("import_media", &json!({"paths": [copy]})).unwrap()).unwrap();
    assert!(out["imported"][0]["picture"].as_bool().unwrap());
    assert!(matches!(tb.new_ops[0], Op::AddMedia { .. }));
    assert!(tb
        .call("import_media", &json!({"paths": ["/no/such.mp4"]}))
        .is_err());

    tb.call(
        "new_project",
        &json!({"name": "Reel", "width": 1080, "height": 1920, "fps": "30"}),
    )
    .unwrap();
    tb.call("export_video", &json!({"path": "/tmp/reel.mp4"}))
        .unwrap();
    assert!(tb
        .call("export_video", &json!({"path": "/tmp/reel.avi"}))
        .is_err());
    assert!(tb
        .call("open_project", &json!({"path": "/no/such.captaine.json"}))
        .is_err());
    assert_eq!(
        tb.app_actions,
        vec![
            captaine_agent::AppAction::NewProject {
                name: "Reel".into(),
                width: 1080,
                height: 1920,
                fps: "30".into()
            },
            captaine_agent::AppAction::Export {
                path: "/tmp/reel.mp4".into()
            },
        ]
    );
    let _ = std::fs::remove_file(copy);
}
