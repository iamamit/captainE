//! The co-editor (docs/08).
//!
//! The agent never touches the real project. It works on a scratch copy through tools that wrap the same
//! operations a person uses, and what it did comes back as **one suggested edit** (an `Op::Transaction`) that
//! the user can preview, apply or discard. Project *metadata* and analysis results are sent to the model; small
//! frames of the video only when the user has let CoCaptain see it (`AgentConfig::allow_frames`). Never sound.
#![recursion_limit = "1024"]

mod color;
mod follow;
mod http;
mod look;
mod loop_;
mod loudness;
mod matte;
mod scenes;
mod silence;
mod speech;
mod stabilize;
mod tools;

pub use color::{auto_color, clip_stats, match_color};
pub use follow::{reframe, track_attach};
pub use http::{
    AnthropicTransport, ClaudeCli, ClaudeLogin, Credentials, Transport, CLAUDE_SIGNED_OUT,
};
pub use loop_::{
    prune_pictures, run, AgentConfig, AgentError, AgentRequest, AgentResult, ChatTurn, Event,
};
pub use loudness::{
    clip_loudness, gain_for, normalize_clip, normalize_sequence, sequence_loudness,
};
pub use matte::remove_background;
pub use scenes::split_at_scenes;
pub use silence::remove_silences;
pub use speech::{clip_transcript, cut_ranges};
pub use stabilize::stabilize;
pub use tools::{tool_definitions, AppAction, ToolBox, IMAGE_BUDGET};
