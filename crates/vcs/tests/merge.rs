mod common;

use std::collections::HashMap;

use captaine_model::*;
use captaine_ops::*;
use captaine_vcs::*;
use common::*;

fn take(p: &MergePlan) -> HashMap<String, Choice> {
    p.entries
        .iter()
        .map(|e| (e.change.id.clone(), Choice::Take))
        .collect()
}

fn merged(base: &Project, ours: &Project, theirs: &Project) -> MergeOutcome {
    let m = merge(base, ours, theirs, &HashMap::new()).unwrap();
    // every merge result is sound and its ops really produce it from our side
    m.project.validate().unwrap();
    assert_eq!(run(ours, &m.ops), m.project);
    m
}

#[test]
fn independent_edits_on_both_sides_combine() {
    let base = base_project();
    let ours = run(
        &base,
        &[Op::Trim {
            item: id(C1),
            edge: Edge::Out,
            delta: t(-24),
        }],
    );
    let mut title = Title::new(id(70), TitleTemplate::LowerThird, t(48));
    title.text = "Hi".into();
    let theirs = run(
        &base,
        &[
            Op::SetVideo {
                item: id(C3),
                video: video(0.4),
            },
            edge_title(&base, title),
        ],
    );
    let plan = plan_merge(&base, &ours, &theirs);
    assert_eq!(plan.conflicts().count(), 0);
    let m = merged(&base, &ours, &theirs);
    assert_eq!(clip_of(&m.project, C1).duration, t(72), "our trim is kept");
    assert_eq!(
        clip_of(&m.project, C3).video.opacity,
        0.4,
        "their change is brought in"
    );
    assert!(has(&m.project, id(70)), "and their new title");
    assert!(m.skipped.is_empty());
}

fn edge_title(base: &Project, title: Title) -> Op {
    edit::overwrite_item(base, id(V2), t(24), Item::Title(title)).unwrap()
}

#[test]
fn our_ripple_delete_does_not_disturb_their_later_edit() {
    let base = base_project();
    // ours: ripple-delete c1 (everything after moves 4 s earlier). theirs: trim c3 and cut c2
    let ours = run(&base, &[Op::RemoveItem { item: id(C1) }]);
    let theirs = run(
        &base,
        &[
            Op::Trim {
                item: id(C3),
                edge: Edge::Out,
                delta: t(-24),
            },
            Op::Split {
                item: id(C2),
                at: t(48),
                new_id: id(80),
            },
        ],
    );
    let m = merged(&base, &ours, &theirs);
    assert!(!has(&m.project, id(C1)));
    assert_eq!(clip_of(&m.project, C3).duration, t(72));
    assert!(has(&m.project, id(80)));
    assert_eq!(
        spans(&m.project, "V1"),
        [(0, 48), (48, 48), (96, 72)],
        "c2 cut in two, then the trimmed c3, all closed up"
    );
}

