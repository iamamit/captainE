//! Rendering (docs/04): decode -> transform/blend in linear light -> sRGB output.
//!
//! This is the CPU reference renderer. It defines the pixel-exact behaviour that the future GPU
//! compositor must match. The sRGB transfer curve stands in for proper OCIO management (Phase 7).

mod color;
mod composite;
mod decode;
mod eval;
mod frame;
mod renderer;

pub use composite::draw_layer;
pub use decode::{FrameCache, VideoReader};
pub use eval::{layers_at, Layer, TimelineIndex};
pub use frame::{contact_sheet, save_rgb_png, Canvas, Frame8};
pub use renderer::{RenderError, RenderOptions, Renderer};
