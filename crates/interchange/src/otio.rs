//! OpenTimelineIO JSON. Clips keep their media reference, source range, retime and transitions; captainE
//! also writes its own properties (picture and sound settings, effects, keyframes, titles) under
//! `metadata.captaine`, so a file written here and read back here loses nothing.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use captaine_model::*;
use captaine_ops::Op;
use captaine_time::{Rational, RationalTime};
use serde_json::{json, Value};

use crate::{file_url, path_from_url, rate_from_f64};

fn rt(value: i64, rate: Rational) -> Value {
    json!({"OTIO_SCHEMA": "RationalTime.1", "rate": rate.as_f64(), "value": value})
}

fn range(start: i64, duration: i64, rate: Rational) -> Value {
    json!({"OTIO_SCHEMA": "TimeRange.1", "start_time": rt(start, rate), "duration": rt(duration, rate)})
}

pub fn export(project: &Project, timeline: Id) -> Result<String, String> {
    let tl = project.timeline(timeline).ok_or("timeline not found")?;
    let rate = tl.rate;
    let mut tracks = Vec::new();
    for track in &tl.tracks {
        let mut children: Vec<Value> = Vec::new();
        for item in &track.items {
            match item {
                Item::Clip(c) => {
                    let m = project.media.get(&c.media);
                    let src_rate = c.source_start.rate;
                    if let Some(t) = c.transition {
                        let (a, b) = t.split();
                        children.push(json!({
                            "OTIO_SCHEMA": "Transition.1",
                            "name": t.kind.label(),
                            "transition_type": "SMPTE_Dissolve",
                            "in_offset": rt(a, rate),
                            "out_offset": rt(b, rate),
                            "metadata": {"captaine": {"kind": t.kind, "duration": t.duration}},
                        }));
                    }
                    let mut effects = Vec::new();
                    if c.retime.speed != 1.0 || c.retime.reverse || c.retime.freeze {
                        let scalar = if c.retime.freeze {
                            0.0
                        } else if c.retime.reverse {
                            -c.retime.speed
                        } else {
                            c.retime.speed
                        };
                        effects.push(json!({
                            "OTIO_SCHEMA": if c.retime.freeze { "FreezeFrame.1" } else { "LinearTimeWarp.1" },
                            "name": "retime",
                            "effect_name": if c.retime.freeze { "FreezeFrame" } else { "LinearTimeWarp" },
                            "time_scalar": scalar,
                        }));
                    }
                    let reference = match m {
                        Some(m) => json!({
                            "OTIO_SCHEMA": "ExternalReference.1",
                            "name": m.name,
                            "target_url": file_url(&m.path),
                            "available_range": m.duration.map(|d| range(0, d.value, d.rate)),
                        }),
                        None => json!({"OTIO_SCHEMA": "MissingReference.1"}),
                    };
                    children.push(json!({
                        "OTIO_SCHEMA": "Clip.2",
                        "name": c.name,
                        "enabled": c.enabled,
                        "source_range": range(c.source_start.value, c.source_span(), src_rate),
                        "media_reference": reference,
                        "effects": effects,
                        "metadata": {"captaine": {
                            "id": c.id, "duration": c.duration.value, "video": c.video, "audio": c.audio,
                            "anim": c.anim, "effects": c.effects, "retime": c.retime,
                        }},
                    }));
                }
                Item::Title(t) => children.push(json!({
                    "OTIO_SCHEMA": "Clip.2",
                    "name": t.name,
                    "enabled": t.enabled,
                    "source_range": range(0, t.duration.value, rate),
                    "media_reference": {"OTIO_SCHEMA": "GeneratorReference.1", "generator_kind": "captaine_title",
                        "parameters": {"text": t.text, "subtitle": t.subtitle}},
                    "metadata": {"captaine": {"title": t}},
                })),
                Item::Gap(g) => children.push(json!({
                    "OTIO_SCHEMA": "Gap.1",
                    "name": "",
                    "source_range": range(0, g.duration.value, rate),
                })),
            }
        }
        tracks.push(json!({
            "OTIO_SCHEMA": "Track.1",
            "name": track.name,
            "kind": if track.kind == TrackKind::Video { "Video" } else { "Audio" },
            "enabled": track.enabled,
            "children": children,
            "metadata": {"captaine": {"gain_db": track.gain_db}},
        }));
    }
    let doc = json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": tl.name,
        "global_start_time": rt(0, rate),
        "tracks": {"OTIO_SCHEMA": "Stack.1", "name": "tracks", "children": tracks},
        "metadata": {"captaine": {"width": project.settings.width, "height": project.settings.height}},
    });
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn time_of(v: &Value) -> Option<(i64, f64)> {
    Some((num(v, "value")?.round() as i64, num(v, "rate")?))
}

