use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};

/// Sends one Messages API request and returns the response JSON. A trait so tests can script the model.
pub trait Transport: Send {
    fn send(&self, body: &Value) -> Result<Value, String>;
}

/// Where the API key comes from. Never logged, never put in a project file.
pub struct Credentials;

impl Credentials {
    pub fn key_file() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".captaine").join("anthropic_key"))
    }

    /// `ANTHROPIC_API_KEY`, else the contents of `~/.captaine/anthropic_key`.
    pub fn load() -> Option<String> {
        let from_env = std::env::var("ANTHROPIC_API_KEY").ok();
        let from_file = Self::key_file().and_then(|p| std::fs::read_to_string(p).ok());
        from_env
            .or(from_file)
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
    }

    /// Save a key typed by the user, readable only by them.
    pub fn save(key: &str) -> Result<(), String> {
        let path = Self::key_file().ok_or("no home directory")?;
        std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| e.to_string())?;
        std::fs::write(&path, key.trim()).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn forget() {
        if let Some(p) = Self::key_file() {
            let _ = std::fs::remove_file(p);
        }
    }
}

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const VERSION: &str = "2023-06-01";

pub struct AnthropicTransport {
    key: String,
    endpoint: String,
}

impl AnthropicTransport {
    pub fn new(key: String) -> AnthropicTransport {
        AnthropicTransport {
            key,
            endpoint: ENDPOINT.to_string(),
        }
    }

    /// Point at another server (tests use a local one).
    pub fn with_endpoint(key: String, endpoint: &str) -> AnthropicTransport {
        AnthropicTransport {
            key,
            endpoint: endpoint.to_string(),
        }
    }
}

impl Transport for AnthropicTransport {
    fn send(&self, body: &Value) -> Result<Value, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut resp = agent
            .post(&self.endpoint)
            .header("x-api-key", &self.key)
            .header("anthropic-version", VERSION)
            .header("content-type", "application/json")
            .send(body.to_string())
            .map_err(|e| format!("could not reach the Anthropic API: {e}"))?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("could not read the response: {e}"))?;
        let json: Value = serde_json::from_str(&text).map_err(|_| {
            format!(
                "unexpected response (HTTP {status}): {}",
                text.chars().take(200).collect::<String>()
            )
        })?;
        if status >= 400 {
            let msg = json["error"]["message"].as_str().unwrap_or("unknown error");
            return Err(match status {
                401 => "the API key was rejected (HTTP 401)".to_string(),
                429 => format!("rate limited by the API: {msg}"),
                _ => format!("API error (HTTP {status}): {msg}"),
            });
        }
        Ok(json)
    }
}

/// The start of the error a request gives when Claude Code is not signed in (or its sign-in has run out), so the
/// editor can offer to sign in instead of only showing the message.
pub const CLAUDE_SIGNED_OUT: &str = "Claude Code is signed out";

/// Claude Code's own sign-in, started from the editor (`claude auth login`). It opens the sign-in page in the
/// browser; when the page ends by showing a code instead of finishing by itself, the code is handed over with
/// [`send_code`](ClaudeLogin::send_code). Dropping it before it finishes stops the sign-in.
pub struct ClaudeLogin {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    output: std::sync::Arc<std::sync::Mutex<String>>,
}

impl ClaudeLogin {
    /// Pass the code the sign-in page showed.
    pub fn send_code(&mut self, code: &str) -> Result<(), String> {
        use std::io::Write;
        let stdin = self.stdin.as_mut().ok_or("the sign-in has already ended")?;
        stdin
            .write_all(format!("{}\n", code.trim()).as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| e.to_string())
    }

