//! The branch workflow through the window: switcher, history, compare, merge, and Copilot's review branches.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use captaine_agent::Transport;
use captaine_app::app::*;
use captaine_app::branches_ui;
use captaine_app::copilot;
use captaine_model::*;
use captaine_ops::Op;
use captaine_time::Rational;
use egui_kittest::{kittest::Queryable, Harness};
use serde_json::{json, Value};

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

type H = Harness<'static, Option<App>>;

fn media_files(tag: &str) -> Vec<PathBuf> {
    let dir = std::env::temp_dir().join(format!("captaine-branches-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
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

/// Three 4 s clips on V1, committed on main.
fn editor(tag: &str) -> H {
    let files = media_files(tag);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1480.0, 940.0))
        .with_pixels_per_point(1.0)
        .build_ui_state(
            move |ui, state: &mut Option<App>| {
                let app = state.get_or_insert_with(|| {
                    let mut app = App::new(
                        &ui.ctx().clone(),
                        captaine_app::blank_session(1280, 720, Rational::FPS_24),
                        None,
                    );
                    app.import(files.clone());
                    let mut ids: Vec<(String, Id)> = app
                        .project()
                        .media
                        .values()
                        .map(|m| (m.name.clone(), m.id))
                        .collect();
                    ids.sort();
                    for (_, id) in ids {
                        app.place_media(id, None, None);
                    }
                    app.selected_clip = None;
                    app.session.commit(Some("Rough cut")).unwrap();
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
    for _ in 0..6 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}

fn v1_ids(a: &App) -> Vec<Id> {
    a.timeline()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == "V1")
        .unwrap()
        .items
        .iter()
        .filter(|i| i.is_content())
        .map(|i| i.id())
        .collect()
}

fn dim(a: &mut App, id: Id, o: f64) {
    assert!(a.do_op(Op::SetVideo {
        item: id,
        video: VideoProps {
            opacity: o,
            ..VideoProps::default()
        }
    }));
}

#[test]
fn the_branch_menu_lists_branches_and_switching_keeps_each_branchs_work() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor("menu");
    let ids = v1_ids(app(&h));
    {
        let a = app_mut(&mut h);
        branches_ui::new_branch(a, "tighter");
        a.do_op(Op::RemoveItem { item: ids[0] });
        assert_eq!(a.session.branch(), "tighter");
        assert!(a.session.has_uncommitted());
        a.branch_menu = true;
    }
    settle(&mut h);
    // the popup shows both branches and the uncommitted edit
    h.get_by_label("tighter");
    h.get_by_label("main");
    h.get_by_label_contains("1 uncommitted edit");
    // go back to main through the real popup: click the "main" row
    h.get_by_label("main").click();
    settle(&mut h);
    let a = app(&h);
    assert_eq!(
        a.session.branch(),
        "main",
        "clicking a branch switches to it"
    );
    assert_eq!(v1_ids(a).len(), 3, "main still has all three clips");
    assert!(
        !a.session.has_uncommitted(),
        "tighter's edit was saved on tighter, not carried over"
    );
    let tighter = a
        .session
        .branch_infos()
        .into_iter()
        .find(|b| b.name == "tighter")
        .unwrap();
    assert_eq!(tighter.ahead, 1);
    assert!(!a.branch_menu, "the menu closes after a choice");
    assert!(a.program.is_some());
}

#[test]
fn the_history_tab_lists_commits_and_commits_work_in_progress() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor("history");
    let ids = v1_ids(app(&h));
    {
        let a = app_mut(&mut h);
        a.inspector_tab = InspectorTab::History;
        dim(a, ids[1], 0.5);
    }
    settle(&mut h);
    h.get_by_label("Rough cut");
    h.get_by_label_contains("1 uncommitted edit");
    app_mut(&mut h).commit_draft = "Dim the middle clip".into();
    h.get_by_label("Commit").click();
    settle(&mut h);
    let a = app(&h);
    assert!(!a.session.has_uncommitted());
    assert_eq!(
        a.session.log("main").unwrap()[0].message,
        "Dim the middle clip"
    );
    h.get_by_label("Dim the middle clip");
    h.get_by_label_contains("changes the properties of 1 item");
    h.get_by_label_contains("Everything is committed");
}

fn two_branches(h: &mut H) -> (Vec<Id>, String) {
    let ids = v1_ids(app(h));
    let a = app_mut(h);
    // an "agent" branch that tightens the cut, saved while main stays as it was
    let ops = vec![
        Op::RemoveItem { item: ids[0] },
        Op::SetVideo {
            item: ids[2],
            video: VideoProps {
                opacity: 0.3,
                ..VideoProps::default()
            },
        },
    ];
    let name = a
        .session
        .commit_to_new_branch(
            "agent/tighten",
            captaine_ops::Author::Agent("copilot".into()),
            "Tighten the cut",
            ops,
        )
        .unwrap();
    // and a change on main in the meantime
    dim(a, ids[1], 0.6);
    a.session.commit(Some("main: dim the middle")).unwrap();
    (ids, name)
}

#[test]
fn compare_shows_both_versions_and_what_differs() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor("compare");
    let (ids, name) = two_branches(&mut h);
    let ctx = h.ctx.clone();
    branches_ui::start_compare(app_mut(&mut h), &ctx, "main", &name);
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(40));
        if app(&h).compare.as_ref().is_some_and(|c| c.frames_ready()) {
            break;
        }
    }
    let cs = app(&h)
        .compare
        .as_ref()
        .expect("the compare screen is open");
    assert!(!cs.is_merge());
    assert!(cs.frames_ready(), "both versions drew a frame");
    // main (dim c2) against the agent branch (c1 removed, c3 dimmed)
    let listed: Vec<(String, ChangeKindName)> = cs
        .listed_changes()
        .iter()
        .map(|c| (c.name.clone(), ChangeKindName::of(c)))
        .collect();
    assert_eq!(listed.len(), 3, "{listed:?}");
    assert!(
        cs.summary_text().contains("removes 1 clip"),
        "{}",
        cs.summary_text()
    );
    let _ = ids;
    // the timelines and the change list are on screen
    h.get_by_label_contains("Differences");
    h.get_by_label_contains("Removed clip");
    // difference view
    h.get_by_label("Difference").click();
    settle(&mut h);
    // close with the button
    h.get_by_label("Close").click();
    settle(&mut h);
    assert!(app(&h).compare.is_none());
    assert!(app(&h).program.is_some(), "the normal editor is back");
}

