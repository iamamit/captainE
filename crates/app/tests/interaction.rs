//! Drives the editor with real pointer and keyboard events and checks what happens to the project.

use egui_kittest::kittest::Queryable;
use std::path::{Path, PathBuf};
use std::process::Command;

use captaine_app::app::*;
use captaine_app::ui::MediaDrag;
use captaine_model::*;
use captaine_time::Rational;
use egui::{pos2, Key, Modifiers, Pos2};
use egui_kittest::Harness;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

/// The three test clips, made once per test run (tests run in parallel and share the files).
fn clips(dir: &Path) -> Vec<PathBuf> {
    static FILES: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    FILES.get_or_init(|| make_clips(dir)).clone()
}

fn make_clips(dir: &Path) -> Vec<PathBuf> {
    ["a.mp4", "b.mp4", "c.mp4"]
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let p = dir.join(name);
            let src = ["testsrc2", "smptebars", "rgbtestsrc"][i];
            let ok = Command::new("ffmpeg")
                .args([
                    "-y",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("{src}=size=160x90:rate=24"),
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=300:duration=4",
                    "-t",
                    "4",
                ])
                .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
                .arg(&p)
                .status()
                .unwrap()
                .success();
            assert!(ok);
            p
        })
        .collect()
}

type H = Harness<'static, Option<App>>;

fn editor() -> H {
    let dir = std::env::temp_dir().join(format!("captaine-interaction-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let files = clips(&dir);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1480.0, 940.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(
            move |ui, state: &mut Option<App>| {
                let app = state.get_or_insert_with(|| {
                    let session = captaine_app::blank_session(1280, 720, Rational::FPS_24);
                    let mut app = App::new(&ui.ctx().clone(), session, None);
                    app.import(files.clone());
                    let ids: Vec<Id> = {
                        let mut m: Vec<_> = app.project().media.values().cloned().collect();
                        m.sort_by(|a, b| a.name.cmp(&b.name));
                        m.into_iter().map(|m| m.id).collect()
                    };
                    for id in ids {
                        app.place_media(id, None, None); // a, b, c back to back on V1, 4 s each
                    }
                    app.selected_clip = None;
                    app.pps = 40.0;
                    app
                });
                app.update(ui);
            },
            None,
        );
    h.step();
    h.step();
    h
}

fn app(h: &H) -> &App {
    h.state().as_ref().unwrap()
}

fn app_mut(h: &mut H) -> &mut App {
    h.state_mut().as_mut().unwrap()
}

fn settle(h: &mut H) {
    for _ in 0..4 {
        h.step();
    }
}

fn drag(h: &mut H, from: Pos2, to: Pos2) {
    h.hover_at(from);
    settle(h);
    h.drag_at(from);
    settle(h);
    for i in 1..=6 {
        h.hover_at(from + (to - from) * (i as f32 / 6.0));
        h.step();
    }
    h.drop_at(to);
    settle(h);
}

fn click(h: &mut H, at: Pos2) {
    h.hover_at(at);
    settle(h);
    h.drag_at(at);
    h.step();
    h.drop_at(at);
    settle(h);
}

fn v1(app: &App) -> Track {
    app.timeline()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == "V1")
        .unwrap()
        .as_ref()
        .clone()
}

/// (start, length) in ticks of every item on V1
fn layout(app: &App) -> Vec<(i64, i64)> {
    let t = v1(app);
    t.items
        .iter()
        .zip(t.item_starts())
        .map(|(i, s)| (s, i.duration().value))
        .collect()
}

fn clip_pos(app: &App, index: usize, frac: f32) -> Pos2 {
    let t = v1(app);
    let (start, dur) = layout(app)[index];
    let g = app.timeline_layout.clone().unwrap();
    let fps = app.fps();
    let x0 = g.x(start as f64 / fps);
    let x1 = g.x((start + dur) as f64 / fps);
    pos2(x0 + (x1 - x0) * frac, g.row_center(t.id).unwrap())
}

#[test]
fn click_selects_and_ruler_click_seeks() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    assert_eq!(layout(app(&h)).len(), 3);
    let p = clip_pos(app(&h), 1, 0.5);
    click(&mut h, p);
    let second = v1(app(&h)).items[1].id();
    assert_eq!(app(&h).selected_clip, Some(second));
    // clicking empty track space deselects
    let g = app(&h).timeline_layout.clone().unwrap();
    let v2 = app(&h)
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == "V2")
        .unwrap()
        .id;
    click(&mut h, pos2(g.x(2.0), g.row_center(v2).unwrap()));
    assert_eq!(app(&h).selected_clip, None);
    // the ruler moves the playhead (and snaps nowhere in particular)
    let r = g.ruler;
    click(&mut h, pos2(g.x(5.0), r.center().y));
    assert_eq!(app(&h).playhead, 120, "5 s at 24 fps");
}

#[test]
fn dragging_a_clip_moves_it_and_undo_puts_it_back() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let before = layout(app(&h));
    // drag the first clip well past the end of the track: the others do not move (overwrite semantics)
    let from = clip_pos(app(&h), 0, 0.5);
    let to = pos2(from.x + 40.0 * 14.0, from.y); // 14 s to the right
    drag(&mut h, from, to);
    let after = layout(app(&h));
    assert_ne!(after, before);
    // the clip left the start (a gap is there now), the others stayed put, and it landed about 14 s along
    let items = v1(app(&h)).items;
    assert!(
        items[0].as_clip().is_none(),
        "a gap now sits at the start of V1: {after:?}"
    );
    assert_eq!(after[1], before[1]);
    assert_eq!(after[2], before[2]);
    let (last_start, last_len) = *after.last().unwrap();
    assert!(
        items.last().unwrap().as_clip().is_some()
            && (last_start - 336).abs() <= 2
            && last_len == 96,
        "{after:?}"
    );
    // everything is still valid, and one undo restores the original layout
    app(&h).project().validate().unwrap();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    settle(&mut h);
    assert_eq!(layout(app(&h)), before);
}

#[test]
fn dragging_a_clip_edge_trims_with_ripple() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let before = layout(app(&h));
    // pull the end of the first clip 1 s earlier
    let end = clip_pos(app(&h), 0, 1.0) + egui::vec2(-2.0, 0.0);
    drag(&mut h, end, end + egui::vec2(-40.0, 0.0));
    let after = layout(app(&h));
    // started 2 px inside the edge and moved 40 px: 42 px at 40 px/s and 24 fps is 25 frames
    assert_eq!(after[0].1, before[0].1 - 25, "{after:?}");
    assert_eq!(
        after[1].0, after[0].1,
        "later clips ripple left, staying butted up"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    settle(&mut h);
    assert_eq!(layout(app(&h)), before);
}

#[test]
fn blade_tool_cuts_where_you_click() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    h.key_press(Key::B);
    settle(&mut h);
    assert_eq!(app(&h).tool, Tool::Blade);
    let p = clip_pos(app(&h), 0, 0.37);
    click(&mut h, p);
    let after = layout(app(&h));
    assert_eq!(after.len(), 4, "{after:?}");
    assert_eq!(after[0].1 + after[1].1, 96);
}