    /// `None` while it waits for the browser; then whether it signed in.
    pub fn finished(&mut self) -> Option<bool> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.success()),
            Ok(None) => None,
            Err(_) => Some(false),
        }
    }

    /// What Claude Code has printed so far (the sign-in address, any error).
    pub fn output(&self) -> String {
        self.output.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for ClaudeLogin {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Talks to Claude through the `claude` command-line program the user is already signed in to, so no API key
/// is needed. Each request becomes one `claude -p` call that must answer in the Messages response format.
pub struct ClaudeCli {
    bin: std::path::PathBuf,
}

impl ClaudeCli {
    /// The `claude` program: `CAPTAINE_CLAUDE_BIN`, the PATH, or the usual install places.
    pub fn find() -> Option<ClaudeCli> {
        if std::env::var_os("CAPTAINE_NO_CLAUDE_CLI").is_some() {
            return None;
        }
        if let Some(p) = std::env::var_os("CAPTAINE_CLAUDE_BIN") {
            return Some(ClaudeCli { bin: p.into() });
        }
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        if let Some(h) = std::env::var_os("HOME").map(PathBuf::from) {
            dirs.push(h.join(".claude/local"));
            dirs.push(h.join(".local/bin"));
            if let Ok(rd) = std::fs::read_dir(h.join(".nvm/versions/node")) {
                dirs.extend(rd.flatten().map(|e| e.path().join("bin")));
            }
        }
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        dirs.into_iter()
            .map(|d| d.join("claude"))
            .find(|p| p.is_file())
            .map(|bin| ClaudeCli { bin })
    }

    /// Whether Claude Code is signed in (`claude auth status`); `None` when it cannot tell.
    pub fn signed_in(&self) -> Option<bool> {
        let out = std::process::Command::new(&self.bin)
            .args(["auth", "status", "--json"])
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        let v: Value = serde_json::from_slice(&out.stdout).ok()?;
        v["loggedIn"].as_bool()
    }

    /// Start signing in: Claude Code opens its sign-in page in the browser.
    pub fn start_login(&self) -> Result<ClaudeLogin, String> {
        use std::io::Read;
        use std::process::{Command, Stdio};
        let mut child = Command::new(&self.bin)
            .args(["auth", "login"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start Claude Code ({}): {e}", self.bin.display()))?;
        let output = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        for pipe in [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let out = output.clone();
            std::thread::spawn(move || {
                let mut pipe = pipe;
                let mut buf = [0u8; 1024];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    if let Ok(mut s) = out.lock() {
                        s.push_str(&String::from_utf8_lossy(&buf[..n]));
                    }
                }
            });
        }
        let stdin = child.stdin.take();
        Ok(ClaudeLogin {
            child,
            stdin,
            output,
        })
    }

    /// The conversation as plain text, with the tools and the reply format spelled out.
    pub fn prompt(body: &Value) -> String {
        let mut out = String::new();
        out.push_str("You are the model behind a chat API for a video editor. There are no other tools than the ones listed here.\n\n");
        if let Some(tools) = body["tools"].as_array().filter(|t| !t.is_empty()) {
            out.push_str("TOOLS you may call (JSON schemas):\n");
            for t in tools {
                out.push_str(&format!(
                    "- {}: {} | input schema: {}\n",
                    t["name"].as_str().unwrap_or(""),
                    t["description"].as_str().unwrap_or(""),
                    t["input_schema"]
                ));
            }
            out.push('\n');
        }
        out.push_str("CONVERSATION so far:\n");
        // pictures travel as attachments after this text, numbered in the order they appear here
        let mut images = 0usize;
        for m in body["messages"].as_array().into_iter().flatten() {
            let role = m["role"].as_str().unwrap_or("user");
            match &m["content"] {
                Value::String(s) => out.push_str(&format!("[{role}] {s}\n")),
                Value::Array(blocks) => {
                    for b in blocks {
                        match b["type"].as_str() {
                            Some("text") => out.push_str(&format!(
                                "[{role}] {}\n",
                                b["text"].as_str().unwrap_or("")
                            )),
                            Some("tool_use") => out.push_str(&format!(
                                "[assistant called tool {} (id {}) with {}]\n",
                                b["name"].as_str().unwrap_or(""),
                                b["id"].as_str().unwrap_or(""),
                                b["input"]
                            )),
                            Some("tool_result") => {
                                let text = match &b["content"] {
                                    Value::String(s) => s.clone(),
                                    Value::Array(parts) => parts
                                        .iter()
                                        .map(|p| match p["type"].as_str() {
                                            Some("image") => {
                                                images += 1;
                                                format!("[picture #{images}, attached]")
                                            }
                                            _ => p["text"].as_str().unwrap_or("").to_string(),
                                        })
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                    other => other.to_string(),
                                };
                                out.push_str(&format!(
                                    "[tool result for id {}] {text}\n",
                                    b["tool_use_id"].as_str().unwrap_or(""),
                                ))
                            }
                            Some("image") => {
                                images += 1;
                                out.push_str(&format!("[{role}] [picture #{images}, attached]\n"));
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        out.push_str(
            "\nANSWER with ONE JSON object and nothing else (no code fence, no commentary):\n\
             {\"stop_reason\": \"tool_use\" or \"end_turn\", \"content\": [ blocks ]}\n\
             where a block is {\"type\":\"text\",\"text\":\"...\"} or {\"type\":\"tool_use\",\"id\":\"toolu_<unique>\",\"name\":\"<tool>\",\"input\":{...}}.\n\
             Use stop_reason \"tool_use\" when you call tools (you may call several), \"end_turn\" for your final reply.\n",
        );
        out
    }

    /// The pictures in the conversation (tool results and user turns), in the order [`prompt`](Self::prompt)
    /// numbers them.
    pub fn images(body: &Value) -> Vec<Value> {
        let mut out = Vec::new();
        for m in body["messages"].as_array().into_iter().flatten() {
            for b in m["content"].as_array().into_iter().flatten() {
                match b["type"].as_str() {
                    Some("image") => out.push(b.clone()),
                    Some("tool_result") => out.extend(
                        b["content"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|p| p["type"] == "image")
                            .cloned(),
                    ),
                    _ => {}
                }
            }
        }
        out
    }

    /// What goes to `claude -p --input-format stream-json`: one user message holding the conversation as text
    /// and then each picture, labelled with its number.
    pub fn stream_input(body: &Value) -> String {
        let mut content = vec![json!({"type": "text", "text": Self::prompt(body)})];
        for (i, img) in Self::images(body).into_iter().enumerate() {
            content.push(json!({"type": "text", "text": format!("picture #{}:", i + 1)}));
            content.push(img);
        }
        let line = json!({"type": "user", "message": {"role": "user", "content": content}});
        format!("{line}\n")
    }

    /// The JSON object in a model's answer, which may come wrapped in a code fence or chatter.
    pub fn parse_reply(text: &str) -> Result<Value, String> {
        // a final reply in plain words (it happens after a long run of tool calls) is just that: the reply
        let plain = |text: &str| {
            json!({"role": "assistant", "stop_reason": "end_turn",
                   "content": [{"type": "text", "text": text.trim()}]})
        };
        let (a, b) = (text.find('{'), text.rfind('}'));
        let slice = match (a, b) {
            (Some(a), Some(b)) if b > a => &text[a..=b],
            _ if !text.trim().is_empty() => return Ok(plain(text)),
            _ => return Err("Claude did not answer".into()),
        };
        let v: Value = match serde_json::from_str(slice) {
            Ok(v) => v,
            // prose that merely contains braces
            Err(_) if !text.trim_start().starts_with(['{', '`']) => return Ok(plain(text)),
            Err(e) => return Err(format!("Claude's answer was not valid JSON ({e})")),
        };
        if !v["content"].is_array() {
            return Err("Claude's answer had no content".into());
        }
        let mut v = v;
        if v["stop_reason"].is_null() {
            let calls = v["content"]
                .as_array()
                .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_use"));
            v["stop_reason"] = Value::String(if calls { "tool_use" } else { "end_turn" }.into());
        }
        v["role"] = Value::String("assistant".into());
        Ok(v)
    }
}

impl Transport for ClaudeCli {
    fn send(&self, body: &Value) -> Result<Value, String> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let system = match &body["system"] {
            Value::String(s) => s.clone(),
            Value::Array(a) => a
                .iter()
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        let mut cmd = Command::new(&self.bin);
        // stream-json input carries pictures as image blocks; it needs stream-json (verbose) output
        cmd.args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--tools",
            "",
            "--no-session-persistence",
            "--system-prompt",
        ])
        .arg(if system.is_empty() {
            "You are a helpful assistant inside a video editor."
        } else {
            &system
        })
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        if let Ok(m) = std::env::var("CAPTAINE_CLAUDE_MODEL") {
            cmd.args(["--model", &m]);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("could not start Claude Code ({}): {e}", self.bin.display()))?;
        child
            .stdin
            .take()
            .expect("piped")
            .write_all(Self::stream_input(body).as_bytes())
            .map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        // the last event of the stream is the result
        let result = stdout
            .lines()
            .rev()
            .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
            .find(|e| e["type"] == "result");
        let json: Value = result.ok_or_else(|| {
            format!(
                "Claude Code gave no answer: {}",
                String::from_utf8_lossy(&out.stderr)
                    .chars()
                    .take(200)
                    .collect::<String>()
            )
        })?;
        if json["is_error"] == true {
            let said = json["result"].as_str().unwrap_or("an error");
            let lower = said.to_lowercase();
            if [
                "authenticate",
                "oauth",
                "/login",
                "log in",
                "logged in",
                "invalid api key",
            ]
            .iter()
            .any(|w| lower.contains(w))
            {
                return Err(format!("{CLAUDE_SIGNED_OUT}: {said}"));
            }
            return Err(format!("Claude Code said: {said}"));
        }
        Self::parse_reply(json["result"].as_str().unwrap_or(""))
    }
}
