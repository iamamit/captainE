//! Titles in the full pipeline. No FFmpeg needed: a title needs no media.

use std::sync::Arc;

use captaine_model::*;
use captaine_ops::*;
use captaine_render::*;
use captaine_time::{Rational, RationalTime};

const R: Rational = Rational::FPS_24;

fn t(v: i64) -> RationalTime {
    RationalTime::new(v, R)
}

fn project_with(title: Title) -> (Project, Id) {
    let mut p = Project::new(ProjectSettings {
        frame_rate: R,
        width: 640,
        height: 360,
        sample_rate: 48_000,
    });
    let mut tl = Timeline::new(Id::new(), "main", R);
    let mut v1 = Track::new(Id::new(), "V1", TrackKind::Video);
    // 24 frames of nothing, then the title for 48 frames
    v1.items.push(Item::Gap(Gap {
        id: Id::new(),
        duration: t(24),
    }));
    v1.items.push(Item::Title(title));
    tl.tracks.push(Arc::new(v1));
    let id = tl.id;
    p.timelines.insert(id, Arc::new(tl));
    (p, id)
}

fn lit(c: &Canvas) -> usize {
    c.data
        .chunks(4)
        .filter(|p| p[0] > 0.002 || p[1] > 0.002 || p[2] > 0.002)
        .count()
}

fn lower_third(dur: i64) -> Title {
    let mut x = Title::new(Id::new(), TitleTemplate::LowerThird, t(dur));
    x.text = "Coastline, Day 3".into();
    x.subtitle = "Lower third · 3 s".into();
    x
}

#[test]
fn a_title_shows_only_while_it_is_on() {
    let (p, tl) = project_with(lower_third(48));
    let mut r = Renderer::new(&p, tl, RenderOptions::default()).unwrap();
    assert_eq!(lit(&r.render(10).unwrap()), 0, "nothing before the title");
    let on = r.render(40).unwrap();
    assert!(lit(&on) > 2000, "the plate and text are on screen");
    // it is in the lower left, over black
    let (w, h) = (on.width as usize, on.height as usize);
    let top_half: usize = (0..h / 2)
        .map(|y| (0..w).filter(|x| on.data[(y * w + x) * 4] > 0.002).count())
        .sum();
    assert_eq!(top_half, 0, "nothing in the top half");
    assert_eq!(
        lit(&r.render(24 + 48).unwrap()),
        0,
        "gone after its last frame"
    );
}

#[test]
fn fades_and_opacity_apply() {
    let mut x = lower_third(48);
    x.fade_in = 12;
    x.fade_out = 12;
    let (p, tl) = project_with(x.clone());
    let mut r = Renderer::new(&p, tl, RenderOptions::default()).unwrap();
    let peak = |c: &Canvas| {
        c.data
            .chunks(4)
            .map(|p| p[0].max(p[1]).max(p[2]))
            .fold(0.0f32, f32::max)
    };
    let first = peak(&r.render(24).unwrap());
    let mid = peak(&r.render(24 + 6).unwrap());
    let full = peak(&r.render(24 + 24).unwrap());
    let last = peak(&r.render(24 + 47).unwrap());
    assert!(first < 0.01, "starts invisible: {first}");
    assert!(
        mid > 0.2 && mid < full * 0.8,
        "half way up: {mid} vs {full}"
    );
    assert!(full > 0.9, "full white text: {full}");
    assert!(last < full * 0.2, "nearly gone on the last frame: {last}");
    // opacity and transform from the title's video properties
    x.video.opacity = 0.5;
    x.video.transform.position = (0.0, -100.0);
    let (p2, tl2) = project_with(x);
    let mut r2 = Renderer::new(&p2, tl2, RenderOptions::default()).unwrap();
    let c = r2.render(24 + 24).unwrap();
    assert!(
        peak(&c) < full * 0.8 && peak(&c) > 0.3,
        "half-transparent: {}",
        peak(&c)
    );
    // moved up 100 px: some ink now appears higher than before
    let (w, h) = (c.width as usize, c.height as usize);
    let rows_with_ink = |c: &Canvas| {
        (0..h)
            .filter(|y| (0..w).any(|x| c.data[(y * w + x) * 4] > 0.002))
            .min()
            .unwrap()
    };
    let base = r.render(24 + 24).unwrap();
    assert!(rows_with_ink(&c) + 80 < rows_with_ink(&base));
}

