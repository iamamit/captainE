//! Rendering (docs/04): decode -> transform/blend in linear light -> sRGB output.
//!
//! This is the CPU reference renderer. It defines the pixel-exact behaviour that the future GPU
//! compositor must match. The sRGB transfer curve stands in for proper OCIO management (Phase 7).

mod analyze;
mod color;
mod composite;
mod decode;
mod effects;
mod eval;
mod frame;
mod preview;
mod renderer;
mod text;
mod title;

pub use analyze::{auto_correction, match_correction, ColorStats};
pub use composite::{draw_layer, draw_layer_fx, LayerFx};
pub use decode::{FrameCache, VideoReader};
pub use effects::{apply_effects, apply_effects_at, apply_effects_with};
pub use eval::{layers_at, ClipLayer, Layer, TimelineIndex, TitleLayer, TransitionState};
pub use frame::{contact_sheet, save_rgb_png, Canvas, Frame8};
pub use preview::preview_project;
pub use renderer::{RenderError, RenderOptions, Renderer};
pub use title::render_title;