#[test]
fn same_field_different_value_conflicts_and_disjoint_fields_do_not() {
    let base = base_project();
    let ours = run(
        &base,
        &[Op::SetVideo {
            item: id(C2),
            video: video(0.2),
        }],
    );
    let theirs = run(
        &base,
        &[Op::SetVideo {
            item: id(C2),
            video: video(0.9),
        }],
    );
    let plan = plan_merge(&base, &ours, &theirs);
    let c = plan.conflicts().next().expect("a conflict");
    assert!(
        matches!(&c.status, Status::Conflict(m) if m.contains("video.opacity")),
        "{:?}",
        c.status
    );
    assert_eq!(c.ours.as_ref().unwrap().kind, ChangeKind::Modified);
    // default: ours wins
    assert_eq!(
        clip_of(&merged(&base, &ours, &theirs).project, C2)
            .video
            .opacity,
        0.2
    );
    // choosing theirs
    let pick = HashMap::from([(c.change.id.clone(), Choice::Take)]);
    let m = merge(&base, &ours, &theirs, &pick).unwrap();
    assert_eq!(clip_of(&m.project, C2).video.opacity, 0.9);
    assert_eq!(run(&ours, &m.ops), m.project);

    // different fields of the same clip merge cleanly
    let pos = VideoProps {
        transform: Transform {
            position: (40.0, 0.0),
            ..Transform::default()
        },
        ..VideoProps::default()
    };
    let theirs2 = run(
        &base,
        &[Op::SetVideo {
            item: id(C2),
            video: pos,
        }],
    );
    assert_eq!(plan_merge(&base, &ours, &theirs2).conflicts().count(), 0);
    let m = merged(&base, &ours, &theirs2);
    let c2 = clip_of(&m.project, C2);
    assert_eq!(
        (c2.video.opacity, c2.video.transform.position.0),
        (0.2, 40.0),
        "both changes survive"
    );
    // the same value on both sides is not a conflict either
    let same = run(
        &base,
        &[Op::SetVideo {
            item: id(C2),
            video: video(0.2),
        }],
    );
    assert_eq!(plan_merge(&base, &ours, &same).conflicts().count(), 0);
}

#[test]
fn deleted_on_one_side_changed_on_the_other_is_a_conflict() {
    let base = base_project();
    let ours = run(&base, &[Op::RemoveItem { item: id(C2) }]);
    let theirs = run(
        &base,
        &[Op::SetVideo {
            item: id(C2),
            video: video(0.5),
        }],
    );
    let plan = plan_merge(&base, &ours, &theirs);
    assert!(
        matches!(&plan.conflicts().next().unwrap().status, Status::Conflict(m) if m.contains("deleted"))
    );
    assert!(
        !has(&merged(&base, &ours, &theirs).project, id(C2)),
        "default keeps our deletion"
    );
    // and the other way round
    let plan = plan_merge(&base, &theirs, &ours);
    assert!(
        matches!(&plan.conflicts().next().unwrap().status, Status::Conflict(m) if m.contains("deleted"))
    );
    // both deleting it is fine
    let both = run(&base, &[Op::RemoveItem { item: id(C2) }]);
    assert_eq!(plan_merge(&base, &ours, &both).conflicts().count(), 0);
}

#[test]
fn both_cutting_the_same_clip_is_a_conflict() {
    let base = base_project();
    let ours = run(
        &base,
        &[Op::Split {
            item: id(C2),
            at: t(30),
            new_id: id(81),
        }],
    );
    let theirs = run(
        &base,
        &[Op::Split {
            item: id(C2),
            at: t(60),
            new_id: id(82),
        }],
    );
    let plan = plan_merge(&base, &ours, &theirs);
    let c = plan.conflicts().next().expect("both re-cut c2");
    assert!(
        matches!(&c.status, Status::Conflict(m) if m.contains("duration")),
        "{:?}",
        c.status
    );
}

#[test]
fn both_inserting_at_the_same_place_keeps_both() {
    let base = base_project();
    let (a, b) = (clip(90, 0, 24), clip(91, 0, 48));
    let ours = run(
        &base,
        &[Op::InsertItem {
            track: id(V1),
            index: 1,
            item: a,
        }],
    );
    let theirs = run(
        &base,
        &[Op::InsertItem {
            track: id(V1),
            index: 1,
            item: b,
        }],
    );
    let m = merged(&base, &ours, &theirs);
    let order: Vec<Id> = items(&m.project, "V1").iter().map(|i| i.id()).collect();
    assert_eq!(order.len(), 5);
    assert!(order.contains(&id(90)) && order.contains(&id(91)));
    let pos = |n| order.iter().position(|x| *x == id(n)).unwrap();
    assert!(
        pos(C1) < pos(90).min(pos(91)) && pos(90).max(pos(91)) < pos(C2),
        "both land between c1 and c2"
    );
}