#[test]
fn editing_a_title_changes_the_next_frame_and_a_disabled_title_vanishes() {
    let (p, tl) = project_with(lower_third(48));
    let mut ed = Editor::new(p);
    let item = ed.project().timeline(tl).unwrap().tracks[0].items[1].id();
    let mut r = Renderer::new(ed.project(), tl, RenderOptions::default()).unwrap();
    let before = r.render(40).unwrap();
    let mut changed = ed.project().timeline(tl).unwrap().tracks[0].items[1]
        .as_title()
        .unwrap()
        .clone();
    changed.style.accent = [255, 0, 0];
    changed.text = "A much longer headline for the plate".into();
    ed.apply(
        Author::User,
        Op::SetTitle {
            item,
            title: changed,
        },
    )
    .unwrap();
    r.set_project(ed.project()).unwrap();
    let after = r.render(40).unwrap();
    assert_ne!(before, after);
    assert!(
        lit(&after) > lit(&before),
        "a longer line lights more pixels"
    );
    ed.apply(
        Author::User,
        Op::SetEnabled {
            item,
            enabled: false,
        },
    )
    .unwrap();
    r.set_project(ed.project()).unwrap();
    assert_eq!(lit(&r.render(40).unwrap()), 0);
}

#[test]
fn layers_report_titles() {
    let (p, tl) = project_with(lower_third(48));
    let idx = TimelineIndex::new(&p, tl).unwrap();
    let layers = layers_at(&p, tl, &idx, 30);
    assert_eq!(layers.len(), 1);
    assert!(matches!(&layers[0], Layer::Title(t) if t.local_tick == 6));
    assert!(layers_at(&p, tl, &idx, 5).is_empty());
}

#[test]
fn keyframed_properties_change_the_picture_frame_by_frame() {
    let mut x = lower_third(48);
    // fade up over the first second while sliding 200 px to the right, then hold
    x.anim.set_track(
        "opacity",
        vec![Keyframe::new(0, 0.0), Keyframe::new(24, 100.0)],
    );
    x.anim.set_track(
        "position.x",
        vec![Keyframe::new(0, 0.0), Keyframe::new(24, 200.0)],
    );
    let (p, tl) = project_with(x);
    let mut r = Renderer::new(&p, tl, RenderOptions::default()).unwrap();
    let peak = |c: &Canvas| {
        c.data
            .chunks(4)
            .map(|p| p[0].max(p[1]).max(p[2]))
            .fold(0.0f32, f32::max)
    };
    let right_edge = |c: &Canvas| {
        let (w, h) = (c.width as usize, c.height as usize);
        (0..w)
            .rev()
            .find(|x| (0..h).any(|y| c.data[(y * w + x) * 4] > 0.002))
            .unwrap_or(0)
    };
    // the title starts at tick 24 (after the gap)
    let f0 = r.render(24).unwrap();
    let f12 = r.render(24 + 12).unwrap();
    let f24 = r.render(24 + 24).unwrap();
    let f40 = r.render(24 + 40).unwrap();
    assert!(peak(&f0) < 0.01, "invisible at its first frame");
    assert!(peak(&f12) > 0.2 && peak(&f12) < peak(&f24), "half way up");
    assert!(
        peak(&f24) > 0.9 && (peak(&f40) - peak(&f24)).abs() < 1e-6,
        "full, then held"
    );
    // it slides: the right edge of the ink moves right by about the animated distance (canvas 640 wide = 0.5 scale of 1280)
    let (e12, e24) = (right_edge(&f12), right_edge(&f24));
    assert!(e24 > e12 + 20, "moving right: {e12} -> {e24}");
    assert_eq!(right_edge(&f40), e24, "and stopped");
}
