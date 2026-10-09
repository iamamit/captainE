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
