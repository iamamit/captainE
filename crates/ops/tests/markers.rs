use std::sync::Arc;

use captaine_model::*;
use captaine_ops::*;
use captaine_time::Rational;

const R: Rational = Rational::FPS_24;

fn editor() -> (Editor, Id) {
    let mut ed = Editor::new(Project::new(ProjectSettings::default()));
    let tl = Id::new();
    let mut t = Timeline::new(tl, "main", R);
    t.tracks
        .push(Arc::new(Track::new(Id::new(), "V1", TrackKind::Video)));
    ed.apply(Author::User, Op::AddTimeline { timeline: t })
        .unwrap();
    (ed, tl)
}

fn markers(ed: &Editor, tl: Id) -> Vec<(i64, String)> {
    ed.project()
        .timeline(tl)
        .unwrap()
        .markers
        .iter()
        .map(|m| (m.tick, m.name.clone()))
        .collect()
}

#[test]
fn markers_add_move_rename_remove_and_undo_exactly() {
    let (mut ed, tl) = editor();
    let start = ed.snapshot();
    let (a, b) = (
        Marker::new(Id::new(), 100, "Intro"),
        Marker::new(Id::new(), 20, "Hook"),
    );
    ed.apply(
        Author::User,
        Op::AddMarker {
            timeline: tl,
            marker: a.clone(),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddMarker {
            timeline: tl,
            marker: b.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        markers(&ed, tl),
        [(20, "Hook".into()), (100, "Intro".into())],
        "kept in time order"
    );
    // move the first one past the second: the order follows
    let mut moved = a.clone();
    moved.tick = 5;
    moved.color = 3;
    moved.note = "cold open".into();
    ed.apply(
        Author::User,
        Op::SetMarker {
            timeline: tl,
            marker: moved,
        },
    )
    .unwrap();
    assert_eq!(markers(&ed, tl), [(5, "Intro".into()), (20, "Hook".into())]);
    ed.apply(
        Author::User,
        Op::RemoveMarker {
            timeline: tl,
            marker: b.id,
        },
    )
    .unwrap();
    assert_eq!(markers(&ed, tl).len(), 1);
    for _ in 0..4 {
        assert!(ed.undo());
    }
    assert_eq!(ed.project(), &start);
    for _ in 0..4 {
        assert!(ed.redo());
    }
    assert_eq!(markers(&ed, tl), [(5, "Intro".into())]);
    ed.project().validate().unwrap();
    // refused: negative time, unknown colour, duplicate id, missing marker, missing timeline
    let bad = |m: Marker| {
        ed.clone_for_test()
            .apply(
                Author::User,
                Op::AddMarker {
                    timeline: tl,
                    marker: m,
                },
            )
            .is_err()
    };
    assert!(bad(Marker::new(Id::new(), -1, "x")));
    assert!(bad(Marker {
        color: 99,
        ..Marker::new(Id::new(), 1, "x")
    }));
    assert!(bad(Marker::new(a.id, 7, "dup")));
    assert!(ed
        .apply(
            Author::User,
            Op::RemoveMarker {
                timeline: tl,
                marker: Id::new()
            }
        )
        .is_err());
    assert!(ed
        .apply(
            Author::User,
            Op::AddMarker {
                timeline: Id::new(),
                marker: Marker::new(Id::new(), 1, "x")
            }
        )
        .is_err());
}

trait CloneForTest {
    fn clone_for_test(&self) -> Editor;
}
impl CloneForTest for Editor {
    fn clone_for_test(&self) -> Editor {
        Editor::new(self.project().clone())
    }
}