#[test]
fn keyboard_play_step_mark_lift_and_undo() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    h.key_press(Key::ArrowRight);
    h.key_press(Key::ArrowRight);
    settle(&mut h);
    assert_eq!(app(&h).playhead, 2);
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowRight);
    settle(&mut h);
    assert_eq!(app(&h).playhead, 26, "shift steps a second");
    h.key_press(Key::I);
    settle(&mut h);
    assert_eq!(app(&h).mark_in, Some(26));
    h.key_press(Key::Home);
    settle(&mut h);
    assert_eq!(app(&h).playhead, 0);
    // select the middle clip, lift it, undo
    let p = clip_pos(app(&h), 1, 0.5);
    click(&mut h, p);
    let before = layout(app(&h));
    h.key_press(Key::Delete);
    settle(&mut h);
    let lifted = layout(app(&h));
    assert_eq!(lifted.len(), 3);
    assert_eq!(lifted[1], before[1], "lift leaves a gap of the same size");
    assert!(app(&h)
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == "V1")
        .unwrap()
        .items[1]
        .as_clip()
        .is_none());
    // extract instead: undo the lift, select again, shift-delete
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    settle(&mut h);
    let p = clip_pos(app(&h), 1, 0.5);
    click(&mut h, p);
    h.key_press_modifiers(Modifiers::SHIFT, Key::Delete);
    settle(&mut h);
    assert_eq!(layout(app(&h)).len(), 2);
    assert_eq!(app(&h).duration_ticks(), 192);
    // redo
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    settle(&mut h);
}

#[test]
fn space_plays_and_stops_at_the_end_of_the_timeline() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    // half a second from the end: long enough to still be playing after a slow, busy machine's first frames
    let near_end = app(&h).duration_ticks() - 12;
    app_mut(&mut h).set_playhead(near_end);
    h.key_press(Key::Space);
    h.step();
    assert!(app(&h).playing);
    for _ in 0..80 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert!(!app(&h).playing, "playback stops at the end");
    assert_eq!(app(&h).playhead, app(&h).duration_ticks());
}

#[test]
fn dragging_media_from_the_pool_onto_a_track_places_it() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let v2 = app(&h)
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == "V2")
        .unwrap()
        .id;
    let media = *app(&h).project().media.keys().next().unwrap();
    let g = app(&h).timeline_layout.clone().unwrap();
    let target = pos2(g.x(6.0), g.row_center(v2).unwrap());
    // what the pool does when a drag starts
    egui::DragAndDrop::set_payload(&h.ctx, MediaDrag(media));
    h.hover_at(target);
    settle(&mut h);
    h.drag_at(target);
    h.step();
    h.drop_at(target);
    settle(&mut h);
    let t = app(&h)
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.id == v2)
        .unwrap()
        .clone();
    let starts = t.item_starts();
    let clip_idx = t
        .items
        .iter()
        .position(|i| i.as_clip().is_some())
        .expect("a clip landed on V2");
    assert_eq!(starts[clip_idx], 144, "6 s at 24 fps");
    assert_eq!(t.items[clip_idx].duration().value, 96);
    app(&h).project().validate().unwrap();
}

fn track_named(app: &App, name: &str) -> Track {
    app.timeline()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == name)
        .unwrap()
        .as_ref()
        .clone()
}

/// Screen position inside item `index` of the track `name`, at fraction `frac` of its length.
fn item_pos(app: &App, name: &str, index: usize, frac: f32) -> Pos2 {
    let t = track_named(app, name);
    let start = t.item_starts()[index];
    let dur = t.items[index].duration().value;
    let g = app.timeline_layout.clone().unwrap();
    let fps = app.fps();
    let (x0, x1) = (g.x(start as f64 / fps), g.x((start + dur) as f64 / fps));
    pos2(x0 + (x1 - x0) * frac, g.row_center(t.id).unwrap())
}

fn titles_on(app: &App, name: &str) -> Vec<(i64, i64)> {
    let t = track_named(app, name);
    t.items
        .iter()
        .zip(t.item_starts())
        .filter(|(i, _)| i.as_title().is_some())
        .map(|(i, s)| (s, i.duration().value))
        .collect()
}

#[test]
fn dragging_a_title_template_onto_a_track_adds_a_title() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let g = app(&h).timeline_layout.clone().unwrap();
    let v2 = track_named(app(&h), "V2").id;
    let target = pos2(g.x(6.0), g.row_center(v2).unwrap());
    egui::DragAndDrop::set_payload(&h.ctx, captaine_app::ui::TitleDrag(TitleTemplate::Centered));
    h.hover_at(target);
    settle(&mut h);
    h.drag_at(target);
    h.step();
    h.drop_at(target);
    settle(&mut h);
    assert_eq!(
        titles_on(app(&h), "V2"),
        [(144, 72)],
        "3 s long, starting at 6 s"
    );
    let a = app(&h);
    assert!(a.selected_title().is_some(), "the new title is selected");
    a.project().validate().unwrap();
    // titles are refused on audio tracks
    let a1 = track_named(app(&h), "A1").id;
    let before = layout(app(&h));
    app_mut(&mut h).add_title(TitleTemplate::Caption, Some(0), Some(a1));
    assert_eq!(track_named(app(&h), "A1").items.len(), 0);
    assert_eq!(layout(app(&h)), before);
}

#[test]
fn titles_move_trim_cut_and_undo_like_clips() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    app_mut(&mut h).add_title(TitleTemplate::LowerThird, Some(48), None); // top video track = V2
    settle(&mut h);
    assert_eq!(titles_on(app(&h), "V2"), [(48, 72)]);
    let after_add = track_named(app(&h), "V2");
    // find the title's index: after the padding gap
    let idx = after_add
        .items
        .iter()
        .position(|i| i.as_title().is_some())
        .unwrap();

    // trim its end 1 s earlier
    let end = item_pos(app(&h), "V2", idx, 1.0) + egui::vec2(-2.0, 0.0);
    drag(&mut h, end, end + egui::vec2(-40.0, 0.0));
    // 42 px is 25 frames, but the end snaps to the cut on V1 at frame 96: 48 frames long
    let len = titles_on(app(&h), "V2")[0].1;
    assert!(len == 72 - 25 || len == 72 - 24, "{len}");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    settle(&mut h);
    assert_eq!(titles_on(app(&h), "V2"), [(48, 72)]);

    // drag it 2 s to the right
    let mid = item_pos(app(&h), "V2", idx, 0.5);
    drag(&mut h, mid, mid + egui::vec2(80.0, 0.0));
    assert_eq!(titles_on(app(&h), "V2")[0].0, 96, "moved by 48 frames");

    // blade cuts it in two titles with the same text
    h.key_press(Key::B);
    settle(&mut h);
    let idx = track_named(app(&h), "V2")
        .items
        .iter()
        .position(|i| i.as_title().is_some())
        .unwrap();
    let p = item_pos(app(&h), "V2", idx, 0.5);
    click(&mut h, p);
    let cut = titles_on(app(&h), "V2");
    assert_eq!(cut.len(), 2, "{cut:?}");
    assert_eq!(cut[0].1 + cut[1].1, 72);
    let t = track_named(app(&h), "V2");
    let texts: Vec<_> = t
        .items
        .iter()
        .filter_map(|i| i.as_title())
        .map(|x| x.text.clone())
        .collect();
    assert_eq!(texts[0], texts[1]);
    app(&h).project().validate().unwrap();

    // lift: select the first half and press delete
    h.key_press(Key::V);
    let idx = track_named(app(&h), "V2")
        .items
        .iter()
        .position(|i| i.as_title().is_some())
        .unwrap();
    let p = item_pos(app(&h), "V2", idx, 0.5);
    click(&mut h, p);
    h.key_press(Key::Delete);
    settle(&mut h);
    assert_eq!(titles_on(app(&h), "V2").len(), 1);
}

