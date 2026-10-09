//! Audio engine pieces (docs/05): decode, mix, measure.
//!
//! Everything here is offline and deterministic; the real-time playback thread (cpal) will call the same
//! [`Mixer`] for each buffer.

mod analyze;
mod decode;
mod dsp;
mod loudness;
mod mixer;
mod waveform;

pub use analyze::{find_silences, silences_in};
pub use decode::{decode_pcm, AudioError};
pub use dsp::apply_audio_effects;
pub use loudness::{measure, Loudness};
pub use mixer::Mixer;
pub use waveform::{media_peaks, peaks, Peak};
