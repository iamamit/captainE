//! A shot's *grade*: its colour settings plus its colour effects (wheels, curves, HSL, LUT). The Color page
//! copies, pastes and resets grades; CoCaptain does the same through its tools.

use crate::{Clip, ColorProps, Effect, EffectKind, VideoProps};

#[derive(Clone, Debug, PartialEq)]
pub struct Grade {
    pub color: ColorProps,
    /// Colour wheels, curves, HSL and LUT effects, in their order
    pub effects: Vec<Effect>,
}

impl Grade {
    /// No grade at all: neutral colour, no colour effects.
    pub fn neutral() -> Grade {
        Grade {
            color: ColorProps::default(),
            effects: Vec::new(),
        }
    }

    pub fn of(c: &Clip) -> Grade {
        Grade {
            color: c.video.color,
            effects: c
                .effects
                .iter()
                .filter(|e| is_color_effect(e.kind))
                .cloned()
                .collect(),
        }
    }

    /// Whether the shot looks different from its untouched footage.
    pub fn is_neutral(&self) -> bool {
        self.color.is_neutral() && self.effects.iter().all(|e| !e.enabled)
    }

    /// `clip`'s video settings and effects with this grade instead of its own; its other effects (blur, a mask)
    /// stay, after the colour ones.
    pub fn applied_to(&self, clip: &Clip) -> (VideoProps, Vec<Effect>) {
        let mut video = clip.video;
        video.color = self.color;
        let mut effects = self.effects.clone();
        effects.extend(
            clip.effects
                .iter()
                .filter(|e| !is_color_effect(e.kind))
                .cloned(),
        );
        (video, effects)
    }
}

/// The animatable colour settings. A grade sets them as fixed values, so applying one (reset, copy, paste)
/// also removes their keyframes; otherwise the keyframes would keep overriding the new grade.
pub const COLOR_PROPERTIES: &[&str] = &[
    "exposure",
    "contrast",
    "saturation",
    "temperature",
    "tint",
    "highlights",
    "shadows",
    "vibrance",
];

/// Effects that belong to a grade (the rest, like blur or a mask, belong to the shot).
pub fn is_color_effect(k: EffectKind) -> bool {
    matches!(
        k,
        EffectKind::ColorWheels | EffectKind::Curves | EffectKind::Hsl | EffectKind::Lut
    )
}