#[derive(Debug, PartialEq)]
struct ChangeKindName(String);
impl ChangeKindName {
    fn of(c: &captaine_project::captaine_vcs::Change) -> ChangeKindName {
        ChangeKindName(format!("{:?}", c.kind))
    }
}

#[test]
fn merging_through_the_review_screen_brings_in_the_chosen_changes_only() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor("merge");
    let (ids, name) = two_branches(&mut h);
    let ctx = h.ctx.clone();
    branches_ui::start_merge(app_mut(&mut h), &ctx, &name);
    settle(&mut h);
    {
        let cs = app(&h).compare.as_ref().expect("the merge screen");
        assert!(cs.is_merge());
        assert_eq!(
            cs.conflict_count(),
            0,
            "different clips changed on each side"
        );
        assert_eq!(cs.listed_changes().len(), 2, "removed c1, dimmed c3");
    }
    h.get_by_label_contains("Bring in 2 of 2 changes");
    // untick the dimming of the last clip: its checkbox is the second one in the list
    let boxes = h
        .get_all_by_role(egui::accesskit::Role::CheckBox)
        .collect::<Vec<_>>();
    assert_eq!(boxes.len(), 2);
    boxes[1].click();
    settle(&mut h);
    h.get_by_label_contains("Bring in 1 of 2 changes");
    h.get_by_label_contains("Merge into main").click();
    settle(&mut h);
    let a = app(&h);
    assert!(a.compare.is_none());
    assert_eq!(v1_ids(a), vec![ids[1], ids[2]], "the removal came in");
    let opacity = |id: Id| {
        a.timeline()
            .unwrap()
            .tracks
            .iter()
            .flat_map(|t| t.items.iter())
            .find_map(|i| i.as_clip().filter(|c| c.id == id).map(|c| c.video.opacity))
            .unwrap()
    };
    assert_eq!(opacity(ids[1]), 0.6, "main's own dimming is kept");
    assert_eq!(opacity(ids[2]), 1.0, "the dimming we unticked was left out");
    assert!(a.session.log("main").unwrap()[0].is_merge);
    assert!(!a.session.has_uncommitted());
}