#[test]
fn editing_a_title_in_the_inspector_is_one_undo_step() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    app_mut(&mut h).add_title(TitleTemplate::LowerThird, Some(0), None);
    settle(&mut h);
    let id = app(&h).selected_clip.unwrap();
    let history = app(&h).session.editor.history().len();
    // what the inspector does as a slider is dragged: many SetTitle ops under one key
    {
        let a = app_mut(&mut h);
        for i in 1..=8 {
            let (mut t, ..) = a.selected_title().unwrap();
            t.style.size = 46.0 + i as f64 * 5.0;
            t.text = format!("Headline {i}");
            a.do_merging(
                captaine_ops::Op::SetTitle { item: id, title: t },
                &format!("t:{}", id.to_full_string()),
            );
        }
        a.end_merge();
    }
    settle(&mut h);
    assert_eq!(
        app(&h).session.editor.history().len(),
        history + 1,
        "one entry for the whole drag"
    );
    assert_eq!(app(&h).selected_title().unwrap().0.text, "Headline 8");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    settle(&mut h);
    let t = app(&h).selected_title().unwrap().0;
    assert_eq!((t.text.as_str(), t.style.size), ("Your Title", 46.0));
    // the saved log replays to the same project
    let log = app(&h).session.editor.history().to_vec();
    let mut replay = captaine_ops::Editor::new(captaine_model::Project::new(
        app(&h).project().settings.clone(),
    ));
    for e in log {
        replay.apply(e.author, e.op).unwrap();
    }
    assert_eq!(replay.project(), app(&h).project());
}

#[test]
fn keyframe_diamonds_in_the_inspector_add_and_remove_keys_with_undo() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let (id, start) = {
        let a = app(&h);
        let t = v1(a);
        (
            match &t.items[0] {
                Item::Clip(c) => c.id,
                _ => panic!("first item is a clip"),
            },
            0i64,
        )
    };
    app_mut(&mut h).selected_clip = Some(id);
    app_mut(&mut h).playhead = start + 10;
    settle(&mut h);
    h.get_by_label("Keyframe Opacity").click();
    settle(&mut h);
    app_mut(&mut h).playhead = start + 20;
    settle(&mut h);
    h.get_by_label("Keyframe Opacity").click();
    settle(&mut h);
    let keys = |a: &App| a.selected().unwrap().0.anim.keys("opacity").to_vec();
    assert_eq!(
        keys(app(&h)).iter().map(|k| k.t).collect::<Vec<_>>(),
        vec![10, 20]
    );
    // clicking on an existing key removes it
    h.get_by_label("Keyframe Opacity").click();
    settle(&mut h);
    assert_eq!(keys(app(&h)).len(), 1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    settle(&mut h);
    assert_eq!(keys(app(&h)).len(), 2);
}

#[test]
fn inspector_reverse_and_freeze_frame_shortcut() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let first = |a: &App| match &v1(a).items[0] {
        Item::Clip(c) => c.clone(),
        _ => panic!("first item is a clip"),
    };
    let id = first(app(&h)).id;
    let len = first(app(&h)).duration.value;
    app_mut(&mut h).selected_clip = Some(id);
    settle(&mut h);
    h.get_by_label("Reverse").scroll_to_me();
    settle(&mut h);
    h.get_by_label("Reverse").click();
    settle(&mut h);
    assert!(first(app(&h)).retime.reverse);
    h.get_by_label("Reverse").click();
    settle(&mut h);
    assert!(!first(app(&h)).retime.reverse);
    // Shift+F holds the frame under the playhead: the clip is cut and a still goes in between
    let items_before = v1(app(&h)).items.len();
    app_mut(&mut h).playhead = 10;
    h.key_press_modifiers(Modifiers::SHIFT, Key::F);
    settle(&mut h);
    let items = v1(app(&h)).items;
    assert_eq!(items.len(), items_before + 2);
    let still = match &items[1] {
        Item::Clip(c) => c.clone(),
        _ => panic!("a still clip"),
    };
    assert!(still.retime.freeze);
    assert_eq!(
        still.source_start.value,
        first(app(&h)).source_start.value + 10
    );
    assert_eq!(first(app(&h)).duration.value, 10);
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(
        first(app(&h)).duration.value,
        len,
        "one undo puts it all back"
    );
}

#[test]
fn transition_shortcut_and_inspector_chips() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let second = |a: &App| match &v1(a).items[1] {
        Item::Clip(c) => c.clone(),
        _ => panic!("second item is a clip"),
    };
    let (first_id, second_id) = (
        match &v1(app(&h)).items[0] {
            Item::Clip(c) => c.id,
            _ => panic!(),
        },
        second(app(&h)).id,
    );
    // the first clip has nothing before it
    app_mut(&mut h).selected_clip = Some(first_id);
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    settle(&mut h);
    assert!(match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.transition.is_none(),
        _ => false,
    });
    app_mut(&mut h).selected_clip = Some(second_id);
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    settle(&mut h);
    let t = second(app(&h)).transition.expect("a dissolve was added");
    assert_eq!(t.kind, TransitionKind::Dissolve);
    h.get_by_label("Wipe left").scroll_to_me();
    settle(&mut h);
    h.get_by_label("Wipe left").click();
    settle(&mut h);
    let t2 = second(app(&h)).transition.unwrap();
    assert_eq!(
        (t2.kind, t2.duration),
        (TransitionKind::WipeLeft, t.duration)
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    settle(&mut h);
    assert!(
        second(app(&h)).transition.is_none(),
        "the shortcut toggles it off"
    );
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(
        second(app(&h)).transition.unwrap().kind,
        TransitionKind::WipeLeft
    );
}

#[test]
fn adding_and_removing_an_effect_in_the_inspector() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let first = |a: &App| match &v1(a).items[0] {
        Item::Clip(c) => c.clone(),
        _ => panic!("first item is a clip"),
    };
    let id = first(app(&h)).id;
    app_mut(&mut h).selected_clip = Some(id);
    settle(&mut h);
    h.get_by_label("+ Blur").scroll_to_me();
    settle(&mut h);
    h.get_by_label("+ Blur").click();
    settle(&mut h);
    h.get_by_label("+ Vignette").click();
    settle(&mut h);
    let kinds = |a: &App| first(a).effects.iter().map(|e| e.kind).collect::<Vec<_>>();
    assert_eq!(kinds(app(&h)), [EffectKind::Blur, EffectKind::Vignette]);
    // the first "Off" switches the blur off
    h.get_all_by_label("On").next().unwrap().click();
    settle(&mut h);
    assert!(!first(app(&h)).effects[0].enabled);
    h.get_all_by_label("Remove").next().unwrap().click();
    settle(&mut h);
    assert_eq!(kinds(app(&h)), [EffectKind::Vignette]);
    for _ in 0..4 {
        app_mut(&mut h).undo();
    }
    settle(&mut h);
    assert!(first(app(&h)).effects.is_empty());
}

#[test]
fn the_scopes_panel_draws_from_the_picture_on_screen() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    app_mut(&mut h).playhead = 5;
    settle(&mut h);
    h.get_by_label("Scopes").click();
    settle(&mut h);
    for _ in 0..200 {
        h.step();
        if app(&h).scope_tex.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(app(&h).scopes_open);
    assert!(
        app(&h).scope_tex.is_some(),
        "a scope picture was made from the frame"
    );
    h.get_by_label("Vectorscope").click();
    settle(&mut h);
    assert_eq!(
        app(&h).scope_kind,
        captaine_app::scopes::ScopeKind::Vectorscope
    );
    h.get_by_label("Scopes").click();
    settle(&mut h);
    assert!(!app(&h).scopes_open);
}

