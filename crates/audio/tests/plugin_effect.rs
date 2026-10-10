//! A CLAP plugin used as a clip effect, through the same path the mixer uses.

use captaine_audio::apply_audio_effects;
use captaine_model::{Effect, EffectKind};
use captaine_plugins::{params, register_entry, PluginRef};
use clack_host::prelude::PluginEntry;
use clack_plugin::entry::SinglePluginEntry;
use clack_plugin_gain::GainPlugin;

#[test]
fn a_plugin_effect_changes_the_sound_and_mix_blends_it_with_the_original() {
    register_entry(
        "builtin:gain",
        PluginEntry::load_from_clack::<SinglePluginEntry<GainPlugin>>(c"").unwrap(),
    );
    let base = PluginRef {
        id: "org.rust-audio.clack.gain".into(),
        path: "builtin:gain".into(),
        params: vec![],
    };
    let gain_id = params(&base).unwrap()[0].id;
    let tone: Vec<f32> = (0..9600)
        .flat_map(|i| {
            let s = (i as f32 * 300.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.6;
            [s, s]
        })
        .collect();
    let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
    let run = |mix: f64, value: f64| {
        let mut e = Effect::new(EffectKind::Plugin);
        e.text = base.clone().with_value(gain_id, value).to_text();
        e.params[0] = mix;
        assert!(e.check().is_ok());
        let mut buf = tone.clone();
        apply_audio_effects(&mut buf, 48_000, &[e]);
        rms(&buf) / rms(&tone)
    };
    assert!((run(100.0, 0.5) - 0.5).abs() < 0.05, "fully wet halves it");
    assert!((run(0.0, 0.5) - 1.0).abs() < 0.02, "fully dry leaves it");
    let half = run(50.0, 0.5);
    assert!((half - 0.75).abs() < 0.05, "an even blend: {half}");
    // an effect without a plugin is refused by the checks, and one that cannot load leaves the sound alone
    assert!(Effect::new(EffectKind::Plugin).check().is_err());
    let mut broken = Effect::new(EffectKind::Plugin);
    broken.text = "clap:nope@/does/not/exist.clap".into();
    let mut buf = tone.clone();
    apply_audio_effects(&mut buf, 48_000, &[broken]);
    assert_eq!(buf, tone);
}