#[test]
fn conflicts_are_flagged_and_resolved_in_the_review_screen() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor("conflict");
    let ids = v1_ids(app(&h));
    {
        let a = app_mut(&mut h);
        branches_ui::new_branch(a, "alt");
        dim(a, ids[1], 0.9);
        a.session.commit(Some("alt: 0.9")).unwrap();
        a.session.checkout("main").unwrap();
        a.session_changed();
        dim(a, ids[1], 0.1);
        a.session.commit(Some("main: 0.1")).unwrap();
    }
    let ctx = h.ctx.clone();
    branches_ui::start_merge(app_mut(&mut h), &ctx, "alt");
    settle(&mut h);
    assert_eq!(app(&h).compare.as_ref().unwrap().conflict_count(), 1);
    h.get_by_label_contains("both sides changed video.opacity");
    h.get_by_label_contains("1 conflict");
    h.get_by_label("Take theirs").click();
    settle(&mut h);
    h.get_by_label_contains("1 will take theirs");
    h.get_by_label_contains("Merge into main").click();
    settle(&mut h);
    let a = app(&h);
    let op = a
        .timeline()
        .unwrap()
        .tracks
        .iter()
        .flat_map(|t| t.items.iter())
        .find_map(|i| {
            i.as_clip()
                .filter(|c| c.id == ids[1])
                .map(|c| c.video.opacity)
        })
        .unwrap();
    assert_eq!(op, 0.9, "theirs won the conflict");
}

struct Script(Mutex<VecDeque<Result<Value, String>>>);
impl Transport for Script {
    fn send(&self, _: &Value) -> Result<Value, String> {
        self.0
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("script exhausted".into()))
    }
}

