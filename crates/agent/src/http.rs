use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

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
                            Some("tool_result") => out.push_str(&format!(
                                "[tool result for id {}] {}\n",
                                b["tool_use_id"].as_str().unwrap_or(""),
                                match &b["content"] {
                                    Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                }
                            )),
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

    /// The JSON object in a model's answer, which may come wrapped in a code fence or chatter.
    pub fn parse_reply(text: &str) -> Result<Value, String> {
        let (a, b) = (text.find('{'), text.rfind('}'));
        let slice = match (a, b) {
            (Some(a), Some(b)) if b > a => &text[a..=b],
            _ => {
                return Err(format!(
                    "Claude did not answer in the expected form: {}",
                    text.chars().take(160).collect::<String>()
                ))
            }
        };
        let v: Value = serde_json::from_str(slice)
            .map_err(|e| format!("Claude's answer was not valid JSON ({e})"))?;
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
        cmd.args([
            "-p",
            "--output-format",
            "json",
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
            .write_all(Self::prompt(body).as_bytes())
            .map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let json: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
            format!(
                "Claude Code gave no answer: {}",
                String::from_utf8_lossy(&out.stderr)
                    .chars()
                    .take(200)
                    .collect::<String>()
            )
        })?;
        if json["is_error"] == true {
            return Err(format!(
                "Claude Code said: {} (is it signed in? run `claude` once in a terminal)",
                json["result"].as_str().unwrap_or("an error")
            ));
        }
        Self::parse_reply(json["result"].as_str().unwrap_or(""))
    }
}
