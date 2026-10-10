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
    // never read the developer's own preferences: an empty place for the whole test run
    static PREFS: std::sync::Once = std::sync::Once::new();
    PREFS.call_once(|| {
        // tests must never reach a real Claude
        std::env::set_var("CAPTAINE_NO_CLAUDE_CLI", "1");
        std::env::set_var(
            "CAPTAINE_PREFS",
            std::env::temp_dir().join(format!("captaine-no-prefs-{}.json", std::process::id())),
        )
    });
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
    for _ in 0..1500 {
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
    for _ in 0..2400 {
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
        // the button reads "Add to queue" while a job is running and "Export" otherwise
        match h.query_by_label("Add to queue") {
            Some(button) => button.click(),
            None => h.get_all_by_label("Export").last().unwrap().click(),
        }
        settle(&mut h);
    }
    for _ in 0..2400 {
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
    settle(&mut h);
    h.get_by_label("Measure mix").click();
    settle(&mut h);
    assert!(app(&h).loudness.is_some());
    h.get_by_label("-23 broadcast").click();
    settle(&mut h);
    assert_eq!(app(&h).loudness_target, -23.0);
}

#[cfg(unix)]
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

#[test]
fn the_lut_browser_lists_cube_files_by_name() {
    let dir = std::env::temp_dir().join(format!("captaine-luts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for n in ["warm.cube", "Cold.CUBE", "notes.txt", "amber.cube"] {
        std::fs::write(dir.join(n), "x").unwrap();
    }
    let names: Vec<String> = captaine_app::effect_widgets::lut_files(&dir)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into())
        .collect();
    assert_eq!(names, ["amber.cube", "Cold.CUBE", "warm.cube"]);
    assert!(captaine_app::effect_widgets::lut_files(&dir.join("missing")).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_new_mask_corner_goes_on_the_nearest_edge() {
    use captaine_app::effect_widgets::polygon_insert;
    let mut sq = vec![(20.0, 20.0), (80.0, 20.0), (80.0, 80.0), (20.0, 80.0)];
    polygon_insert(&mut sq, (50.0, 10.0));
    assert_eq!(sq[1], (50.0, 10.0), "between the first two corners");
    polygon_insert(&mut sq, (10.0, 50.0));
    assert_eq!(
        *sq.last().unwrap(),
        (10.0, 50.0),
        "after the last, closing the shape"
    );
    let text = polygon_text(&sq);
    assert_eq!(parse_polygon(&text).unwrap().len(), 6);
    let mut many: Vec<(f64, f64)> = (0..64).map(|i| (i as f64, 0.0)).collect();
    polygon_insert(&mut many, (5.0, 5.0));
    assert_eq!(many.len(), 64, "at most 64 corners");
}

#[test]
fn a_mask_can_be_drawn_with_the_mouse() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let id = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.id,
        _ => panic!("a clip"),
    };
    let mut m = Effect::new(EffectKind::Mask);
    m.params[0] = 2.0;
    m.text = "25,25 75,25 75,75 25,75".into();
    app_mut(&mut h).do_op(captaine_ops::Op::SetEffects {
        item: id,
        effects: vec![m],
    });
    app_mut(&mut h).selected_clip = Some(id);
    settle(&mut h);
    let fx = |a: &App| a.clip_by_id(id).unwrap().0.effects;
    h.get_by_label("Mask corner 1").scroll_to_me();
    settle(&mut h);
    let p = h.get_by_label("Mask corner 1").rect().center();
    drag(&mut h, p, pos2(p.x - 20.0, p.y - 10.0));
    let pts = captaine_model::parse_polygon(&fx(app(&h))[0].text).unwrap();
    assert_eq!(pts.len(), 4);
    assert!(
        pts[0].0 < 20.0 && pts[0].1 < 25.0,
        "corner moved out: {pts:?}"
    );
    assert_eq!(pts[2], (75.0, 75.0), "the others stayed");
    // clicking the canvas adds a corner, undo takes it away
    let r = h.get_by_label("Mask outline: click to add a corner").rect();
    click(&mut h, pos2(r.center().x, r.top() + 4.0));
    let pts = captaine_model::parse_polygon(&fx(app(&h))[0].text).unwrap();
    assert_eq!(pts.len(), 5);
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(
        captaine_model::parse_polygon(&fx(app(&h))[0].text)
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn media_can_be_tagged_binned_and_searched_from_the_pool() {
    use captaine_app::media_org::*;
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let media: Vec<_> = {
        let mut m: Vec<_> = app(&h).project().media.values().cloned().collect();
        m.sort_by(|a, b| a.name.cmp(&b.name));
        m
    };
    app_mut(&mut h).selected_clip = None;
    app_mut(&mut h).selected_media = Some(media[1].id);
    app_mut(&mut h).inspector_tab = InspectorTab::Metadata;
    settle(&mut h);
    // type a tag and a bin name in the metadata tab
    h.get_by_label("Media Tags").click();
    settle(&mut h);
    h.get_by_label("Media Tags")
        .type_text("Drone, sunset, drone");
    h.key_press(Key::Enter);
    settle(&mut h);
    h.get_by_label("Media Bin").click();
    settle(&mut h);
    h.get_by_label("Media Bin").type_text("Day 1");
    h.key_press(Key::Enter);
    settle(&mut h);
    let meta = app(&h).project().meta[&media[1].id].clone();
    assert_eq!(meta.tags, ["Drone", "sunset"], "duplicates dropped");
    assert_eq!(meta.bin, "Day 1");
    // the pool lists the bin, and the search finds the clip by its tag
    let counts = bin_counts(&app(&h).project().meta, &app(&h).project().media);
    assert_eq!(counts, [("Day 1".to_string(), 1)]);
    assert!(matches_search(&media[1], Some(&meta), "sunset drone"));
    assert!(!matches_search(&media[0], None, "sunset"));
    assert!(matches_search(&media[0], None, "a"), "names still match");
    h.get_by_label("Bin Day 1").click();
    settle(&mut h);
    assert_eq!(app(&h).user_bin.as_deref(), Some("Day 1"));
    // undo takes the organising back
    app_mut(&mut h).undo();
    app_mut(&mut h).undo();
    settle(&mut h);
    assert!(app(&h).project().meta.is_empty());
}

#[test]
fn proxies_are_made_in_the_background_and_attached() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let dir = std::env::temp_dir().join(format!("captaine-proxy-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    app_mut(&mut h).path = Some(dir.join("p.captaine"));
    let id = app(&h)
        .project()
        .media
        .values()
        .find(|m| m.has_video)
        .unwrap()
        .id;
    app_mut(&mut h).make_proxies(&[id], 90);
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        h.step();
        if app(&h).project().media[&id].proxy.is_some() {
            break;
        }
    }
    let proxy = app(&h).project().media[&id]
        .proxy
        .clone()
        .expect("a proxy was attached");
    assert!(std::path::Path::new(&proxy).exists());
    assert!(proxy.contains("p.captaine.cache"));
    // asking again does nothing new
    app_mut(&mut h).make_proxies(&[id], 90);
    assert!(app(&h).proxies.busy.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// Trim every clip on V1 so each has 10 frames of spare footage on both sides.
fn give_handles(h: &mut H) {
    let ids: Vec<Id> = v1(app(h)).items.iter().map(|i| i.id()).collect();
    for id in ids {
        for (edge, d) in [(captaine_ops::Edge::In, 10), (captaine_ops::Edge::Out, -10)] {
            let t = captaine_time::RationalTime::new(d, Rational::FPS_24);
            assert!(app_mut(h).do_op(captaine_ops::Op::Trim {
                item: id,
                edge,
                delta: t
            }));
        }
    }
    settle(h);
}

#[test]
fn roll_slip_and_slide_tools_work_with_the_mouse() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    give_handles(&mut h);
    let lay = layout(app(&h));
    assert_eq!(lay, [(0, 76), (76, 76), (152, 76)]);
    let src = |a: &App, i: usize| match &v1(a).items[i] {
        Item::Clip(c) => c.source_start.value,
        _ => panic!("clip"),
    };
    let start = [src(app(&h), 0), src(app(&h), 1), src(app(&h), 2)];
    // roll: the cut between the first two clips moves 8 frames right (13.4 px)
    app_mut(&mut h).tool = Tool::Roll;
    let g = app(&h).timeline_layout.clone().unwrap();
    let cut = pos2(g.x(76.0 / 24.0), g.row_center(v1(app(&h)).id).unwrap());
    drag(&mut h, cut, pos2(cut.x + 13.4, cut.y));
    let lay = layout(app(&h));
    let said: Vec<_> = app(&h).toasts.iter().map(|t| t.text.clone()).collect();
    assert_eq!((lay[0], lay[1]), ((0, 84), (84, 68)), "{lay:?} {said:?}");
    assert_eq!(lay[2], (152, 76), "the total length did not change");
    assert_eq!(
        src(app(&h), 1),
        start[1] + 8,
        "the right clip starts later in its footage"
    );
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(layout(app(&h))[0], (0, 76));
    // slip: drag the middle clip 10 px left = 6 frames later in its footage; position and length stay
    app_mut(&mut h).tool = Tool::Slip;
    let mid = clip_pos(app(&h), 1, 0.5);
    drag(&mut h, mid, pos2(mid.x - 10.0, mid.y));
    assert_eq!(layout(app(&h)), [(0, 76), (76, 76), (152, 76)]);
    assert_eq!(src(app(&h), 1), start[1] + 6, "later footage");
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(src(app(&h), 1), start[1]);
    // slide: the middle clip moves 8 frames right; the first grows, the third shrinks
    app_mut(&mut h).tool = Tool::Slide;
    let mid = clip_pos(app(&h), 1, 0.5);
    drag(&mut h, mid, pos2(mid.x + 13.4, mid.y));
    let lay = layout(app(&h));
    assert_eq!(lay, [(0, 84), (84, 76), (160, 68)], "{lay:?}");
    assert_eq!(src(app(&h), 1), start[1], "its own footage is untouched");
    app_mut(&mut h).undo();
    settle(&mut h);
    assert_eq!(layout(app(&h)), [(0, 76), (76, 76), (152, 76)]);
    // the keys pick the tools
    h.key_press(Key::N);
    settle(&mut h);
    assert_eq!(app(&h).tool, Tool::Roll);
    h.key_press(Key::Y);
    settle(&mut h);
    assert_eq!(app(&h).tool, Tool::Slip);
    h.key_press(Key::U);
    settle(&mut h);
    assert_eq!(app(&h).tool, Tool::Slide);
}

#[test]
fn an_unclean_exit_is_noticed_and_a_panic_leaves_a_crash_log() {
    let d = std::env::temp_dir().join(format!("captaine-crash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    assert!(
        !captaine_app::mark_running(&d),
        "a first run is not a crash"
    );
    assert!(
        captaine_app::mark_running(&d),
        "no clean exit in between: the last run crashed"
    );
    captaine_app::mark_finished(&d);
    assert!(!captaine_app::mark_running(&d), "a clean exit clears it");
    captaine_app::mark_finished(&d);
    // a panic on another thread writes the report
    captaine_app::install_crash_log(d.clone());
    let _ = std::thread::spawn(|| panic!("boom in the test")).join();
    let logs: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("crash-"))
        .collect();
    assert_eq!(logs.len(), 1);
    let text = std::fs::read_to_string(logs[0].path()).unwrap();
    assert!(
        text.contains("boom in the test") && text.contains("crashed"),
        "{text}"
    );
    let _ = std::panic::take_hook();
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn opening_a_damaged_project_falls_back_to_its_backup_and_says_so() {
    let d = std::env::temp_dir().join(format!("captaine-open-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("p.captaine.json");
    let good = captaine_app::blank_session(640, 360, Rational::FPS_24);
    good.save(&p).unwrap();
    let mut changed = captaine_app::blank_session(640, 360, Rational::FPS_24);
    changed
        .apply(captaine_ops::Op::RenameTimeline {
            id: changed
                .editor
                .project()
                .timelines
                .keys()
                .next()
                .copied()
                .unwrap(),
            name: "Edited".into(),
        })
        .unwrap();
    changed.save(&p).unwrap(); // the first version is now the .bak
    std::fs::write(&p, "{ truncated").unwrap();
    let mut h = editor();
    captaine_app::open_project(app_mut(&mut h), p.clone());
    settle(&mut h);
    let said: Vec<_> = app(&h).toasts.iter().map(|t| t.text.clone()).collect();
    assert!(said.iter().any(|t| t.contains("backup")), "{said:?}");
    assert_eq!(app(&h).timeline().unwrap().name, "Timeline 1");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn shortcuts_can_be_changed_in_the_preferences_and_are_remembered() {
    use captaine_app::keymap::*;
    if !have_ffmpeg() {
        return;
    }
    // the text form of a binding survives a round trip
    for a in ACTIONS.iter() {
        assert_eq!(Chord::from_text(&a.2.to_text()), Some(a.2), "{}", a.1);
    }
    assert_eq!(Chord::from_text("Cmd+Shift+B").unwrap().label(), "⌘⇧B");
    assert!(Chord::from_text("Cmd+Nonsense").is_none());
    let saved = std::env::temp_dir().join(format!("captaine-prefs-ui-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&saved);
    let mut h = editor();
    app_mut(&mut h).prefs_path = Some(saved.clone());
    app_mut(&mut h).prefs_open = true;
    settle(&mut h);
    // bind the blade tool to Q
    h.get_by_label("Shortcut Blade tool").scroll_to_me();
    settle(&mut h);
    h.get_by_label("Shortcut Blade tool").click();
    settle(&mut h);
    assert_eq!(app(&h).rebinding, Some(Action::ToolBlade));
    h.key_press(Key::Q);
    settle(&mut h);
    assert_eq!(app(&h).rebinding, None);
    assert_eq!(app(&h).keymap.label(Action::ToolBlade), "Q");
    // the new key works, the old one no longer does
    app_mut(&mut h).prefs_open = false;
    settle(&mut h);
    h.key_press(Key::Q);
    settle(&mut h);
    assert_eq!(app(&h).tool, Tool::Blade);
    h.key_press(Key::V);
    settle(&mut h);
    assert_eq!(app(&h).tool, Tool::Select);
    h.key_press(Key::B);
    settle(&mut h);
    assert_eq!(app(&h).tool, Tool::Select, "B no longer picks the blade");
    // it was written to the file and a fresh app reads it back
    let prefs: Prefs = serde_json::from_str(&std::fs::read_to_string(&saved).unwrap()).unwrap();
    assert_eq!(prefs.keys.get("ToolBlade").map(String::as_str), Some("Q"));
    assert_eq!(
        Keymap::from_overrides(&prefs.keys).label(Action::ToolBlade),
        "Q"
    );
    // a key that another action uses is refused with a message
    let mut km = app(&h).keymap.clone();
    let err = km
        .set(Action::ToolHand, Chord::from_text("Q").unwrap())
        .unwrap_err();
    assert!(err.contains("Blade tool"), "{err}");
    // preferences have sane limits
    let p = Prefs {
        ui_scale: 9.0,
        snapshot_minutes: 1000,
        proxy_height: 5,
        ..Prefs::default()
    }
    .sane();
    assert_eq!(
        (p.ui_scale, p.snapshot_minutes, p.proxy_height),
        (1.6, 120, 120)
    );
    let _ = std::fs::remove_file(saved);
}

fn track_names(a: &App) -> Vec<String> {
    a.timeline()
        .unwrap()
        .tracks
        .iter()
        .map(|t| t.name.clone())
        .collect()
}

fn right_click(h: &mut H, at: Pos2) {
    // close anything still open, then aim
    h.hover_at(pos2(700.0, 300.0));
    settle(h);
    h.hover_at(at);
    settle(h);
    h.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    settle(h);
}

#[test]
fn the_track_header_has_icons_and_a_right_click_menu() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let a1 = app(&h)
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == "A1")
        .unwrap()
        .id;
    let before = track_names(app(&h));
    assert_eq!(before, ["V1", "V2", "A1", "A2"]);
    // the lock icon
    h.get_by_label("Lock A1").click();
    settle(&mut h);
    assert!(
        app(&h)
            .timeline()
            .unwrap()
            .tracks
            .iter()
            .find(|t| t.id == a1)
            .unwrap()
            .locked
    );
    h.get_by_label("Lock A1").click();
    settle(&mut h);
    // mute and solo icons
    h.get_by_label("Mute A1").click();
    settle(&mut h);
    assert!(
        !app(&h)
            .timeline()
            .unwrap()
            .tracks
            .iter()
            .find(|t| t.id == a1)
            .unwrap()
            .enabled
    );
    h.get_by_label("Solo A1").click();
    settle(&mut h);
    assert!(
        app(&h)
            .timeline()
            .unwrap()
            .tracks
            .iter()
            .find(|t| t.id == a1)
            .unwrap()
            .solo
    );
    // right-click the header of A1
    let g = app(&h).timeline_layout.clone().unwrap();
    let at = pos2(40.0, g.row_center(a1).unwrap());
    right_click(&mut h, at);
    h.get_by_label("Add Track").click();
    settle(&mut h);
    assert_eq!(
        app(&h).timeline().unwrap().tracks.len(),
        5,
        "{:?}",
        track_names(app(&h))
    );
    // move A1 down one place (below the new audio track after it)
    let g = app(&h).timeline_layout.clone().unwrap();
    right_click(&mut h, pos2(40.0, g.row_center(a1).unwrap()));
    h.get_by_label("Move Track Down").click();
    settle(&mut h);
    let names = track_names(app(&h));
    let pos = |n: &str| names.iter().position(|x| x == n).unwrap();
    assert!(pos("A1") > pos("A3") && pos("A1") < pos("A2"), "{names:?}");
    // the colour changes the label and survives undo
    let g = app(&h).timeline_layout.clone().unwrap();
    right_click(&mut h, pos2(40.0, g.row_center(a1).unwrap()));
    h.get_by_label_contains("Change Track Color").click();
    settle(&mut h);
    h.get_by_label_contains("Blue").click();
    settle(&mut h);
    assert_eq!(
        app(&h)
            .timeline()
            .unwrap()
            .tracks
            .iter()
            .find(|t| t.id == a1)
            .unwrap()
            .color,
        Some([0x4f, 0x8c, 0xf5])
    );
    // delete the empty tracks: only tracks without clips go, and one track always stays
    let g = app(&h).timeline_layout.clone().unwrap();
    right_click(&mut h, pos2(40.0, g.row_center(a1).unwrap()));
    h.get_by_label("Delete Empty Tracks").click();
    settle(&mut h);
    assert_eq!(track_names(app(&h)), ["V1"], "V1 holds the clips");
    for _ in 0..3 {
        app_mut(&mut h).undo();
    }
    settle(&mut h);
    assert!(app(&h).timeline().unwrap().tracks.len() >= 4);
}

#[test]
fn a_track_can_be_renamed_and_several_added_at_once() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let v1 = v1(app(&h)).id;
    let g = app(&h).timeline_layout.clone().unwrap();
    right_click(&mut h, pos2(40.0, g.row_center(v1).unwrap()));
    h.get_by_label("Rename Track…").click();
    settle(&mut h);
    assert!(app(&h).track_rename.is_some());
    app_mut(&mut h).track_rename = Some((v1, "Main video".into()));
    settle(&mut h);
    h.key_press(Key::Enter);
    settle(&mut h);
    assert!(track_names(app(&h)).contains(&"Main video".to_string()));
    app_mut(&mut h).undo();
    settle(&mut h);
    assert!(track_names(app(&h)).contains(&"V1".to_string()));
    app_mut(&mut h).add_tracks(TrackKind::Audio, 3);
    settle(&mut h);
    let audio = app(&h)
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .count();
    assert_eq!(audio, 5);
    app_mut(&mut h).undo();
    settle(&mut h);
    let audio = app(&h)
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .count();
    assert_eq!(audio, 2, "all three go in one undo step");
}

#[test]
fn auto_select_marker_colours_and_the_captions_track() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    // switching V1 out of auto-select makes a split at the playhead leave it alone
    app_mut(&mut h).set_playhead(40);
    h.get_by_label("Auto-select V1").click();
    settle(&mut h);
    assert!(!app(&h).untargeted.is_empty());
    let before = layout(app(&h));
    app_mut(&mut h).selected_clip = None;
    captaine_app::ui::split_at_playhead(app_mut(&mut h));
    settle(&mut h);
    assert_eq!(layout(app(&h)), before, "V1 was not cut");
    h.get_by_label("Auto-select V1").click();
    settle(&mut h);
    captaine_app::ui::split_at_playhead(app_mut(&mut h));
    settle(&mut h);
    assert_eq!(layout(app(&h)).len(), before.len() + 1, "now it is");
    // a marked range lifted with V1 left out removes nothing
    let v1_id = v1(app(&h)).id;
    app_mut(&mut h).untargeted.insert(v1_id);
    app_mut(&mut h).mark_in = Some(10);
    app_mut(&mut h).mark_out = Some(30);
    app_mut(&mut h).selected_clip = None;
    let n = layout(app(&h)).len();
    captaine_app::ui::lift_selection(app_mut(&mut h));
    assert_eq!(layout(app(&h)).len(), n);
    app_mut(&mut h).untargeted.clear();
    captaine_app::ui::lift_selection(app_mut(&mut h));
    assert_ne!(layout(app(&h)), before);
    // new markers take the chosen colour
    app_mut(&mut h).marker_color = 3;
    let id = app_mut(&mut h).add_marker(Some(5), None).unwrap();
    let m = app(&h).markers().into_iter().find(|m| m.id == id).unwrap();
    assert_eq!(m.color, 3);
    // the Captions track
    let right = app(&h).timeline().unwrap().tracks[0].id;
    let g = app(&h).timeline_layout.clone().unwrap();
    right_click(&mut h, pos2(40.0, g.row_center(right).unwrap()));
    h.get_by_label("Add Subtitle Track").click();
    settle(&mut h);
    let cap = app(&h)
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == "Captions")
        .map(|t| (t.kind, t.color));
    assert_eq!(cap, Some((TrackKind::Video, Some([0xe8, 0xc5, 0x3a]))));
    app_mut(&mut h).add_subtitle_track();
    let count = app(&h)
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .filter(|t| t.name == "Captions")
        .count();
    assert_eq!(count, 1, "only one");
}

#[test]
fn panel_sizes_are_remembered_between_runs() {
    use captaine_app::keymap::*;
    let mut p = Prefs::default();
    assert!(
        !p.note_panel(PanelKind::Pool, 340.5),
        "a pixel of noise is not a change"
    );
    assert!(p.note_panel(PanelKind::Pool, 480.0));
    assert!(
        p.note_panel(PanelKind::Inspector, 9999.0),
        "kept inside the allowed range"
    );
    assert_eq!((p.pool_w, p.inspector_w), (480.0, 600.0));
    assert!(p.note_panel(PanelKind::Timeline, 400.0));
    let text = serde_json::to_string(&p).unwrap();
    let back: Prefs = serde_json::from_str(&text).unwrap();
    assert_eq!((back.pool_w, back.timeline_h), (480.0, 400.0));
    // a preferences file from before these existed still loads
    let old: Prefs =
        serde_json::from_str(r#"{"ui_scale":1.0,"snapshot_minutes":5,"proxy_height":540}"#)
            .unwrap();
    assert_eq!(old.pool_w, 340.0);
}

#[test]
fn a_dragged_panel_keeps_its_width_while_the_pointer_moves_about() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    let width = |h: &H| h.get_by_label("+").rect().right();
    drag(&mut h, pos2(345.0, 300.0), pos2(465.0, 300.0));
    let set = width(&h);
    assert!(set > 400.0, "it was widened: {set}");
    // hovering around repaints the window many times; the width must not creep back
    for i in 0..60 {
        h.hover_at(pos2(200.0 + i as f32 * 12.0, 200.0 + (i % 7) as f32 * 40.0));
        h.step();
    }
    assert!((width(&h) - set).abs() < 1.0, "{set} -> {}", width(&h));
    // the timeline and the inspector hold their sizes too
    let top = |h: &H| h.get_by_label("Select").rect().top();
    let t0 = top(&h);
    drag(&mut h, pos2(700.0, t0 - 5.0), pos2(700.0, t0 - 85.0));
    let t1 = top(&h);
    assert!(t1 < t0 - 60.0, "the timeline grew: {t0} -> {t1}");
    for i in 0..40 {
        h.hover_at(pos2(100.0 + i as f32 * 30.0, 100.0 + (i % 9) as f32 * 80.0));
        h.step();
    }
    assert!((top(&h) - t1).abs() < 1.0, "{t1} -> {}", top(&h));
}

#[test]
fn the_scopes_window_can_be_moved_and_resized() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor();
    app_mut(&mut h).scopes_open = true;
    app_mut(&mut h).scope_pos = Some(pos2(500.0, 100.0));
    settle(&mut h);
    let start = app(&h).scope_pos.unwrap();
    drag(&mut h, pos2(520.0, 220.0), pos2(580.0, 250.0));
    let moved = app(&h).scope_pos.unwrap();
    assert!(
        (moved.x - start.x - 60.0).abs() < 4.0 && (moved.y - start.y - 30.0).abs() < 4.0,
        "{start:?} -> {moved:?}"
    );
    // the corner makes it bigger, up to a limit
    let side = app(&h).scope_side;
    let corner = pos2(
        moved.x + side + 14.0 - 8.0,
        moved.y + side + 14.0 + 34.0 + 22.0 - 8.0,
    );
    drag(&mut h, corner, pos2(corner.x + 60.0, corner.y + 60.0));
    assert!(
        app(&h).scope_side > side + 40.0,
        "{} -> {}",
        side,
        app(&h).scope_side
    );
    for _ in 0..3 {
        let c = {
            let p = app(&h).scope_pos.unwrap();
            let s = app(&h).scope_side;
            pos2(p.x + s + 6.0, p.y + s + 62.0)
        };
        drag(&mut h, c, pos2(c.x + 300.0, c.y + 300.0));
    }
    assert!(app(&h).scope_side <= 560.0);
}

#[test]
fn an_audio_plugin_is_added_from_the_inspector_and_its_controls_are_sliders() {
    use clack_host::prelude::PluginEntry;
    use clack_plugin::entry::SinglePluginEntry;
    use clack_plugin_gain::GainPlugin;
    if !have_ffmpeg() {
        return;
    }
    captaine_plugins::register_entry(
        "builtin:gain",
        PluginEntry::load_from_clack::<SinglePluginEntry<GainPlugin>>(c"").unwrap(),
    );
    let mut h = editor();
    let id = match &v1(app(&h)).items[0] {
        Item::Clip(c) => c.id,
        _ => panic!("a clip"),
    };
    app_mut(&mut h).selected_clip = Some(id);
    settle(&mut h);
    h.get_by_label_contains("Audio plugin").scroll_to_me();
    settle(&mut h);
    h.get_by_label_contains("Audio plugin").click();
    settle(&mut h);
    // the look for plugins runs in the background
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.step();
        if h.query_by_label_contains("Clack Gain Example").is_some() {
            break;
        }
    }
    h.get_by_label_contains("Clack Gain Example").click();
    settle(&mut h);
    let effects = |a: &App| a.clip_by_id(id).unwrap().0.effects;
    let e = effects(app(&h))
        .into_iter()
        .find(|e| e.kind == EffectKind::Plugin)
        .expect("the plugin was added");
    let r = captaine_plugins::PluginRef::parse(&e.text).unwrap();
    assert_eq!(r.id, "org.rust-audio.clack.gain");
    // its own control shows as a slider; dragging it stores a value in the effect
    let control = captaine_plugins::params(&r).unwrap()[0].clone();
    // the clip has a "Volume" of its own too: the plugin's control is the last one listed
    h.get_all_by_label_contains(&control.name)
        .last()
        .unwrap()
        .scroll_to_me();
    settle(&mut h);
    let q = h
        .get_all_by_label_contains(&control.name)
        .last()
        .unwrap()
        .rect();
    drag(
        &mut h,
        pos2(q.right() + 30.0, q.center().y),
        pos2(q.right() + 100.0, q.center().y),
    );
    let e = effects(app(&h))
        .into_iter()
        .find(|e| e.kind == EffectKind::Plugin)
        .unwrap();
    let r2 = captaine_plugins::PluginRef::parse(&e.text).unwrap();
    assert!(
        r2.value(control.id).is_some(),
        "the setting was stored: {}",
        e.text
    );
    // and it undoes
    app_mut(&mut h).undo();
    settle(&mut h);
}