#[test]
fn moving_a_clip_and_editing_it_elsewhere_both_apply() {
    let base = base_project();
    let ours = run(
        &base,
        &[Op::SetVideo {
            item: id(C2),
            video: video(0.5),
        }],
    );
    let theirs = run(
        &base,
        &[edit::move_item(&base, id(C2), id(V2), t(24)).unwrap()],
    );
    let m = merged(&base, &ours, &theirs);
    let c2 = clip_of(&m.project, C2);
    assert_eq!(c2.video.opacity, 0.5, "our property edit");
    assert!(
        items(&m.project, "V2").iter().any(|i| i.id() == id(C2)),
        "their move"
    );
    assert!(!items(&m.project, "V1").iter().any(|i| i.id() == id(C2)));
}

#[test]
fn a_lift_brings_its_gap_with_it_and_skipping_it_leaves_no_stray_gap() {
    let base = base_project();
    let ours = run(
        &base,
        &[Op::SetVideo {
            item: id(C3),
            video: video(0.7),
        }],
    );
    let theirs = run(&base, &[edit::lift(&base, id(C2)).unwrap()]);
    let m = merged(&base, &ours, &theirs);
    assert!(
        items(&m.project, "V1")[1].as_clip().is_none(),
        "c2 became a gap of the same length"
    );
    assert_eq!(spans(&m.project, "V1")[1], (96, 96));
    // skipping the lift: c2 stays and no gap appears
    let plan = plan_merge(&base, &ours, &theirs);
    let skip: HashMap<String, Choice> = plan
        .entries
        .iter()
        .map(|e| (e.change.id.clone(), Choice::Skip))
        .collect();
    let m = merge(&base, &ours, &theirs, &skip).unwrap();
    assert_eq!(m.project, ours, "skipping everything changes nothing");
    assert!(m.ops.is_empty());
}

#[test]
fn partial_merge_takes_only_the_chosen_changes() {
    let base = base_project();
    let ours = base.clone();
    let mut title = Title::new(id(71), TitleTemplate::Centered, t(48));
    title.text = "Intro".into();
    let theirs = run(
        &base,
        &[
            Op::SetVideo {
                item: id(C1),
                video: video(0.5),
            },
            Op::SetVideo {
                item: id(C3),
                video: video(0.25),
            },
            edge_title(&base, title),
            Op::RemoveItem { item: id(C2) },
        ],
    );
    let plan = plan_merge(&base, &ours, &theirs);
    assert_eq!(plan.entries.len(), 4);
    let mut pick: HashMap<String, Choice> = take(&plan);
    for e in &plan.entries {
        if e.change.name == "c23" || e.change.name == "c22" {
            pick.insert(e.change.id.clone(), Choice::Skip);
        }
    }
    let m = merge(&base, &ours, &theirs, &pick).unwrap();
    assert_eq!(clip_of(&m.project, C1).video.opacity, 0.5);
    assert_eq!(clip_of(&m.project, C3).video.opacity, 1.0, "skipped");
    assert!(has(&m.project, id(C2)), "the removal was skipped");
    assert!(has(&m.project, id(71)), "the title was taken");
    assert_eq!(m.applied.len(), 2);
    assert_eq!(run(&ours, &m.ops), m.project);
    // skipping a new item's dependency leaves things consistent: an added title without its track is reported, not crashed on
}

#[test]
fn their_new_track_and_media_come_along() {
    let base = base_project();
    let ours = run(
        &base,
        &[Op::SetVideo {
            item: id(C1),
            video: video(0.9),
        }],
    );
    let mut v3 = Track::new(id(60), "V3", TrackKind::Video);
    v3.items.push(Item::Clip(Clip::new(
        id(61),
        "overlay",
        id(55),
        t(0),
        t(48),
    )));
    let theirs = run(
        &base,
        &[
            Op::AddMedia {
                media: media(55, 30),
            },
            Op::AddTrack {
                timeline: id(TL),
                index: 2,
                track: v3,
            },
            Op::SetTrack {
                track: id(A1),
                enabled: false,
                gain_db: -4.0,
            },
        ],
    );
    let m = merged(&base, &ours, &theirs);
    assert!(m.project.media.contains_key(&id(55)));
    assert_eq!(items(&m.project, "V3").len(), 1);
    assert!(!track(&m.project, "A1").enabled);
    assert_eq!(clip_of(&m.project, C1).video.opacity, 0.9);
    assert_eq!(
        m.project
            .timelines
            .values()
            .next()
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.name.clone())
            .collect::<Vec<_>>(),
        ["V1", "V2", "V3", "A1"]
    );
}

