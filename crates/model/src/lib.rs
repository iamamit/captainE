//! Project model (docs/03).
//!
//! Plain data with stable [`Id`]s. Snapshots are cheap to clone: timelines and
//! tracks sit behind `Arc` and are copy-on-write, so an edit only copies the
//! track it touches and older snapshots stay valid for readers on other threads.
//! The model has no editing logic; all changes go through the `ops` crate.

mod anim;
pub mod anim_presets;
mod effects;
mod grade;
mod id;
pub mod presets;
mod project;

pub use anim::{ease, is_property, value_at, Anim, Interp, Keyframe, PROPERTIES};
pub use effects::{
    check_effects, effects_at, fx_property, param_key, parse_fx_property, parse_polygon,
    polygon_text, Category, Effect, EffectKind, ParamDef,
};
pub use grade::{is_color_effect, Grade, COLOR_PROPERTIES};
pub use id::Id;
pub use project::*;