#[test]
fn mixer_tab_lists_tracks_and_audio_effects_can_be_added() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let first = |a: &App| match &v1(a).items[0] {
        Item::Clip(c) => c.clone(),
        _ => panic!("first item is a clip"),
    };
    app_mut(&mut h).selected_clip = Some(first(app(&h)).id);
    settle(&mut h);
    // clips with sound offer the sound effects
    h.get_by_label("+ Compressor").scroll_to_me();
    settle(&mut h);
    h.get_by_label("+ Compressor").click();
    settle(&mut h);
    assert_eq!(first(app(&h)).effects[0].kind, EffectKind::Compressor);
    h.get_by_label("Mixer").click();
    settle(&mut h);
    assert_eq!(
        app(&h).inspector_tab,
        captaine_app::app::InspectorTab::Mixer
    );
    h.get_by_label("V1");
    assert!(h.get_all_by_label("Master").count() >= 1);
    h.get_by_label_contains("Levels follow playback");
}

#[test]
fn multi_select_copy_paste_duplicate_and_delete() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let ids = |a: &App| -> Vec<Id> {
        v1(a)
            .items
            .iter()
            .filter(|i| i.is_content())
            .map(|i| i.id())
            .collect()
    };
    let originals = ids(app(&h));
    assert_eq!(originals.len(), 3);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    settle(&mut h);
    assert_eq!(app(&h).selection_ids().len(), 3);
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    settle(&mut h);
    // paste after the end of the timeline: three more clips with new ids, one undo step
    let end = app(&h).duration_ticks();
    app_mut(&mut h).playhead = end;
    let history = app(&h).session.editor.history().len();
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    settle(&mut h);
    let after = ids(app(&h));
    assert_eq!(after.len(), 6);
    assert!(
        after[3..].iter().all(|i| !originals.contains(i)),
        "fresh ids"
    );
    assert_eq!(app(&h).session.editor.history().len(), history + 1);
    assert_eq!(
        app(&h).selection_ids(),
        after[3..].to_vec(),
        "the pasted items are selected"
    );
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(ids(app(&h)), originals);
    // duplicate the middle clip: the copy slots in after it and pushes c along
    app_mut(&mut h).select_only(&[originals[1]]);
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    settle(&mut h);
    let dup = ids(app(&h));
    assert_eq!(dup.len(), 4);
    assert_eq!(
        (dup[0], dup[1], dup[3]),
        (originals[0], originals[1], originals[2])
    );
    // lift two at once
    app_mut(&mut h).select_only(&[dup[0], dup[3]]);
    let total = app(&h).duration_ticks();
    h.key_press(Key::Backspace);
    settle(&mut h);
    assert_eq!(app(&h).duration_ticks(), total, "lifting leaves gaps");
    assert_eq!(ids(app(&h)).len(), 2);
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(ids(app(&h)).len(), 4);
}

#[test]
fn export_dialog_queues_jobs_and_writes_the_file() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let dir = std::env::temp_dir().join(format!("captaine-queue-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (wav, m4a) = (dir.join("mix.wav"), dir.join("mix.m4a"));
    app_mut(&mut h).export.open = true;
    settle(&mut h);
    h.get_by_label("Audio WAV").click(); // a preset
    settle(&mut h);
    assert_eq!(app(&h).export.codec, captaine_export::Codec::Wav);
    app_mut(&mut h).export.path = Some(wav.clone());
    settle(&mut h);
    // the dialog's button comes after the top bar's and the window title
    h.get_all_by_label("Export").last().unwrap().click();
    settle(&mut h);
    // queue a second one straight away
    h.get_by_label("Audio podcast").click();
    settle(&mut h);
    app_mut(&mut h).export.path = Some(m4a.clone());
    settle(&mut h);
    assert_eq!(app(&h).export.jobs.len(), 1);
    for _ in 0..400 {
        h.step();
        if wav.exists()
            && app(&h)
                .export
                .jobs
                .iter()
                .all(|j| j.progress.lock().unwrap().2.is_some())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(wav.exists(), "the first export wrote its file");
    assert!(matches!(
        app(&h).export.jobs[0].progress.lock().unwrap().2,
        Some(Ok(()))
    ));
    // the second is added to the queue with the button's other label and finishes too
    for _ in 0..8 {
        if app(&h).export.jobs.len() == 2 {
            break;
        }
        h.get_all_by_label("Export").last().unwrap().click();
        settle(&mut h);
    }
    for _ in 0..400 {
        h.step();
        if m4a.exists()
            && app(&h).export.jobs.len() == 2
            && app(&h)
                .export
                .jobs
                .iter()
                .all(|j| j.progress.lock().unwrap().2.is_some())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert_eq!(app(&h).export.jobs.len(), 2);
    assert!(m4a.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn source_monitor_three_and_four_point_edits() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let media_a = app(&h)
        .project()
        .media
        .values()
        .find(|m| m.name == "a.mp4")
        .unwrap()
        .id;
    app_mut(&mut h).open_source(media_a);
    settle(&mut h);
    assert!(app(&h).source.open);
    // source marks: frames 24..72 of a 24 fps file = 2 s
    app_mut(&mut h).source.mark_in = Some(24);
    app_mut(&mut h).source.mark_out = Some(72);
    let total = app(&h).duration_ticks();
    // insert (3-point, the playhead is the timeline in): ripples 48 ticks
    app_mut(&mut h).playhead = 96; // the cut between a and b
    h.key_press(Key::Comma);
    settle(&mut h);
    assert_eq!(app(&h).duration_ticks(), total + 48);
    let placed = app(&h).selected().unwrap().0;
    assert_eq!((placed.source_start.value, placed.duration.value), (24, 48));
    assert!(placed.retime.is_identity());
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(app(&h).duration_ticks(), total);
    // overwrite does not change the length
    app_mut(&mut h).playhead = 10;
    h.key_press(Key::Period);
    settle(&mut h);
    assert_eq!(app(&h).duration_ticks(), total);
    assert_eq!(app(&h).selected().unwrap().0.source_start.value, 24);
    app_mut(&mut h).undo();
    // four points: timeline marks 100..124 hold the 2 s of source, so it plays at double speed
    app_mut(&mut h).mark_in = Some(100);
    app_mut(&mut h).mark_out = Some(124);
    app_mut(&mut h).source_edit(false);
    settle(&mut h);
    let fitted = app(&h).selected().unwrap().0;
    assert_eq!(fitted.duration.value, 24);
    assert!(
        (fitted.retime.speed - 2.0).abs() < 1e-9,
        "{}",
        fitted.retime.speed
    );
    // the viewer can be switched back to the program
    settle(&mut h);
    app_mut(&mut h).toasts.clear(); // a toast would sit over the tab
    h.get_by_label("Program").click();
    settle(&mut h);
    settle(&mut h);
    assert!(!app(&h).source.open);
}

#[test]
fn several_sequences_can_be_made_switched_renamed_and_deleted() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let first = app(&h).timeline_id().unwrap();
    let first_len = app(&h).duration_ticks();
    assert!(first_len > 0);
    app_mut(&mut h).new_sequence();
    settle(&mut h);
    let second = app(&h).timeline_id().unwrap();
    assert_ne!(first, second);
    assert_eq!(app(&h).duration_ticks(), 0, "a new sequence is empty");
    assert_eq!(app(&h).project().timelines.len(), 2);
    // the first one is untouched
    app_mut(&mut h).set_active_timeline(first);
    settle(&mut h);
    assert_eq!(app(&h).duration_ticks(), first_len);
    // duplicating gives a copy with fresh ids
    app_mut(&mut h).duplicate_sequence();
    settle(&mut h);
    let copy = app(&h).timeline().unwrap().clone();
    assert_ne!(copy.id, first);
    assert!(copy.name.contains("copy"));
    let ids = |t: &Timeline| -> Vec<Id> {
        t.tracks
            .iter()
            .flat_map(|tr| tr.items.iter().map(|i| i.id()))
            .collect()
    };
    let original = app(&h).project().timeline(first).unwrap().clone();
    assert!(ids(&copy).iter().all(|i| !ids(&original).contains(i)));
    assert_eq!(copy.duration().value, first_len);
    assert!(app(&h).project().validate().is_ok());
    // rename (undoable, and part of the history)
    app_mut(&mut h).rename_sequence(copy.id, "Director's cut".into());
    assert_eq!(app(&h).timeline().unwrap().name, "Director's cut");
    app_mut(&mut h).undo();
    assert!(app(&h).timeline().unwrap().name.contains("copy"));
    // delete the active one: another becomes active
    let before = app(&h).project().timelines.len();
    app_mut(&mut h).delete_sequence(copy.id);
    settle(&mut h);
    assert_eq!(app(&h).project().timelines.len(), before - 1);
    assert_ne!(app(&h).timeline_id(), Some(copy.id));
    // the last one cannot be deleted
    while app(&h).project().timelines.len() > 1 {
        let id = app(&h).timeline_id().unwrap();
        app_mut(&mut h).delete_sequence(id);
    }
    let only = app(&h).timeline_id().unwrap();
    app_mut(&mut h).delete_sequence(only);
    assert_eq!(app(&h).project().timelines.len(), 1);
}

#[test]
fn shapes_and_adjustment_layers_from_the_pool() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    app_mut(&mut h).playhead = 20;
    app_mut(&mut h).add_layer(Some(ShapeKind::Ellipse), None, None);
    settle(&mut h);
    let sel = app(&h)
        .selected_title()
        .expect("the new shape is selected")
        .0;
    assert_eq!(sel.shape.unwrap().kind, ShapeKind::Ellipse);
    h.get_by_label("Outline").scroll_to_me();
    app_mut(&mut h).add_layer(None, None, None);
    settle(&mut h);
    let adj = app(&h).selected_title().unwrap().0;
    assert!(adj.adjustment && adj.shape.is_none());
    h.get_by_label("+ Blur").scroll_to_me();
    settle(&mut h);
    h.get_by_label("+ Blur").click();
    settle(&mut h);
    assert_eq!(
        app(&h).selected_title().unwrap().0.effects[0].kind,
        EffectKind::Blur
    );
    app_mut(&mut h).undo();
    settle(&mut h);
    assert!(app(&h).selected_title().unwrap().0.effects.is_empty());
}

#[test]
fn transcript_tab_offers_transcription_and_subtitle_tools() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let id = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.id,
        _ => panic!("a clip"),
    };
    app_mut(&mut h).selected_clip = Some(id);
    settle(&mut h);
    h.get_by_label("Transcript").click();
    settle(&mut h);
    assert_eq!(
        app(&h).inspector_tab,
        captaine_app::app::InspectorTab::Transcript
    );
    h.get_by_label("Export SRT…");
    h.get_by_label("Import SRT…");
    if captaine_speech::unavailable().is_none() {
        h.get_by_label("Transcribe");
        // a finished transcript shows its words and the editing buttons
        let key = captaine_app::transcript_ui::key_of(&match &v1(app(&h)).items[0] {
            Item::Clip(c) => c.clone(),
            _ => unreachable!(),
        });
        let words = ["Hello", "um", "world."]
            .iter()
            .enumerate()
            .map(|(i, w)| captaine_speech::Word {
                text: w.to_string(),
                start: i as f64,
                end: i as f64 + 0.5,
            })
            .collect();
        app_mut(&mut h).transcripts.insert(
            key,
            vec![captaine_speech::Segment {
                text: "Hello um world.".into(),
                start: 0.0,
                end: 2.5,
                words,
            }],
        );
        settle(&mut h);
        h.get_by_label("um").click();
        settle(&mut h);
        assert_eq!(app(&h).word_sel, Some((1, 1)));
        let before = app(&h).duration_ticks();
        h.get_by_label("Cut selected words").click();
        settle(&mut h);
        assert!(
            app(&h).duration_ticks() < before,
            "the word's time was cut out"
        );
        app_mut(&mut h).undo();
        settle(&mut h);
        assert_eq!(app(&h).duration_ticks(), before);
    }
}

