//! Per-clip picture effects. An effect is a kind plus a list of numbers; the table of parameters per
//! kind drives validation, the inspector and the CoCaptain, so adding an effect is one table entry and one
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
    Mask,
    Matte,
    Glow,
    Grain,
    Aberration,
    Duotone,
    Invert,
    Posterize,
    /// Black bars that turn the picture into a wider (letterbox) or narrower (pillarbox) shape
    Letterbox,
    /// Streaks the picture along one direction, like fast movement
    DirectionalBlur,
    /// Blurs in circles around a centre, like a spinning camera
    RadialBlur,
    /// Streaks outward from a centre, like a fast zoom
    ZoomBlur,
    /// A sharp band with the rest blurred, so the scene looks like a miniature
    TiltShift,
    /// Smooths out video noise while keeping edges
    Denoise,
    /// Simple brightness and contrast
    BrightnessContrast,
    /// Sets the black point, white point and mid-tone gamma
    Levels,
    /// Turns the picture grey, with how much each colour brightens it
    BlackWhite,
    /// Keeps one colour and turns the rest grey
    ColorIsolate,
    /// Builds each output channel from a mix of the input channels
    ChannelMixer,
    /// Pure black and white: brighter than the level is white
    Threshold,
    /// Barrel (positive) or pincushion (negative) bending, like a wide or long lens
    LensDistortion,
    /// Swells (positive) or pinches (negative) a round area
    Bulge,
    /// Twists the picture around a centre
    Twirl,
    /// Moving waves through the picture
    Wave,
    /// Wobbles the picture with flowing noise, like heat haze or water
    Turbulence,
    /// Moves the four corners of the picture (in % of the frame), to fit it onto a screen or sign
    CornerPin,
    /// Slides the picture and wraps it around the edges
    Offset,
    /// Repeats the picture in a grid
    Tile,
    /// A smooth gradient between four corner colours, mixed over the picture
    Gradient,
    /// Draws grid lines over the picture
    Grid,
    /// A bright light source with glare, a streak and reflections across the frame
    LensFlare,
    /// Bright areas cast rays away from a centre
    LightRays,
    /// Warm moving light washing over the picture, like film fogging
    LightLeak,
    /// Makes dark (or with invert, bright) parts transparent
    LumaKey,
    /// Scratches, dust, flicker and a sepia tone
    FilmDamage,
    /// Worn videotape: colour bleed, noise and tracking lines
    Analog,
    /// A soft shadow behind the layer (for titles, shapes and scaled-down clips)
    DropShadow,
    /// Turns the picture in 3D: swivel (left/right), tilt (up/down) and distance
    Perspective,
    /// Draws the outlines of shapes
    FindEdges,
    /// Raised, stamped look lit from one direction
    Emboss,
    /// Printed-dot look
    Halftone,
    /// Dark horizontal lines, like an old screen
    Scanlines,
    /// Flat colours with dark outlines
    Cartoon,
    /// Looks drawn in pencil
    Sketch,
    /// Painterly brush look
    Paint,
    /// Digital break-up: shifted blocks and split colours that jump around
    Glitch,
    /// Flashes to white at a steady rate
    Strobe,
    /// Holds each picture longer, for a choppy stop-motion look
    LowFrameRate,
    /// Hand-held or impact shake (zoomed in a little so no edge shows)
    CameraShake,
    // sound
    Eq,
    Compressor,
    Limiter,
    DeEsser,
    NoiseReduction,
    Ducking,
    /// A CLAP plugin; which one and its settings are in `Effect::text`
    Plugin,
}

/// Where an effect sits in the effects library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Blur,
    Color,
    Distort,
    Generate,
    Keying,
    Light,
    Noise,
    Perspective,
    Stylize,
    Time,
    Transform,
    Sound,
}

