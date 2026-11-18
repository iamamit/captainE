//! Ask CoCaptain to edit a project, headless, exactly as the panel does (same agent loop, tools and Claude
//! connection), and keep the result on a branch for review — the open window follows the file.
//!
//!     cargo run --release -p captaine-app --example cocaptain_ask -- <project.json> "<request>" [--log DIR]
//!
//! Every request and reply to the model is appended to `<DIR>/cocaptain-<time>.jsonl` (default: next to the
//! project), so a wrong or failed edit can be traced step by step.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use captaine_agent::{
    AgentConfig, AgentRequest, AnthropicTransport, ClaudeCli, Credentials, Event, Transport,
};
use captaine_ops::Author;
use captaine_project::Session;
use serde_json::{json, Value};

/// Passes every exchange through to the real transport and writes it to the log.
struct Logged {
    inner: Box<dyn Transport + Sync>,
    log: Mutex<std::fs::File>,
}

impl Transport for Logged {
    fn send(&self, body: &Value) -> Result<Value, String> {
        let t = std::time::Instant::now();
        let r = self.inner.send(body);
        if let Ok(mut f) = self.log.lock() {
            let line = json!({
                "request_messages": body["messages"],
                "ms": t.elapsed().as_millis() as u64,
                "reply": r.as_ref().map_or_else(|e| json!({"error": e}), |v| v.clone()),
            });
            let _ = writeln!(f, "{line}");
        }
        r
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let (proj, text) = match (args.get(1), args.get(2)) {
        (Some(p), Some(t)) => (PathBuf::from(p), t.clone()),
        _ => return Err("usage: cocaptain_ask <project.json> \"<request>\" [--log DIR]".into()),
    };
    let log_dir = args
        .iter()
        .position(|a| a == "--log")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| proj.parent().unwrap_or(Path::new(".")).to_path_buf());
    std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let log_path = log_dir.join(format!("cocaptain-{stamp}.jsonl"));

    // the same choice the panel makes: an API key if one is saved, else the Claude app's `claude` command
    let inner: Box<dyn Transport + Sync> = match Credentials::load().filter(|k| !k.is_empty()) {
        Some(key) => Box::new(AnthropicTransport::new(key)),
        None => Box::new(
            ClaudeCli::find()
                .ok_or("no API key and no `claude` command: CoCaptain cannot reach Claude")?,
        ),
    };
    let transport = Logged {
        inner,
        log: Mutex::new(std::fs::File::create(&log_path).map_err(|e| e.to_string())?),
    };

    let mut session = Session::load(&proj)?;
    let project = session.editor.project().clone();
    let req = AgentRequest {
        project: &project,
        selection: None,
        playhead_secs: 0.0,
        history: &[],
        text: &text,
        base_ops: &[],
        images: &[],
        files: &[],
    };
    println!("CoCaptain <- {text}");
    let t = std::time::Instant::now();
    let res = captaine_agent::run(
        &transport,
        &AgentConfig::default(),
        &req,
        &mut |e| match e {
            Event::Step(s) => println!("  · {s}"),
            Event::Picture(png) => println!("  · looked at a picture ({} KB)", png.len() / 1024),
        },
    )
    .map_err(|e| format!("CoCaptain failed: {e:?} (log: {})", log_path.display()))?;
    println!("CoCaptain -> {}", res.reply);
    println!(
        "{} tool calls, {} edits, {:.0} s; log: {}",
        res.steps.len(),
        res.new_ops.len(),
        t.elapsed().as_secs_f64(),
        log_path.display()
    );
    if res.new_ops.is_empty() {
        return Ok(());
    }
    // like "Review on a branch" in the panel
    let slug = captaine_project::slugify(&text, 5);
    let branch = session.commit_to_new_branch(
        &format!("agent/{slug}"),
        Author::Agent("copilot".into()),
        &format!("CoCaptain: {text}"),
        res.new_ops,
    )?;
    session.checkout(&branch)?;
    session.save(&proj)?;
    println!("saved on branch '{branch}' (the open window reloads it)");
    Ok(())
}