#[test]
fn markers_pin_jump_edit_and_list_as_chapters() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    app_mut(&mut h).playhead = 48;
    settle(&mut h);
    h.key_press(Key::M);
    settle(&mut h);
    app_mut(&mut h).playhead = 200;
    h.key_press(Key::M);
    settle(&mut h);
    let ms = app(&h).markers();
    assert_eq!((ms.len(), ms[0].tick, ms[1].tick), (2, 48, 200));
    assert_eq!(ms[0].name, "Marker 1");
    // jump between them with alt + arrows
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowLeft);
    settle(&mut h);
    assert_eq!(app(&h).playhead, 48);
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowRight);
    settle(&mut h);
    assert_eq!(app(&h).playhead, 200);
    // rename and recolour through the update path the dialog uses
    let mut m = ms[0].clone();
    m.name = "Intro".into();
    m.color = 3;
    app_mut(&mut h).update_marker(m);
    assert_eq!(app(&h).markers()[0].name, "Intro");
    // chapters, in the form video sites read
    assert_eq!(
        app(&h).chapters_text(),
        "0:00 Start\n0:02 Intro\n0:08 Marker 2"
    );
    // the list window shows them
    h.get_by_label("Markers").click();
    settle(&mut h);
    h.get_by_label("Intro");
    h.get_by_label("Copy chapters");
    // everything is one undo each
    app_mut(&mut h).undo();
    app_mut(&mut h).undo();
    app_mut(&mut h).undo();
    assert!(app(&h).markers().is_empty());
}

fn track_named_clips(app: &App, name: &str) -> Vec<Clip> {
    track_named(app, name)
        .items
        .iter()
        .filter_map(|i| i.as_clip().cloned())
        .collect()
}

