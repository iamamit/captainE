//! A damaged project file never costs the work: the last good copy and timed snapshots take over.

use captaine_model::*;
use captaine_ops::*;
use captaine_project::*;
use captaine_time::Rational;

fn session(name: &str) -> Session {
    let mut s = Session::new(ProjectSettings::default());
    s.apply(Op::AddTimeline {
        timeline: Timeline::new(Id::from_u128(2), name, Rational::FPS_30),
    })
    .unwrap();
    s
}

trait Names {
    fn editor_project_names(&self) -> Vec<String>;
}

impl Names for Session {
    fn editor_project_names(&self) -> Vec<String> {
        self.editor
            .project()
            .timelines
            .values()
            .map(|t| t.name.clone())
            .collect()
    }
}

fn title(path: &std::path::Path) -> String {
    Session::load(path)
        .unwrap()
        .editor_project_names()
        .join(",")
}

fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("captaine-recovery-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_damaged_file_opens_from_the_last_good_copy() {
    let d = dir("bak");
    let p = d.join("p.captaine.json");
    session("first").save(&p).unwrap();
    assert!(
        !backup_path(&p).exists(),
        "nothing to keep on the very first save"
    );
    // let the 20 s window pass by backdating the (missing) bak: save twice with a changed project
    let mut s = session("second");
    s.apply(Op::RenameTimeline {
        id: Id::from_u128(2),
        name: "second!".into(),
    })
    .unwrap();
    s.save(&p).unwrap();
    assert!(backup_path(&p).exists(), "the first version was kept");
    assert_eq!(title(&backup_path(&p)), "first");
    assert_eq!(title(&p), "second!");
    // a crash in the middle of a write, or a full disk, leaves rubbish
    std::fs::write(&p, "{ \"version\": 2, \"repo\": {").unwrap();
    assert!(Session::load(&p).is_err());
    let (back, note) = Session::load_recovering(&p).unwrap();
    assert_eq!(back.editor_project_names().join(","), "first");
    let note = note.expect("the user is told");
    assert!(
        note.contains("backup") && note.contains("p.captaine.json"),
        "{note}"
    );
    // a healthy file opens with no note
    session("fine").save(&d.join("ok.captaine.json")).unwrap();
    assert!(Session::load_recovering(&d.join("ok.captaine.json"))
        .unwrap()
        .1
        .is_none());
    // nothing to fall back on: the original error comes back
    std::fs::write(d.join("lonely.captaine.json"), "nope").unwrap();
    assert!(Session::load_recovering(&d.join("lonely.captaine.json")).is_err());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn timed_snapshots_are_spaced_pruned_and_used_when_nothing_else_is_left() {
    let d = dir("snap");
    let p = d.join("p.captaine.json");
    let s = session("snap");
    assert!(s.snapshot(&p, 300, 3).unwrap(), "the first one is taken");
    assert!(!s.snapshot(&p, 300, 3).unwrap(), "too soon for another");
    assert_eq!(snapshots(&p).len(), 1);
    // with no minimum gap and a keep of 2, only the newest stay (file names are seconds, so fake older ones)
    let dirpath = snapshots(&p)[0].parent().unwrap().to_path_buf();
    for n in [100u64, 200, 300] {
        std::fs::copy(&snapshots(&p)[0], dirpath.join(format!("{n}.json"))).unwrap();
    }
    assert_eq!(snapshots(&p).len(), 4);
    s.snapshot(&p, 0, 2).unwrap();
    let left = snapshots(&p);
    assert_eq!(left.len(), 2, "{left:?}");
    let names: Vec<_> = left
        .iter()
        .map(|f| f.file_stem().unwrap().to_str().unwrap().to_string())
        .collect();
    assert!(
        !names.contains(&"100".to_string()) && !names.contains(&"200".to_string()),
        "the oldest went: {names:?}"
    );
    // the project file and its .bak are gone or broken: the newest snapshot saves the day
    std::fs::write(&p, "garbage").unwrap();
    let (back, note) = Session::load_recovering(&p).unwrap();
    assert_eq!(back.editor_project_names().join(","), "snap");
    assert!(note.is_some());
    let _ = std::fs::remove_dir_all(d);
}