impl Category {
    pub const ALL: [Category; 12] = [
        Category::Blur,
        Category::Color,
        Category::Distort,
        Category::Generate,
        Category::Keying,
        Category::Light,
        Category::Noise,
        Category::Perspective,
        Category::Stylize,
        Category::Time,
        Category::Transform,
        Category::Sound,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Blur => "Blur & sharpen",
            Category::Color => "Color",
            Category::Distort => "Distort",
            Category::Generate => "Generate",
            Category::Keying => "Keying & masks",
            Category::Light => "Light & glow",
            Category::Noise => "Noise, grain & texture",
            Category::Perspective => "Perspective",
            Category::Stylize => "Stylize",
            Category::Time => "Time",
            Category::Transform => "Transform",
            Category::Sound => "Sound",
        }
    }
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
    pub const ALL: [EffectKind; 67] = [
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
        EffectKind::Mask,
        EffectKind::Matte,
        EffectKind::Glow,
        EffectKind::Grain,
        EffectKind::Aberration,
        EffectKind::Duotone,
        EffectKind::Invert,
        EffectKind::Posterize,
        EffectKind::Letterbox,
        EffectKind::DirectionalBlur,
        EffectKind::RadialBlur,
        EffectKind::ZoomBlur,
        EffectKind::TiltShift,
        EffectKind::Denoise,
        EffectKind::BrightnessContrast,
        EffectKind::Levels,
        EffectKind::BlackWhite,
        EffectKind::ColorIsolate,
        EffectKind::ChannelMixer,
        EffectKind::Threshold,
        EffectKind::LensDistortion,
        EffectKind::Bulge,
        EffectKind::Twirl,
        EffectKind::Wave,
        EffectKind::Turbulence,
        EffectKind::CornerPin,
        EffectKind::Offset,
        EffectKind::Tile,
        EffectKind::Gradient,
        EffectKind::Grid,
        EffectKind::LensFlare,
        EffectKind::LightRays,
        EffectKind::LightLeak,
        EffectKind::LumaKey,
        EffectKind::FilmDamage,
        EffectKind::Analog,
        EffectKind::DropShadow,
        EffectKind::Perspective,
        EffectKind::FindEdges,
        EffectKind::Emboss,
        EffectKind::Halftone,
        EffectKind::Scanlines,
        EffectKind::Cartoon,
        EffectKind::Sketch,
        EffectKind::Paint,
        EffectKind::Glitch,
        EffectKind::Strobe,
        EffectKind::LowFrameRate,
        EffectKind::CameraShake,
        EffectKind::Eq,
        EffectKind::Compressor,
        EffectKind::Limiter,
        EffectKind::DeEsser,
        EffectKind::NoiseReduction,
        EffectKind::Ducking,
        EffectKind::Plugin,
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
                | EffectKind::Ducking
                | EffectKind::Plugin
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
            EffectKind::Mask => "Mask",
            EffectKind::Matte => "Subject matte",
            EffectKind::Glow => "Glow",
            EffectKind::Grain => "Film grain",
            EffectKind::Aberration => "Chromatic aberration",
            EffectKind::Duotone => "Duotone",
            EffectKind::Invert => "Invert",
            EffectKind::Posterize => "Posterize",
            EffectKind::Letterbox => "Letterbox",
            EffectKind::DirectionalBlur => "Directional blur",
            EffectKind::RadialBlur => "Spin blur",
            EffectKind::ZoomBlur => "Zoom blur",
            EffectKind::TiltShift => "Tilt-shift",
            EffectKind::Denoise => "Denoise",
            EffectKind::BrightnessContrast => "Brightness & contrast",
            EffectKind::Levels => "Levels",
            EffectKind::BlackWhite => "Black & white",
            EffectKind::ColorIsolate => "Color isolate",
            EffectKind::ChannelMixer => "Channel mixer",
            EffectKind::Threshold => "Threshold",
            EffectKind::LensDistortion => "Lens distortion",
            EffectKind::Bulge => "Bulge",
            EffectKind::Twirl => "Twirl",
            EffectKind::Wave => "Wave",
            EffectKind::Turbulence => "Turbulence",
            EffectKind::CornerPin => "Corner pin",
            EffectKind::Offset => "Offset",
            EffectKind::Tile => "Tile",
            EffectKind::Gradient => "Four-colour gradient",
            EffectKind::Grid => "Grid",
            EffectKind::LensFlare => "Lens flare",
            EffectKind::LightRays => "Light rays",
            EffectKind::LightLeak => "Light leak",
            EffectKind::LumaKey => "Luma key",
            EffectKind::FilmDamage => "Old film",
            EffectKind::Analog => "Analog tape",
            EffectKind::DropShadow => "Drop shadow",
            EffectKind::Perspective => "3D tilt",
            EffectKind::FindEdges => "Find edges",
            EffectKind::Emboss => "Emboss",
            EffectKind::Halftone => "Halftone",
            EffectKind::Scanlines => "Scanlines",
            EffectKind::Cartoon => "Cartoon",
            EffectKind::Sketch => "Pencil sketch",
            EffectKind::Paint => "Oil paint",
            EffectKind::Glitch => "Glitch",
            EffectKind::Strobe => "Strobe",
            EffectKind::LowFrameRate => "Low frame rate",
            EffectKind::CameraShake => "Camera shake",
            EffectKind::Eq => "EQ",
            EffectKind::Compressor => "Compressor",
            EffectKind::Limiter => "Limiter",
            EffectKind::DeEsser => "De-esser",
            EffectKind::NoiseReduction => "Noise reduction",
            EffectKind::Ducking => "Auto duck",
            EffectKind::Plugin => "Audio plugin",
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
            EffectKind::Mask => "mask",
            EffectKind::Matte => "matte",
            EffectKind::Glow => "glow",
            EffectKind::Grain => "grain",
            EffectKind::Aberration => "aberration",
            EffectKind::Duotone => "duotone",
            EffectKind::Invert => "invert",
            EffectKind::Posterize => "posterize",
            EffectKind::Letterbox => "letterbox",
            EffectKind::DirectionalBlur => "directional_blur",
            EffectKind::RadialBlur => "radial_blur",
            EffectKind::ZoomBlur => "zoom_blur",
            EffectKind::TiltShift => "tilt_shift",
            EffectKind::Denoise => "denoise",
            EffectKind::BrightnessContrast => "brightness_contrast",
            EffectKind::Levels => "levels",
            EffectKind::BlackWhite => "black_white",
            EffectKind::ColorIsolate => "color_isolate",
            EffectKind::ChannelMixer => "channel_mixer",
            EffectKind::Threshold => "threshold",
            EffectKind::LensDistortion => "lens_distortion",
            EffectKind::Bulge => "bulge",
            EffectKind::Twirl => "twirl",
            EffectKind::Wave => "wave",
            EffectKind::Turbulence => "turbulence",
            EffectKind::CornerPin => "corner_pin",
            EffectKind::Offset => "offset",
            EffectKind::Tile => "tile",
            EffectKind::Gradient => "gradient",
            EffectKind::Grid => "grid",
            EffectKind::LensFlare => "lens_flare",
            EffectKind::LightRays => "light_rays",
            EffectKind::LightLeak => "light_leak",
            EffectKind::LumaKey => "luma_key",
            EffectKind::FilmDamage => "film_damage",
            EffectKind::Analog => "analog",
            EffectKind::DropShadow => "drop_shadow",
            EffectKind::Perspective => "perspective",
            EffectKind::FindEdges => "find_edges",
            EffectKind::Emboss => "emboss",
            EffectKind::Halftone => "halftone",
            EffectKind::Scanlines => "scanlines",
            EffectKind::Cartoon => "cartoon",
            EffectKind::Sketch => "sketch",
            EffectKind::Paint => "paint",
            EffectKind::Glitch => "glitch",
            EffectKind::Strobe => "strobe",
            EffectKind::LowFrameRate => "low_frame_rate",
            EffectKind::CameraShake => "camera_shake",
            EffectKind::Eq => "eq",
            EffectKind::Compressor => "compressor",
            EffectKind::Limiter => "limiter",
            EffectKind::DeEsser => "de_esser",
            EffectKind::NoiseReduction => "noise_reduction",
            EffectKind::Ducking => "ducking",
            EffectKind::Plugin => "plugin",
        }
    }

    /// The library category.
    pub fn category(self) -> Category {
        match self {
            EffectKind::Blur => Category::Blur,
            EffectKind::Sharpen => Category::Blur,
            EffectKind::Vignette => Category::Stylize,
            EffectKind::Mirror => Category::Distort,
            EffectKind::Crop => Category::Transform,
            EffectKind::ChromaKey => Category::Keying,
            EffectKind::Pixelate => Category::Stylize,
            EffectKind::ColorWheels => Category::Color,
            EffectKind::Curves => Category::Color,
            EffectKind::Hsl => Category::Color,
            EffectKind::Lut => Category::Color,
            EffectKind::Mask => Category::Keying,
            EffectKind::Matte => Category::Keying,
            EffectKind::Glow => Category::Light,
            EffectKind::Grain => Category::Noise,
            EffectKind::Aberration => Category::Light,
            EffectKind::Duotone => Category::Color,
            EffectKind::Invert => Category::Color,
            EffectKind::Posterize => Category::Stylize,
            EffectKind::Letterbox => Category::Transform,
            EffectKind::DirectionalBlur => Category::Blur,
            EffectKind::RadialBlur => Category::Blur,
            EffectKind::ZoomBlur => Category::Blur,
            EffectKind::TiltShift => Category::Blur,
            EffectKind::Denoise => Category::Noise,
            EffectKind::BrightnessContrast => Category::Color,
            EffectKind::Levels => Category::Color,
            EffectKind::BlackWhite => Category::Color,
            EffectKind::ColorIsolate => Category::Color,
            EffectKind::ChannelMixer => Category::Color,
            EffectKind::Threshold => Category::Stylize,
            EffectKind::LensDistortion => Category::Distort,
            EffectKind::Bulge => Category::Distort,
            EffectKind::Twirl => Category::Distort,
            EffectKind::Wave => Category::Distort,
            EffectKind::Turbulence => Category::Distort,
            EffectKind::CornerPin => Category::Distort,
            EffectKind::Offset => Category::Transform,
            EffectKind::Tile => Category::Transform,
            EffectKind::Gradient => Category::Generate,
            EffectKind::Grid => Category::Generate,
            EffectKind::LensFlare => Category::Light,
            EffectKind::LightRays => Category::Light,
            EffectKind::LightLeak => Category::Light,
            EffectKind::LumaKey => Category::Keying,
            EffectKind::FilmDamage => Category::Noise,
            EffectKind::Analog => Category::Noise,
            EffectKind::DropShadow => Category::Perspective,
            EffectKind::Perspective => Category::Perspective,
            EffectKind::FindEdges => Category::Stylize,
            EffectKind::Emboss => Category::Stylize,
            EffectKind::Halftone => Category::Stylize,
            EffectKind::Scanlines => Category::Stylize,
            EffectKind::Cartoon => Category::Stylize,
            EffectKind::Sketch => Category::Stylize,
            EffectKind::Paint => Category::Stylize,
            EffectKind::Glitch => Category::Stylize,
            EffectKind::Strobe => Category::Time,
            EffectKind::LowFrameRate => Category::Time,
            EffectKind::CameraShake => Category::Transform,
            EffectKind::Eq => Category::Sound,
            EffectKind::Compressor => Category::Sound,
            EffectKind::Limiter => Category::Sound,
            EffectKind::DeEsser => Category::Sound,
            EffectKind::NoiseReduction => Category::Sound,
            EffectKind::Ducking => Category::Sound,
            EffectKind::Plugin => Category::Sound,
        }
    }

    /// One line on what it does, for the library and for CoCaptain.
    pub fn about(self) -> &'static str {
        match self {
            EffectKind::Blur => "Soft, even blur",
            EffectKind::Sharpen => "Crisper detail (unsharp mask on brightness)",
            EffectKind::Vignette => "Darkens the corners",
            EffectKind::Mirror => "Reflects one half onto the other",
            EffectKind::Crop => "Cuts away the edges, with rounded corners if wanted",
            EffectKind::ChromaKey => "Makes a green (or any) screen transparent",
            EffectKind::Pixelate => "Big square blocks (mosaic), e.g. to hide a face",
            EffectKind::ColorWheels => "Lift, gamma and gain colour wheels",
            EffectKind::Curves => "Master and red, green, blue curves",
            EffectKind::Hsl => "Hue, saturation and lightness per colour band",
            EffectKind::Lut => "A .cube look-up table",
            EffectKind::Mask => "Shows only a rectangle, ellipse or drawn shape",
            EffectKind::Matte => "Keeps only the subject, cut out by AI",
            EffectKind::Glow => "Bright parts bloom",
            EffectKind::Grain => "Film grain that changes every frame",
            EffectKind::Aberration => "Red and blue split apart toward the edges (RGB split)",
            EffectKind::Duotone => "Shadows one colour, highlights another",
            EffectKind::Invert => "Negative picture",
            EffectKind::Posterize => "Fewer colour steps",
            EffectKind::Letterbox => "Cinema bars for a wider (or narrower) shape",
            EffectKind::DirectionalBlur => "Streaks the picture along one direction, like fast movement",
            EffectKind::RadialBlur => "Blurs in circles around a centre, like a spinning camera",
            EffectKind::ZoomBlur => "Streaks outward from a centre, like a fast zoom",
            EffectKind::TiltShift => "A sharp band with the rest blurred, so the scene looks like a miniature",
            EffectKind::Denoise => "Smooths out video noise while keeping edges",
            EffectKind::BrightnessContrast => "Simple brightness and contrast",
            EffectKind::Levels => "Sets the black point, white point and mid-tone gamma",
            EffectKind::BlackWhite => "Turns the picture grey, with how much each colour brightens it",
            EffectKind::ColorIsolate => "Keeps one colour and turns the rest grey",
            EffectKind::ChannelMixer => "Builds each output channel from a mix of the input channels",
            EffectKind::Threshold => "Pure black and white: brighter than the level is white",
            EffectKind::LensDistortion => "Barrel (positive) or pincushion (negative) bending, like a wide or long lens",
            EffectKind::Bulge => "Swells (positive) or pinches (negative) a round area",
            EffectKind::Twirl => "Twists the picture around a centre",
            EffectKind::Wave => "Moving waves through the picture",
            EffectKind::Turbulence => "Wobbles the picture with flowing noise, like heat haze or water",
            EffectKind::CornerPin => "Moves the four corners of the picture (in % of the frame), to fit it onto a screen or sign",
            EffectKind::Offset => "Slides the picture and wraps it around the edges",
            EffectKind::Tile => "Repeats the picture in a grid",
            EffectKind::Gradient => "A smooth gradient between four corner colours, mixed over the picture",
            EffectKind::Grid => "Draws grid lines over the picture",
            EffectKind::LensFlare => "A bright light source with glare, a streak and reflections across the frame",
            EffectKind::LightRays => "Bright areas cast rays away from a centre",
            EffectKind::LightLeak => "Warm moving light washing over the picture, like film fogging",
            EffectKind::LumaKey => "Makes dark (or with invert, bright) parts transparent",
            EffectKind::FilmDamage => "Scratches, dust, flicker and a sepia tone",
            EffectKind::Analog => "Worn videotape: colour bleed, noise and tracking lines",
            EffectKind::DropShadow => "A soft shadow behind the layer (for titles, shapes and scaled-down clips)",
            EffectKind::Perspective => "Turns the picture in 3D: swivel (left/right), tilt (up/down) and distance",
            EffectKind::FindEdges => "Draws the outlines of shapes",
            EffectKind::Emboss => "Raised, stamped look lit from one direction",
            EffectKind::Halftone => "Printed-dot look",
            EffectKind::Scanlines => "Dark horizontal lines, like an old screen",
            EffectKind::Cartoon => "Flat colours with dark outlines",
            EffectKind::Sketch => "Looks drawn in pencil",
            EffectKind::Paint => "Painterly brush look",
            EffectKind::Glitch => "Digital break-up: shifted blocks and split colours that jump around",
            EffectKind::Strobe => "Flashes to white at a steady rate",
            EffectKind::LowFrameRate => "Holds each picture longer, for a choppy stop-motion look",
            EffectKind::CameraShake => "Hand-held or impact shake (zoomed in a little so no edge shows)",
            EffectKind::Eq => "Four-band equaliser",
            EffectKind::Compressor => "Evens out loud and quiet parts",
            EffectKind::Limiter => "Stops peaks going over a ceiling",
            EffectKind::DeEsser => "Tames harsh s sounds",
            EffectKind::NoiseReduction => "Removes steady background noise",
            EffectKind::Ducking => "Turns this clip down while other sound plays",
            EffectKind::Plugin => "A CLAP audio plugin",
        }
    }

    /// Changes over time by itself (waves, shake, flicker...), so every frame differs even on a still.
    pub fn is_animated(self) -> bool {
        matches!(
            self,
            EffectKind::Grain
                | EffectKind::Wave
                | EffectKind::Turbulence
                | EffectKind::LightLeak
                | EffectKind::FilmDamage
                | EffectKind::Analog
                | EffectKind::Glitch
                | EffectKind::Strobe
                | EffectKind::CameraShake
        )
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
            EffectKind::Mask => &MASK,
            EffectKind::Matte => &MATTE,
            EffectKind::Glow => &GLOW,
            EffectKind::Grain => &GRAIN,
            EffectKind::Aberration => &ABERRATION,
            EffectKind::Duotone => &DUOTONE,
            EffectKind::Invert => &INVERT,
            EffectKind::Posterize => &POSTERIZE,
            EffectKind::Letterbox => &LETTERBOX,
            EffectKind::DirectionalBlur => &DIRECTIONAL_BLUR_P,
            EffectKind::RadialBlur => &RADIAL_BLUR_P,
            EffectKind::ZoomBlur => &ZOOM_BLUR_P,
            EffectKind::TiltShift => &TILT_SHIFT_P,
            EffectKind::Denoise => &DENOISE_P,
            EffectKind::BrightnessContrast => &BRIGHTNESS_CONTRAST_P,
            EffectKind::Levels => &LEVELS_P,
            EffectKind::BlackWhite => &BLACK_WHITE_P,
            EffectKind::ColorIsolate => &COLOR_ISOLATE_P,
            EffectKind::ChannelMixer => &CHANNEL_MIXER_P,
            EffectKind::Threshold => &THRESHOLD_P,
            EffectKind::LensDistortion => &LENS_DISTORTION_P,
            EffectKind::Bulge => &BULGE_P,
            EffectKind::Twirl => &TWIRL_P,
            EffectKind::Wave => &WAVE_P,
            EffectKind::Turbulence => &TURBULENCE_P,
            EffectKind::CornerPin => &CORNER_PIN_P,
            EffectKind::Offset => &OFFSET_P,
            EffectKind::Tile => &TILE_P,
            EffectKind::Gradient => &GRADIENT_P,
            EffectKind::Grid => &GRID_P,
            EffectKind::LensFlare => &LENS_FLARE_P,
            EffectKind::LightRays => &LIGHT_RAYS_P,
            EffectKind::LightLeak => &LIGHT_LEAK_P,
            EffectKind::LumaKey => &LUMA_KEY_P,
            EffectKind::FilmDamage => &FILM_DAMAGE_P,
            EffectKind::Analog => &ANALOG_P,
            EffectKind::DropShadow => &DROP_SHADOW_P,
            EffectKind::Perspective => &PERSPECTIVE_P,
            EffectKind::FindEdges => &FIND_EDGES_P,
            EffectKind::Emboss => &EMBOSS_P,
            EffectKind::Halftone => &HALFTONE_P,
            EffectKind::Scanlines => &SCANLINES_P,
            EffectKind::Cartoon => &CARTOON_P,
            EffectKind::Sketch => &SKETCH_P,
            EffectKind::Paint => &PAINT_P,
            EffectKind::Glitch => &GLITCH_P,
            EffectKind::Strobe => &STROBE_P,
            EffectKind::LowFrameRate => &LOW_FRAME_RATE_P,
            EffectKind::CameraShake => &CAMERA_SHAKE_P,
            EffectKind::Eq => &EQ,
            EffectKind::Compressor => &COMPRESSOR,
            EffectKind::Limiter => &LIMITER,
            EffectKind::DeEsser => &DEESSER,
            EffectKind::NoiseReduction => &NOISE,
            EffectKind::Ducking => &DUCKING,
            EffectKind::Plugin => &PLUGIN,
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
static CROP: [ParamDef; 5] = [
    p("Left", 0.0, 100.0, 0.0, 1),
    p("Top", 0.0, 100.0, 0.0, 1),
    p("Right", 0.0, 100.0, 0.0, 1),
    p("Bottom", 0.0, 100.0, 0.0, 1),
    // rounds the corners of what is left, in % of its shorter side
    p("Roundness", 0.0, 50.0, 0.0, 1),
];
static CHROMA: [ParamDef; 8] = [
    p("Key red", 0.0, 255.0, 0.0, 0),
    p("Key green", 0.0, 255.0, 255.0, 0),
    p("Key blue", 0.0, 255.0, 0.0, 0),
    p("Tolerance", 0.0, 100.0, 35.0, 0),
    p("Softness", 0.0, 100.0, 15.0, 0),
    p("Spill removal", 0.0, 100.0, 50.0, 0),
    p("Edge shrink", -5.0, 5.0, 0.0, 1),
    p("Matte view", 0.0, 1.0, 0.0, 0),
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
// shape 0 = rectangle, 1 = ellipse, 2 = free-drawn polygon (points in the effect's text); centre and size in % of the frame; feather in % of the shorter side
static MASK: [ParamDef; 8] = [
    p("Shape", 0.0, 2.0, 1.0, 0),
    p("Center x", 0.0, 100.0, 50.0, 1),
    p("Center y", 0.0, 100.0, 50.0, 1),
    p("Width", 1.0, 200.0, 60.0, 1),
    p("Height", 1.0, 200.0, 60.0, 1),
    p("Rotation", -180.0, 180.0, 0.0, 0),
    p("Feather", 0.0, 50.0, 8.0, 1),
    p("Invert", 0.0, 1.0, 0.0, 0),
];
// The picture is kept where the matte video (the effect's `text`: a grey video, white = keep) is white.
// `Offset` is the source frame the matte video's first frame belongs to. `Refine` (%) snaps the matte's soft
// edge to the edges in the picture, so no ring of the old background is left around the subject.
static MATTE: [ParamDef; 5] = [
    p("Invert", 0.0, 1.0, 0.0, 0),
    p("Feather", 0.0, 30.0, 1.5, 1),
    p("Choke", -10.0, 10.0, 0.0, 1),
    p("Offset", 0.0, 100_000_000.0, 0.0, 0),
    p("Refine", 0.0, 100.0, 100.0, 0),
];
// bright areas bloom: what is brighter than the threshold is blurred and added back
static GLOW: [ParamDef; 3] = [
    p("Threshold", 0.0, 100.0, 70.0, 0),
    p("Radius", 1.0, 100.0, 24.0, 0),
    p("Intensity", 0.0, 200.0, 80.0, 0),
];
// noise that changes every frame; size in pixels on a 1080-pixel-tall frame
static GRAIN: [ParamDef; 3] = [
    p("Amount", 0.0, 100.0, 20.0, 0),
    p("Size", 1.0, 8.0, 1.5, 1),
    p("Colour", 0.0, 100.0, 20.0, 0),
];
// red and blue pulled apart toward the edges of the frame, in pixels on a 1080-pixel-tall frame
static ABERRATION: [ParamDef; 1] = [p("Amount", 0.0, 30.0, 6.0, 1)];
// shadows take one colour and highlights another
static DUOTONE: [ParamDef; 7] = [
    p("Shadow red", 0.0, 255.0, 18.0, 0),
    p("Shadow green", 0.0, 255.0, 24.0, 0),
    p("Shadow blue", 0.0, 255.0, 64.0, 0),
    p("Highlight red", 0.0, 255.0, 255.0, 0),
    p("Highlight green", 0.0, 255.0, 190.0, 0),
    p("Highlight blue", 0.0, 255.0, 120.0, 0),
    p("Strength", 0.0, 100.0, 100.0, 0),
];
static INVERT: [ParamDef; 1] = [p("Amount", 0.0, 100.0, 100.0, 0)];
static POSTERIZE: [ParamDef; 1] = [p("Levels", 2.0, 32.0, 6.0, 0)];
// the shape left visible (width / height, 2.35 = cinema scope) and how solid the bars are
static LETTERBOX: [ParamDef; 2] = [
    p("Aspect", 0.5, 4.0, 2.35, 2),
    p("Opacity", 0.0, 100.0, 100.0, 0),
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
// turn this clip down while any other sound on the timeline is louder than the threshold
static DUCKING: [ParamDef; 4] = [
    p("Threshold dB", -60.0, 0.0, -35.0, 1),
    p("Reduction dB", 0.0, 40.0, 14.0, 1),
    p("Attack ms", 1.0, 500.0, 40.0, 0),
    p("Release ms", 10.0, 3000.0, 450.0, 0),
];
// how much of the plugin's sound is heard; the plugin and its settings are `Effect::text`
static PLUGIN: [ParamDef; 1] = [p("Mix", 0.0, 100.0, 100.0, 0)];
// Streaks the picture along one direction, like fast movement
static DIRECTIONAL_BLUR_P: [ParamDef; 2] = [
    p("Angle", -180.0, 180.0, 0.0, 0),
    p("Length", 0.0, 200.0, 30.0, 0),
];
// Blurs in circles around a centre, like a spinning camera
static RADIAL_BLUR_P: [ParamDef; 3] = [
    p("Amount", 0.0, 90.0, 10.0, 1),
    p("Center x", 0.0, 100.0, 50.0, 1),
    p("Center y", 0.0, 100.0, 50.0, 1),
];
// Streaks outward from a centre, like a fast zoom
static ZOOM_BLUR_P: [ParamDef; 3] = [
    p("Amount", 0.0, 100.0, 20.0, 0),
    p("Center x", 0.0, 100.0, 50.0, 1),
    p("Center y", 0.0, 100.0, 50.0, 1),
];
// A sharp band with the rest blurred, so the scene looks like a miniature
static TILT_SHIFT_P: [ParamDef; 4] = [
    p("Focus", 0.0, 100.0, 50.0, 1),
    p("Band", 0.0, 100.0, 20.0, 1),
    p("Falloff", 1.0, 100.0, 25.0, 1),
    p("Radius", 0.0, 100.0, 20.0, 1),
];
// Smooths out video noise while keeping edges
static DENOISE_P: [ParamDef; 2] = [
    p("Strength", 0.0, 100.0, 50.0, 0),
    p("Radius", 1.0, 10.0, 3.0, 0),
];
// Simple brightness and contrast
static BRIGHTNESS_CONTRAST_P: [ParamDef; 2] = [
    p("Brightness", -100.0, 100.0, 0.0, 0),
    p("Contrast", -100.0, 100.0, 0.0, 0),
];
// Sets the black point, white point and mid-tone gamma
static LEVELS_P: [ParamDef; 5] = [
    p("Input black", 0.0, 254.0, 0.0, 0),
    p("Input white", 1.0, 255.0, 255.0, 0),
    p("Gamma", 0.1, 10.0, 1.0, 2),
    p("Output black", 0.0, 255.0, 0.0, 0),
    p("Output white", 0.0, 255.0, 255.0, 0),
];
// Turns the picture grey, with how much each colour brightens it
static BLACK_WHITE_P: [ParamDef; 4] = [
    p("Amount", 0.0, 100.0, 100.0, 0),
    p("Red", -200.0, 300.0, 30.0, 0),
    p("Green", -200.0, 300.0, 59.0, 0),
    p("Blue", -200.0, 300.0, 11.0, 0),
];
// Keeps one colour and turns the rest grey
static COLOR_ISOLATE_P: [ParamDef; 4] = [
    p("Hue", 0.0, 360.0, 0.0, 0),
    p("Range", 1.0, 180.0, 30.0, 0),
    p("Softness", 0.0, 90.0, 15.0, 0),
    p("Invert", 0.0, 1.0, 0.0, 0),
];
// Builds each output channel from a mix of the input channels
static CHANNEL_MIXER_P: [ParamDef; 9] = [
    p("Red from red", -200.0, 200.0, 100.0, 0),
    p("Red from green", -200.0, 200.0, 0.0, 0),
    p("Red from blue", -200.0, 200.0, 0.0, 0),
    p("Green from red", -200.0, 200.0, 0.0, 0),
    p("Green from green", -200.0, 200.0, 100.0, 0),
    p("Green from blue", -200.0, 200.0, 0.0, 0),
    p("Blue from red", -200.0, 200.0, 0.0, 0),
    p("Blue from green", -200.0, 200.0, 0.0, 0),
    p("Blue from blue", -200.0, 200.0, 100.0, 0),
];
// Pure black and white: brighter than the level is white
static THRESHOLD_P: [ParamDef; 1] = [p("Level", 0.0, 255.0, 128.0, 0)];
// Barrel (positive) or pincushion (negative) bending, like a wide or long lens
static LENS_DISTORTION_P: [ParamDef; 2] = [
    p("Curvature", -100.0, 100.0, 20.0, 0),
    p("Zoom", 50.0, 200.0, 100.0, 0),
];
// Swells (positive) or pinches (negative) a round area
static BULGE_P: [ParamDef; 4] = [
    p("Amount", -100.0, 100.0, 50.0, 0),
    p("Radius", 1.0, 100.0, 35.0, 1),
    p("Center x", 0.0, 100.0, 50.0, 1),
    p("Center y", 0.0, 100.0, 50.0, 1),
];
// Twists the picture around a centre
static TWIRL_P: [ParamDef; 4] = [
    p("Angle", -720.0, 720.0, 90.0, 0),
    p("Radius", 1.0, 100.0, 40.0, 1),
    p("Center x", 0.0, 100.0, 50.0, 1),
    p("Center y", 0.0, 100.0, 50.0, 1),
];
// Moving waves through the picture
static WAVE_P: [ParamDef; 4] = [
    p("Amplitude", 0.0, 200.0, 10.0, 1),
    p("Wavelength", 4.0, 1000.0, 120.0, 0),
    p("Direction", 0.0, 1.0, 0.0, 0),
    p("Speed", -10.0, 10.0, 1.0, 2),
];
// Wobbles the picture with flowing noise, like heat haze or water
static TURBULENCE_P: [ParamDef; 3] = [
    p("Amount", 0.0, 200.0, 20.0, 1),
    p("Size", 4.0, 500.0, 120.0, 0),
    p("Speed", 0.0, 10.0, 1.0, 2),
];
// Moves the four corners of the picture (in % of the frame), to fit it onto a screen or sign
static CORNER_PIN_P: [ParamDef; 8] = [
    p("Top left x", -50.0, 150.0, 0.0, 1),
    p("Top left y", -50.0, 150.0, 0.0, 1),
    p("Top right x", -50.0, 150.0, 100.0, 1),
    p("Top right y", -50.0, 150.0, 0.0, 1),
    p("Bottom right x", -50.0, 150.0, 100.0, 1),
    p("Bottom right y", -50.0, 150.0, 100.0, 1),
    p("Bottom left x", -50.0, 150.0, 0.0, 1),
    p("Bottom left y", -50.0, 150.0, 100.0, 1),
];
// Slides the picture and wraps it around the edges
static OFFSET_P: [ParamDef; 2] = [p("X", -100.0, 100.0, 0.0, 1), p("Y", -100.0, 100.0, 0.0, 1)];
// Repeats the picture in a grid
static TILE_P: [ParamDef; 1] = [p("Count", 1.0, 16.0, 2.0, 0)];
// A smooth gradient between four corner colours, mixed over the picture
static GRADIENT_P: [ParamDef; 13] = [
    p("Top left red", 0.0, 255.0, 255.0, 0),
    p("Top left green", 0.0, 255.0, 120.0, 0),
    p("Top left blue", 0.0, 255.0, 40.0, 0),
    p("Top right red", 0.0, 255.0, 200.0, 0),
    p("Top right green", 0.0, 255.0, 40.0, 0),
    p("Top right blue", 0.0, 255.0, 160.0, 0),
    p("Bottom left red", 0.0, 255.0, 30.0, 0),
    p("Bottom left green", 0.0, 255.0, 60.0, 0),
    p("Bottom left blue", 0.0, 255.0, 200.0, 0),
    p("Bottom right red", 0.0, 255.0, 20.0, 0),
    p("Bottom right green", 0.0, 255.0, 180.0, 0),
    p("Bottom right blue", 0.0, 255.0, 170.0, 0),
    p("Opacity", 0.0, 100.0, 50.0, 0),
];
// Draws grid lines over the picture
static GRID_P: [ParamDef; 6] = [
    p("Spacing", 4.0, 500.0, 120.0, 0),
    p("Thickness", 1.0, 50.0, 2.0, 0),
    p("Opacity", 0.0, 100.0, 60.0, 0),
    p("Red", 0.0, 255.0, 255.0, 0),
    p("Green", 0.0, 255.0, 255.0, 0),
    p("Blue", 0.0, 255.0, 255.0, 0),
];
// A bright light source with glare, a streak and reflections across the frame
static LENS_FLARE_P: [ParamDef; 4] = [
    p("Center x", 0.0, 100.0, 30.0, 1),
    p("Center y", 0.0, 100.0, 25.0, 1),
    p("Brightness", 0.0, 300.0, 100.0, 0),
    p("Size", 10.0, 300.0, 100.0, 0),
];
// Bright areas cast rays away from a centre
static LIGHT_RAYS_P: [ParamDef; 5] = [
    p("Center x", 0.0, 100.0, 50.0, 1),
    p("Center y", 0.0, 100.0, 30.0, 1),
    p("Length", 0.0, 100.0, 40.0, 0),
    p("Threshold", 0.0, 100.0, 50.0, 0),
    p("Intensity", 0.0, 300.0, 100.0, 0),
];
// Warm moving light washing over the picture, like film fogging
static LIGHT_LEAK_P: [ParamDef; 4] = [
    p("Intensity", 0.0, 100.0, 60.0, 0),
    p("Speed", 0.0, 5.0, 0.5, 2),
    p("Hue", 0.0, 360.0, 25.0, 0),
    p("Seed", 0.0, 1000.0, 1.0, 0),
];
// Makes dark (or with invert, bright) parts transparent
static LUMA_KEY_P: [ParamDef; 3] = [
    p("Threshold", 0.0, 100.0, 20.0, 0),
    p("Softness", 0.0, 100.0, 10.0, 0),
    p("Invert", 0.0, 1.0, 0.0, 0),
];
// Scratches, dust, flicker and a sepia tone
static FILM_DAMAGE_P: [ParamDef; 4] = [
    p("Scratches", 0.0, 100.0, 40.0, 0),
    p("Dust", 0.0, 100.0, 40.0, 0),
    p("Flicker", 0.0, 100.0, 30.0, 0),
    p("Sepia", 0.0, 100.0, 50.0, 0),
];
// Worn videotape: colour bleed, noise and tracking lines
static ANALOG_P: [ParamDef; 3] = [
    p("Amount", 0.0, 100.0, 60.0, 0),
    p("Noise", 0.0, 100.0, 40.0, 0),
    p("Tracking", 0.0, 100.0, 30.0, 0),
];
// A soft shadow behind the layer (for titles, shapes and scaled-down clips)
static DROP_SHADOW_P: [ParamDef; 7] = [
    p("Distance", 0.0, 200.0, 20.0, 0),
    p("Angle", -180.0, 180.0, 135.0, 0),
    p("Softness", 0.0, 100.0, 15.0, 0),
    p("Opacity", 0.0, 100.0, 60.0, 0),
    p("Red", 0.0, 255.0, 0.0, 0),
    p("Green", 0.0, 255.0, 0.0, 0),
    p("Blue", 0.0, 255.0, 0.0, 0),
];
// Turns the picture in 3D: swivel (left/right), tilt (up/down) and distance
static PERSPECTIVE_P: [ParamDef; 3] = [
    p("Swivel", -80.0, 80.0, 20.0, 0),
    p("Tilt", -80.0, 80.0, 0.0, 0),
    p("Distance", -50.0, 200.0, 0.0, 0),
];
// Draws the outlines of shapes
static FIND_EDGES_P: [ParamDef; 2] = [
    p("Amount", 0.0, 100.0, 100.0, 0),
    p("Invert", 0.0, 1.0, 0.0, 0),
];
// Raised, stamped look lit from one direction
static EMBOSS_P: [ParamDef; 4] = [
    p("Direction", -180.0, 180.0, 45.0, 0),
    p("Relief", 0.0, 10.0, 2.0, 1),
    p("Amount", 0.0, 100.0, 100.0, 0),
    p("Colour", 0.0, 100.0, 0.0, 0),
];
// Printed-dot look
static HALFTONE_P: [ParamDef; 3] = [
    p("Dot size", 2.0, 100.0, 10.0, 0),
    p("Angle", -90.0, 90.0, 45.0, 0),
    p("Colour", 0.0, 1.0, 0.0, 0),
];
// Dark horizontal lines, like an old screen
static SCANLINES_P: [ParamDef; 3] = [
    p("Spacing", 2.0, 40.0, 4.0, 0),
    p("Opacity", 0.0, 100.0, 40.0, 0),
    p("Speed", -100.0, 100.0, 0.0, 0),
];
// Flat colours with dark outlines
static CARTOON_P: [ParamDef; 3] = [
    p("Levels", 2.0, 16.0, 6.0, 0),
    p("Edges", 0.0, 100.0, 70.0, 0),
    p("Smoothness", 0.0, 10.0, 3.0, 0),
];
// Looks drawn in pencil
static SKETCH_P: [ParamDef; 2] = [
    p("Strength", 0.0, 100.0, 80.0, 0),
    p("Colour", 0.0, 100.0, 0.0, 0),
];
// Painterly brush look
static PAINT_P: [ParamDef; 1] = [p("Brush", 1.0, 15.0, 4.0, 0)];
// Digital break-up: shifted blocks and split colours that jump around
static GLITCH_P: [ParamDef; 4] = [
    p("Amount", 0.0, 100.0, 50.0, 0),
    p("Block size", 4.0, 200.0, 40.0, 0),
    p("Speed", 0.0, 30.0, 8.0, 1),
    p("Seed", 0.0, 1000.0, 1.0, 0),
];
// Flashes to white at a steady rate
static STROBE_P: [ParamDef; 3] = [
    p("Rate", 0.5, 30.0, 4.0, 1),
    p("Length", 1.0, 100.0, 20.0, 0),
    p("Brightness", 0.0, 100.0, 100.0, 0),
];
// Holds each picture longer, for a choppy stop-motion look
static LOW_FRAME_RATE_P: [ParamDef; 1] = [p("Frame rate", 1.0, 60.0, 12.0, 0)];
// Hand-held or impact shake (zoomed in a little so no edge shows)
static CAMERA_SHAKE_P: [ParamDef; 4] = [
    p("Amount", 0.0, 200.0, 20.0, 0),
    p("Rotation", 0.0, 20.0, 1.0, 1),
    p("Speed", 0.1, 20.0, 4.0, 1),
    p("Zoom", 100.0, 150.0, 110.0, 0),
];
// the .cube file is `Effect::text`
static LUT: [ParamDef; 1] = [p("Strength", 0.0, 100.0, 100.0, 0)];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawEffect")]
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

/// An effect as stored. Files written before a kind gained settings have fewer numbers: the new ones take
/// their defaults, so old projects keep opening.
#[derive(Deserialize)]
struct RawEffect {
    kind: EffectKind,
    #[serde(default = "yes")]
    enabled: bool,
    params: Vec<f64>,
    #[serde(default)]
    text: String,
}

impl From<RawEffect> for Effect {
    fn from(r: RawEffect) -> Effect {
        let mut params = r.params;
        let defs = r.kind.params();
        while params.len() < defs.len() {
            params.push(defs[params.len()].default);
        }
        Effect {
            kind: r.kind,
            enabled: r.enabled,
            params,
            text: r.text,
        }
    }
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
        if self.kind == EffectKind::Mask
            && self.params[0] >= 1.5
            && parse_polygon(&self.text).is_none()
        {
            return Err("a drawn mask needs 3 to 64 points, as 'x,y x,y x,y' in percent");
        }
        if self.kind == EffectKind::Plugin && !self.text.starts_with("clap:") {
            return Err("an audio plugin effect needs a plugin");
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

/// The corners of a drawn mask: `x,y x,y ...` in percent of the frame, 3 to 64 of them.
pub fn parse_polygon(text: &str) -> Option<Vec<(f64, f64)>> {
    let pts: Option<Vec<(f64, f64)>> = text
        .split_whitespace()
        .map(|w| {
            let (x, y) = w.split_once(',')?;
            let (x, y): (f64, f64) = (x.parse().ok()?, y.parse().ok()?);
            (x.is_finite()
                && y.is_finite()
                && (-100.0..=200.0).contains(&x)
                && (-100.0..=200.0).contains(&y))
            .then_some((x, y))
        })
        .collect();
    pts.filter(|p| (3..=64).contains(&p.len()))
}

pub fn polygon_text(points: &[(f64, f64)]) -> String {
    points
        .iter()
        .map(|(x, y)| format!("{x:.2},{y:.2}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn check_effects(effects: &[Effect]) -> Result<(), &'static str> {
    if effects.len() > 16 {
        return Err("too many effects on one clip");
    }
    effects.iter().try_for_each(Effect::check)
}

/// A parameter's name as tools and keyframes write it: "Center x" -> "center_x".
pub fn param_key(name: &str) -> String {
    name.to_lowercase().replace(' ', "_")
}

/// The keyframe property of setting `param` of `effects[index]`: `fx.<kind>.<setting>` for the first effect of
/// that kind, `fx.<kind>#2.<setting>` for the second, and so on. Naming by kind, not position, keeps the
/// keyframes with their effect when the stack is reordered.
pub fn fx_property(effects: &[Effect], index: usize, param: usize) -> String {
    let e = &effects[index];
    let nth = effects[..index].iter().filter(|x| x.kind == e.kind).count() + 1;
    let p = param_key(e.kind.params()[param].name);
    if nth == 1 {
        format!("fx.{}.{p}", e.kind.key())
    } else {
        format!("fx.{}#{nth}.{p}", e.kind.key())
    }
}

/// `fx.<kind>[#n].<setting>` -> (kind, n starting at 1, setting index).
pub fn parse_fx_property(name: &str) -> Option<(EffectKind, usize, usize)> {
    let rest = name.strip_prefix("fx.")?;
    let (head, setting) = rest.split_once('.')?;
    let (kind, nth) = match head.split_once('#') {
        Some((k, n)) => (k, n.parse::<usize>().ok().filter(|n| *n >= 1)?),
        None => (head, 1),
    };
    let kind = EffectKind::from_key(kind)?;
    let idx = kind
        .params()
        .iter()
        .position(|d| param_key(d.name) == setting)?;
    Some((kind, nth, idx))
}

/// `effects` with their keyframed settings at `t` (clamped to each setting's range). Borrowed when nothing is
/// keyframed.
pub fn effects_at<'a>(
    effects: &'a [Effect],
    anim: &crate::Anim,
    t: i64,
) -> std::borrow::Cow<'a, [Effect]> {
    let keyed: Vec<(&String, (EffectKind, usize, usize))> = anim
        .0
        .keys()
        .filter_map(|k| parse_fx_property(k).map(|p| (k, p)))
        .collect();
    if keyed.is_empty() {
        return std::borrow::Cow::Borrowed(effects);
    }
    let mut out = effects.to_vec();
    for (prop, (kind, nth, param)) in keyed {
        let Some(v) = anim.value(prop, t) else {
            continue;
        };
        if let Some(e) = out.iter_mut().filter(|e| e.kind == kind).nth(nth - 1) {
            let d = &kind.params()[param];
            e.params[param] = v.clamp(d.min, d.max);
        }
    }
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
mod fx_property_tests {
    use super::*;
    use crate::{Anim, Interp, Keyframe};

    #[test]
    fn names_round_trip_and_follow_the_kind() {
        let fx = vec![
            Effect::new(EffectKind::Blur),
            Effect::new(EffectKind::Glow),
            Effect::new(EffectKind::Blur),
        ];
        assert_eq!(fx_property(&fx, 0, 0), "fx.blur.radius");
        assert_eq!(fx_property(&fx, 2, 0), "fx.blur#2.radius");
        assert_eq!(fx_property(&fx, 1, 2), "fx.glow.intensity");
        assert_eq!(
            parse_fx_property("fx.blur#2.radius"),
            Some((EffectKind::Blur, 2, 0))
        );
        assert_eq!(
            parse_fx_property("fx.zoom_blur.center_x"),
            Some((EffectKind::ZoomBlur, 1, 1))
        );
        assert_eq!(parse_fx_property("fx.blur.nope"), None);
        assert_eq!(parse_fx_property("fx.nope.radius"), None);
        assert_eq!(parse_fx_property("exposure"), None);
    }

    #[test]
    fn keyframed_settings_move_and_stay_in_range() {
        let fx = vec![Effect::new(EffectKind::Glow), Effect::new(EffectKind::Blur)];
        let mut anim = Anim::default();
        let key = |t, v| Keyframe {
            t,
            v,
            interp: Interp::Linear,
        };
        anim.set_track("fx.blur.radius", vec![key(0, 0.0), key(10, 40.0)]);
        anim.set_track("fx.glow.intensity", vec![key(0, 999.0)]);
        let mid = effects_at(&fx, &anim, 5);
        assert_eq!(mid[1].params[0], 20.0);
        assert_eq!(mid[0].params[2], 200.0, "clamped to the maximum");
        assert!(matches!(
            effects_at(&fx, &Anim::default(), 5),
            std::borrow::Cow::Borrowed(_)
        ));
    }
}
