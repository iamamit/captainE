//! The co-editor (docs/08).
//!
//! The agent never touches the real project. It works on a scratch copy through tools that wrap the same
//! operations a person uses, and what it did comes back as **one suggested edit** (an `Op::Transaction`) that
//! the user can preview, apply or discard. Only project *metadata* and analysis results are sent to the model,
//! never pictures or sound.

mod http;
mod loop_;
mod scenes;
mod silence;
mod speech;
mod tools;

pub use http::{AnthropicTransport, Credentials, Transport};
pub use loop_::{run, AgentConfig, AgentError, AgentRequest, AgentResult, ChatTurn, Event};
pub use scenes::split_at_scenes;
pub use silence::remove_silences;
pub use speech::{clip_transcript, cut_ranges};
pub use tools::{tool_definitions, ToolBox};
