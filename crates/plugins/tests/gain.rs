//! Runs real audio through a real CLAP plugin (the gain example from the clack project, loaded in-process).

use captaine_plugins::*;
use clack_host::prelude::PluginEntry;
use clack_plugin::entry::SinglePluginEntry;
use clack_plugin_gain::GainPlugin;

const PATH: &str = "builtin:gain";
const ID: &str = "org.rust-audio.clack.gain";

fn setup() {
    register_entry(
        PATH,
        PluginEntry::load_from_clack::<SinglePluginEntry<GainPlugin>>(c"").unwrap(),
    );
}

#[test]
fn a_plugin_is_found_listed_and_its_controls_are_read() {
    setup();
    let found = scan(&[]);
    let gain = found
        .iter()
        .find(|p| p.id == ID)
        .expect("the plugin shows up in a scan");
    assert_eq!(gain.name, "Clack Gain Example");
    let refr = PluginRef {
        id: ID.into(),
        path: PATH.into(),
        params: vec![],
    };
    let params = params(&refr).unwrap();
    assert!(!params.is_empty(), "it has a gain control");
    let p = &params[0];
    assert!(p.min < p.max && !p.name.is_empty(), "{p:?}");
}

#[test]
fn audio_comes_back_changed_by_the_plugin_and_the_plugin_keeps_its_state() {
    setup();
    // a tone, stereo interleaved
    let tone: Vec<f32> = (0..4800)
        .flat_map(|i| {
            let s = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
            [s, s]
        })
        .collect();
    let plain = PluginRef {
        id: ID.into(),
        path: PATH.into(),
        params: vec![],
    };
    let info = params(&plain).unwrap();
    let gain_id = info[0].id;
    let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
    let unity = process(&plain, 48_000.0, &tone).unwrap();
    assert_eq!(unity.len(), tone.len());
    // half the level when asked: the value 0.5 on the plugin's own scale
    let quiet = plain.clone().with_value(gain_id, 0.5);
    let halved = process(&quiet, 48_000.0, &tone).unwrap();
    let ratio = rms(&halved) / rms(&unity);
    assert!((ratio - 0.5).abs() < 0.05, "ratio {ratio}");
    assert!(halved.iter().all(|s| s.is_finite()));
    // a second call goes through the same loaded plugin, still at that setting
    let again = process(&quiet, 48_000.0, &tone).unwrap();
    assert!((rms(&again) / rms(&unity) - 0.5).abs() < 0.05);
    // something that is not a plugin is an error, not a crash
    let bad = PluginRef {
        id: "nope".into(),
        path: "/does/not/exist.clap".into(),
        params: vec![],
    };
    assert!(process(&bad, 48_000.0, &tone).is_err());
    assert!(params(&bad).is_err());
}
