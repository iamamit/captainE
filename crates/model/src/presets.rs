//! Effect presets: named lists of effects to apply in one click, and the way they are stored.

use serde::{Deserialize, Serialize};

use crate::{Effect, EffectKind};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    /// Sound presets go on clips with audio, look presets on clips with picture
    pub audio: bool,
    pub effects: Vec<Effect>,
}

/// An effect of `kind` with some settings changed by name (the names shown in the inspector).
fn fx(kind: EffectKind, set: &[(&str, f64)]) -> Effect {
    let mut e = Effect::new(kind);
    for (name, v) in set {
        let i = kind
            .params()
            .iter()
            .position(|d| d.name == *name)
            .unwrap_or_else(|| panic!("{kind:?} has no setting {name}"));
        e.params[i] = *v;
    }
    e
}

/// The presets that come with captainE.
pub fn builtin() -> Vec<Preset> {
    let p = |name: &str, audio: bool, effects: Vec<Effect>| Preset {
        name: name.to_string(),
        audio,
        effects,
    };
    vec![
        p(
            "Cinematic",
            false,
            vec![
                fx(
                    EffectKind::Curves,
                    &[("Master 25%", 0.20), ("Master 75%", 0.80)],
                ),
                fx(
                    EffectKind::ColorWheels,
                    &[
                        ("Lift red", -0.04),
                        ("Lift green", 0.01),
                        ("Lift blue", 0.05),
                        ("Gain red", 0.05),
                        ("Gain green", 0.01),
                        ("Gain blue", -0.06),
                    ],
                ),
                fx(EffectKind::Vignette, &[("Amount", 35.0)]),
            ],
        ),
        p(
            "Vintage",
            false,
            vec![
                fx(
                    EffectKind::Curves,
                    &[("Master 0%", 0.07), ("Master 100%", 0.94)],
                ),
                fx(
                    EffectKind::Hsl,
                    &[
                        ("Red sat", 80.0),
                        ("Green sat", 70.0),
                        ("Blue sat", 75.0),
                        ("Yellow sat", 90.0),
                    ],
                ),
                fx(EffectKind::Grain, &[("Amount", 30.0)]),
                fx(EffectKind::Vignette, &[("Amount", 45.0)]),
            ],
        ),
        p(
            "Dreamy",
            false,
            vec![
                fx(
                    EffectKind::Glow,
                    &[("Threshold", 55.0), ("Radius", 40.0), ("Intensity", 90.0)],
                ),
                fx(EffectKind::Blur, &[("Radius", 2.0)]),
                fx(EffectKind::Curves, &[("Master 0%", 0.05)]),
            ],
        ),
        p(
            "Noir",
            false,
            vec![
                fx(
                    EffectKind::Hsl,
                    &[
                        ("Red sat", 0.0),
                        ("Yellow sat", 0.0),
                        ("Green sat", 0.0),
                        ("Cyan sat", 0.0),
                        ("Blue sat", 0.0),
                        ("Magenta sat", 0.0),
                    ],
                ),
                fx(
                    EffectKind::Curves,
                    &[("Master 25%", 0.16), ("Master 75%", 0.86)],
                ),
                fx(EffectKind::Vignette, &[("Amount", 60.0)]),
                fx(EffectKind::Grain, &[("Amount", 25.0), ("Colour", 0.0)]),
            ],
        ),
        p(
            "Punchy",
            false,
            vec![
                fx(EffectKind::Sharpen, &[("Amount", 110.0)]),
                fx(
                    EffectKind::Curves,
                    &[("Master 25%", 0.22), ("Master 75%", 0.78)],
                ),
                fx(
                    EffectKind::Hsl,
                    &[
                        ("Red sat", 115.0),
                        ("Yellow sat", 112.0),
                        ("Green sat", 110.0),
                        ("Blue sat", 115.0),
                    ],
                ),
            ],
        ),
        p(
            "Clean voice",
            true,
            vec![
                fx(
                    EffectKind::NoiseReduction,
                    &[("Reduction dB", 12.0), ("Sensitivity %", 55.0)],
                ),
                fx(
                    EffectKind::Eq,
                    &[
                        ("Low gain", -8.0),
                        ("Low freq", 90.0),
                        ("Mid 2 gain", 2.5),
                        ("Mid 2 freq", 3200.0),
                    ],
                ),
                fx(EffectKind::DeEsser, &[]),
                fx(
                    EffectKind::Compressor,
                    &[("Threshold dB", -20.0), ("Ratio", 3.0), ("Makeup dB", 3.0)],
                ),
                fx(EffectKind::Limiter, &[]),
            ],
        ),
        p(
            "Podcast",
            true,
            vec![
                fx(
                    EffectKind::Eq,
                    &[
                        ("Low gain", -6.0),
                        ("Low freq", 80.0),
                        ("Mid 1 gain", 2.0),
                        ("Mid 1 freq", 180.0),
                        ("High gain", 1.5),
                        ("High freq", 9000.0),
                    ],
                ),
                fx(
                    EffectKind::Compressor,
                    &[("Threshold dB", -22.0), ("Ratio", 4.0), ("Makeup dB", 4.0)],
                ),
                fx(EffectKind::Limiter, &[("Ceiling dB", -1.5)]),
            ],
        ),
        p(
            "Telephone",
            true,
            vec![fx(
                EffectKind::Eq,
                &[
                    ("Low gain", -18.0),
                    ("Low freq", 500.0),
                    ("Mid 1 gain", 6.0),
                    ("Mid 1 freq", 1400.0),
                    ("Mid 1 Q", 1.5),
                    ("High gain", -18.0),
                    ("High freq", 3800.0),
                ],
            )],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_is_valid_and_picture_or_sound_not_both() {
        let all = builtin();
        assert!(all.len() >= 7);
        for p in &all {
            assert!(!p.effects.is_empty(), "{}", p.name);
            crate::check_effects(&p.effects).unwrap_or_else(|e| panic!("{}: {e}", p.name));
            assert!(
                p.effects.iter().all(|e| e.kind.is_audio() == p.audio),
                "{} mixes picture and sound",
                p.name
            );
        }
        let mut names: Vec<_> = all.iter().map(|p| p.name.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len(), "names are unique");
    }

    #[test]
    fn presets_round_trip_through_json() {
        let p = &builtin()[0];
        let back: Preset = serde_json::from_str(&serde_json::to_string(p).unwrap()).unwrap();
        assert_eq!(&back, p);
    }
}
