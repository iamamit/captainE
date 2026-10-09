use std::collections::HashMap;
use std::sync::Arc;

use captaine_model::*;
use captaine_ops::*;
use captaine_project::captaine_vcs::{ChangeKind, Choice, Status};
use captaine_project::*;
use captaine_time::{Rational, RationalTime};

const R: Rational = Rational::FPS_24;
fn t(v: i64) -> RationalTime {
    RationalTime::new(v, R)
}
fn id(n: u128) -> Id {
    Id::from_u128(n.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835) | (1u128 << 100))
}

fn clip(n: u128, dur: i64) -> Item {
    Item::Clip(Clip::new(
        id(n),
        format!("c{n}"),
        id(1),
        t(n as i64 * 10),
        t(dur),
    ))
}

/// A session with media, one timeline (V1, V2, A1) and three 4 s clips on V1, all committed.
fn session() -> Session {
    let mut s = Session::new(ProjectSettings {
        frame_rate: R,
        width: 640,
        height: 360,
        sample_rate: 48_000,
    });
    s.apply(Op::AddMedia {
        media: MediaRef {
            id: id(1),
            name: "m".into(),
            path: "/m.mp4".into(),
            duration: Some(t(24 * 600)),
            has_audio: true,
            width: 640,
            height: 360,
            has_video: true,
            proxy: None,
        },
    })
    .unwrap();
    let mut v1 = Track::new(id(11), "V1", TrackKind::Video);
    v1.items = vec![clip(21, 96), clip(22, 96), clip(23, 96)];
    let mut tl = Timeline::new(id(10), "Timeline 1", R);
    tl.tracks.push(Arc::new(v1));
    tl.tracks
        .push(Arc::new(Track::new(id(12), "V2", TrackKind::Video)));
    tl.tracks
        .push(Arc::new(Track::new(id(13), "A1", TrackKind::Audio)));
    s.apply(Op::AddTimeline { timeline: tl }).unwrap();
    s.commit(Some("Rough cut")).unwrap();
    s
}

fn names(s: &Session, track: &str) -> Vec<String> {
    s.editor
        .project()
        .timelines
        .values()
        .next()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == track)
        .unwrap()
        .items
        .iter()
        .filter_map(|i| i.as_clip().map(|c| c.name.clone()))
        .collect()
}

fn opacity(s: &Session, n: u128) -> f64 {
    s.editor
        .project()
        .timelines
        .values()
        .flat_map(|t| t.tracks.iter())
        .flat_map(|t| t.items.iter())
        .find_map(|i| {
            i.as_clip()
                .filter(|c| c.id == id(n))
                .map(|c| c.video.opacity)
        })
        .unwrap()
}

fn dim(s: &mut Session, n: u128, o: f64) {
    s.apply(Op::SetVideo {
        item: id(n),
        video: VideoProps {
            opacity: o,
            ..VideoProps::default()
        },
    })
    .unwrap();
}

#[test]
fn commit_collects_the_working_edits() {
    let mut s = session();
    assert!(!s.has_uncommitted());
    assert_eq!(s.commit(None).unwrap(), None, "nothing to commit");
    dim(&mut s, 21, 0.5);
    s.apply(Op::RemoveItem { item: id(22) }).unwrap();
    assert!(s.has_uncommitted());
    assert_eq!(s.uncommitted_count(), 2);
    let c = s.commit(None).unwrap().expect("a commit");
    assert!(!s.has_uncommitted());
    let log = s.log("main").unwrap();
    assert_eq!(log[0].id, c);
    assert_eq!(
        log[0].message,
        "Set video properties of c21 and ripple delete c22"
            .replace("c21", &id(21).to_string())
            .replace("c22", &id(22).to_string()),
        "{}",
        log[0].message
    );
    assert_eq!(
        log.iter().map(|c| c.message.clone()).collect::<Vec<_>>()[1],
        "Rough cut"
    );
    assert_eq!(
        s.commit_summary(&c),
        "removes 1 clip or title (4.0 s), changes the properties of 1 item"
    );
    // the committed state is exactly what was on screen
    assert_eq!(s.repo().head_snapshot().unwrap(), *s.editor.project());
    // and undo still works across the commit
    s.undo();
    assert_eq!(names(&s, "V1"), ["c21", "c22", "c23"]);
    assert!(
        s.has_uncommitted(),
        "undoing a committed edit makes new working state"
    );
    let c2 = s.commit(Some("Put it back")).unwrap().unwrap();
    assert_eq!(s.repo().snapshot(&c2).unwrap(), *s.editor.project());
    assert_eq!(opacity(&s, 21), 0.5, "only the delete was undone");
}