#[test]
fn merging_an_unchanged_side_is_a_no_op() {
    let base = base_project();
    let ours = run(&base, &[Op::RemoveItem { item: id(C1) }]);
    let m = merge(&base, &ours, &base, &HashMap::new()).unwrap();
    assert_eq!(m.project, ours);
    assert!(m.ops.is_empty() && m.applied.is_empty());
    // and when ours is unchanged, the merge reproduces theirs exactly (a fast-forward in effect)
    let theirs = run(
        &base,
        &[
            Op::RemoveItem { item: id(C2) },
            Op::SetVideo {
                item: id(C3),
                video: video(0.1),
            },
        ],
    );
    assert_eq!(
        merge(&base, &base, &theirs, &HashMap::new())
            .unwrap()
            .project,
        theirs
    );
}

#[test]
fn an_invalid_result_is_refused_not_produced() {
    let base = base_project();
    // theirs trims c1 so long the fade ours added no longer fits: merged result would be invalid
    let ours = run(
        &base,
        &[Op::SetAudio {
            item: id(C1),
            audio: AudioProps {
                fade_in: 40,
                fade_out: 40,
                ..AudioProps::default()
            },
        }],
    );
    let theirs = run(
        &base,
        &[Op::Trim {
            item: id(C1),
            edge: Edge::Out,
            delta: t(-60),
        }],
    );
    let r = merge(&base, &ours, &theirs, &HashMap::new());
    assert!(r.is_err() && r.unwrap_err().contains("invalid"), "refused");
}

#[test]
fn fuzz_merging_random_edits_from_both_sides() {
    let base = base_project();
    let (mut ok, mut conflicted, mut refused) = (0, 0, 0);
    for seed in 1..=120u64 {
        let (ours, _) = edit_randomly(&base, seed, 8);
        let (theirs, _) = edit_randomly(&base, seed + 5000, 8);
        let plan = plan_merge(&base, &ours, &theirs);
        match merge(&base, &ours, &theirs, &HashMap::new()) {
            Ok(m) => {
                m.project
                    .validate()
                    .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
                assert_eq!(
                    run(&ours, &m.ops),
                    m.project,
                    "seed {seed}: the merge ops reproduce the result"
                );
                if plan.conflicts().count() == 0 && m.skipped.is_empty() {
                    ok += 1;
                    // every clean change of theirs is in the result
                    for e in &plan.entries {
                        if let Target::Item(i) = e.change.target {
                            match e.change.kind {
                                ChangeKind::Added if e.change.what != What::Gap => assert!(
                                    has(&m.project, i),
                                    "seed {seed}: added item missing: {}",
                                    e.change.summary
                                ),
                                ChangeKind::Removed if e.change.what != What::Gap => assert!(
                                    !has(&m.project, i),
                                    "seed {seed}: removed item still there: {}",
                                    e.change.summary
                                ),
                                _ => {}
                            }
                        }
                    }
                } else {
                    conflicted += 1;
                }
                // taking everything, conflicts included, is also sound or refused, never wrong
                if let Ok(all) = merge(&base, &ours, &theirs, &take(&plan)) {
                    all.project.validate().unwrap();
                    assert_eq!(run(&ours, &all.ops), all.project, "seed {seed} (take all)");
                }
            }
            Err(e) => {
                assert!(
                    e.contains("invalid") || e.contains("no op") || e.contains("renamed"),
                    "seed {seed}: {e}"
                );
                refused += 1;
            }
        }
    }
    eprintln!("merge fuzz: {ok} clean, {conflicted} with conflicts/skips, {refused} refused");
    assert!(ok >= 15, "a good share of random pairs merge cleanly: {ok}");
    assert!(refused * 5 <= 120, "refusals stay rare: {refused}");
}