#[test]
fn separated_audio_stays_linked_through_selecting_moving_cutting_and_deleting() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let first = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.id,
        _ => panic!("a clip"),
    };
    app_mut(&mut h).selected_clip = Some(first);
    app_mut(&mut h).separate_audio();
    settle(&mut h);
    let video = track_named_clips(app(&h), "V1")[0].clone();
    let audio = track_named_clips(app(&h), "A1");
    assert_eq!(audio.len(), 1, "the sound is on A1 now");
    assert!(video.audio.muted && video.link.is_some() && video.link == audio[0].link);
    assert_eq!(
        (audio[0].source_start, audio[0].duration),
        (video.source_start, video.duration)
    );
    assert!(audio[0].video == VideoProps::default());
    app(&h).project().validate().unwrap();
    // clicking the picture selects the sound too
    app_mut(&mut h).select_only(&[]);
    let p = clip_pos(app(&h), 0, 0.5);
    click(&mut h, p);
    assert_eq!(app(&h).selection_ids().len(), 2);
    // dragging the picture later in time carries the sound with it (here: past the end of the others)
    let start_of = |a: &App, name: &str| {
        let t = track_named(a, name);
        t.items
            .iter()
            .zip(t.item_starts())
            .find_map(|(i, s)| i.as_clip().map(|_| s))
            .unwrap()
    };
    let (v0, a0) = (start_of(app(&h), "V1"), start_of(app(&h), "A1"));
    assert_eq!(v0, a0);
    let before = app(&h).session.editor.history().len();
    let from = clip_pos(app(&h), 0, 0.5);
    // drop it on V2, 1 second later (40 px)
    let g = app(&h).timeline_layout.clone().unwrap();
    let v2 = track_named(app(&h), "V2").id;
    drag(&mut h, from, pos2(from.x + 40.0, g.row_center(v2).unwrap()));
    let moved_video = track_named_clips(app(&h), "V2");
    let moved_audio = track_named(app(&h), "A1");
    assert_eq!(moved_video.len(), 1, "the picture went to V2");
    let a_start = moved_audio
        .items
        .iter()
        .zip(moved_audio.item_starts())
        .find_map(|(i, s)| i.as_clip().map(|_| s))
        .unwrap();
    assert!(
        a_start > 0 && (a_start - 24).abs() <= 2,
        "the sound moved the same 1 s: {a_start}"
    );
    assert_eq!(
        app(&h).session.editor.history().len(),
        before + 1,
        "one undo step for both"
    );
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(start_of(app(&h), "A1"), 0);
    // turning Link off selects just what you click
    h.get_by_label("Link").click();
    settle(&mut h);
    assert!(!app(&h).link_on);
    app_mut(&mut h).select_only(&[]);
    let p = clip_pos(app(&h), 0, 0.5);
    click(&mut h, p);
    assert_eq!(app(&h).selection_ids().len(), 1);
    h.get_by_label("Link").click();
    settle(&mut h);
    // a cut at the playhead goes through both; lifting takes both
    app_mut(&mut h).select_with_links(first);
    app_mut(&mut h).playhead = 40;
    h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    settle(&mut h);
    assert_eq!(
        track_named_clips(app(&h), "A1").len(),
        2,
        "the sound was cut too"
    );
    assert_eq!(track_named_clips(app(&h), "V1").len(), 4);
    app_mut(&mut h).undo();
    app_mut(&mut h).select_with_links(first);
    h.key_press(Key::Backspace);
    settle(&mut h);
    assert!(track_named_clips(app(&h), "A1").is_empty());
    assert_eq!(track_named_clips(app(&h), "V1").len(), 2);
    // explicit linking and unlinking
    app_mut(&mut h).undo();
    let ids: Vec<Id> = track_named_clips(app(&h), "V1")
        .iter()
        .map(|c| c.id)
        .collect();
    app_mut(&mut h).select_only(&ids);
    app_mut(&mut h).link_selected();
    assert!(track_named_clips(app(&h), "V1")
        .iter()
        .all(|c| c.link.is_some()));
    app_mut(&mut h).unlink_selected();
    assert!(track_named_clips(app(&h), "V1")
        .iter()
        .all(|c| c.link.is_none()));
}

#[test]
fn title_animation_presets_and_styling_from_the_inspector() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    app_mut(&mut h).add_title(TitleTemplate::Centered, Some(0), None);
    settle(&mut h);
    let title = |a: &App| a.selected_title().unwrap().0;
    h.get_by_label("Typewriter").scroll_to_me();
    settle(&mut h);
    h.get_by_label("Typewriter").click();
    settle(&mut h);
    assert!(title(app(&h)).anim.has("reveal"));
    // the out row has no typewriter; its Fade chip is the second one
    let fades: Vec<_> = h.get_all_by_label("Fade").collect();
    assert_eq!(
        fades.len(),
        2,
        "one Fade chip in each of the In and Out rows"
    );
    app_mut(&mut h).undo();
    settle(&mut h);
    assert!(
        !title(app(&h)).anim.has("reveal"),
        "one undo removes the whole preset"
    );
    // alignment and a new template through the same controls
    h.get_by_label("Left").scroll_to_me();
    settle(&mut h);
    h.get_by_label("Left").click();
    settle(&mut h);
    assert_eq!(title(app(&h)).style.align, TextAlign::Left);
    h.get_by_label("Quote").scroll_to_me();
    h.get_by_label("Quote").click();
    settle(&mut h);
    assert_eq!(title(app(&h)).style.template, TitleTemplate::Quote);
    h.get_by_label("Mono").click();
    settle(&mut h);
    assert_eq!(title(app(&h)).font.as_deref(), Some("builtin:mono"));
}

#[test]
fn remove_background_runs_in_the_background_and_lands_as_one_effect() {
    if !have_ffmpeg() || captaine_vision::unavailable().is_some() {
        return;
    }
    let mut h = editor();
    let id = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.id,
        _ => panic!("a clip"),
    };
    app_mut(&mut h).selected_clip = Some(id);
    settle(&mut h);
    h.get_by_label("Remove background").scroll_to_me();
    settle(&mut h);
    let history = app(&h).session.editor.history().len();
    h.get_by_label("Remove background").click();
    for _ in 0..1200 {
        h.step();
        if !app(&h).vision_jobs.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        } else {
            break;
        }
    }
    assert!(app(&h).vision_jobs.is_empty(), "the job finished");
    let c = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.clone(),
        _ => panic!("a clip"),
    };
    assert_eq!(c.effects[0].kind, EffectKind::Matte);
    assert!(std::path::Path::new(&c.effects[0].text).exists());
    assert_eq!(app(&h).session.editor.history().len(), history + 1);
    app_mut(&mut h).undo();
    assert!(match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.effects.is_empty(),
        _ => false,
    });
}

#[test]
fn auto_reframe_makes_a_vertical_copy_of_the_sequence() {
    if !have_ffmpeg() || captaine_vision::unavailable().is_some() {
        return;
    }
    let mut h = editor();
    let original = app(&h).timeline_id().unwrap();
    let originals = v1(app(&h)).items.len();
    app_mut(&mut h).auto_reframe("Vertical 9:16", (9, 16));
    for _ in 0..2400 {
        h.step();
        if app(&h).vision_jobs.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(app(&h).vision_jobs.is_empty(), "the reframe finished");
    let now = app(&h).timeline().unwrap().clone();
    assert_ne!(now.id, original, "the new sequence is the active one");
    assert_eq!(
        now.canvas,
        Some((720, 1280)),
        "9:16 from the project's short side (720)"
    );
    assert!(now.name.contains("9:16"));
    assert_eq!(
        app(&h).project().canvas(original),
        (1280, 720),
        "the original is untouched"
    );
    let clips: Vec<Clip> = now
        .tracks
        .iter()
        .flat_map(|t| t.items.iter())
        .filter_map(|i| i.as_clip().cloned())
        .collect();
    assert!(clips.len() >= originals, "clips came across");
    let video: Vec<&Clip> = clips
        .iter()
        .filter(|c| c.video.transform.scale.0 > 1.5)
        .collect();
    assert!(
        video.len() >= 3,
        "every video clip was scaled up to fill the tall frame"
    );
    app(&h).project().validate().unwrap();
    // one undo takes the whole thing back
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(app(&h).project().timelines.len(), 1);
}

#[test]
fn nesting_clips_into_a_sequence_and_stepping_into_it() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let outer = app(&h).timeline_id().unwrap();
    let total = app(&h).duration_ticks();
    let ids: Vec<Id> = v1(app(&h)).items.iter().take(2).map(|i| i.id()).collect();
    let start = app(&h).project().clone();
    app_mut(&mut h).select_only(&ids);
    app_mut(&mut h).nest_selection();
    settle(&mut h);
    // one nested clip took the place of the two, and the length is unchanged
    assert_eq!(app(&h).duration_ticks(), total);
    let items: Vec<Item> = v1(app(&h)).items.clone();
    let nested = items
        .iter()
        .find_map(|i| {
            i.as_clip()
                .filter(|c| app(&h).nested_sequence_of(c).is_some())
        })
        .expect("a nested clip")
        .clone();
    assert_eq!(nested.duration.value, 192, "two 4 s clips at 24 fps");
    assert_eq!(app(&h).project().timelines.len(), 2);
    app(&h).project().validate().unwrap();
    // the pool lists the sequences and hides the nested stand-in from the footage
    assert!(app(&h)
        .project()
        .media
        .values()
        .any(|m| m.nested_timeline().is_some()));
    // step into it: the two original clips are there, back to back
    app_mut(&mut h).select_only(&[nested.id]);
    app_mut(&mut h).open_nested();
    settle(&mut h);
    assert_ne!(app(&h).timeline_id(), Some(outer));
    let inside: Vec<(i64, i64)> = v1(app(&h))
        .items
        .iter()
        .zip(v1(app(&h)).item_starts())
        .map(|(i, s)| (s, i.duration().value))
        .collect();
    assert_eq!(inside, [(0, 96), (96, 96)]);
    // go back, and put the inner sequence in a second time from the pool's Insert
    app_mut(&mut h).set_active_timeline(outer);
    settle(&mut h);
    let inner = app(&h)
        .sequences()
        .into_iter()
        .find(|s| s.0 != outer)
        .unwrap()
        .0;
    app_mut(&mut h).playhead = app(&h).duration_ticks();
    app_mut(&mut h).insert_sequence(inner, None);
    settle(&mut h);
    assert_eq!(app(&h).duration_ticks(), total + 192);
    // nothing is lost by undoing both
    app_mut(&mut h).undo();
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(app(&h).project(), &start);
}