#[test]
fn working_edits_and_undo_history_survive_save_and_load() {
    let dir = std::env::temp_dir().join(format!("captaine-session-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.captaine.json");
    let mut s = session();
    dim(&mut s, 21, 0.4);
    dim(&mut s, 22, 0.3);
    s.undo();
    s.save(&path).unwrap();
    let mut back = Session::load(&path).unwrap();
    assert_eq!(back.editor.project(), s.editor.project());
    assert_eq!(back.log("main").unwrap().len(), 2);
    assert!(back.has_uncommitted());
    assert_eq!(opacity(&back, 21), 0.4);
    assert_eq!(opacity(&back, 22), 1.0);
    back.redo();
    assert_eq!(
        opacity(&back, 22),
        0.3,
        "the redo stack came through the file"
    );
    back.undo();
    back.undo();
    assert_eq!(
        opacity(&back, 21),
        1.0,
        "and so did the undo stack of the working edits"
    );
    // branches come through too
    s.create_branch("try", false).unwrap();
    s.save(&path).unwrap();
    assert!(Session::load(&path)
        .unwrap()
        .branch_infos()
        .iter()
        .any(|b| b.name == "try"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn old_files_open_as_a_project_with_a_main_branch() {
    let dir = std::env::temp_dir().join(format!("captaine-v1-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.json");
    // a version 1 file as the earlier releases wrote it
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: R,
        width: 640,
        height: 360,
        sample_rate: 48_000,
    }));
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(id(10), "Main", R),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: id(10),
            index: 0,
            track: Track::new(id(11), "V1", TrackKind::Video),
        },
    )
    .unwrap();
    let v1 = serde_json::json!({"version": 1, "settings": ed.project().settings, "applied": ed.history(), "redo": []});
    std::fs::write(&path, serde_json::to_string(&v1).unwrap()).unwrap();
    let s = Session::load(&path).unwrap();
    assert_eq!(s.branch(), "main");
    assert_eq!(s.editor.project(), ed.project());
    assert!(s.has_uncommitted(), "the old log is the working edits");
    assert_eq!(s.uncommitted_count(), 2);
    // saving upgrades it
    s.save(&path).unwrap();
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("\"version\": 2"));
    assert_eq!(Session::load(&path).unwrap().editor.project(), ed.project());
    let bad = dir.join("bad.json");
    std::fs::write(&bad, r#"{"version": 9}"#).unwrap();
    assert!(Session::load(&bad)
        .err()
        .unwrap()
        .contains("unsupported project version 9"));
    std::fs::write(&bad, "nonsense").unwrap();
    assert!(Session::load(&bad)
        .err()
        .unwrap()
        .contains("not a captaine project"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn branches_keep_work_apart_and_switching_never_loses_edits() {
    let mut s = session();
    s.create_branch("tighter", true).unwrap();
    assert_eq!(s.branch(), "tighter");
    s.apply(Op::RemoveItem { item: id(21) }).unwrap(); // working edit travels with the new branch
    s.checkout("main").unwrap(); // commits it on `tighter` first
    assert_eq!(names(&s, "V1"), ["c21", "c22", "c23"], "main is untouched");
    let infos = s.branch_infos();
    let tighter = infos.iter().find(|b| b.name == "tighter").unwrap();
    assert_eq!((tighter.ahead, tighter.behind), (1, 0));
    assert!(
        tighter
            .last_message
            .starts_with("Work in progress on tighter"),
        "{}",
        tighter.last_message
    );
    s.checkout("tighter").unwrap();
    assert_eq!(names(&s, "V1"), ["c22", "c23"]);
    assert!(!s.has_uncommitted());
    // errors
    assert!(s.create_branch("tighter", false).is_err());
    assert!(s.create_branch("has space", false).is_err());
    assert!(s.checkout("nope").is_err());
    assert!(
        s.delete_branch("tighter").is_err(),
        "cannot delete the current branch"
    );
    s.checkout("main").unwrap();
    s.delete_branch("tighter").unwrap();
    assert_eq!(s.branch_infos().len(), 1);
    assert_eq!(s.unique_branch_name("agent/x"), "agent/x");
    s.create_branch("agent/x", false).unwrap();
    assert_eq!(s.unique_branch_name("agent/x"), "agent/x-2");
}

#[test]
fn merging_a_branch_that_only_moved_forward_is_a_fast_forward() {
    let mut s = session();
    s.create_branch("feature", true).unwrap();
    dim(&mut s, 22, 0.5);
    s.commit(Some("dim")).unwrap();
    s.checkout("main").unwrap();
    let setup = s.prepare_merge("feature").unwrap();
    assert!(setup.fast_forward && !setup.up_to_date);
    assert_eq!(setup.plan.entries.len(), 1);
    assert_eq!(
        s.merge("feature", &HashMap::new(), None).unwrap(),
        MergeReport::FastForward
    );
    assert_eq!(opacity(&s, 22), 0.5);
    assert_eq!(
        s.repo().head_commit(),
        s.repo().branch_head("feature").unwrap()
    );
    assert_eq!(
        s.merge("feature", &HashMap::new(), None).unwrap(),
        MergeReport::UpToDate
    );
    assert!(s.prepare_merge("main").is_err(), "not into itself");
}

#[test]
fn a_real_merge_combines_both_branches_and_a_second_merge_only_brings_the_new_work() {
    let mut s = session();
    s.create_branch("feature", true).unwrap();
    dim(&mut s, 23, 0.25);
    s.commit(Some("feature: dim c23")).unwrap();
    s.checkout("main").unwrap();
    s.apply(Op::RemoveItem { item: id(21) }).unwrap(); // ripple on main
    s.commit(Some("main: cut the start")).unwrap();
    let MergeReport::Merged {
        commit,
        applied,
        skipped,
    } = s.merge("feature", &HashMap::new(), None).unwrap()
    else {
        panic!("expected a merge commit")
    };
    assert_eq!((applied, skipped.len()), (1, 0));
    assert_eq!(names(&s, "V1"), ["c22", "c23"]);
    assert_eq!(opacity(&s, 23), 0.25);
    let c = s.repo().commit(&commit).unwrap();
    assert_eq!(c.parents.len(), 2);
    assert_eq!(c.author, Author::Merge);
    assert!(c.message.starts_with("Merge feature into main"));
    assert!(s.log("main").unwrap()[0].is_merge);

    // the feature branch carries on after being merged
    s.checkout("feature").unwrap();
    s.apply(Op::SetVideo {
        item: id(22),
        video: VideoProps {
            opacity: 0.6,
            ..VideoProps::default()
        },
    })
    .unwrap();
    s.commit(Some("feature: dim c22")).unwrap();
    s.checkout("main").unwrap();
    let setup = s.prepare_merge("feature").unwrap();
    assert_eq!(
        setup.plan.entries.len(),
        1,
        "only the new change is offered, not the one already merged: {:?}",
        setup
            .plan
            .entries
            .iter()
            .map(|e| e.change.summary.clone())
            .collect::<Vec<_>>()
    );
    assert!(matches!(
        s.merge("feature", &HashMap::new(), None).unwrap(),
        MergeReport::Merged { .. }
    ));
    assert_eq!((opacity(&s, 22), opacity(&s, 23)), (0.6, 0.25));
    // after a merge the two branches are the same content
    // main keeps its own deletion of c21; feature still has it, and everything else agrees
    assert_eq!(names(&s, "V1"), ["c22", "c23"]);
    let diff: Vec<_> = s
        .compare("main", "feature")
        .unwrap()
        .into_iter()
        .filter(|c| !c.is_noise())
        .map(|c| (c.name, c.kind))
        .collect();
    assert_eq!(diff, [("c21".to_string(), ChangeKind::Added)]);
}

#[test]
fn conflicts_default_to_ours_and_can_be_decided_one_by_one() {
    let mut s = session();
    s.create_branch("alt", true).unwrap();
    dim(&mut s, 22, 0.9);
    dim(&mut s, 23, 0.9);
    s.commit(Some("alt dims")).unwrap();
    s.checkout("main").unwrap();
    dim(&mut s, 22, 0.1); // clashes with alt on c22
    s.commit(Some("main dims c22")).unwrap();
    let setup = s.prepare_merge("alt").unwrap();
    let conflict = setup.plan.conflicts().next().expect("one conflict").clone();
    assert!(matches!(&conflict.status, Status::Conflict(m) if m.contains("video.opacity")));
    assert_eq!(setup.plan.entries.len(), 2);
    // take theirs for the conflict
    let choices = HashMap::from([(conflict.change.id.clone(), Choice::Take)]);
    s.merge("alt", &choices, Some("Take alt's dimming"))
        .unwrap();
    assert_eq!((opacity(&s, 22), opacity(&s, 23)), (0.9, 0.9));
    assert_eq!(s.log("main").unwrap()[0].message, "Take alt's dimming");
    // and the default (ours wins) on a fresh attempt
    let mut s2 = session();
    s2.create_branch("alt", true).unwrap();
    dim(&mut s2, 22, 0.9);
    s2.commit(None).unwrap();
    s2.checkout("main").unwrap();
    dim(&mut s2, 22, 0.1);
    s2.commit(None).unwrap();
    let MergeReport::Merged { applied, .. } = s2.merge("alt", &HashMap::new(), None).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (applied, opacity(&s2, 22)),
        (0, 0.1),
        "nothing taken, our value kept"
    );
}

#[test]
fn choosing_to_skip_a_change_means_a_merge_commit_not_a_fast_forward() {
    let mut s = session();
    s.create_branch("feature", true).unwrap();
    dim(&mut s, 21, 0.5);
    dim(&mut s, 22, 0.5);
    s.commit(None).unwrap();
    s.checkout("main").unwrap();
    let setup = s.prepare_merge("feature").unwrap();
    assert!(setup.fast_forward);
    let skip_c22 = setup
        .plan
        .entries
        .iter()
        .find(|e| e.change.name == "c22")
        .unwrap()
        .change
        .id
        .clone();
    let MergeReport::Merged { applied, .. } = s
        .merge("feature", &HashMap::from([(skip_c22, Choice::Skip)]), None)
        .unwrap()
    else {
        panic!("a true merge")
    };
    assert_eq!(applied, 1);
    assert_eq!((opacity(&s, 21), opacity(&s, 22)), (0.5, 1.0));
}

#[test]
fn copilot_style_branches_leave_the_current_branch_alone_until_merged() {
    let mut s = session();
    dim(&mut s, 21, 0.8); // uncommitted work the suggestion was built on
    let ops = vec![
        Op::RemoveItem { item: id(22) },
        Op::SetVideo {
            item: id(23),
            video: VideoProps {
                opacity: 0.2,
                ..VideoProps::default()
            },
        },
    ];
    let name = s
        .commit_to_new_branch(
            &format!("agent/{}", slugify("Tighten the pauses in the intro!", 4)),
            Author::Agent("copilot".into()),
            "Tighten the pauses",
            ops,
        )
        .unwrap();
    assert_eq!(name, "agent/tighten-the-pauses-in");
    assert_eq!(s.branch(), "main", "we stay where we were");
    assert_eq!(
        names(&s, "V1"),
        ["c21", "c22", "c23"],
        "the suggestion is not applied here"
    );
    assert!(!s.has_uncommitted(), "the work in progress was saved first");
    assert_eq!(opacity(&s, 21), 0.8);
    // compare shows what the agent did, with the right author on its commit
    let d = s.compare("main", &name).unwrap();
    let mut real: Vec<_> = d
        .iter()
        .filter(|c| !c.is_noise())
        .map(|c| (c.name.clone(), c.kind))
        .collect();
    real.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        real,
        [
            ("c22".to_string(), ChangeKind::Removed),
            ("c23".to_string(), ChangeKind::Modified)
        ]
    );
    assert_eq!(
        s.log(&name).unwrap()[0].author,
        Author::Agent("copilot".into())
    );
    let infos = s.branch_infos();
    assert_eq!(infos.iter().find(|b| b.name == name).unwrap().ahead, 1);
    // merging it is the user's decision
    assert!(matches!(
        s.merge(&name, &HashMap::new(), None).unwrap(),
        MergeReport::FastForward
    ));
    assert_eq!(names(&s, "V1"), ["c21", "c23"]);
    // a second suggestion with the same name gets its own branch
    assert_eq!(
        s.unique_branch_name("agent/tighten-the-pauses-in"),
        "agent/tighten-the-pauses-in-2"
    );
}

#[test]
fn compare_accepts_working_branches_and_commit_ids() {
    let mut s = session();
    let base = s.repo().head_commit().to_string();
    dim(&mut s, 21, 0.5);
    let live = s.compare(&base[..8], "working").unwrap();
    assert_eq!(live.iter().filter(|c| !c.is_noise()).count(), 1);
    assert!(s
        .compare("main", "working")
        .unwrap()
        .iter()
        .any(|c| c.name == "c21"));
    assert!(s.compare("main", "main").unwrap().is_empty());
    assert!(s.compare("main", "nope").is_err());
    assert_eq!(s.resolve("@").unwrap(), *s.editor.project());
}

#[test]
fn small_helpers() {
    assert_eq!(
        slugify("Tighten the pauses, please!", 3),
        "tighten-the-pauses"
    );
    assert_eq!(slugify("!!!", 3), "edit");
    assert_eq!(auto_message(&[]), "No changes");
    let one = Op::SetEnabled {
        item: id(1),
        enabled: false,
    };
    assert!(auto_message(std::slice::from_ref(&one)).starts_with("Disable"));
    assert!(auto_message(&[one.clone(), one.clone()]).contains(" and "));
    assert!(auto_message(&[one.clone(), one.clone(), one]).ends_with("and 2 more edits"));
}
