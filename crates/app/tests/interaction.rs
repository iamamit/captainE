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

fn clips(dir: &Path) -> Vec<PathBuf> {
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
    let near_end = app(&h).duration_ticks() - 3;
    app_mut(&mut h).set_playhead(near_end);
    h.key_press(Key::Space);
    settle(&mut h);
    assert!(app(&h).playing);
    for _ in 0..40 {
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
    h.get_by_label("Scopes").click();
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