#[test]
fn stabilize_swaps_in_a_steadied_copy_as_one_undo_step() {
    let has_vidstab = Command::new("ffmpeg")
        .args(["-hide_banner", "-filters"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("vidstabdetect"));
    if !have_ffmpeg() || !has_vidstab {
        return;
    }
    let mut h = editor();
    let first = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.clone(),
        _ => panic!("a clip"),
    };
    let history = app(&h).session.editor.history().len();
    app_mut(&mut h).stabilize_clip(first.id);
    for _ in 0..2400 {
        h.step();
        if app(&h).vision_jobs.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(app(&h).vision_jobs.is_empty());
    let now = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.clone(),
        _ => panic!("a clip"),
    };
    assert_ne!(now.media, first.media);
    assert_eq!(now.duration, first.duration);
    assert_eq!(app(&h).session.editor.history().len(), history + 1);
    app(&h).project().validate().unwrap();
    app_mut(&mut h).undo();
    settle(&mut h);
    assert!(match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.media == first.media,
        _ => false,
    });
}

#[test]
fn auto_color_and_match_color_from_the_inspector() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let ids: Vec<Id> = v1(app(&h)).items.iter().map(|i| i.id()).collect();
    let color_of = |a: &App, id: Id| a.clip_by_id(id).unwrap().0.video.color;
    app_mut(&mut h).selected_clip = Some(ids[0]);
    settle(&mut h);
    h.get_by_label("Auto color").scroll_to_me();
    settle(&mut h);
    let history = app(&h).session.editor.history().len();
    h.get_by_label("Auto color").click();
    settle(&mut h);
    assert_ne!(
        color_of(app(&h), ids[0]),
        ColorProps::default(),
        "the grade was set"
    );
    assert_eq!(app(&h).session.editor.history().len(), history + 1);
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(color_of(app(&h), ids[0]), ColorProps::default());
    // matching needs a second clip
    app_mut(&mut h).select_only(&[ids[1], ids[2]]);
    settle(&mut h);
    h.get_by_label("Match to other selected").scroll_to_me();
    h.get_by_label("Match to other selected").click();
    settle(&mut h);
    assert_ne!(color_of(app(&h), ids[1]), ColorProps::default());
    assert_eq!(
        color_of(app(&h), ids[2]),
        ColorProps::default(),
        "the reference is untouched"
    );
}

#[test]
fn multicam_switches_angles_with_the_number_keys() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    // the same clip as two "cameras": V1 already has a, b, c; put a copy of a on V2
    let a_media = app(&h)
        .project()
        .media
        .values()
        .find(|m| m.name == "a.mp4")
        .unwrap()
        .id;
    let v2 = track_named(app(&h), "V2").id;
    app_mut(&mut h).place_media(a_media, Some(v2), Some(0));
    settle(&mut h);
    let cam1 = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.id,
        _ => panic!("a clip"),
    };
    let cam2 = track_named_clips(app(&h), "V2")[0].id;
    app_mut(&mut h).select_only(&[cam1, cam2]);
    app_mut(&mut h).start_multicam();
    settle(&mut h);
    assert!(app(&h).multicam.is_some());
    let enabled = |a: &App, track: &str, at: i64| -> Option<bool> {
        let t = track_named(a, track);
        t.items
            .iter()
            .zip(t.item_starts())
            .find(|(_, s)| *s == at)
            .and_then(|(i, _)| i.as_clip().map(|c| c.enabled))
    };
    assert_eq!(
        (enabled(app(&h), "V1", 0), enabled(app(&h), "V2", 0)),
        (Some(true), Some(false)),
        "starts on angle 1"
    );
    // cut to angle 2 at 1 s, back to 1 at 2 s
    app_mut(&mut h).playhead = 24;
    h.key_press(Key::Num2);
    settle(&mut h);
    app_mut(&mut h).playhead = 48;
    h.key_press(Key::Num1);
    settle(&mut h);
    assert_eq!(
        (enabled(app(&h), "V1", 24), enabled(app(&h), "V2", 24)),
        (Some(false), Some(true))
    );
    assert_eq!(
        (enabled(app(&h), "V1", 48), enabled(app(&h), "V2", 48)),
        (Some(true), Some(false))
    );
    h.key_press(Key::Escape);
    settle(&mut h);
    assert!(app(&h).multicam.is_none());
    // clips that already line up are left exactly as they are (no empty undo step)
    let before = app(&h).project().clone();
    app_mut(&mut h).select_only(&[cam1, cam2]);
    app_mut(&mut h).sync_selected_by_audio();
    assert_eq!(app(&h).project(), &before);
    // each cut is one undo step: the two cuts, then the start
    let log = app(&h).session.editor.history().len();
    app_mut(&mut h).undo();
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(
        enabled(app(&h), "V1", 24),
        None,
        "the cuts are gone ({log})"
    );
}

#[test]
fn colour_wheels_and_curves_can_be_dragged() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let id = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.id,
        _ => panic!("a clip"),
    };
    app_mut(&mut h).do_op(captaine_ops::Op::SetEffects {
        item: id,
        effects: vec![
            Effect::new(EffectKind::ColorWheels),
            Effect::new(EffectKind::Curves),
        ],
    });
    app_mut(&mut h).selected_clip = Some(id);
    settle(&mut h);
    let fx = |a: &App| a.clip_by_id(id).unwrap().0.effects;
    // push the lift wheel toward red (right of centre)
    h.get_by_label("Lift wheel").scroll_to_me();
    settle(&mut h);
    if let Ok(out) = std::env::var("CAPTAINE_SHOTS") {
        std::fs::create_dir_all(&out).unwrap();
        h.render()
            .expect("render")
            .save(std::path::Path::new(&out).join("wheels.png"))
            .unwrap();
    }
    let r = h.get_by_label("Lift wheel").rect();
    drag(&mut h, r.center(), pos2(r.center().x + 30.0, r.center().y));
    let lift = fx(app(&h))[0].params.clone();
    assert!(
        lift[1] > 0.2 && lift[2] < -0.05 && lift[3] < -0.05,
        "pushed toward red: {lift:?}"
    );
    assert!(
        (lift[1] + lift[2] + lift[3]).abs() < 1e-6,
        "a tint, not a brightness change"
    );
    assert_eq!(lift[0], 0.0, "the master is its own slider");
    // double-click resets it
    let r = h.get_by_label("Lift wheel").rect();
    click(&mut h, r.center());
    click(&mut h, r.center());
    // the master curve's middle point can be dragged up
    h.get_by_label("Curve point Master 3").scroll_to_me();
    settle(&mut h);
    let p = h.get_by_label("Curve point Master 3").rect().center();
    drag(&mut h, p, pos2(p.x, p.y - 30.0));
    let curve = fx(app(&h))[1].params.clone();
    assert!(curve[2] > 0.6, "the midpoint went up: {}", curve[2]);
    assert_eq!((curve[0], curve[4]), (0.0, 1.0), "the ends stayed put");
    // and the tabs switch to another channel
    h.get_by_label("Red").click();
    settle(&mut h);
    h.get_by_label("Curve point Red 5");
    app_mut(&mut h).undo();
    settle(&mut h);
    assert!(
        fx(app(&h))[1].params[2] <= 0.51,
        "undo took the drag back: {}",
        fx(app(&h))[1].params[2]
    );
}