/// Empty space after the last item means nothing; ignore it when comparing.
fn trim_gaps(mut v: Vec<Item>) -> Vec<Item> {
    while v.last().is_some_and(|i| !i.is_content()) {
        v.pop();
    }
    v
}

/// Edits restricted to the given tracks: the way two people (or a person and the agent) usually work.
fn edits_on(base: &Project, seed: u64, n: usize, allowed: &[u128]) -> Project {
    let mut rng = Rng::new(seed);
    let mut next = seed as u128 * 10_000;
    let mut cur = base.clone();
    let mut done = 0;
    for _ in 0..n * 20 {
        if done >= n {
            break;
        }
        let Some(op) = random_edit(&cur, &mut rng, &mut next) else {
            continue;
        };
        let after = apply(&cur, &op).unwrap().0;
        // only keep the edit if everything that differs lives on an allowed track
        // cross-track moves touch two tracks (the one it left too), so they are not "on one track"
        let ok = diff(&cur, &after).iter().all(|c| {
            c.kind != ChangeKind::Moved
                && match c.track {
                    Some(t) => allowed.iter().any(|a| id(*a) == t),
                    None => matches!(c.what, What::Media),
                }
        });
        if ok {
            cur = after;
            done += 1;
        }
    }
    cur
}

#[test]
fn fuzz_edits_on_different_tracks_almost_always_merge_cleanly() {
    let base = base_project();
    let (mut clean, mut total) = (0, 0);
    for seed in 1..=80u64 {
        let ours = edits_on(&base, seed, 6, &[V1]);
        let theirs = edits_on(&base, seed + 9000, 6, &[V2, A1]);
        let plan = plan_merge(&base, &ours, &theirs);
        let m = merge(&base, &ours, &theirs, &HashMap::new())
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(run(&ours, &m.ops), m.project);
        total += 1;
        if plan.conflicts().count() == 0 && m.skipped.is_empty() {
            clean += 1;
            // nothing of ours was lost: V1 is exactly what we made it
            assert_eq!(items(&m.project, "V1"), items(&ours, "V1"), "seed {seed}");
            // and their tracks are exactly theirs
            for name in ["V2", "A1"] {
                assert_eq!(
                    trim_gaps(items(&m.project, name)),
                    trim_gaps(items(&theirs, name)),
                    "seed {seed} {name}"
                );
            }
        }
    }
    eprintln!("different tracks: {clean}/{total} clean");
    assert!(clean * 10 >= total * 9, "{clean}/{total}");
}

#[test]
fn a_merge_with_no_conflicts_never_skips_anything() {
    // "skipped" means a change could not be brought in; without a conflict that should not happen
    let base = base_project();
    let (mut no_conflict, mut skipped_anyway) = (0, Vec::new());
    for seed in 1..=600u64 {
        let (ours, _) = edit_randomly(&base, seed, 6);
        let (theirs, _) = edit_randomly(&base, seed + 7000, 6);
        if plan_merge(&base, &ours, &theirs).conflicts().count() > 0 {
            continue;
        }
        no_conflict += 1;
        if let Ok(m) = merge(&base, &ours, &theirs, &HashMap::new()) {
            if !m.skipped.is_empty() {
                skipped_anyway.push((seed, m.skipped.clone()));
            }
        }
    }
    eprintln!(
        "no-conflict merges: {no_conflict}; with skips: {}",
        skipped_anyway.len()
    );
    assert!(no_conflict >= 40, "need enough samples: {no_conflict}");
    assert!(skipped_anyway.is_empty(), "{skipped_anyway:?}");
}
