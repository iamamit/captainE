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
    ColorWheels,
    Curves,
    Hsl,
    Lut,
    // sound
    Eq,
    Compressor,
    Limiter,
    DeEsser,
    NoiseReduction,
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
    pub const ALL: [EffectKind; 16] = [
        EffectKind::Blur,
        EffectKind::Sharpen,
        EffectKind::Vignette,
        EffectKind::Mirror,
        EffectKind::Crop,
        EffectKind::ChromaKey,
        EffectKind::Pixelate,
        EffectKind::ColorWheels,
        EffectKind::Curves,
        EffectKind::Hsl,
        EffectKind::Lut,
        EffectKind::Eq,
        EffectKind::Compressor,
        EffectKind::Limiter,
        EffectKind::DeEsser,
        EffectKind::NoiseReduction,
    ];

    /// Works on a clip's sound rather than its picture.
    pub fn is_audio(self) -> bool {
        matches!(
            self,
            EffectKind::Eq
                | EffectKind::Compressor
                | EffectKind::Limiter
                | EffectKind::DeEsser
                | EffectKind::NoiseReduction
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            EffectKind::Blur => "Blur",
            EffectKind::Sharpen => "Sharpen",
            EffectKind::Vignette => "Vignette",
            EffectKind::Mirror => "Mirror",
            EffectKind::Crop => "Crop",
            EffectKind::ChromaKey => "Chroma key",
            EffectKind::Pixelate => "Pixelate",
            EffectKind::ColorWheels => "Color wheels",
            EffectKind::Curves => "Curves",
            EffectKind::Hsl => "Hue / Sat / Lum",
            EffectKind::Lut => "LUT",
            EffectKind::Eq => "EQ",
            EffectKind::Compressor => "Compressor",
            EffectKind::Limiter => "Limiter",
            EffectKind::DeEsser => "De-esser",
            EffectKind::NoiseReduction => "Noise reduction",
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
            EffectKind::ColorWheels => "color_wheels",
            EffectKind::Curves => "curves",
            EffectKind::Hsl => "hsl",
            EffectKind::Lut => "lut",
            EffectKind::Eq => "eq",
            EffectKind::Compressor => "compressor",
            EffectKind::Limiter => "limiter",
            EffectKind::DeEsser => "de_esser",
            EffectKind::NoiseReduction => "noise_reduction",
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
            EffectKind::ColorWheels => &WHEELS,
            EffectKind::Curves => &CURVES,
            EffectKind::Hsl => &HSL,
            EffectKind::Lut => &LUT,
            EffectKind::Eq => &EQ,
            EffectKind::Compressor => &COMPRESSOR,
            EffectKind::Limiter => &LIMITER,
            EffectKind::DeEsser => &DEESSER,
            EffectKind::NoiseReduction => &NOISE,
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

// lift (shadows), gamma (midtones), gain (highlights): a master and a tint per channel, -1..1
static WHEELS: [ParamDef; 12] = [
    p("Lift master", -1.0, 1.0, 0.0, 2),
    p("Lift red", -1.0, 1.0, 0.0, 2),
    p("Lift green", -1.0, 1.0, 0.0, 2),
    p("Lift blue", -1.0, 1.0, 0.0, 2),
    p("Gamma master", -1.0, 1.0, 0.0, 2),
    p("Gamma red", -1.0, 1.0, 0.0, 2),
    p("Gamma green", -1.0, 1.0, 0.0, 2),
    p("Gamma blue", -1.0, 1.0, 0.0, 2),
    p("Gain master", -1.0, 1.0, 0.0, 2),
    p("Gain red", -1.0, 1.0, 0.0, 2),
    p("Gain green", -1.0, 1.0, 0.0, 2),
    p("Gain blue", -1.0, 1.0, 0.0, 2),
];
// output level at input 0, 25, 50, 75 and 100 %, for the master curve and then red, green and blue
static CURVES: [ParamDef; 20] = [
    p("Master 0%", 0.0, 1.0, 0.0, 2),
    p("Master 25%", 0.0, 1.0, 0.25, 2),
    p("Master 50%", 0.0, 1.0, 0.5, 2),
    p("Master 75%", 0.0, 1.0, 0.75, 2),
    p("Master 100%", 0.0, 1.0, 1.0, 2),
    p("Red 0%", 0.0, 1.0, 0.0, 2),
    p("Red 25%", 0.0, 1.0, 0.25, 2),
    p("Red 50%", 0.0, 1.0, 0.5, 2),
    p("Red 75%", 0.0, 1.0, 0.75, 2),
    p("Red 100%", 0.0, 1.0, 1.0, 2),
    p("Green 0%", 0.0, 1.0, 0.0, 2),
    p("Green 25%", 0.0, 1.0, 0.25, 2),
    p("Green 50%", 0.0, 1.0, 0.5, 2),
    p("Green 75%", 0.0, 1.0, 0.75, 2),
    p("Green 100%", 0.0, 1.0, 1.0, 2),
    p("Blue 0%", 0.0, 1.0, 0.0, 2),
    p("Blue 25%", 0.0, 1.0, 0.25, 2),
    p("Blue 50%", 0.0, 1.0, 0.5, 2),
    p("Blue 75%", 0.0, 1.0, 0.75, 2),
    p("Blue 100%", 0.0, 1.0, 1.0, 2),
];
// six hue bands (red, yellow, green, cyan, blue, magenta): hue shift in degrees, saturation %, lightness
static HSL: [ParamDef; 18] = [
    p("Red hue", -60.0, 60.0, 0.0, 0),
    p("Red sat", 0.0, 200.0, 100.0, 0),
    p("Red lum", -50.0, 50.0, 0.0, 0),
    p("Yellow hue", -60.0, 60.0, 0.0, 0),
    p("Yellow sat", 0.0, 200.0, 100.0, 0),
    p("Yellow lum", -50.0, 50.0, 0.0, 0),
    p("Green hue", -60.0, 60.0, 0.0, 0),
    p("Green sat", 0.0, 200.0, 100.0, 0),
    p("Green lum", -50.0, 50.0, 0.0, 0),
    p("Cyan hue", -60.0, 60.0, 0.0, 0),
    p("Cyan sat", 0.0, 200.0, 100.0, 0),
    p("Cyan lum", -50.0, 50.0, 0.0, 0),
    p("Blue hue", -60.0, 60.0, 0.0, 0),
    p("Blue sat", 0.0, 200.0, 100.0, 0),
    p("Blue lum", -50.0, 50.0, 0.0, 0),
    p("Magenta hue", -60.0, 60.0, 0.0, 0),
    p("Magenta sat", 0.0, 200.0, 100.0, 0),
    p("Magenta lum", -50.0, 50.0, 0.0, 0),
];
// four bands: low shelf, two peaking bands, high shelf (gain in dB, frequency in Hz)
static EQ: [ParamDef; 10] = [
    p("Low gain", -18.0, 18.0, 0.0, 1),
    p("Low freq", 20.0, 500.0, 100.0, 0),
    p("Mid 1 gain", -18.0, 18.0, 0.0, 1),
    p("Mid 1 freq", 100.0, 8000.0, 500.0, 0),
    p("Mid 1 Q", 0.2, 10.0, 1.0, 2),
    p("Mid 2 gain", -18.0, 18.0, 0.0, 1),
    p("Mid 2 freq", 500.0, 16000.0, 3000.0, 0),
    p("Mid 2 Q", 0.2, 10.0, 1.0, 2),
    p("High gain", -18.0, 18.0, 0.0, 1),
    p("High freq", 2000.0, 16000.0, 8000.0, 0),
];
static COMPRESSOR: [ParamDef; 5] = [
    p("Threshold dB", -60.0, 0.0, -18.0, 1),
    p("Ratio", 1.0, 20.0, 3.0, 1),
    p("Attack ms", 1.0, 200.0, 10.0, 0),
    p("Release ms", 10.0, 1000.0, 120.0, 0),
    p("Makeup dB", 0.0, 24.0, 0.0, 1),
];
static LIMITER: [ParamDef; 2] = [
    p("Ceiling dB", -12.0, 0.0, -1.0, 1),
    p("Release ms", 10.0, 500.0, 80.0, 0),
];
static DEESSER: [ParamDef; 2] = [
    p("Threshold dB", -50.0, 0.0, -24.0, 1),
    p("Reduction %", 0.0, 100.0, 60.0, 0),
];
static NOISE: [ParamDef; 2] = [
    p("Reduction dB", 0.0, 40.0, 12.0, 0),
    p("Sensitivity %", 0.0, 100.0, 50.0, 0),
];
// the .cube file is `Effect::text`
static LUT: [ParamDef; 1] = [p("Strength", 0.0, 100.0, 100.0, 0)];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub params: Vec<f64>,
    /// Free text a kind may need: the path of a LUT's .cube file
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
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
            text: String::new(),
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