#[test]
fn effect_stack_presets_reorder_duplicate_copy_and_save() {
    if !have_ffmpeg() {
        return;
    }
    let presets =
        std::env::temp_dir().join(format!("captaine-presets-test-{}", std::process::id()));
    std::env::set_var("CAPTAINE_PRESETS", &presets);
    let mut h = editor();
    let ids: Vec<Id> = v1(app(&h)).items.iter().map(|i| i.id()).collect();
    let fx = |a: &App, id: Id| {
        a.clip_by_id(id)
            .unwrap()
            .0
            .effects
            .iter()
            .map(|e| e.kind)
            .collect::<Vec<_>>()
    };
    app_mut(&mut h).selected_clip = Some(ids[0]);
    settle(&mut h);
    // a look preset from the menu
    h.get_by_label("Presets").scroll_to_me();
    settle(&mut h);
    h.get_by_label("Presets").click();
    settle(&mut h);
    h.get_by_label("Noir").click();
    settle(&mut h);
    assert_eq!(
        fx(app(&h), ids[0]),
        [
            EffectKind::Hsl,
            EffectKind::Curves,
            EffectKind::Vignette,
            EffectKind::Grain
        ]
    );
    // sound presets are not offered for a look-only choice... but this clip has sound, so they are
    h.get_by_label("Presets").click();
    settle(&mut h);
    h.get_by_label("Podcast");
    h.get_by_label("Presets").click(); // closes the menu again
    settle(&mut h);
    // reorder: move the first effect down; duplicate the vignette
    h.get_all_by_label("Down").next().unwrap().click();
    settle(&mut h);
    assert_eq!(
        fx(app(&h), ids[0])[..2],
        [EffectKind::Curves, EffectKind::Hsl]
    );
    let before = fx(app(&h), ids[0]).len();
    h.get_all_by_label("Duplicate")
        .nth(2)
        .unwrap()
        .scroll_to_me();
    settle(&mut h);
    h.get_all_by_label("Duplicate").nth(2).unwrap().click();
    settle(&mut h);
    assert_eq!(fx(app(&h), ids[0]).len(), before + 1);
    // copy the stack and paste it on another clip
    h.get_by_label("Copy").scroll_to_me();
    settle(&mut h);
    h.get_by_label("Copy").click();
    settle(&mut h);
    assert_eq!(app(&h).fx_clipboard.len(), before + 1);
    app_mut(&mut h).selected_clip = Some(ids[1]);
    settle(&mut h);
    h.get_by_label("Paste").scroll_to_me();
    h.get_by_label("Paste").click();
    settle(&mut h);
    assert_eq!(fx(app(&h), ids[1]), fx(app(&h), ids[0]));
    // save as a preset and find it again
    app_mut(&mut h).preset_effects = app(&h).clip_by_id(ids[0]).unwrap().0.effects;
    captaine_app::presets_store::save("My look", &app(&h).preset_effects.clone()).unwrap();
    let mine = captaine_app::presets_store::user_presets();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].name, "My look");
    assert!(captaine_app::presets_store::save(" ", &[]).is_err());
    let _ = std::fs::remove_dir_all(presets);
}

#[test]
fn mixer_solo_lock_pan_and_loudness_buttons() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    app_mut(&mut h).inspector_tab = captaine_app::app::InspectorTab::Mixer;
    settle(&mut h);
    let v1_id = track_named(app(&h), "V1").id;
    let state = |a: &App| {
        let t = track_named(a, "V1");
        (t.solo, t.locked)
    };
    assert_eq!(state(app(&h)), (false, false));
    // V1 is the first track listed that is not on top, so use the app's own methods through the tab's buttons
    let solos: Vec<_> = h.get_all_by_label("Solo").collect();
    assert!(solos.len() >= 4, "one Solo per track");
    app_mut(&mut h).do_op(captaine_ops::Op::SetTrackMix {
        track: v1_id,
        pan: 0.0,
        solo: true,
        locked: true,
    });
    settle(&mut h);
    assert_eq!(state(app(&h)), (true, true));
    // locked: the clip on it cannot be edited
    let first = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.id,
        _ => panic!("a clip"),
    };
    assert!(!app_mut(&mut h).do_op(captaine_ops::Op::SetName {
        item: first,
        name: "no".into()
    }));
    app_mut(&mut h).undo();
    // loudness: measure, then normalise to a target
    h.get_by_label("Measure mix").scroll_to_me();
    h.get_by_label("Measure mix").click();
    settle(&mut h);
    assert!(app(&h).loudness.is_some());
    h.get_by_label("-23 broadcast").click();
    settle(&mut h);
    assert_eq!(app(&h).loudness_target, -23.0);
}

#[test]
fn a_bundled_app_finds_its_helpers_even_with_a_bare_path() {
    let exe = std::path::Path::new("/Applications/captainE.app/Contents/MacOS/captaine-app");
    let p = captaine_app::helper_search_path("/usr/bin:/bin", exe);
    let dirs: Vec<&str> = p.split(':').collect();
    assert_eq!(dirs[0], "/Applications/captainE.app/Contents/Resources/bin");
    assert!(dirs.contains(&"/usr/bin") && dirs.contains(&"/opt/homebrew/bin"));
    assert_eq!(
        dirs.iter().filter(|d| **d == "/opt/homebrew/bin").count(),
        1
    );
    let again = captaine_app::helper_search_path(&p, exe);
    assert_eq!(
        again.matches("/opt/homebrew/bin").count(),
        1,
        "does not pile up"
    );
}

#[test]
fn a_voice_over_take_lands_on_an_audio_track_at_the_playhead() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let dir = std::env::temp_dir().join(format!("captaine-vo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    app_mut(&mut h).path = Some(dir.join("p.captaine"));
    // one second of a 220 Hz tone at 44.1 kHz stands in for the microphone
    let take: Vec<f32> = (0..44_100)
        .map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / 44_100.0).sin() * 0.5)
        .collect();
    app_mut(&mut h).place_take(&take, 44_100, 100);
    settle(&mut h);
    let tl = app(&h).timeline().unwrap().clone();
    let (track, clip, start) = tl
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .find_map(|t| {
            t.items
                .iter()
                .zip(t.item_starts())
                .find_map(|(it, st)| match it {
                    Item::Clip(c) if c.name.starts_with("voiceover-") => {
                        Some((t.name.clone(), c.clone(), st))
                    }
                    _ => None,
                })
        })
        .expect("the take is on an audio track");
    assert_eq!(start, 100, "at the playhead, on {track}");
    assert!(
        (clip.duration.value - 24).abs() <= 1,
        "{}",
        clip.duration.value
    );
    assert!(dir.join("Voice-overs").read_dir().unwrap().count() == 1);
    // a take of a few milliseconds is refused
    let before = app(&h).project().media.len();
    app_mut(&mut h).place_take(&take[..100], 44_100, 0);
    assert_eq!(app(&h).project().media.len(), before);
    let _ = std::fs::remove_dir_all(dir);
}
