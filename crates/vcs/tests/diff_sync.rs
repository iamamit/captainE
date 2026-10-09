mod common;

use captaine_model::*;
use captaine_ops::*;
use captaine_vcs::*;
use common::*;

fn kinds(changes: &[Change]) -> Vec<(String, ChangeKind)> {
    changes
        .iter()
        .filter(|c| !c.is_noise())
        .map(|c| (c.name.clone(), c.kind))
        .collect()
}

#[test]
fn identical_projects_have_no_differences() {
    let p = base_project();
    assert!(diff(&p, &p).is_empty());
    assert_eq!(summarize(&[], R), "no differences");
}

#[test]
fn a_trim_is_a_trim_and_what_follows_only_shifts() {
    let a = base_project();
    let b = run(
        &a,
        &[Op::Trim {
            item: id(C2),
            edge: Edge::Out,
            delta: t(-24),
        }],
    );
    let d = diff(&a, &b);
    assert_eq!(kinds(&d), [("c22".to_string(), ChangeKind::Trimmed)]);
    let shifted: Vec<_> = d.iter().filter(|c| c.kind == ChangeKind::Shifted).collect();
    assert_eq!(shifted.len(), 1, "c3 is pushed earlier, not edited");
    assert_eq!(
        d.iter().find(|c| c.name == "c22").unwrap().range,
        Some((96, 96 + 72))
    );
    assert!(
        d.iter()
            .find(|c| c.name == "c22")
            .unwrap()
            .summary
            .contains("4.0 s → 3.0 s"),
        "{:?}",
        d[0].summary
    );
    assert!(summarize(&d, R).contains("trims 1 item"));
}

#[test]
fn removing_adding_lifting_and_modifying() {
    let a = base_project();
    // ripple delete c1; lift c2; change c3's opacity; add a title
    let mut title = Title::new(id(77), TitleTemplate::LowerThird, t(48));
    title.text = "Hello".into();
    title.name = "Title: Hello".into();
    let lift = edit::lift(&a, id(C2)).unwrap();
    let b = run(
        &a,
        &[
            Op::RemoveItem { item: id(C1) },
            lift,
            Op::SetVideo {
                item: id(C3),
                video: video(0.5),
            },
            edit::overwrite_item(&a, id(V2), t(24), Item::Title(title)).unwrap(),
        ],
    );
    let d = diff(&a, &b);
    let k = kinds(&d);
    assert!(
        k.contains(&("c21".to_string(), ChangeKind::Removed)),
        "{k:?}"
    );
    assert!(
        k.contains(&("c22".to_string(), ChangeKind::Removed)),
        "a lifted clip is a removed clip: {k:?}"
    );
    assert!(
        k.contains(&("c23".to_string(), ChangeKind::Modified)),
        "{k:?}"
    );
    assert!(
        k.contains(&("Title: Hello".to_string(), ChangeKind::Added)),
        "{k:?}"
    );
    let c3 = d.iter().find(|c| c.name == "c23").unwrap();
    assert_eq!(c3.fields, ["video.opacity"]);
    assert!(
        c3.summary.contains("video.opacity 1 → 0.50"),
        "{}",
        c3.summary
    );
    // the lift's gap is bookkeeping, not a listed change
    assert!(d.iter().any(|c| c.what == What::Gap && c.is_noise()));
    let s = summarize(&d, R);
    assert!(
        s.contains("removes 2 clip or titles (8.0 s)")
            && s.contains("adds 1 title")
            && s.contains("changes the properties of 1 item"),
        "{s}"
    );
}

#[test]
fn moves_between_tracks_and_reordering_are_moves() {
    let a = base_project();
    let b = run(&a, &[edit::move_item(&a, id(C2), id(V2), t(0)).unwrap()]);
    let k = kinds(&diff(&a, &b));
    assert!(k.contains(&("c22".to_string(), ChangeKind::Moved)), "{k:?}");
    // swap order within a track: only the item that left the sequence counts as moved, not both neighbours
    let c = run(
        &a,
        &[
            Op::RemoveItem { item: id(C1) },
            Op::InsertItem {
                track: id(V1),
                index: 2,
                item: clip(C1, 0, 96),
            },
        ],
    );
    let k = kinds(&diff(&a, &c));
    assert_eq!(k, [("c21".to_string(), ChangeKind::Moved)], "{k:?}");
}

