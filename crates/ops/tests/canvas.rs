use std::sync::Arc;

use captaine_model::*;
use captaine_ops::*;
use captaine_time::Rational;

#[test]
fn a_sequence_can_have_its_own_frame_size() {
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: Rational::FPS_30,
        width: 1920,
        height: 1080,
        sample_rate: 48_000,
    }));
    let tl = Id::new();
    let mut t = Timeline::new(tl, "main", Rational::FPS_30);
    t.tracks
        .push(Arc::new(Track::new(Id::new(), "V1", TrackKind::Video)));
    ed.apply(Author::User, Op::AddTimeline { timeline: t })
        .unwrap();
    let before = ed.snapshot();
    assert_eq!(ed.project().canvas(tl), (1920, 1080));
    ed.apply(
        Author::User,
        Op::SetCanvas {
            timeline: tl,
            canvas: Some((1080, 1920)),
        },
    )
    .unwrap();
    assert_eq!(ed.project().canvas(tl), (1080, 1920));
    assert_eq!(
        ed.project().settings.width,
        1920,
        "the project is untouched"
    );
    ed.undo();
    assert_eq!(ed.project(), &before);
    for bad in [(0, 0), (1081, 1920), (100000, 1000), (8, 8)] {
        assert!(
            ed.apply(
                Author::User,
                Op::SetCanvas {
                    timeline: tl,
                    canvas: Some(bad)
                }
            )
            .is_err(),
            "{bad:?}"
        );
    }
    assert!(ed
        .apply(
            Author::User,
            Op::SetCanvas {
                timeline: Id::new(),
                canvas: None
            }
        )
        .is_err());
}