fn tool(id: &str, name: &str, input: Value) -> Result<Value, String> {
    Ok(
        json!({"stop_reason": "tool_use", "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}),
    )
}

#[test]
fn copilot_can_put_its_edit_on_a_branch_that_you_then_review_and_merge() {
    if !have_ffmpeg() {
        return;
    }
    let mut h = editor("agent");
    let ids = v1_ids(app(&h));
    let responses = VecDeque::from([
        tool(
            "a",
            "delete_item",
            json!({"item_id": ids[0].to_string(), "ripple": true}),
        ),
        tool(
            "b",
            "set_clip",
            json!({"item_id": ids[1].to_string(), "opacity": 0.5}),
        ),
        tool(
            "c",
            "set_clip",
            json!({"item_id": ids[2].to_string(), "opacity": 0.5}),
        ),
        Ok(
            json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": "Removed the first clip and dimmed the others."}]}),
        ),
    ]);
    let queue = Arc::new(Mutex::new(responses));
    {
        let a = app_mut(&mut h);
        a.copilot.transport = Some(Arc::new(move || -> Box<dyn Transport> {
            Box::new(Script(Mutex::new(std::mem::take(
                &mut *queue.lock().unwrap(),
            ))))
        }));
        a.show_copilot = true;
        copilot::ask(a, "Tighten the cut and dim the rest".into());
    }
    for _ in 0..80 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(25));
        if app(&h).copilot.suggestion.is_some() {
            break;
        }
    }
    assert!(app(&h).copilot.suggestion.is_some(), "a suggestion arrived");
    settle(&mut h);
    if let Ok(dir) = std::env::var("CAPTAINE_SHOTS") {
        h.render()
            .unwrap()
            .save(std::path::Path::new(&dir).join("agent_card.png"))
            .unwrap();
    }
    // three changes: the card offers the branch as the main choice
    h.get_by_label("Review on a branch").click();
    settle(&mut h);
    let a = app(&h);
    let branch = a
        .session
        .branch_infos()
        .into_iter()
        .find(|b| b.name.starts_with("agent/"))
        .expect("an agent branch");
    assert_eq!(branch.name, "agent/tighten-the-cut-and");
    assert_eq!(a.session.branch(), "main");
    assert_eq!(v1_ids(a).len(), 3, "main is untouched");
    assert_eq!(
        a.session.log(&branch.name).unwrap()[0].author,
        captaine_ops::Author::Agent("copilot".into())
    );
    // open the review from Copilot's button and merge it
    h.get_by_label_contains("Review agent/tighten").click();
    settle(&mut h);
    assert!(app(&h).compare.as_ref().is_some_and(|c| c.is_merge()));
    h.get_by_label_contains("Merge into main").click();
    settle(&mut h);
    let a = app(&h);
    assert_eq!(v1_ids(a), vec![ids[1], ids[2]]);
    assert_eq!(
        a.session
            .log("main")
            .unwrap()
            .iter()
            .filter(|c| c.is_merge)
            .count()
            + usize::from(
                a.session.log("main").unwrap()[0]
                    .message
                    .contains("Tighten")
            ),
        1
    );
}

#[test]
fn screenshots_of_the_branch_screens() {
    let Ok(dir) = std::env::var("CAPTAINE_SHOTS") else {
        return;
    };
    if !have_ffmpeg() {
        return;
    }
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = editor("shots");
    let (ids, name) = two_branches(&mut h);
    // a conflicting branch too, so the merge screen has something to decide
    {
        let a = app_mut(&mut h);
        a.session.create_branch("alt-colour", false).unwrap();
        let cur = a.session.branch().to_string();
        a.session.checkout("alt-colour").unwrap();
        a.session_changed();
        dim(a, ids[1], 0.9);
        a.session.commit(Some("alt: brighter middle")).unwrap();
        a.session.checkout(&cur).unwrap();
        a.session_changed();
        // main moved on in the same clip, so merging alt-colour has to be decided
        dim(a, ids[1], 0.2);
        a.session.commit(Some("main: moodier middle")).unwrap();
    }
    settle(&mut h);
    // 1. the branch menu
    app_mut(&mut h).branch_menu = true;
    settle(&mut h);
    h.render()
        .unwrap()
        .save(dir.join("branch_menu.png"))
        .unwrap();
    app_mut(&mut h).branch_menu = false;
    // 2. the history tab
    {
        let a = app_mut(&mut h);
        a.inspector_tab = InspectorTab::History;
        dim(a, ids[0], 0.8); // something uncommitted
    }
    settle(&mut h);
    h.render().unwrap().save(dir.join("history.png")).unwrap();
    app_mut(&mut h)
        .session
        .commit(Some("Dim the first clip"))
        .unwrap();
    // 3. compare main with the agent's branch
    let ctx = h.ctx.clone();
    branches_ui::start_compare(app_mut(&mut h), &ctx, "main", &name);
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(40));
        if app(&h).compare.as_ref().is_some_and(|c| c.frames_ready()) {
            break;
        }
    }
    h.render().unwrap().save(dir.join("compare.png")).unwrap();
    // 4. merge with a conflict
    app_mut(&mut h).compare = None;
    branches_ui::start_merge(app_mut(&mut h), &ctx, "alt-colour");
    for _ in 0..30 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    h.render().unwrap().save(dir.join("merge.png")).unwrap();
}