/// Ops that add the OTIO timeline as a new timeline of the project, plus the media it needs: references
/// that point at a file already in the project reuse it, other files are probed and added.
pub fn import(project: &Project, text: &str) -> Result<Vec<Op>, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    if doc
        .get("OTIO_SCHEMA")
        .and_then(Value::as_str)
        .map_or(true, |s| !s.starts_with("Timeline"))
    {
        return Err("this is not an OpenTimelineIO timeline".into());
    }
    let stack = doc.get("tracks").ok_or("the timeline has no tracks")?;
    let kids = stack
        .get("children")
        .and_then(Value::as_array)
        .ok_or("the timeline has no tracks")?;
    // the timeline's rate: from its start time, or the first range found
    let fps = doc
        .get("global_start_time")
        .and_then(time_of)
        .map(|t| t.1)
        .or_else(|| {
            kids.iter()
                .flat_map(|t| {
                    t.get("children")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .filter_map(|c| {
                    c.get("source_range")?
                        .get("duration")
                        .and_then(time_of)
                        .map(|t| t.1)
                })
                .next()
        })
        .unwrap_or(project.settings.frame_rate.as_f64());
    let rate = rate_from_f64(fps);
    let mut ops: Vec<Op> = Vec::new();
    let mut known: HashMap<String, MediaRef> = project
        .media
        .values()
        .map(|m| (m.path.clone(), m.clone()))
        .collect();
    let mut tl = Timeline::new(
        Id::new(),
        doc.get("name")
            .and_then(Value::as_str)
            .unwrap_or("Imported"),
        rate,
    );
    for track in kids {
        let kind = match track.get("kind").and_then(Value::as_str) {
            Some("Audio") => TrackKind::Audio,
            _ => TrackKind::Video,
        };
        let mut t = Track::new(
            Id::new(),
            track.get("name").and_then(Value::as_str).unwrap_or("Track"),
            kind,
        );
        if let Some(e) = track.get("enabled").and_then(Value::as_bool) {
            t.enabled = e;
        }
        if let Some(g) = track
            .pointer("/metadata/captaine/gain_db")
            .and_then(Value::as_f64)
        {
            t.gain_db = g;
        }
        let mut pending: Option<Transition> = None;
        for child in track
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let schema = child
                .get("OTIO_SCHEMA")
                .and_then(Value::as_str)
                .unwrap_or("");
            let dur = |sr: &Value| sr.get("duration").and_then(time_of);
            if schema.starts_with("Transition") {
                let a = child.get("in_offset").and_then(time_of).map_or(0, |t| t.0);
                let b = child.get("out_offset").and_then(time_of).map_or(0, |t| t.0);
                let kind = child
                    .pointer("/metadata/captaine/kind")
                    .and_then(|k| serde_json::from_value(k.clone()).ok())
                    .unwrap_or(TransitionKind::Dissolve);
                pending = Some(Transition {
                    kind,
                    duration: (a + b).max(2),
                });
            } else if schema.starts_with("Gap") {
                let d = child
                    .get("source_range")
                    .and_then(dur)
                    .ok_or("a gap has no duration")?
                    .0;
                t.items.push(Item::Gap(Gap {
                    id: Id::new(),
                    duration: RationalTime::new(d, rate),
                }));
            } else if schema.starts_with("Clip") {
                let meta = child.pointer("/metadata/captaine");
                if let Some(title) = meta.and_then(|m| m.get("title")) {
                    let mut title: Title = serde_json::from_value(title.clone())
                        .map_err(|e| format!("bad title: {e}"))?;
                    title.id = Id::new();
                    title.duration = RationalTime::new(title.duration.value, rate);
                    t.items.push(Item::Title(title));
                    pending = None;
                    continue;
                }
                let sr = child
                    .get("source_range")
                    .ok_or("a clip has no source range")?;
                let (s_start, s_rate) = sr
                    .get("start_time")
                    .and_then(time_of)
                    .ok_or("a clip has no start")?;
                let (s_dur, _) = dur(sr).ok_or("a clip has no duration")?;
                let src_rate = rate_from_f64(s_rate);
                let reference = child.get("media_reference").ok_or("a clip has no media")?;
                let url = reference
                    .get("target_url")
                    .and_then(Value::as_str)
                    .ok_or("a clip's media has no file")?;
                let path = path_from_url(url);
                let media = match known.get(&path) {
                    Some(m) => m.clone(),
                    None => {
                        let m = captaine_media::media_ref(Path::new(&path), Id::new())
                            .map_err(|e| format!("cannot read {path}: {e}"))?;
                        ops.push(Op::AddMedia { media: m.clone() });
                        known.insert(path.clone(), m.clone());
                        m
                    }
                };
                // length on the timeline: ours if we wrote it, else the source length corrected for speed
                let mut retime = Retime::default();
                for fx in child
                    .get("effects")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let scalar = num(fx, "time_scalar").unwrap_or(1.0);
                    let schema = fx.get("OTIO_SCHEMA").and_then(Value::as_str).unwrap_or("");
                    if schema.starts_with("FreezeFrame") {
                        retime.freeze = true;
                    } else if schema.starts_with("LinearTimeWarp") && scalar != 0.0 {
                        retime.speed = scalar.abs();
                        retime.reverse = scalar < 0.0;
                    }
                }
                let own = meta.and_then(|m| m.get("duration")).and_then(Value::as_i64);
                let ticks = own.unwrap_or_else(|| {
                    let secs = s_dur as f64
                        / src_rate.as_f64()
                        / if retime.freeze { 1.0 } else { retime.speed };
                    ((secs * rate.as_f64()).round() as i64).max(1)
                });
                let mut c = Clip::new(
                    Id::new(),
                    child
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&media.name),
                    media.id,
                    RationalTime::new(s_start, src_rate),
                    RationalTime::new(ticks, rate),
                );
                c.retime = retime;
                if let Some(e) = child.get("enabled").and_then(Value::as_bool) {
                    c.enabled = e;
                }
                if let Some(m) = meta {
                    let get = |k: &str| m.get(k).cloned();
                    if let Some(v) = get("video").and_then(|v| serde_json::from_value(v).ok()) {
                        c.video = v;
                    }
                    if let Some(a) = get("audio").and_then(|v| serde_json::from_value(v).ok()) {
                        c.audio = a;
                    }
                    if let Some(a) = get("anim").and_then(|v| serde_json::from_value(v).ok()) {
                        c.anim = a;
                    }
                    if let Some(e) = get("effects").and_then(|v| serde_json::from_value(v).ok()) {
                        c.effects = e;
                    }
                }
                c.transition = pending.take();
                t.items.push(Item::Clip(c));
            }
        }
        tl.tracks.push(Arc::new(t));
    }
    ops.push(Op::AddTimeline { timeline: tl });
    Ok(ops)
}