#[test]
fn tracks_media_and_properties() {
    let a = base_project();
    let b = run(
        &a,
        &[
            Op::SetTrack {
                track: id(A1),
                enabled: false,
                gain_db: -6.0,
            },
            Op::AddTrack {
                timeline: id(TL),
                index: 3,
                track: Track::new(id(99), "V3", TrackKind::Video),
            },
            Op::SetProxy {
                media: id(MEDIA),
                proxy: Some("/p.mp4".into()),
            },
        ],
    );
    let d = diff(&a, &b);
    let s: Vec<_> = d.iter().map(|c| c.summary.clone()).collect();
    assert!(
        s.iter()
            .any(|x| x.contains("Track A1 hidden or muted") && x.contains("gain_db")),
        "{s:?}"
    );
    assert!(s.iter().any(|x| x == "Added track V3"), "{s:?}");
    assert!(
        s.iter().any(|x| x.contains("Changed media 'm1': proxy")),
        "{s:?}"
    );
    let sentence = summarize(&d, R);
    assert!(
        sentence.contains("adds 1 track")
            && sentence.contains("changes 1 track or timeline setting")
            && sentence.contains("changes 1 item in the media pool"),
        "{sentence}"
    );
}

#[test]
fn diff_is_directional_and_keys_are_stable() {
    let a = base_project();
    let b = run(&a, &[Op::RemoveItem { item: id(C1) }]);
    let (fwd, back) = (diff(&a, &b), diff(&b, &a));
    assert_eq!(
        fwd.iter().find(|c| !c.is_noise()).unwrap().kind,
        ChangeKind::Removed
    );
    assert_eq!(
        back.iter().find(|c| !c.is_noise()).unwrap().kind,
        ChangeKind::Added
    );
    assert_eq!(
        fwd.iter().find(|c| !c.is_noise()).unwrap().id,
        back.iter().find(|c| !c.is_noise()).unwrap().id
    );
}

#[test]
fn sync_ops_reproduce_the_target_exactly() {
    let a = base_project();
    // a handful of hand-made cases
    let cases: Vec<Project> = vec![
        run(&a, &[Op::RemoveItem { item: id(C1) }]),
        run(&a, &[edit::move_item(&a, id(C1), id(V1), t(200)).unwrap()]),
        run(
            &a,
            &[
                Op::SetVideo {
                    item: id(C2),
                    video: video(0.3),
                },
                Op::SetTrack {
                    track: id(V2),
                    enabled: false,
                    gain_db: -3.0,
                },
            ],
        ),
        run(
            &a,
            &[
                Op::AddTrack {
                    timeline: id(TL),
                    index: 0,
                    track: Track::new(id(98), "V0", TrackKind::Video),
                },
                Op::MoveTrack {
                    track: id(V2),
                    index: 0,
                },
            ],
        ),
        run(&a, &[Op::RemoveTrack { track: id(A1) }]),
    ];
    for target in &cases {
        let ops = sync_ops(&a, target).unwrap();
        assert_eq!(&run(&a, &ops), target);
        assert!(
            sync_ops(target, target).unwrap().is_empty(),
            "no change, no ops"
        );
    }
    // renaming a timeline is an op; changing its frame rate has none and says so instead of guessing
    let mut renamed = a.clone();
    renamed.timeline_mut(id(TL)).unwrap().name = "other".into();
    let ops = sync_ops(&a, &renamed).unwrap();
    assert_eq!(&run(&a, &ops), &renamed);
    // markers: new, moved and removed ones
    let mut marked = a.clone();
    {
        let tl = marked.timeline_mut(id(TL)).unwrap();
        tl.markers.push(Marker::new(Id::new(), 30, "Chapter 1"));
        tl.markers.push(Marker::new(Id::new(), 10, "Start"));
        tl.markers.sort_by_key(|m| (m.tick, m.id));
    }
    let ops = sync_ops(&a, &marked).unwrap();
    assert_eq!(&run(&a, &ops), &marked);
    let mut moved = marked.clone();
    {
        let tl = moved.timeline_mut(id(TL)).unwrap();
        tl.markers[0].tick = 50;
        tl.markers.sort_by_key(|m| (m.tick, m.id));
        tl.markers.pop();
    }
    let ops = sync_ops(&marked, &moved).unwrap();
    assert_eq!(&run(&marked, &ops), &moved);
    let mut retimed = a.clone();
    retimed.timeline_mut(id(TL)).unwrap().rate = captaine_time::Rational::FPS_25;
    assert!(sync_ops(&a, &retimed).unwrap_err().contains("retimed"));
}

#[test]
fn fuzz_sync_between_any_two_edited_states() {
    let base = base_project();
    for seed in 1..=60u64 {
        let (x, _) = edit_randomly(&base, seed, 12);
        let (y, _) = edit_randomly(&base, seed + 1000, 12);
        x.validate().unwrap();
        for (from, to) in [(&base, &x), (&x, &base), (&x, &y)] {
            let ops = sync_ops(from, to).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            assert_eq!(&run(from, &ops), to, "seed {seed}");
        }
    }
}
