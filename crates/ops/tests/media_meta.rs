//! Bins, tags and notes on media.

use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

fn editor() -> (Editor, Id) {
    let mut ed = Editor::new(Project::new(ProjectSettings::default()));
    let id = Id::from_u128(7);
    ed.apply(
        Author::User,
        Op::AddMedia {
            media: MediaRef {
                id,
                name: "beach.mp4".into(),
                path: "/beach.mp4".into(),
                duration: Some(RationalTime::new(100, Rational::FPS_30)),
                has_audio: true,
                width: 16,
                height: 9,
                has_video: true,
                proxy: None,
            },
        },
    )
    .unwrap();
    (ed, id)
}

#[test]
fn media_can_be_filed_tagged_and_found_and_it_undoes() {
    let (mut ed, id) = editor();
    let before = ed.snapshot();
    let meta = MediaMeta {
        bin: "Day 1".into(),
        tags: vec!["Drone".into(), "sunset".into()],
        note: "golden hour, wide".into(),
    };
    ed.apply(
        Author::User,
        Op::SetMediaMeta {
            media: id,
            meta: Some(meta.clone()),
        },
    )
    .unwrap();
    assert_eq!(ed.project().meta[&id], meta);
    assert!(meta.matches("drone") && meta.matches("day 1") && meta.matches("golden"));
    assert!(!meta.matches("interview"));
    // it survives a save and load
    let json = serde_json::to_string(ed.project()).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.meta[&id], meta);
    // an empty one removes the entry, so the project file stays clean
    ed.apply(
        Author::User,
        Op::SetMediaMeta {
            media: id,
            meta: Some(MediaMeta::default()),
        },
    )
    .unwrap();
    assert!(ed.project().meta.is_empty());
    ed.undo();
    assert_eq!(ed.project().meta[&id], meta);
    ed.undo();
    assert_eq!(ed.project(), &before);
    // nonsense is refused
    for bad in [
        MediaMeta {
            tags: vec!["".into()],
            ..MediaMeta::default()
        },
        MediaMeta {
            bin: "x".repeat(61),
            ..MediaMeta::default()
        },
    ] {
        assert!(ed
            .apply(
                Author::User,
                Op::SetMediaMeta {
                    media: id,
                    meta: Some(bad)
                }
            )
            .is_err());
    }
    assert!(ed
        .apply(
            Author::User,
            Op::SetMediaMeta {
                media: Id::from_u128(99),
                meta: None
            }
        )
        .is_err());
}
