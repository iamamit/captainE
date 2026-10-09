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

fn px8(c: &Canvas, x: u32, y: u32) -> [u8; 3] {
    let rgba = c.to_rgba8();
    let i = ((y * c.width + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

fn render_at(p: &Project, tl: Id, tick: i64) -> Canvas {
    Renderer::new(p, tl, RenderOptions::default())
        .unwrap()
        .render(tick)
        .unwrap()
}

#[test]
fn shapes_and_solids_are_drawn_without_any_media() {
    let mut s = Title::generator(Id::new(), Some(ShapeKind::Rectangle), t(48));
    s.shape = Some(Shape {
        color: [255, 0, 0, 255],
        width: 50.0,
        height: 50.0,
        ..Shape::new(ShapeKind::Rectangle)
    });
    let (p, tl) = project_with(s.clone());
    let c = render_at(&p, tl, 30);
    assert!(
        px8(&c, 320, 180)[0] > 250 && px8(&c, 320, 180)[1] < 5,
        "red in the middle"
    );
    assert_eq!(px8(&c, 20, 20), [0, 0, 0], "black outside the rectangle");
    assert_eq!(
        px8(&render_at(&p, tl, 5), 320, 180),
        [0, 0, 0],
        "nothing before it starts"
    );
    // ellipse leaves the corners of its box empty
    let mut e = s.clone();
    e.shape = Some(Shape {
        kind: ShapeKind::Ellipse,
        ..e.shape.unwrap()
    });
    let (p, tl) = project_with(e);
    let c = render_at(&p, tl, 30);
    assert!(px8(&c, 320, 180)[0] > 250);
    assert_eq!(px8(&c, 170, 100), [0, 0, 0], "corner of the ellipse's box");
    // a gradient runs top to bottom, and a solid covers the frame
    let mut g = Title::generator(Id::new(), Some(ShapeKind::Solid), t(48));
    g.shape = Some(Shape {
        color: [255, 255, 255, 255],
        gradient_to: Some([0, 0, 0, 255]),
        ..Shape::new(ShapeKind::Solid)
    });
    let (p, tl) = project_with(g);
    let c = render_at(&p, tl, 30);
    assert!(px8(&c, 100, 5)[0] > 240 && px8(&c, 100, 355)[0] < 15 && px8(&c, 100, 180)[0] > 100);
    // an outline paints the edge in its own colour
    let mut o = s;
    o.shape = Some(Shape {
        outline: 30.0,
        outline_color: [0, 0, 255, 255],
        ..o.shape.unwrap()
    });
    let (p, tl) = project_with(o);
    let c = render_at(&p, tl, 30);
    let edge = px8(&c, 165, 180);
    assert!(edge[2] > 200 && edge[0] < 60, "{edge:?}");
    assert!(px8(&c, 320, 180)[0] > 250, "the inside keeps the fill");
}

#[test]
fn an_adjustment_layer_changes_what_is_under_it_only_where_its_effects_reach() {
    // a red solid on V1, an adjustment layer on V2 that turns red grey, limited to the left half by a mask
    let red = {
        let mut s = Title::generator(Id::new(), Some(ShapeKind::Solid), t(48));
        s.shape = Some(Shape {
            color: [255, 0, 0, 255],
            ..Shape::new(ShapeKind::Solid)
        });
        s
    };
    let mut adj = Title::generator(Id::new(), None, t(48));
    let mut hsl = Effect::new(EffectKind::Hsl);
    hsl.params[1] = 0.0;
    let mut mask = Effect::new(EffectKind::Mask);
    mask.params = vec![0.0, 25.0, 50.0, 50.0, 100.0, 0.0, 0.0, 0.0]; // the left half
    adj.effects = vec![hsl, mask];
    let (mut p, tl) = project_with(red);
    let mut v2 = Track::new(Id::new(), "V2", TrackKind::Video);
    v2.items.push(Item::Gap(Gap {
        id: Id::new(),
        duration: t(24),
    }));
    v2.items.push(Item::Title(adj.clone()));
    let mut timeline = (*p.timelines[&tl]).clone();
    timeline.tracks.push(Arc::new(v2));
    p.timelines.insert(tl, Arc::new(timeline));
    p.validate().unwrap();
    let c = render_at(&p, tl, 30);
    let (left, right) = (px8(&c, 100, 180), px8(&c, 540, 180));
    assert!(
        left[0].abs_diff(left[1]) < 12 && left[0].abs_diff(left[2]) < 12,
        "left is grey: {left:?}"
    );
    assert!(right[0] > 250 && right[1] < 5, "right stays red: {right:?}");
    // before the layer starts nothing is changed; at half opacity it is half way
    let before = render_at(&p, tl, 12);
    assert_eq!(px8(&before, 100, 180), [0, 0, 0]);
    let mut half = adj;
    half.video.opacity = 0.5;
    let mut timeline = (*p.timelines[&tl]).clone();
    let mut v2 = (*timeline.tracks[1]).clone();
    v2.items[1] = Item::Title(half);
    timeline.tracks[1] = Arc::new(v2);
    p.timelines.insert(tl, Arc::new(timeline));
    let mid = px8(&render_at(&p, tl, 30), 100, 180);
    assert!(
        mid[0] > 150 && mid[1] > 50 && mid[1] < 200 && mid[0] > mid[1],
        "{mid:?}"
    );
    // adjustment layers and shapes are ordinary items for the command layer
    let mut ed = Editor::new(p);
    let id = match &ed.project().timelines[&tl].tracks[1].items[1] {
        Item::Title(t) => t.id,
        _ => unreachable!(),
    };
    ed.apply(
        Author::User,
        Op::SetEffects {
            item: id,
            effects: vec![],
        },
    )
    .unwrap();
    ed.undo();
    assert_eq!(
        match &ed.project().timelines[&tl].tracks[1].items[1] {
            Item::Title(t) => t.effects.len(),
            _ => 0,
        },
        2
    );
}

#[test]
fn text_animation_presets_show_up_frame_by_frame() {
    use captaine_model::anim_presets::{apply, tracks, Preset};
    let canvas = (640.0, 360.0);
    let len = 12;
    let mut title = Title::new(Id::new(), TitleTemplate::Centered, t(48));
    title.text = "TYPEWRITER".into();
    let base = title.video;
    // typewriter: the amount of lit picture grows while it types, then stays
    apply(
        &mut title.anim,
        tracks(Preset::Typewriter, true, len, 48, &base, canvas),
        true,
        len,
        48,
    );
    let (p, tl) = project_with(title.clone());
    let early = lit(&render_at(&p, tl, 24 + 2));
    let mid = lit(&render_at(&p, tl, 24 + 6));
    let done = lit(&render_at(&p, tl, 24 + 20));
    assert!(early < mid && mid < done, "{early} < {mid} < {done}");
    assert!(early > 0, "the first letters are already there");
    // slide up + fade: starts lower and fainter, ends at rest
    let mut title = Title::new(Id::new(), TitleTemplate::Centered, t(48));
    title.text = "SLIDE".into();
    apply(
        &mut title.anim,
        tracks(Preset::SlideUp, true, len, 48, &base, canvas),
        true,
        len,
        48,
    );
    apply(
        &mut title.anim,
        tracks(Preset::Fade, false, len, 48, &base, canvas),
        false,
        len,
        48,
    );
    let (p, tl) = project_with(title);
    let centre_y = |c: &Canvas| {
        let (mut sum, mut n) = (0.0f64, 0.0f64);
        for (i, px) in c.data.chunks(4).enumerate() {
            let v = (px[0] + px[1] + px[2]) as f64;
            sum += v * (i as u32 / c.width) as f64;
            n += v;
        }
        sum / n.max(1e-9)
    };
    let (a, b) = (render_at(&p, tl, 24 + 1), render_at(&p, tl, 24 + 30));
    assert!(
        centre_y(&a) > centre_y(&b) + 5.0,
        "it starts lower: {} vs {}",
        centre_y(&a),
        centre_y(&b)
    );
    assert!(lit(&a) > 0 && px8(&a, 320, 180) != px8(&b, 320, 180) || lit(&a) != lit(&b));
    let end = render_at(&p, tl, 24 + 47);
    let energy = |c: &Canvas| {
        c.data
            .chunks(4)
            .map(|p| (p[0] + p[1] + p[2]) as f64)
            .sum::<f64>()
    };
    assert!(
        energy(&end) < energy(&b) * 0.2,
        "faded out by the last frame"
    );
}

#[test]
fn a_sequence_plays_inside_another_and_follows_edits() {
    // B: a red solid. A: a clip that plays B.
    let rate = R;
    let mut p = Project::new(ProjectSettings {
        frame_rate: rate,
        width: 640,
        height: 360,
        sample_rate: 48_000,
    });
    let solid = |rgb: [u8; 3]| {
        let mut s = Title::generator(Id::new(), Some(ShapeKind::Solid), t(40));
        s.shape = Some(Shape {
            color: [rgb[0], rgb[1], rgb[2], 255],
            ..Shape::new(ShapeKind::Solid)
        });
        s
    };
    let mut b = Timeline::new(Id::new(), "Inner", rate);
    let mut vb = Track::new(Id::new(), "V1", TrackKind::Video);
    vb.items.push(Item::Title(solid([255, 0, 0])));
    b.tracks.push(Arc::new(vb));
    let media = MediaRef::nested(&b, (640, 360));
    assert_eq!(media.nested_timeline(), Some(b.id));
    let mut a = Timeline::new(Id::new(), "Outer", rate);
    let mut va = Track::new(Id::new(), "V1", TrackKind::Video);
    va.items.push(Item::Gap(Gap {
        id: Id::new(),
        duration: t(10),
    }));
    va.items.push(Item::Clip(Clip::new(
        Id::new(),
        "inner",
        media.id,
        t(5),
        t(20),
    )));
    a.tracks.push(Arc::new(va));
    let (a_id, b_id) = (a.id, b.id);
    p.media.insert(media.id, media);
    p.timelines.insert(b_id, Arc::new(b));
    p.timelines.insert(a_id, Arc::new(a));
    p.validate().unwrap();
    // before the clip: black; inside it: the inner sequence (shown from its tick 5)
    let mut r = Renderer::new(&p, a_id, RenderOptions::default()).unwrap();
    assert_eq!(px8(&r.render(3).unwrap(), 100, 100), [0, 0, 0]);
    let c = px8(&r.render(15).unwrap(), 100, 100);
    assert!(c[0] > 250 && c[1] < 5, "the inner sequence is red: {c:?}");
    assert_eq!(
        px8(&r.render(31).unwrap(), 100, 100),
        [0, 0, 0],
        "after the clip"
    );
    // editing the inner sequence changes the outer picture at once, through set_project
    let mut p2 = p.clone();
    {
        let tl = p2.timeline_mut(b_id).unwrap();
        tl.track_mut(0).items[0] = Item::Title(solid([0, 0, 255]));
    }
    r.set_project(&p2).unwrap();
    let c = px8(&r.render(15).unwrap(), 100, 100);
    assert!(c[2] > 250 && c[0] < 5, "now blue: {c:?}");
    // a sequence cannot contain itself, directly or through another
    let mut ed = Editor::new(p);
    let outer_media = MediaRef::nested(ed.project().timeline(a_id).unwrap(), (640, 360));
    ed.apply(
        Author::User,
        Op::AddMedia {
            media: outer_media.clone(),
        },
    )
    .unwrap();
    let bad = Clip::new(Id::new(), "loop", outer_media.id, t(0), t(10));
    let b_track = ed.project().timeline(b_id).unwrap().tracks[0].id;
    let err = ed
        .apply(
            Author::User,
            Op::InsertItem {
                track: b_track,
                index: 0,
                item: Item::Clip(bad),
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("contain itself"), "{err}");
    // and the inner sequence cannot be deleted while it plays inside the other
    assert!(ed
        .apply(Author::User, Op::RemoveTimeline { id: b_id })
        .is_err());
}
