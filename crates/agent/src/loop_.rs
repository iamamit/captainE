use std::fmt;

use captaine_model::{Id, Project};
use captaine_ops::Op;
use serde_json::{json, Value};

use crate::tools::{check_applies, tool_definitions, ToolBox};
use crate::Transport;

const SYSTEM: &str = "You are Copilot, the co-editor inside captainE, a video editor. You edit the user's timeline by calling tools; \
you never describe an edit without making it.\n\
\n\
How the timeline works:\n\
- Times are in seconds on the timeline. Video tracks stack bottom to top (V2 draws over V1). Clips on video tracks play their own sound; audio tracks hold music and voice-over.\n\
- Items on a track are sequential: trimming an edge or deleting with ripple shifts later items on that track; 'lift' leaves a gap; moving an item overwrites what it lands on.\n\
- Item ids are the short ids from get_timeline. Never invent ids.\n\
\n\
How to work:\n\
1. Call get_timeline first (and list_media if you need media), then make the smallest set of edits that does what was asked.\n\
2. Your edits are applied to a scratch copy and shown to the user as ONE suggestion they can preview, apply or discard, so it is safe to act; do not ask for permission to edit.\n\
3. If the request is ambiguous in a way that changes the result (which clip? how much?), ask one short question instead of guessing. If a clip is selected and the request says 'this' or 'it', it means the selected item.\n\
4. If a tool returns an error, read it, fix your arguments and try again; do not repeat the same failing call.\n\
5. Things you cannot do: speed changes, transitions, keyframes, speech transcripts, beat detection, anything outside the tools. Say so plainly instead of approximating.\n\
6. When finished, reply in one to three plain sentences saying what you changed, with the numbers that matter (seconds removed, new length). No markdown headings or lists.";

pub struct AgentConfig {
    pub model: String,
    pub max_tokens: u32,
    /// Most tool calls in one request before it stops and returns what it has.
    pub max_steps: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            model: std::env::var("CAPTAINE_MODEL").unwrap_or_else(|_| "claude-sonnet-5-5".into()),
            max_tokens: 2048,
            max_steps: 40,
        }
    }
}

/// An earlier exchange, kept as plain text so the model remembers the conversation.
#[derive(Clone, Debug)]
pub struct ChatTurn {
    pub user: String,
    pub assistant: String,
}

pub struct AgentRequest<'a> {
    pub project: &'a Project,
    pub selection: Option<Id>,
    pub playhead_secs: f64,
    pub history: &'a [ChatTurn],
    pub text: &'a str,
    /// Ops of a suggestion the user has not applied yet; new edits build on top of them.
    pub base_ops: &'a [Op],
}

pub enum Event {
    /// A line of progress, e.g. "get_timeline" or "split V1 at 3.2 s"
    Step(String),
}

#[derive(Debug)]
pub struct AgentResult {
    pub reply: String,
    /// Everything the agent changed on top of `base_ops` (empty when it only looked or answered).
    pub new_ops: Vec<Op>,
    pub steps: Vec<String>,
}

#[derive(Debug)]
pub enum AgentError {
    Transport(String),
    BadResponse(String),
    Setup(String),
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentError::Transport(s) | AgentError::BadResponse(s) | AgentError::Setup(s) => {
                write!(f, "{s}")
            }
        }
    }
}
impl std::error::Error for AgentError {}

fn describe_call(name: &str, input: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(o) = input.as_object() {
        for (k, v) in o.iter().take(4) {
            let s = match v {
                Value::String(s) => s.chars().take(24).collect::<String>(),
                other => other.to_string(),
            };
            parts.push(format!("{k}={s}"));
        }
    }
    if parts.is_empty() {
        name.to_string()
    } else {
        format!("{name} {}", parts.join(", "))
    }
}

/// Run one request to completion: talk to the model, run its tool calls, return its reply and what it changed.
pub fn run(
    transport: &dyn Transport,
    cfg: &AgentConfig,
    req: &AgentRequest,
    on_event: &mut dyn FnMut(Event),
) -> Result<AgentResult, AgentError> {
    let mut toolbox = ToolBox::new(req.project, req.base_ops, req.selection, req.playhead_secs)
        .map_err(AgentError::Setup)?;
    let mut messages: Vec<Value> = Vec::new();
    for t in req.history {
        messages.push(json!({"role": "user", "content": t.user}));
        messages.push(json!({"role": "assistant", "content": t.assistant}));
    }
    let preamble = format!(
        "[Current state]\n{}\n\n[Request]\n{}",
        toolbox.state_summary(),
        req.text
    );
    messages.push(json!({"role": "user", "content": preamble}));

    let tools = tool_definitions();
    let mut steps = Vec::new();
    let mut calls = 0usize;
    loop {
        let body = json!({
            "model": cfg.model,
            "max_tokens": cfg.max_tokens,
            "system": SYSTEM,
            "tools": tools,
            "messages": messages,
        });
        let resp = transport.send(&body).map_err(AgentError::Transport)?;
        let content = resp["content"]
            .as_array()
            .ok_or_else(|| AgentError::BadResponse("the reply had no content".into()))?
            .clone();
        let reply_text: String = content
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        let tool_uses: Vec<&Value> = content.iter().filter(|b| b["type"] == "tool_use").collect();
        if tool_uses.is_empty() || resp["stop_reason"] != "tool_use" {
            let mut reply = reply_text;
            if resp["stop_reason"] == "max_tokens" {
                reply.push_str(" (I ran out of room to finish that thought.)");
            }
            return finish(toolbox, req, reply, steps);
        }
        messages.push(json!({"role": "assistant", "content": content}));
        let mut results = Vec::new();
        for tu in tool_uses {
            let (id, name, input) = (
                tu["id"].as_str().unwrap_or(""),
                tu["name"].as_str().unwrap_or(""),
                &tu["input"],
            );
            calls += 1;
            let line = describe_call(name, input);
            on_event(Event::Step(line.clone()));
            steps.push(line);
            let out = if calls > cfg.max_steps {
                Err("step limit reached: stop and summarise what you have done".to_string())
            } else {
                toolbox.call(name, input)
            };
            results.push(match out {
                Ok(s) => json!({"type": "tool_result", "tool_use_id": id, "content": s}),
                Err(e) => json!({"type": "tool_result", "tool_use_id": id, "content": e, "is_error": true}),
            });
        }
        messages.push(json!({"role": "user", "content": results}));
        if calls > cfg.max_steps + 3 {
            let reply = format!(
                "{} (I stopped after {} steps, so this may be incomplete.)",
                reply_text, cfg.max_steps
            );
            return finish(toolbox, req, reply.trim().to_string(), steps);
        }
    }
}

fn finish(
    toolbox: ToolBox,
    req: &AgentRequest,
    reply: String,
    steps: Vec<String>,
) -> Result<AgentResult, AgentError> {
    let new_ops = toolbox.new_ops;
    // never offer something that would not apply to the real project
    let mut all: Vec<Op> = req.base_ops.to_vec();
    all.extend(new_ops.iter().cloned());
    check_applies(req.project, &all)
        .map_err(|e| AgentError::BadResponse(format!("the suggested edit does not apply: {e}")))?;
    Ok(AgentResult {
        reply,
        new_ops,
        steps,
    })
}
