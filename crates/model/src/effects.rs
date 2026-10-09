//! Per-clip picture effects. An effect is a kind plus a list of numbers; the table of parameters per
//! kind drives validation, the inspector and the Copilot, so adding an effect is one table entry and one
//! function in the renderer.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EffectKind {
    Blur,
    Sharpen,
    Vignette,
    Mirror,
    Crop,
    ChromaKey,
    Pixelate,
}

pub struct ParamDef {
    pub name: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub decimals: usize,
}

const fn p(name: &'static str, min: f64, max: f64, default: f64, decimals: usize) -> ParamDef {
    ParamDef {
        name,
        min,
        max,
        default,
        decimals,
    }
}

impl EffectKind {
    pub const ALL: [EffectKind; 7] = [
        EffectKind::Blur,
        EffectKind::Sharpen,
        EffectKind::Vignette,
        EffectKind::Mirror,
        EffectKind::Crop,
        EffectKind::ChromaKey,
        EffectKind::Pixelate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EffectKind::Blur => "Blur",
            EffectKind::Sharpen => "Sharpen",
            EffectKind::Vignette => "Vignette",
            EffectKind::Mirror => "Mirror",
            EffectKind::Crop => "Crop",
            EffectKind::ChromaKey => "Chroma key",
            EffectKind::Pixelate => "Pixelate",
        }
    }

    /// Stable lowercase name for tools and files.
    pub fn key(self) -> &'static str {
        match self {
            EffectKind::Blur => "blur",
            EffectKind::Sharpen => "sharpen",
            EffectKind::Vignette => "vignette",
            EffectKind::Mirror => "mirror",
            EffectKind::Crop => "crop",
            EffectKind::ChromaKey => "chroma_key",
            EffectKind::Pixelate => "pixelate",
        }
    }

    pub fn from_key(k: &str) -> Option<EffectKind> {
        EffectKind::ALL.into_iter().find(|e| e.key() == k)
    }

    pub fn params(self) -> &'static [ParamDef] {
        match self {
            EffectKind::Blur => &BLUR,
            EffectKind::Sharpen => &SHARPEN,
            EffectKind::Vignette => &VIGNETTE,
            EffectKind::Mirror => &MIRROR,
            EffectKind::Crop => &CROP,
            EffectKind::ChromaKey => &CHROMA,
            EffectKind::Pixelate => &PIXELATE,
        }
    }
}

// radius in pixels on a 1080-pixel-tall frame
static BLUR: [ParamDef; 1] = [p("Radius", 0.0, 100.0, 12.0, 1)];
static SHARPEN: [ParamDef; 1] = [p("Amount", 0.0, 400.0, 100.0, 0)];
static VIGNETTE: [ParamDef; 2] = [
    p("Amount", 0.0, 100.0, 50.0, 0),
    p("Softness", 0.0, 100.0, 60.0, 0),
];
// 0 = flip left onto right, 1 = flip top onto bottom
static MIRROR: [ParamDef; 1] = [p("Vertical", 0.0, 1.0, 0.0, 0)];
// percent of the frame removed from each side
static CROP: [ParamDef; 4] = [
    p("Left", 0.0, 100.0, 0.0, 1),
    p("Top", 0.0, 100.0, 0.0, 1),
    p("Right", 0.0, 100.0, 0.0, 1),
    p("Bottom", 0.0, 100.0, 0.0, 1),
];
static CHROMA: [ParamDef; 6] = [
    p("Key red", 0.0, 255.0, 0.0, 0),
    p("Key green", 0.0, 255.0, 255.0, 0),
    p("Key blue", 0.0, 255.0, 0.0, 0),
    p("Tolerance", 0.0, 100.0, 35.0, 0),
    p("Softness", 0.0, 100.0, 15.0, 0),
    p("Spill removal", 0.0, 100.0, 50.0, 0),
];
// block size in pixels on a 1080-pixel-tall frame
static PIXELATE: [ParamDef; 1] = [p("Block size", 1.0, 200.0, 24.0, 0)];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub params: Vec<f64>,
}

fn yes() -> bool {
    true
}

impl Effect {
    pub fn new(kind: EffectKind) -> Effect {
        Effect {
            kind,
            enabled: true,
            params: kind.params().iter().map(|d| d.default).collect(),
        }
    }

    pub fn check(&self) -> Result<(), &'static str> {
        let defs = self.kind.params();
        if self.params.len() != defs.len() {
            return Err("wrong number of effect parameters");
        }
        for (v, d) in self.params.iter().zip(defs) {
            if !v.is_finite() || *v < d.min || *v > d.max {
                return Err("effect parameter out of range");
            }
        }
        if self.kind == EffectKind::Crop
            && (self.params[0] + self.params[2] >= 100.0
                || self.params[1] + self.params[3] >= 100.0)
        {
            return Err("crop leaves nothing of the picture");
        }
        Ok(())
    }
}

pub fn check_effects(effects: &[Effect]) -> Result<(), &'static str> {
    if effects.len() > 16 {
        return Err("too many effects on one clip");
    }
    effects.iter().try_for_each(Effect::check)
}
