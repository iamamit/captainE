//! Run a list of CoCaptain tool calls against a project, headless, and keep the result on a branch for review.
//!
//!     cargo run --release -p captaine-app --example copilot_script -- <project.json> <script.json> [branch]
//!
//! script.json is `[{"tool": "set_speed", "input": {...}}, ...]`. A string value "$N" inside an input is replaced
//! by the id returned by call N (0-based) — the first `item_id`/`id` found in that call's result — and "@name" by the
//! id of the media whose file name starts with `name`. The edits are
//! made exactly as CoCaptain makes them (same ToolBox, same ops) and committed as `Script("copilot_script")`.
use captaine_agent::ToolBox;
use captaine_ops::Author;
use captaine_project::Session;
use serde_json::Value;
use std::path::Path;

fn first_id(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    fn walk(v: &Value) -> Option<String> {
        match v {
            Value::Object(m) => {
                for k in ["item_id", "id"] {
                    if let Some(s) = m.get(k).and_then(|x| x.as_str()) {
                        return Some(s.to_string());
                    }
                }
                m.values().find_map(walk)
            }
            Value::Array(a) => a.iter().find_map(walk),
            _ => None,
        }
    }
    walk(&v)
}

fn substitute(v: &Value, ids: &[Option<String>], media: &[(String, String)]) -> Value {
    match v {
        Value::String(s) if s.starts_with('@') => media
            .iter()
            .find(|(name, _)| name.starts_with(&s[1..]))
            .map(|(_, id)| Value::String(id.clone()))
            .unwrap_or_else(|| v.clone()),
        Value::String(s) if s.starts_with('$') => s[1..]
            .parse::<usize>()
            .ok()
            .and_then(|n| ids.get(n).cloned().flatten())
            .map(Value::String)
            .unwrap_or_else(|| v.clone()),
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, ids, media)).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, x)| (k.clone(), substitute(x, ids, media)))
                .collect(),
        ),
        _ => v.clone(),
    }
}

fn main() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().collect();
    let live = args.iter().any(|a| a == "--live");
    args.retain(|a| a != "--live");
    if args.len() < 3 {
        return Err(
            "usage: copilot_script <project.json> <script.json> [branch | .] [--live]".into(),
        );
    }
    let (proj, script) = (Path::new(&args[1]), Path::new(&args[2]));
    let branch = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| "agent/script".into());
    let mut session = Session::load(proj)?;
    let calls: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(script).map_err(|e| format!("cannot read script: {e}"))?,
    )
    .map_err(|e| format!("bad script: {e}"))?;
    let name = script
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("script")
        .to_string();
    // "." keeps working on the current branch; otherwise the edits go on a new branch for review
    if branch != "." {
        session.commit(Some(&format!("Save before: {name}")))?;
        let b = session.unique_branch_name(&branch);
        session.create_branch(&b, true)?;
        println!("working on branch '{b}'");
    }
    let author = Author::Script("copilot_script".into());

    let mut tb = ToolBox::new(session.editor.project(), &[], None, 0.0)?;
    let mut ids: Vec<Option<String>> = Vec::new();
    for (n, c) in calls.iter().enumerate() {
        let tool = c["tool"].as_str().ok_or(format!("call {n}: no tool"))?;
        let media: Vec<(String, String)> = tb
            .project()
            .media
            .values()
            .map(|m| (m.name.clone(), m.id.to_string()))
            .collect();
        let input = substitute(&c["input"], &ids, &media);
        let before = tb.new_ops.len();
        match tb.call(tool, &input) {
            Ok(out) => {
                // CAPTAINE_FULL=1 prints whole results (get_timeline and the like)
                let keep = if std::env::var_os("CAPTAINE_FULL").is_some() {
                    usize::MAX
                } else {
                    140
                };
                let short: String = out.chars().take(keep).collect();
                println!("[{n:02}] {tool}: ok  {short}");
                ids.push(first_id(&out));
            }
            Err(e) => {
                session.save(proj)?;
                return Err(format!("[{n:02}] {tool} failed: {e}\ninput: {input}"));
            }
        }
        for op in &tb.new_ops[before..] {
            session.apply_as(author.clone(), op.clone())?;
        }
        if live && tb.new_ops.len() > before {
            session.save(proj)?;
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
    }
    let msg = format!("{name} ({} tool calls)", calls.len());
    session.commit(Some(&msg))?;
    session.save(proj)?;
    println!(
        "committed {} ops on branch '{}'",
        tb.new_ops.len(),
        session.branch()
    );
    Ok(())
}
