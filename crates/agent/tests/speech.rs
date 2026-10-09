//! Edits driven by speech. The cutting maths runs everywhere; the end-to-end test needs the local Whisper
//! and the Mac's `say` voice and is skipped without them.

use std::process::Command;
use std::sync::Arc;

use captaine_agent::{cut_ranges, ToolBox};
use captaine_media::media_ref;
use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};
use serde_json::json;

const R: Rational = Rational::FPS_30;
fn t(v: i64) -> RationalTime {
    RationalTime::new(v, R)
}

fn project_with_clip(duration: i64) -> (Editor, Clip, Id) {
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: R,
        ..ProjectSettings::default()
    }));
    let m = MediaRef {
        id: Id::new(),
        name: "talk.wav".into(),
        path: "/nowhere/talk.wav".into(),
        duration: Some(t(3000)),
        has_audio: true,
        width: 0,
        height: 0,
        has_video: false,
        proxy: None,
    };
    ed.apply(Author::User, Op::AddMedia { media: m.clone() })
        .unwrap();
    let (tl, tr) = (Id::new(), Id::new());
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "m", R),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: tl,
            index: 0,
            track: Track::new(tr, "A1", TrackKind::Audio),
        },
    )
    .unwrap();
    let clip = Clip::new(Id::new(), "talk", m.id, t(0), t(duration));
    ed.apply(Author::User, edit::append(ed.project(), tr, clip.clone()))
        .unwrap();
    (ed, clip, tl)
}

fn lengths(ed: &Editor, tl: Id) -> Vec<i64> {
    ed.project().timeline(tl).unwrap().tracks[0]
        .items
        .iter()
        .map(|i| i.duration().value)
        .collect()
}

#[test]
fn cutting_ranges_closes_up_and_handles_both_ends() {
    let (mut ed, clip, tl) = project_with_clip(300); // 10 s
    let before = ed.snapshot();
    // the middle, the very start and the very end
    let (ops, removed) = cut_ranges(&clip, 30.0, &[(2.0, 3.0), (0.0, 0.5), (9.5, 10.0)]);
    assert!((removed - 2.0).abs() < 1e-9);
    ed.apply(
        Author::User,
        Op::Transaction {
            description: "cuts".into(),
            ops,
        },
    )
    .unwrap();
    assert_eq!(lengths(&ed, tl), [45, 195]);
    assert_eq!(
        ed.project().timeline(tl).unwrap().duration().value,
        300 - 60
    );
    ed.undo();
    assert_eq!(ed.project(), &before, "one undo step");
    // ranges outside the clip or empty do nothing
    let (none, zero) = cut_ranges(&clip, 30.0, &[(11.0, 12.0), (3.0, 3.0)]);
    assert!(none.is_empty() && zero == 0.0);
    // everything
    let (all, _) = cut_ranges(&clip, 30.0, &[(0.0, 10.0)]);
    ed.apply(
        Author::User,
        Op::Transaction {
            description: "all".into(),
            ops: all,
        },
    )
    .unwrap();
    assert_eq!(ed.project().timeline(tl).unwrap().duration().value, 0);
}

fn have(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn fillers_and_captions_from_real_speech() {
    if captaine_speech::unavailable().is_some() || !have("say") || !have("ffmpeg") {
        eprintln!("skipped: {:?}", captaine_speech::unavailable());
        return;
    }
    let dir = std::env::temp_dir().join(format!("captaine-agent-speech-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let aiff = dir.join("talk.aiff");
    let wav = dir.join("talk.wav");
    assert!(Command::new("say")
        .arg("-o")
        .arg(&aiff)
        .arg("Hello everyone. Um, this is a test of the video editor. Uh, thank you for watching.")
        .status()
        .unwrap()
        .success());
    assert!(Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i"])
        .arg(&aiff)
        .arg(&wav)
        .status()
        .unwrap()
        .success());
    let media = media_ref(&wav, Id::new()).unwrap();
    let rate = Rational::FPS_30;
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: rate,
        ..ProjectSettings::default()
    }));
    let (tl, tr) = (Id::new(), Id::new());
    ed.apply(
        Author::User,
        Op::AddMedia {
            media: media.clone(),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "m", rate),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: tl,
            index: 0,
            track: Track::new(tr, "A1", TrackKind::Audio),
        },
    )
    .unwrap();
    let len = media
        .duration
        .unwrap()
        .rescale(rate, captaine_time::Rounding::Floor)
        .unwrap()
        .value;
    let clip = Clip::new(
        Id::new(),
        "talk",
        media.id,
        RationalTime::new(0, media.duration.unwrap().rate),
        RationalTime::new(len, rate),
    );
    let id = clip.id;
    ed.apply(Author::User, edit::append(ed.project(), tr, clip))
        .unwrap();

    let mut tools = ToolBox::new(ed.project(), &[], None, 0.0).unwrap();
    let out = tools
        .call("add_captions", &json!({"item_id": id.to_string()}))
        .unwrap();
    assert!(out.contains("\"captions\""), "{out}");
    let p = tools.project().clone();
    let caps = p
        .timeline(tl)
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == "Captions")
        .expect("a captions track");
    let texts: Vec<String> = caps
        .items
        .iter()
        .filter_map(|i| i.as_title())
        .map(|t| t.text.to_lowercase())
        .collect();
    assert!(texts.len() >= 2, "{texts:?}");
    assert!(texts.join(" ").contains("thank you"), "{texts:?}");
    // captions start when the speech does and never overlap
    let starts = caps.item_starts();
    assert!(starts.windows(2).all(|w| w[0] <= w[1]));
    // SRT export has the same lines
    let srt = captaine_interchange::srt::export(&p, tl).unwrap();
    assert!(
        srt.contains("-->") && srt.to_lowercase().contains("hello everyone"),
        "{srt}"
    );

    // cutting the hesitations shortens the clip and keeps the rest
    let mut tools = ToolBox::new(ed.project(), &[], None, 0.0).unwrap();
    let out = tools
        .call("remove_filler_words", &json!({"item_id": id.to_string()}))
        .unwrap();
    assert!(out.contains("\"cuts\""), "{out}");
    let after = tools.project().timeline(tl).unwrap().duration().value;
    assert!(after < len - 5, "fillers were removed: {len} -> {after}");
    assert!(after > len / 2);
    let _ = std::fs::remove_dir_all(dir);
    let _ = Arc::new(0);
}
