//! The user's own effect presets, one JSON file each in `~/.captaine/presets`.

use std::path::PathBuf;

use captaine_model::presets::Preset;
use captaine_model::Effect;

pub fn dir() -> PathBuf {
    let home = std::env::var("CAPTAINE_PRESETS")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".captaine/presets")))
        .unwrap_or_else(|_| std::env::temp_dir().join("captaine-presets"));
    home
}

/// File names are the preset name with anything odd turned into a dash.
fn file_for(name: &str) -> PathBuf {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    dir().join(format!("{}.json", clean.trim()))
}

pub fn user_presets() -> Vec<Preset> {
    let Ok(read) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut out: Vec<Preset> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|t| serde_json::from_str::<Preset>(&t).ok())
        .filter(|p| captaine_model::check_effects(&p.effects).is_ok())
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn save(name: &str, effects: &[Effect]) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("give the preset a name".into());
    }
    if effects.is_empty() {
        return Err("there are no effects to save".into());
    }
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let preset = Preset {
        name: name.to_string(),
        audio: effects.iter().any(|e| e.kind.is_audio()),
        effects: effects.to_vec(),
    };
    let text = serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())?;
    std::fs::write(file_for(name), text).map_err(|e| e.to_string())
}
