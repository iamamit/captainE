//! The agent's tools. Read tools describe the project; edit tools apply the same ops a person's gestures
//! produce, to a scratch copy. Everything the agent changed is recorded in `new_ops`.

use captaine_model::*;
use captaine_ops::{apply, edit, Author, Edge, Editor, Op, OpError};
use captaine_time::{Rational, RationalTime};
use serde_json::{json, Value};

use crate::silence::remove_silences;

pub struct ToolBox {
    ed: Editor,
    pub new_ops: Vec<Op>,
    selection: Option<Id>,
    playhead_secs: f64,
}

type R = Result<String, String>;

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn flag(v: &Value, key: &str) -> Option<bool> {
    v.get(key).and_then(Value::as_bool)
}

fn req_num(v: &Value, key: &str) -> Result<f64, String> {
    num(v, key).ok_or_else(|| format!("missing number '{key}'"))
}

fn req_text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    text(v, key).ok_or_else(|| format!("missing text '{key}'"))
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

impl ToolBox {
    /// A scratch copy of `project` with `base_ops` (an earlier, still-pending suggestion) already applied.
    pub fn new(
        project: &Project,
        base_ops: &[Op],
        selection: Option<Id>,
        playhead_secs: f64,
    ) -> Result<ToolBox, String> {
        let mut ed = Editor::new(project.clone());
        for op in base_ops {
            ed.apply(Author::Agent("copilot".into()), op.clone())
                .map_err(|e| format!("pending edit no longer applies: {e}"))?;
        }
        Ok(ToolBox {
            ed,
            new_ops: Vec::new(),
            selection,
            playhead_secs,
        })
    }

    pub fn project(&self) -> &Project {
        self.ed.project()
    }

    fn timeline(&self) -> Result<&Timeline, String> {
        self.project()
            .timelines
            .values()
            .next()
            .map(|t| t.as_ref())
            .ok_or_else(|| "the project has no timeline".to_string())
    }

    fn rate(&self) -> Rational {
        self.timeline().map_or(Rational::FPS_30, |t| t.rate)
    }

    fn fps(&self) -> f64 {
        self.rate().as_f64()
    }

    fn ticks(&self, secs: f64) -> i64 {
        (secs * self.fps()).round() as i64
    }

    fn secs(&self, ticks: i64) -> f64 {
        r2(ticks as f64 / self.fps())
    }

    fn time(&self, secs: f64) -> RationalTime {
        RationalTime::new(self.ticks(secs), self.rate())
    }

    fn commit(&mut self, op: Op) -> Result<(), String> {
        self.ed
            .apply(Author::Agent("copilot".into()), op.clone())
            .map_err(|e| e.to_string())?;
        self.new_ops.push(op);
        Ok(())
    }

    fn plan(&mut self, f: impl FnOnce(&Project) -> Result<Op, OpError>) -> Result<(), String> {
        let op = f(self.ed.project()).map_err(|e| e.to_string())?;
        self.commit(op)
    }

    // ---- lookups -----------------------------------------------------------------------------------

    fn find_track(&self, key: &str) -> Result<Track, String> {
        let tl = self.timeline()?;
        let hits: Vec<&Track> = tl
            .tracks
            .iter()
            .map(|t| t.as_ref())
            .filter(|t| t.name.eq_ignore_ascii_case(key) || t.id.to_string() == key)
            .collect();
        match hits.as_slice() {
            [t] => Ok((*t).clone()),
            [] => Err(format!(
                "no track '{key}'; tracks are: {}",
                tl.tracks
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            _ => Err(format!("track '{key}' is ambiguous")),
        }
    }

    /// `(item, track id, start tick)` for an item id (short or full form).
    fn find_item(&self, key: &str) -> Result<(Item, Id, i64), String> {
        let tl = self.timeline()?;
        let mut hits = Vec::new();
        for t in &tl.tracks {
            for (it, st) in t.items.iter().zip(t.item_starts()) {
                let (short, full) = (it.id().to_string(), it.id().to_full_string());
                if it.is_content()
                    && (short == key || full == key || (key.len() >= 4 && full.starts_with(key)))
                {
                    hits.push((it.clone(), t.id, st));
                }
            }
        }
        match hits.len() {
            1 => Ok(hits.remove(0)),
            0 => Err(format!(
                "no clip or title with id '{key}'; call get_timeline for the current ids"
            )),
            _ => Err(format!("id '{key}' is ambiguous")),
        }
    }

    fn find_media(&self, key: &str) -> Result<MediaRef, String> {
        let hits: Vec<&MediaRef> = self
            .project()
            .media
            .values()
            .filter(|m| m.id.to_string() == key || m.id.to_full_string() == key || m.name == key)
            .collect();
        match hits.as_slice() {
            [m] => Ok((*m).clone()),
            [] => Err(format!("no media '{key}'; call list_media")),
            _ => Err(format!("media '{key}' is ambiguous")),
        }
    }

    // ---- descriptions ------------------------------------------------------------------------------

    fn item_json(&self, item: &Item, start: i64) -> Value {
        let mut v = json!({
            "id": item.id().to_string(),
            "start_sec": self.secs(start),
            "duration_sec": self.secs(item.duration().value),
        });
        let o = v.as_object_mut().expect("object");
        match item {
            Item::Gap(_) => {
                o.insert("type".into(), json!("gap"));
            }
            Item::Clip(c) => {
                o.insert("type".into(), json!("clip"));
                o.insert("name".into(), json!(c.name));
                o.insert("media".into(), json!(c.media.to_string()));
                o.insert(
                    "source_start_sec".into(),
                    json!(r2(c.source_start.to_seconds_f64())),
                );
                if !c.enabled {
                    o.insert("enabled".into(), json!(false));
                }
                self.video_props(o, &c.video);
                if c.audio.gain_db != 0.0 {
                    o.insert("volume_db".into(), json!(r2(c.audio.gain_db)));
                }
                if c.audio.pan != 0.0 {
                    o.insert("pan".into(), json!(r2(c.audio.pan)));
                }
                if c.audio.muted {
                    o.insert("muted".into(), json!(true));
                }
                if c.audio.fade_in > 0 {
                    o.insert("fade_in_sec".into(), json!(self.secs(c.audio.fade_in)));
                }
                if c.audio.fade_out > 0 {
                    o.insert("fade_out_sec".into(), json!(self.secs(c.audio.fade_out)));
                }
            }
            Item::Title(t) => {
                o.insert("type".into(), json!("title"));
                o.insert("name".into(), json!(t.name));
                o.insert("text".into(), json!(t.text));
                if !t.subtitle.is_empty() {
                    o.insert("subtitle".into(), json!(t.subtitle));
                }
                o.insert("template".into(), json!(template_name(t.style.template)));
                o.insert("size".into(), json!(t.style.size));
                if !t.enabled {
                    o.insert("enabled".into(), json!(false));
                }
                self.video_props(o, &t.video);
                if t.fade_in > 0 {
                    o.insert("fade_in_sec".into(), json!(self.secs(t.fade_in)));
                }
                if t.fade_out > 0 {
                    o.insert("fade_out_sec".into(), json!(self.secs(t.fade_out)));
                }
            }
        }
        v
    }

    /// Only the picture properties that differ from the defaults.
    fn video_props(&self, o: &mut serde_json::Map<String, Value>, v: &VideoProps) {
        let d = VideoProps::default();
        if v.opacity != d.opacity {
            o.insert("opacity".into(), json!(r2(v.opacity)));
        }
        if v.transform.position != d.transform.position {
            o.insert(
                "position".into(),
                json!([r2(v.transform.position.0), r2(v.transform.position.1)]),
            );
        }
        if v.transform.scale != d.transform.scale {
            o.insert(
                "scale_percent".into(),
                json!(r2(v.transform.scale.0 * 100.0)),
            );
        }
        if v.transform.rotation_deg != 0.0 {
            o.insert("rotation_deg".into(), json!(r2(v.transform.rotation_deg)));
        }
        if v.color != d.color {
            o.insert("color".into(), json!({"exposure": r2(v.color.exposure), "contrast": r2(v.color.contrast), "saturation": r2(v.color.saturation), "temperature_k": v.color.temperature.round()}));
        }
    }

    fn timeline_json(&self) -> Result<Value, String> {
        let tl = self.timeline()?;
        let tracks: Vec<Value> = tl
            .tracks
            .iter()
            .map(|t| {
                let items: Vec<Value> = t.items.iter().zip(t.item_starts()).map(|(i, s)| self.item_json(i, s)).collect();
                let mut o = json!({"name": t.name, "id": t.id.to_string(), "kind": if t.kind == TrackKind::Video { "video" } else { "audio" }, "items": items});
                if !t.enabled {
                    o["enabled"] = json!(false);
                }
                if t.gain_db != 0.0 {
                    o["volume_db"] = json!(t.gain_db);
                }
                o
            })
            .collect();
        let s = &self.project().settings;
        Ok(json!({
            "fps": r2(self.fps()),
            "canvas": [s.width, s.height],
            "duration_sec": self.secs(tl.duration().value),
            "playhead_sec": r2(self.playhead_secs),
            "selected_item": self.selection.map(|i| i.to_string()),
            "tracks": tracks,
            "markers": tl.markers.iter().map(|m| json!({"id": m.id.to_string(), "at_sec": r2(self.secs(m.tick)), "name": m.name, "note": m.note, "length_sec": r2(self.secs(m.duration))})).collect::<Vec<_>>(),
            "note": "track order is bottom to top for video; later video tracks draw over earlier ones",
        }))
    }

    pub fn state_summary(&self) -> Value {
        let tl = match self.timeline() {
            Ok(t) => t,
            Err(_) => return json!({"timeline": null}),
        };
        json!({
            "fps": r2(self.fps()),
            "duration_sec": self.secs(tl.duration().value),
            "playhead_sec": r2(self.playhead_secs),
            "selected_item": self.selection.map(|i| i.to_string()),
            "tracks": tl.tracks.iter().map(|t| json!({"name": t.name, "kind": if t.kind == TrackKind::Video {"video"} else {"audio"}, "items": t.items.iter().filter(|i| i.is_content()).count()})).collect::<Vec<_>>(),
            "media_items": self.project().media.len(),
        })
    }

    // ---- dispatch ----------------------------------------------------------------------------------

    pub fn call(&mut self, name: &str, input: &Value) -> R {
        match name {
            "get_timeline" => Ok(self.timeline_json()?.to_string()),
            "list_media" => self.list_media(),
            "find_silences" => self.find_silences(input),
            "split" => self.split(input),
            "delete_item" => self.delete_item(input),
            "trim_item" => self.trim_item(input),
            "move_item" => self.move_item(input),
            "add_clip" => self.add_clip(input),
            "set_clip" => self.set_clip(input),
            "set_keyframes" => self.set_keyframes(input),
            "set_speed" => self.set_speed(input),
            "set_transition" => self.set_transition(input),
            "set_effects" => self.set_effects(input),
            "add_layer" => self.add_layer(input),
            "measure_loudness" => self.measure_loudness_tool(input),
            "normalize_loudness" => self.normalize_tool(input),
            "learn_noise_reduction" => self.learn_noise_tool(input),
            "sync_by_audio" => self.sync_tool(input),
            "multicam_cut" => self.multicam_tool(input),
            "auto_color" => self.auto_color_tool(input),
            "match_color" => self.match_color_tool(input),
            "stabilize" => self.stabilize_tool(input),
            "attach_title_to_subject" => self.attach_title_tool(input),
            "reframe_sequence" => self.reframe_tool(input),
            "remove_background" => self.remove_background_tool(input),
            "animate_title" => self.animate_title(input),
            "add_marker" => self.add_marker_tool(input),
            "remove_marker" => self.remove_marker_tool(input),
            "transcribe" => self.transcribe_tool(input),
            "remove_filler_words" => self.remove_fillers_tool(input),
            "add_captions" => self.add_captions_tool(input),
            "split_at_scenes" => self.split_at_scenes_tool(input),
            "add_title" => self.add_title(input),
            "edit_title" => self.edit_title(input),
            "remove_silences" => self.remove_silences_tool(input),
            "set_track" => self.set_track(input),
            other => Err(format!("unknown tool '{other}'")),
        }
    }

    fn list_media(&self) -> R {
        let mut m: Vec<&MediaRef> = self.project().media.values().collect();
        m.sort_by(|a, b| a.name.cmp(&b.name));
        let v: Vec<Value> = m
            .iter()
            .map(|m| {
                json!({
                    "id": m.id.to_string(), "name": m.name,
                    "duration_sec": m.duration.map(|d| r2(d.to_seconds_f64())),
                    "has_video": m.has_video, "has_audio": m.has_audio,
                    "size": if m.has_video { json!([m.width, m.height]) } else { Value::Null },
                })
            })
            .collect();
        Ok(Value::Array(v).to_string())
    }

    fn clip_with_media(&self, key: &str) -> Result<(Clip, MediaRef, i64), String> {
        let (item, _, start) = self.find_item(key)?;
        let Item::Clip(c) = item else {
            return Err("that item is a title, not a clip with audio".into());
        };
        let m = self
            .project()
            .media
            .get(&c.media)
            .cloned()
            .ok_or("the clip's media is missing")?;
        Ok((c, m, start))
    }

    fn find_silences(&self, input: &Value) -> R {
        let (clip, media, start) = self.clip_with_media(req_text(input, "item_id")?)?;
        if !media.has_audio {
            return Err(format!("{} has no audio to analyse", clip.name));
        }
        let thr = num(input, "threshold_db").unwrap_or(-42.0) as f32;
        let min = num(input, "min_seconds").unwrap_or(0.45);
        let found = captaine_audio::find_silences(
            &media.path,
            clip.source_start.to_seconds_f64(),
            clip.duration.value as f64 / self.fps(),
            thr,
            min,
        )
        .map_err(|e| e.to_string())?;
        let t0 = start as f64 / self.fps();
        let v: Vec<Value> = found.iter().map(|(a, b)| json!({"start_sec": r2(t0 + a), "end_sec": r2(t0 + b), "length_sec": r2(b - a)})).collect();
        Ok(json!({"silences": v, "threshold_db": thr, "min_seconds": min}).to_string())
    }

    fn split(&mut self, input: &Value) -> R {
        let track = self.find_track(req_text(input, "track")?)?;
        let at = req_num(input, "at_sec")?;
        let at_t = self.time(at);
        self.plan(|p| edit::split_at(p, track.id, at_t))?;
        let t = self.find_track(&track.name)?;
        let near: Vec<Value> = t
            .items
            .iter()
            .zip(t.item_starts())
            .filter(|(i, s)| (*s - at_t.value).abs() <= i.duration().value && i.is_content())
            .map(|(i, s)| self.item_json(i, s))
            .collect();
        Ok(json!({"ok": true, "items_around_the_cut": near}).to_string())
    }

    fn delete_item(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        let ripple = flag(input, "ripple").unwrap_or(false);
        if ripple {
            self.commit(Op::RemoveItem { item: item.id() })?;
        } else {
            self.plan(|p| edit::lift(p, item.id()))?;
        }
        Ok(json!({"ok": true, "mode": if ripple { "ripple (later items moved earlier)" } else { "lift (a gap was left)" }, "timeline_duration_sec": self.secs(self.timeline()?.duration().value)}).to_string())
    }

    fn trim_item(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        let edge = match req_text(input, "edge")? {
            "in" => Edge::In,
            "out" => Edge::Out,
            other => return Err(format!("edge must be 'in' or 'out', not '{other}'")),
        };
        let delta = self.time(req_num(input, "delta_sec")?);
        self.commit(Op::Trim {
            item: item.id(),
            edge,
            delta,
        })?;
        self.describe(item.id())
    }

    fn move_item(&mut self, input: &Value) -> R {
        let (item, from, _) = self.find_item(req_text(input, "item_id")?)?;
        let to = match text(input, "to_track") {
            Some(k) => self.find_track(k)?.id,
            None => from,
        };
        let at = self.time(req_num(input, "at_sec")?);
        self.plan(|p| edit::move_item(p, item.id(), to, at))?;
        self.describe(item.id())
    }

    fn describe(&self, id: Id) -> R {
        let key = id.to_string();
        let (item, _, start) = self.find_item(&key)?;
        Ok(self.item_json(&item, start).to_string())
    }

    fn add_clip(&mut self, input: &Value) -> R {
        let m = self.find_media(req_text(input, "media_id")?)?;
        let tl = self.timeline()?.clone();
        let kind = if m.has_video {
            TrackKind::Video
        } else {
            TrackKind::Audio
        };
        let track = match text(input, "track") {
            Some(k) => self.find_track(k)?,
            None => tl
                .tracks
                .iter()
                .find(|t| t.kind == kind)
                .map(|t| t.as_ref().clone())
                .ok_or("there is no suitable track")?,
        };
        if (track.kind == TrackKind::Video && !m.has_video)
            || (track.kind == TrackKind::Audio && !m.has_audio)
        {
            return Err(format!("{} cannot go on track {}", m.name, track.name));
        }
        let len = m.duration.ok_or("media length unknown")?;
        let from = num(input, "from_sec").unwrap_or(0.0).max(0.0);
        let src_rate = len.rate;
        let start = RationalTime::new((from * src_rate.as_f64()).round() as i64, src_rate);
        let avail = len.to_seconds_f64() - from;
        let dur = num(input, "duration_sec").unwrap_or(avail).min(avail);
        if dur <= 0.0 {
            return Err("nothing left of that media after from_sec".into());
        }
        let clip = Clip::new(Id::new(), m.name.clone(), m.id, start, self.time(dur));
        let id = clip.id;
        match num(input, "at_sec") {
            Some(at) => {
                let at = self.time(at);
                self.plan(|p| edit::overwrite(p, track.id, at, clip))?
            }
            None => self.commit(edit::append(self.project(), track.id, clip))?,
        }
        self.describe(id)
    }

    fn set_clip(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        let id = item.id();
        let mut ops = Vec::new();
        let (video, audio, is_clip) = match &item {
            Item::Clip(c) => (c.video, Some(c.audio), true),
            Item::Title(t) => (t.video, None, false),
            Item::Gap(_) => return Err("that is a gap".into()),
        };
        let mut v = video;
        if let Some(x) = num(input, "opacity") {
            v.opacity = x.clamp(0.0, 1.0);
        }
        if let Some(x) = num(input, "position_x") {
            v.transform.position.0 = x;
        }
        if let Some(y) = num(input, "position_y") {
            v.transform.position.1 = y;
        }
        if let Some(p) = num(input, "scale_percent") {
            v.transform.scale = (p / 100.0, p / 100.0);
        }
        if let Some(r) = num(input, "rotation_deg") {
            v.transform.rotation_deg = r;
        }
        if let Some(x) = num(input, "exposure") {
            v.color.exposure = x;
        }
        if let Some(x) = num(input, "contrast") {
            v.color.contrast = x;
        }
        if let Some(x) = num(input, "saturation") {
            v.color.saturation = x;
        }
        if let Some(x) = num(input, "temperature_k") {
            v.color.temperature = x;
        }
        if let Some(x) = num(input, "tint") {
            v.color.tint = x;
        }
        if let Some(x) = num(input, "highlights") {
            v.color.highlights = x;
        }
        if let Some(x) = num(input, "shadows") {
            v.color.shadows = x;
        }
        if let Some(x) = num(input, "vibrance") {
            v.color.vibrance = x;
        }
        if v != video {
            ops.push(Op::SetVideo { item: id, video: v });
        }
        if let Some(mut a) = audio {
            let before = a;
            if let Some(x) = num(input, "volume_db") {
                a.gain_db = x;
            }
            if let Some(x) = num(input, "pan") {
                a.pan = x.clamp(-1.0, 1.0);
            }
            if let Some(m) = flag(input, "muted") {
                a.muted = m;
            }
            if let Some(f) = num(input, "fade_in_sec") {
                a.fade_in = self.ticks(f).max(0);
            }
            if let Some(f) = num(input, "fade_out_sec") {
                a.fade_out = self.ticks(f).max(0);
            }
            if a != before {
                ops.push(Op::SetAudio { item: id, audio: a });
            }
        } else if input.get("volume_db").is_some()
            || input.get("pan").is_some()
            || input.get("muted").is_some()
        {
            return Err("titles have no sound".into());
        }
        if let Some(n) = text(input, "name") {
            ops.push(Op::SetName {
                item: id,
                name: n.to_string(),
            });
        }
        if let Some(e) = flag(input, "enabled") {
            ops.push(Op::SetEnabled {
                item: id,
                enabled: e,
            });
        }
        let _ = is_clip;
        if ops.is_empty() {
            return Err("nothing to change: pass at least one property".into());
        }
        self.commit(Op::Transaction {
            description: "Copilot: set properties".into(),
            ops,
        })?;
        self.describe(id)
    }

    fn set_keyframes(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        let prop = req_text(input, "property")?.to_string();
        if !is_property(&prop) {
            return Err(format!(
                "unknown property '{prop}'; use one of {}",
                PROPERTIES
                    .iter()
                    .map(|p| p.0)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let mut keys = Vec::new();
        for k in input
            .get("keys")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let at =
                num(k, "at_sec").ok_or("each key needs at_sec (seconds from the item's start)")?;
            let v = num(k, "value").ok_or("each key needs a value")?;
            let interp = match text(k, "interp").unwrap_or("linear") {
                "hold" => Interp::Hold,
                "linear" => Interp::Linear,
                "ease_in" => Interp::EaseIn,
                "ease_out" => Interp::EaseOut,
                "ease_in_out" => Interp::EaseInOut,
                other => return Err(format!("unknown interp '{other}'")),
            };
            keys.push(Keyframe {
                t: self.ticks(at).max(0),
                v,
                interp,
            });
        }
        keys.sort_by_key(|k| k.t);
        self.commit(Op::SetAnimTrack {
            item: item.id(),
            prop,
            keys,
        })?;
        self.describe(item.id())
    }

    fn set_speed(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        let Item::Clip(c) = &item else {
            return Err("only clips can change speed".into());
        };
        let mut rt = c.retime;
        if let Some(p) = num(input, "speed_percent") {
            rt.speed = p / 100.0;
        }
        if let Some(r) = flag(input, "reverse") {
            rt.reverse = r;
        }
        if let Some(f) = flag(input, "freeze") {
            rt.freeze = f;
        }
        if rt == c.retime {
            return Err("nothing to change: pass speed_percent, reverse or freeze".into());
        }
        self.commit(Op::SetRetime {
            item: item.id(),
            retime: rt,
        })?;
        self.describe(item.id())
    }

    fn set_transition(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        if !matches!(item, Item::Clip(_)) {
            return Err("transitions go before a clip".into());
        }
        let kind = match text(input, "kind").unwrap_or("dissolve") {
            "none" => None,
            "dissolve" => Some(TransitionKind::Dissolve),
            "dip" => Some(TransitionKind::Dip),
            "wipe_right" => Some(TransitionKind::WipeRight),
            "wipe_left" => Some(TransitionKind::WipeLeft),
            "push_left" => Some(TransitionKind::PushLeft),
            "push_right" => Some(TransitionKind::PushRight),
            other => return Err(format!("unknown transition '{other}'")),
        };
        let transition = kind.map(|kind| Transition {
            kind,
            duration: self.ticks(num(input, "duration_sec").unwrap_or(1.0)).max(2),
        });
        if let Some(t) = &transition {
            captaine_ops::check_transition(self.project(), item.id(), t)
                .map_err(|e| e.to_string())?;
        }
        self.commit(Op::SetTransition {
            item: item.id(),
            transition,
        })?;
        self.describe(item.id())
    }

    fn set_effects(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        if matches!(item, Item::Gap(_)) {
            return Err("effects go on clips and layers".into());
        }
        let mut list = Vec::new();
        for e in input
            .get("effects")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let name = req_text(e, "kind")?;
            let kind = EffectKind::from_key(name).ok_or_else(|| {
                format!(
                    "unknown effect '{name}'; use one of {}",
                    EffectKind::ALL
                        .iter()
                        .map(|k| k.key())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            let mut fx = Effect::new(kind);
            if let Some(en) = flag(e, "enabled") {
                fx.enabled = en;
            }
            if let Some(path) = text(e, "path") {
                fx.text = path.to_string();
            }
            if let Some(ps) = e.get("params").and_then(Value::as_object) {
                for (k, v) in ps {
                    let idx = kind
                        .params()
                        .iter()
                        .position(|d| d.name.to_lowercase().replace(' ', "_") == *k)
                        .ok_or_else(|| {
                            format!(
                                "{name} has no parameter '{k}'; it has {}",
                                kind.params()
                                    .iter()
                                    .map(|d| d.name.to_lowercase().replace(' ', "_"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        })?;
                    fx.params[idx] = v.as_f64().ok_or("parameters are numbers")?;
                }
            }
            fx.check().map_err(|m| format!("{name}: {m}"))?;
            list.push(fx);
        }
        self.commit(Op::SetEffects {
            item: item.id(),
            effects: list,
        })?;
        self.describe(item.id())
    }

    fn split_at_scenes_tool(&mut self, input: &Value) -> R {
        let (clip, media, start) = self.clip_with_media(req_text(input, "item_id")?)?;
        let thr = num(input, "threshold").unwrap_or(0.3).clamp(0.05, 0.95);
        let min = num(input, "min_seconds").unwrap_or(0.5);
        let (ops, secs) = crate::scenes::split_at_scenes(&clip, &media, self.fps(), thr, min)?;
        if ops.is_empty() {
            return Ok(json!({"ok": true, "cuts": 0, "note": "no scene changes found; try a lower threshold"}).to_string());
        }
        self.commit(Op::Transaction {
            description: "Copilot: split at scene changes".into(),
            ops,
        })?;
        let t0 = start as f64 / self.fps();
        let at: Vec<Value> = secs.iter().map(|s| json!(r2(t0 + s))).collect();
        Ok(json!({"ok": true, "cuts": secs.len(), "cut_times_sec": at}).to_string())
    }

    fn add_layer(&mut self, input: &Value) -> R {
        let kind = match req_text(input, "kind")? {
            "rectangle" => Some(ShapeKind::Rectangle),
            "ellipse" => Some(ShapeKind::Ellipse),
            "solid" => Some(ShapeKind::Solid),
            "adjustment" => None,
            other => return Err(format!("unknown layer kind '{other}'")),
        };
        let tl = self.timeline()?.clone();
        let track = match text(input, "track") {
            Some(k) => self.find_track(k)?,
            None => tl
                .tracks
                .iter()
                .rev()
                .find(|t| t.kind == TrackKind::Video)
                .map(|t| t.as_ref().clone())
                .ok_or("there is no video track")?,
        };
        if track.kind != TrackKind::Video {
            return Err("layers go on video tracks".into());
        }
        let dur = num(input, "duration_sec").unwrap_or(5.0);
        if dur <= 0.0 {
            return Err("duration_sec must be positive".into());
        }
        let mut layer = Title::generator(Id::new(), kind, self.time(dur));
        if let Some(sh) = layer.shape.as_mut() {
            if let Some(c) = text(input, "color") {
                let hex = c.trim_start_matches('#');
                let v = u32::from_str_radix(hex, 16)
                    .map_err(|_| "color is a hex value like #ff8800")?;
                if hex.len() != 6 {
                    return Err("color is a hex value like #ff8800".into());
                }
                sh.color = [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255];
            }
            if let Some(w) = num(input, "width_percent") {
                sh.width = w;
            }
            if let Some(h) = num(input, "height_percent") {
                sh.height = h;
            }
            sh.check().map_err(|e| e.to_string())?;
        }
        let id = layer.id;
        let at = self.time(req_num(input, "at_sec")?);
        self.plan(|p| edit::overwrite_item(p, track.id, at, Item::Title(layer)))?;
        self.describe(id)
    }

    fn transcribe_tool(&mut self, input: &Value) -> R {
        let (clip, media, start) = self.clip_with_media(req_text(input, "item_id")?)?;
        let segs = crate::speech::clip_transcript(&clip, &media, text(input, "language"))?;
        let t0 = start as f64 / self.fps();
        let v: Vec<Value> = segs
            .iter()
            .map(|s| json!({"start_sec": r2(t0 + s.start), "end_sec": r2(t0 + s.end), "text": s.text}))
            .collect();
        Ok(json!({"segments": v}).to_string())
    }

    fn remove_fillers_tool(&mut self, input: &Value) -> R {
        let (clip, media, start) = self.clip_with_media(req_text(input, "item_id")?)?;
        let segs = crate::speech::clip_transcript(&clip, &media, text(input, "language"))?;
        let strict = flag(input, "strict").unwrap_or(false);
        let ranges = captaine_speech::filler_ranges(&segs, strict, 0.04);
        let (ops, removed) = crate::speech::cut_ranges(&clip, self.fps(), &ranges);
        if ops.is_empty() {
            return Ok(json!({"ok": true, "cuts": 0, "note": "no filler words found"}).to_string());
        }
        self.commit(Op::Transaction {
            description: "Copilot: remove filler words".into(),
            ops,
        })?;
        let t0 = start as f64 / self.fps();
        let at: Vec<Value> = ranges
            .iter()
            .map(|(a, b)| json!([r2(t0 + a), r2(t0 + b)]))
            .collect();
        Ok(json!({"ok": true, "cuts": ranges.len(), "removed_sec": r2(removed), "removed_ranges_in_original_timeline_sec": at}).to_string())
    }

    fn add_captions_tool(&mut self, input: &Value) -> R {
        let (clip, media, start) = self.clip_with_media(req_text(input, "item_id")?)?;
        let segs = crate::speech::clip_transcript(&clip, &media, text(input, "language"))?;
        let max_chars = num(input, "max_chars").unwrap_or(42.0).clamp(10.0, 120.0) as usize;
        let lines = captaine_speech::caption_lines(&segs, max_chars, 6.0);
        if lines.is_empty() {
            return Ok(json!({"ok": true, "captions": 0, "note": "no speech found"}).to_string());
        }
        let tl = self.timeline()?.id;
        let ops = captaine_interchange::srt::caption_ops(
            self.project(),
            tl,
            &lines,
            start as f64 / self.fps(),
        )?;
        self.commit(Op::Transaction {
            description: "Copilot: add captions".into(),
            ops,
        })?;
        Ok(json!({"ok": true, "captions": lines.len(), "track": captaine_interchange::srt::CAPTION_TRACK}).to_string())
    }

    fn add_marker_tool(&mut self, input: &Value) -> R {
        let tl = self.timeline()?.id;
        let tick = self.ticks(req_num(input, "at_sec")?).max(0);
        let mut m = Marker::new(Id::new(), tick, req_text(input, "name")?);
        m.note = text(input, "note").unwrap_or("").to_string();
        if let Some(d) = num(input, "duration_sec") {
            m.duration = self.ticks(d).max(0);
        }
        if let Some(c) = text(input, "color") {
            m.color = Marker::COLORS
                .iter()
                .position(|(n, _)| n.eq_ignore_ascii_case(c))
                .ok_or("color is one of blue, green, yellow, red, purple, cyan")?
                as u8;
        }
        let id = m.id;
        self.commit(Op::AddMarker {
            timeline: tl,
            marker: m,
        })?;
        Ok(json!({"ok": true, "marker_id": id.to_string()}).to_string())
    }

    fn remove_marker_tool(&mut self, input: &Value) -> R {
        let tl = self.timeline()?.clone();
        let key = req_text(input, "marker_id")?;
        let m = tl
            .markers
            .iter()
            .find(|m| m.id.to_string() == key || m.id.to_full_string() == key || m.name == key)
            .ok_or_else(|| format!("no marker '{key}'"))?;
        self.commit(Op::RemoveMarker {
            timeline: tl.id,
            marker: m.id,
        })?;
        Ok(json!({"ok": true}).to_string())
    }

    fn animate_title(&mut self, input: &Value) -> R {
        use captaine_model::anim_presets::{apply, tracks, Preset};
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        let Item::Title(t) = item else {
            return Err("only titles and layers can use animation presets".into());
        };
        let name = req_text(input, "preset")?;
        let preset = Preset::from_key(name).ok_or_else(|| {
            format!(
                "unknown preset '{name}'; use one of {}",
                Preset::ALL
                    .iter()
                    .map(|p| p.key())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        let entering = match text(input, "direction").unwrap_or("in") {
            "in" => true,
            "out" => false,
            other => return Err(format!("direction is 'in' or 'out', not '{other}'")),
        };
        if !entering && preset == Preset::Typewriter {
            return Err("the typewriter only comes on".into());
        }
        let total = t.duration.value;
        let len = self
            .ticks(num(input, "duration_sec").unwrap_or(0.6))
            .clamp(1, total);
        let (cw, ch) = self.project().canvas(self.timeline()?.id);
        let canvas = (cw as f64, ch as f64);
        let new = tracks(preset, entering, len, total, &t.video, canvas);
        let props: Vec<&str> = new.iter().map(|(p, _)| *p).collect();
        let mut anim = t.anim.clone();
        apply(&mut anim, new, entering, len, total);
        let ops = props
            .into_iter()
            .map(|p| Op::SetAnimTrack {
                item: t.id,
                prop: p.to_string(),
                keys: anim.keys(p).to_vec(),
            })
            .collect();
        self.commit(Op::Transaction {
            description: format!("Copilot: {name} {}", if entering { "in" } else { "out" }),
            ops,
        })?;
        self.describe(t.id)
    }

    fn remove_background_tool(&mut self, input: &Value) -> R {
        let (clip, media, _) = self.clip_with_media(req_text(input, "item_id")?)?;
        let mode = match text(input, "mode").unwrap_or("subject") {
            "subject" => captaine_vision::MatteMode::Subject,
            "person" => captaine_vision::MatteMode::Person,
            other => return Err(format!("mode is 'subject' or 'person', not '{other}'")),
        };
        let fx = crate::matte::remove_background(&clip, &media, mode)?;
        let mut effects = clip.effects.clone();
        effects.retain(|e| e.kind != EffectKind::Matte);
        effects.insert(0, fx);
        self.commit(Op::SetEffects {
            item: clip.id,
            effects,
        })?;
        self.describe(clip.id)
    }

    fn attach_title_tool(&mut self, input: &Value) -> R {
        let (clip, media, cs) = self.clip_with_media(req_text(input, "clip_id")?)?;
        let (item, _, ts) = self.find_item(req_text(input, "title_id")?)?;
        let Item::Title(title) = item else {
            return Err("title_id must be a title or a layer".into());
        };
        let tl = self.timeline()?.id;
        let canvas = self.project().canvas(tl);
        let ops =
            crate::follow::track_attach(&clip, &media, &title, cs, ts, canvas, self.fps(), None)?;
        let n = ops.len();
        self.commit(Op::Transaction {
            description: "Copilot: attach title to subject".into(),
            ops,
        })?;
        Ok(json!({"ok": true, "tracks_written": n, "title": title.name}).to_string())
    }

    fn reframe_tool(&mut self, input: &Value) -> R {
        let aspect = req_text(input, "aspect")?;
        let (label, ratio) = match aspect {
            "9:16" => ("9:16", (9u32, 16u32)),
            "1:1" => ("1:1", (1, 1)),
            "4:5" => ("4:5", (4, 5)),
            other => return Err(format!("aspect is 9:16, 1:1 or 4:5, not '{other}'")),
        };
        let cur = self.timeline()?.clone();
        let s = &self.project().settings;
        let short = s.width.min(s.height).max(2);
        let even = |v: f64| ((v.round() as u32).max(2)) & !1;
        let canvas = (
            even(short as f64),
            even(short as f64 * ratio.1 as f64 / ratio.0 as f64),
        );
        let mut copy = cur.duplicate(format!("{} ({label})", cur.name));
        copy.canvas = Some(canvas);
        let mut ops = vec![Op::AddTimeline {
            timeline: copy.clone(),
        }];
        let fps = self.fps();
        for track in copy.tracks.iter().filter(|t| t.kind == TrackKind::Video) {
            for item in &track.items {
                let Item::Clip(c) = item else { continue };
                let Some(m) = self
                    .project()
                    .media
                    .get(&c.media)
                    .filter(|m| m.has_video)
                    .cloned()
                else {
                    continue;
                };
                ops.extend(crate::follow::reframe(c, &m, canvas, fps)?);
            }
        }
        self.commit(Op::Transaction {
            description: format!("Copilot: reframe to {label}"),
            ops,
        })?;
        Ok(
            json!({"ok": true, "new_sequence": copy.name, "frame": [canvas.0, canvas.1]})
                .to_string(),
        )
    }

    fn stabilize_tool(&mut self, input: &Value) -> R {
        let (clip, media, _) = self.clip_with_media(req_text(input, "item_id")?)?;
        let smoothing = num(input, "smoothing").unwrap_or(15.0).clamp(1.0, 60.0) as u32;
        let ops = crate::stabilize::stabilize(&clip, &media, smoothing)?;
        self.commit(Op::Transaction {
            description: "Copilot: stabilize".into(),
            ops,
        })?;
        self.describe(clip.id)
    }

    fn auto_color_tool(&mut self, input: &Value) -> R {
        let (clip, media, _) = self.clip_with_media(req_text(input, "item_id")?)?;
        let color = crate::color::auto_color(&clip, &media)?;
        let mut video = clip.video;
        video.color = color;
        self.commit(Op::SetVideo {
            item: clip.id,
            video,
        })?;
        Ok(json!({"ok": true, "exposure": r2(color.exposure), "contrast": r2(color.contrast), "saturation": r2(color.saturation), "temperature_k": color.temperature.round()}).to_string())
    }

    fn match_color_tool(&mut self, input: &Value) -> R {
        let (clip, media, _) = self.clip_with_media(req_text(input, "item_id")?)?;
        let (reference, ref_media, _) = self.clip_with_media(req_text(input, "reference_id")?)?;
        let color = crate::color::match_color(&clip, &media, &reference, &ref_media)?;
        let mut video = clip.video;
        video.color = color;
        self.commit(Op::SetVideo {
            item: clip.id,
            video,
        })?;
        Ok(json!({"ok": true, "exposure": r2(color.exposure), "contrast": r2(color.contrast), "saturation": r2(color.saturation), "temperature_k": color.temperature.round()}).to_string())
    }

    fn sync_tool(&mut self, input: &Value) -> R {
        let ids: Vec<&str> = input
            .get("item_ids")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if ids.len() < 2 {
            return Err(
                "give at least two clips: the first is the one the others line up with".into(),
            );
        }
        let (first, fm, first_start) = self.clip_with_media(ids[0])?;
        let fps = self.fps();
        let mut moved = Vec::new();
        for key in &ids[1..] {
            let (c, m, _) = self.clip_with_media(key)?;
            let (_, track, current) = self.find_item(key)?;
            let window = (first.duration.value.min(c.duration.value) as f64 / fps).clamp(2.0, 40.0);
            let al = captaine_audio::find_offset(
                &fm.path,
                first.source_start.to_seconds_f64(),
                &m.path,
                c.source_start.to_seconds_f64(),
                window,
                30.0,
            )
            .map_err(|e| e.to_string())?;
            if al.confidence < 3.5 {
                moved.push(json!({"clip": c.name, "moved": false, "reason": "no clear match in the sound"}));
                continue;
            }
            let new_start = (first_start + (al.offset_secs * fps).round() as i64).max(0);
            if new_start != current {
                let at = RationalTime::new(new_start, self.rate());
                self.plan(|p| edit::move_item(p, c.id, track, at))?;
            }
            moved.push(json!({"clip": c.name, "moved": new_start != current, "to_sec": r2(self.secs(new_start)), "confidence": r2(al.confidence)}));
        }
        Ok(json!({"ok": true, "clips": moved}).to_string())
    }

    fn multicam_tool(&mut self, input: &Value) -> R {
        let names: Vec<String> = input
            .get("angle_tracks")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        if names.len() < 2 {
            return Err(
                "angle_tracks lists the track names of the camera angles, at least two".into(),
            );
        }
        let mut angles = Vec::new();
        for n in &names {
            angles.push(self.find_track(n)?.id);
        }
        let angle = req_num(input, "angle")? as usize;
        if angle == 0 || angle > angles.len() {
            return Err(format!("angle is 1 to {}", angles.len()));
        }
        let at = self.time(req_num(input, "at_sec")?);
        self.plan(|p| edit::multicam_cut(p, &angles, angle - 1, at))?;
        Ok(json!({"ok": true, "now_showing": names[angle - 1]}).to_string())
    }

    fn measure_loudness_tool(&mut self, input: &Value) -> R {
        let l = match text(input, "item_id") {
            Some(k) => {
                let (clip, ..) = self.clip_with_media(k)?;
                crate::loudness::clip_loudness(self.project(), &clip)?
            }
            None => crate::loudness::sequence_loudness(self.project(), self.timeline()?.id)?,
        };
        Ok(json!({
            "integrated_lufs": r2(l.integrated_lufs),
            "loudness_range_lu": r2(l.loudness_range_lu),
            "sample_peak_db": r2(l.sample_peak_db),
            "true_peak_db": r2(l.true_peak_db),
        })
        .to_string())
    }

    fn learn_noise_tool(&mut self, input: &Value) -> R {
        let (clip, media, _) = self.clip_with_media(req_text(input, "item_id")?)?;
        if !media.has_audio {
            return Err(format!("{} has no audio to learn from", clip.name));
        }
        let from = num(input, "from_sec")
            .ok_or("from_sec is required")?
            .max(0.0);
        let to = num(input, "to_sec").ok_or("to_sec is required")?;
        let len = clip.duration.value as f64 / self.fps();
        if to <= from || from >= len {
            return Err(format!("pick a stretch inside the clip (0 to {len:.1} s)"));
        }
        let sr = self.project().settings.sample_rate;
        let count = ((to.min(len) - from) * sr as f64) as usize;
        let mono = captaine_audio::decode_mono_window(
            &media.path,
            clip.source_start.to_seconds_f64() + from,
            count,
            sr,
        )
        .map_err(|e| e.to_string())?;
        let profile = captaine_audio::learn_noise_profile(&mono)?;
        let mut effects = clip.effects.clone();
        let mut fx = match effects
            .iter()
            .position(|e| e.kind == EffectKind::NoiseReduction)
        {
            Some(i) => effects.remove(i),
            None => Effect::new(EffectKind::NoiseReduction),
        };
        fx.text = profile;
        fx.enabled = true;
        if let Some(v) = num(input, "reduction_db") {
            fx.params[0] = v;
        }
        if let Some(v) = num(input, "sensitivity") {
            fx.params[1] = v;
        }
        fx.check()?;
        effects.push(fx);
        self.commit(Op::SetEffects {
            item: clip.id,
            effects,
        })?;
        Ok(json!({"ok": true, "learned_seconds": r2(to.min(len) - from)}).to_string())
    }

    fn normalize_tool(&mut self, input: &Value) -> R {
        let target = num(input, "target_lufs")
            .unwrap_or(-16.0)
            .clamp(-40.0, -5.0);
        let (ops, gain) = match text(input, "item_id") {
            Some(k) => {
                let (clip, ..) = self.clip_with_media(k)?;
                let before = crate::loudness::clip_loudness(self.project(), &clip)?;
                (
                    crate::loudness::normalize_clip(self.project(), &clip, target)?,
                    crate::loudness::gain_for(&before, target)?,
                )
            }
            None => {
                crate::loudness::normalize_sequence(self.project(), self.timeline()?.id, target)?
            }
        };
        self.commit(Op::Transaction {
            description: format!("Copilot: normalize to {target} LUFS"),
            ops,
        })?;
        Ok(json!({"ok": true, "target_lufs": target, "changed_by_db": r2(gain)}).to_string())
    }

    fn add_title(&mut self, input: &Value) -> R {
        let template = parse_template(text(input, "template").unwrap_or("lower_third"))?;
        let tl = self.timeline()?.clone();
        let track = match text(input, "track") {
            Some(k) => self.find_track(k)?,
            None => tl
                .tracks
                .iter()
                .rev()
                .find(|t| t.kind == TrackKind::Video)
                .map(|t| t.as_ref().clone())
                .ok_or("there is no video track")?,
        };
        if track.kind != TrackKind::Video {
            return Err("titles go on video tracks".into());
        }
        let dur = num(input, "duration_sec").unwrap_or(3.0);
        if dur <= 0.0 {
            return Err("duration_sec must be positive".into());
        }
        let mut t = Title::new(Id::new(), template, self.time(dur));
        t.text = req_text(input, "text")?.to_string();
        t.subtitle = text(input, "subtitle").unwrap_or("").to_string();
        t.name = format!("Title: {}", t.text);
        let id = t.id;
        let at = self.time(req_num(input, "at_sec")?);
        self.plan(|p| edit::overwrite_item(p, track.id, at, Item::Title(t)))?;
        self.describe(id)
    }

    fn edit_title(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        let Item::Title(old) = item else {
            return Err("that item is not a title".into());
        };
        let mut t = old.clone();
        if let Some(x) = text(input, "text") {
            if old.name == format!("Title: {}", old.text) {
                t.name = format!("Title: {x}");
            }
            t.text = x.to_string();
        }
        if let Some(x) = text(input, "subtitle") {
            t.subtitle = x.to_string();
        }
        if let Some(x) = text(input, "template") {
            t.style.template = parse_template(x)?;
            if num(input, "size").is_none() {
                t.style.size = t.style.template.default_size();
            }
        }
        if let Some(x) = num(input, "size") {
            t.style.size = x;
        }
        if let Some(f) = num(input, "fade_in_sec") {
            t.fade_in = self.ticks(f).max(0);
        }
        if let Some(f) = num(input, "fade_out_sec") {
            t.fade_out = self.ticks(f).max(0);
        }
        if let Some(x) = num(input, "outline") {
            t.style.outline = x;
        }
        if let Some(x) = num(input, "shadow") {
            t.style.shadow = x;
        }
        if let Some(x) = num(input, "letter_spacing") {
            t.style.tracking = x;
        }
        if let Some(a) = text(input, "align") {
            t.style.align = match a {
                "left" => TextAlign::Left,
                "center" => TextAlign::Center,
                "right" => TextAlign::Right,
                other => return Err(format!("align is left, center or right, not '{other}'")),
            };
        }
        if let Some(f) = text(input, "font") {
            t.font = match f {
                "default" => None,
                "mono" => Some("builtin:mono".into()),
                path => Some(path.to_string()),
            };
        }
        if t == old {
            return Err("nothing to change".into());
        }
        t.check_props(self.timeline()?.rate)
            .map_err(|e| e.to_string())?;
        self.commit(Op::SetTitle {
            item: old.id,
            title: t,
        })?;
        self.describe(old.id)
    }

    fn remove_silences_tool(&mut self, input: &Value) -> R {
        let (clip, media, start) = self.clip_with_media(req_text(input, "item_id")?)?;
        if !media.has_audio {
            return Err(format!("{} has no audio to analyse", clip.name));
        }
        let thr = num(input, "threshold_db").unwrap_or(-42.0) as f32;
        let min = num(input, "min_seconds").unwrap_or(0.45);
        let keep = num(input, "keep_seconds").unwrap_or(0.12);
        let (ops, removed) = remove_silences(&clip, &media, self.fps(), thr, min, keep)?;
        if ops.is_empty() {
            return Ok(json!({"ok": true, "cuts": 0, "note": "no silences that long"}).to_string());
        }
        let total: f64 = removed.iter().map(|(a, b)| b - a).sum();
        self.commit(Op::Transaction {
            description: "Copilot: remove silences".into(),
            ops,
        })?;
        let t0 = start as f64 / self.fps();
        let cuts: Vec<Value> = removed
            .iter()
            .map(|(a, b)| json!([r2(t0 + a), r2(t0 + b)]))
            .collect();
        Ok(json!({"ok": true, "cuts": removed.len(), "removed_sec": r2(total), "removed_ranges_in_original_timeline_sec": cuts, "timeline_duration_sec": self.secs(self.timeline()?.duration().value)}).to_string())
    }

    fn set_track(&mut self, input: &Value) -> R {
        let t = self.find_track(req_text(input, "track")?)?;
        let enabled = flag(input, "enabled").unwrap_or(t.enabled);
        let gain = num(input, "volume_db").unwrap_or(t.gain_db);
        if enabled == t.enabled && gain == t.gain_db {
            return Err("nothing to change".into());
        }
        self.commit(Op::SetTrack {
            track: t.id,
            enabled,
            gain_db: gain,
        })?;
        Ok(json!({"ok": true, "track": t.name, "enabled": enabled, "volume_db": gain}).to_string())
    }
}

fn template_name(t: TitleTemplate) -> &'static str {
    match t {
        TitleTemplate::LowerThird => "lower_third",
        TitleTemplate::Centered => "centered",
        TitleTemplate::Caption => "caption",
        TitleTemplate::Minimal => "minimal",
        TitleTemplate::Boxed => "boxed",
        TitleTemplate::Quote => "quote",
    }
}

fn parse_template(s: &str) -> Result<TitleTemplate, String> {
    match s {
        "lower_third" => Ok(TitleTemplate::LowerThird),
        "centered" => Ok(TitleTemplate::Centered),
        "caption" => Ok(TitleTemplate::Caption),
        "minimal" => Ok(TitleTemplate::Minimal),
        "boxed" => Ok(TitleTemplate::Boxed),
        "quote" => Ok(TitleTemplate::Quote),
        other => Err(format!(
            "template must be lower_third, centered or caption, not '{other}'"
        )),
    }
}

/// What a recorded list of ops does to `project`; used to check a suggestion before offering it.
pub fn check_applies(project: &Project, ops: &[Op]) -> Result<Project, String> {
    let mut p = project.clone();
    for op in ops {
        p = apply(&p, op).map_err(|e| e.to_string())?.0;
    }
    Ok(p)
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

/// The tool list sent with every request.
pub fn tool_definitions() -> Value {
    let item =
        json!({"type": "string", "description": "id of a clip or title, as shown by get_timeline"});
    let track = json!({"type": "string", "description": "track name such as V1 or A2"});
    json!([
        {"name": "get_timeline", "description": "The whole timeline: tracks, every clip/title/gap with its id, start and duration in seconds, and non-default properties. Call this before editing.", "input_schema": obj(json!({}), &[])},
        {"name": "list_media", "description": "The media pool: ids, names, lengths, whether each has picture and sound.", "input_schema": obj(json!({}), &[])},
        {"name": "find_silences", "description": "Analyse a clip's audio and list quiet stretches (timeline seconds). Read-only.", "input_schema": obj(json!({"item_id": item, "threshold_db": {"type": "number", "description": "level below which audio counts as silent; default -42"}, "min_seconds": {"type": "number", "description": "shortest silence to report; default 0.45"}}), &["item_id"])},
        {"name": "split", "description": "Cut the item under a time on one track into two (nothing moves).", "input_schema": obj(json!({"track": track, "at_sec": {"type": "number"}}), &["track", "at_sec"])},
        {"name": "delete_item", "description": "Remove a clip or title. ripple=false leaves a gap (lift); ripple=true closes the gap, moving later items on that track earlier.", "input_schema": obj(json!({"item_id": item, "ripple": {"type": "boolean"}}), &["item_id"])},
        {"name": "trim_item", "description": "Move one edge of a clip or title. edge 'in' = its start (positive delta shortens it from the front); edge 'out' = its end (positive delta lengthens it). Ripple: later items on the track shift.", "input_schema": obj(json!({"item_id": item, "edge": {"type": "string", "enum": ["in", "out"]}, "delta_sec": {"type": "number"}}), &["item_id", "edge", "delta_sec"])},
        {"name": "move_item", "description": "Move a clip or title to a time (and optionally another track of the same kind). It overwrites whatever it lands on.", "input_schema": obj(json!({"item_id": item, "at_sec": {"type": "number"}, "to_track": track}), &["item_id", "at_sec"])},
        {"name": "add_clip", "description": "Put media on a track. Without at_sec it is appended to the end of the track; with at_sec it overwrites from there.", "input_schema": obj(json!({"media_id": {"type": "string"}, "track": track, "at_sec": {"type": "number"}, "from_sec": {"type": "number", "description": "where in the source to start"}, "duration_sec": {"type": "number"}}), &["media_id"])},
        {"name": "set_clip", "description": "Change properties of a clip or title: opacity 0-1, position_x/position_y in canvas pixels from centre, scale_percent, rotation_deg, exposure (stops), contrast (1 = unchanged), saturation (100 = unchanged), temperature_k (6500 neutral, higher warmer), tint / highlights / shadows / vibrance (-100 to 100), volume_db, pan -1..1, muted, fade_in_sec, fade_out_sec (audio), name, enabled.", "input_schema": obj(json!({
            "item_id": item, "opacity": {"type": "number"}, "position_x": {"type": "number"}, "position_y": {"type": "number"}, "scale_percent": {"type": "number"}, "rotation_deg": {"type": "number"},
            "exposure": {"type": "number"}, "contrast": {"type": "number"}, "saturation": {"type": "number"}, "temperature_k": {"type": "number"}, "tint": {"type": "number"}, "highlights": {"type": "number"}, "shadows": {"type": "number"}, "vibrance": {"type": "number"},
            "volume_db": {"type": "number"}, "pan": {"type": "number"}, "muted": {"type": "boolean"}, "fade_in_sec": {"type": "number"}, "fade_out_sec": {"type": "number"},
            "name": {"type": "string"}, "enabled": {"type": "boolean"}}), &["item_id"])},
        {"name": "set_keyframes", "description": "Animate one property of a clip or title over time. Replaces that property's keyframes (an empty list removes the animation). property: position.x, position.y (canvas px), scale (percent), rotation (deg), opacity (percent 0-100), exposure, contrast, saturation, temperature, volume (dB), pan. Each key: at_sec from the item's start, value, optional interp (linear, hold, ease_in, ease_out, ease_in_out) used to reach the next key.", "input_schema": obj(json!({"item_id": item, "property": {"type": "string"}, "keys": {"type": "array", "items": {"type": "object", "properties": {"at_sec": {"type": "number"}, "value": {"type": "number"}, "interp": {"type": "string"}}, "required": ["at_sec", "value"]}}}), &["item_id", "property", "keys"])},
        {"name": "set_speed", "description": "Change a clip's speed (speed_percent: 100 normal, 200 twice as fast, 50 slow motion), play it backwards (reverse) or hold its first frame (freeze). The same stretch of source plays, so the clip gets shorter or longer and later items on its track move (ripple).", "input_schema": obj(json!({"item_id": item, "speed_percent": {"type": "number"}, "reverse": {"type": "boolean"}, "freeze": {"type": "boolean"}}), &["item_id"])},
        {"name": "set_transition", "description": "Put a transition on the cut between a clip and the clip right before it (item_id is the LATER clip). kind: dissolve, dip, wipe_right, wipe_left, push_left, push_right, or none to remove. The overlap is centred on the cut and does not change any lengths; it cannot be longer than twice the shorter of the two clips.", "input_schema": obj(json!({"item_id": item, "kind": {"type": "string", "enum": ["dissolve", "dip", "wipe_right", "wipe_left", "push_left", "push_right", "none"]}, "duration_sec": {"type": "number"}}), &["item_id"])},
        {"name": "set_effects", "description": "Replace the picture effects of a clip, shape or adjustment layer (an empty list removes them). Effects run in order before the transform. kinds: blur (radius px at 1080p), sharpen (amount %), vignette (amount, softness %), mirror (vertical 0/1), crop (left, top, right, bottom in % of the frame), mask (shape 0 rectangle/1 ellipse, center_x, center_y, width, height in % of the frame, rotation deg, feather %, invert 0/1), chroma_key (key_red, key_green, key_blue 0-255 — default pure green —, tolerance, softness, spill_removal %, edge_shrink px (negative grows), matte_view 0/1), glow (threshold %, radius px, intensity %), grain (amount %, size px, colour %), aberration (amount px), duotone (shadow_red/green/blue, highlight_red/green/blue 0-255, strength %), invert (amount %), posterize (levels), pixelate (block_size px at 1080p), color_wheels (lift_/gamma_/gain_ master, red, green, blue, each -1..1), curves (master_0%, master_25%.. up to master_100%, same for red/green/blue, output level 0..1 at that input), hsl (red_hue, red_sat, red_lum ... for red, yellow, green, cyan, blue, magenta), lut (strength %, plus path to a .cube file); sound effects: eq (low_gain, low_freq, mid_1_gain, mid_1_freq, mid_1_q, mid_2_gain, mid_2_freq, mid_2_q, high_gain, high_freq; gains in dB), compressor (threshold_db, ratio, attack_ms, release_ms, makeup_db), limiter (ceiling_db, release_ms), de_esser (threshold_db, reduction_%), noise_reduction (reduction_db, sensitivity_%). Each entry: {kind, enabled?, params?: {name: number}}; unspecified params keep their defaults.", "input_schema": obj(json!({"item_id": item, "effects": {"type": "array", "items": {"type": "object", "properties": {"kind": {"type": "string"}, "enabled": {"type": "boolean"}, "params": {"type": "object"}}, "required": ["kind"]}}}), &["item_id", "effects"])},
        {"name": "split_at_scenes", "description": "Cut a clip wherever the picture changes abruptly (a hard cut in the footage). threshold 0.05-0.95 (default 0.3; lower finds more), min_seconds is the shortest piece to leave (default 0.5).", "input_schema": obj(json!({"item_id": item, "threshold": {"type": "number"}, "min_seconds": {"type": "number"}}), &["item_id"])},
        {"name": "add_layer", "description": "Add a shape, a solid colour or an adjustment layer on a video track (default: the top one). An adjustment layer draws nothing but runs its effects (set them with set_effects on its item_id) over everything on the tracks below it. kind: rectangle, ellipse, solid, adjustment. color is hex like #ff8800; width_percent/height_percent size a rectangle or ellipse as a share of the frame.", "input_schema": obj(json!({"kind": {"type": "string", "enum": ["rectangle", "ellipse", "solid", "adjustment"]}, "at_sec": {"type": "number"}, "duration_sec": {"type": "number"}, "track": track, "color": {"type": "string"}, "width_percent": {"type": "number"}, "height_percent": {"type": "number"}}), &["kind", "at_sec"])},
        {"name": "transcribe", "description": "Speech to text for a clip, locally. Returns segments with start/end in timeline seconds. Read-only; takes a few seconds per minute of audio.", "input_schema": obj(json!({"item_id": item, "language": {"type": "string", "description": "ISO code like en; omit to detect"}}), &["item_id"])},
        {"name": "remove_filler_words", "description": "Transcribe a clip and cut out hesitations (um, uh, er, hmm); the rest closes up. strict=true also cuts words like 'like', 'basically', 'literally', 'actually', 'so'.", "input_schema": obj(json!({"item_id": item, "strict": {"type": "boolean"}, "language": {"type": "string"}}), &["item_id"])},
        {"name": "add_captions", "description": "Transcribe a clip and put the words on screen as caption titles on a 'Captions' track (made if needed), timed to the speech. max_chars limits a caption line (default 42).", "input_schema": obj(json!({"item_id": item, "max_chars": {"type": "number"}, "language": {"type": "string"}}), &["item_id"])},
        {"name": "add_marker", "description": "Pin a marker (a chapter, a note to self) on the timeline at a time. Optional note, duration_sec for a range, color (blue, green, yellow, red, purple, cyan).", "input_schema": obj(json!({"at_sec": {"type": "number"}, "name": {"type": "string"}, "note": {"type": "string"}, "duration_sec": {"type": "number"}, "color": {"type": "string"}}), &["at_sec", "name"])},
        {"name": "remove_marker", "description": "Remove a marker by its id (see get_timeline) or its exact name.", "input_schema": obj(json!({"marker_id": {"type": "string"}}), &["marker_id"])},
        {"name": "animate_title", "description": "Animate how a title or layer comes on (direction in) or goes off (out) with a preset: fade, slide_up, slide_down, slide_left, slide_right, pop, zoom, typewriter (in only). duration_sec is how long the move takes (default 0.6).", "input_schema": obj(json!({"item_id": item, "preset": {"type": "string"}, "direction": {"type": "string", "enum": ["in", "out"]}, "duration_sec": {"type": "number"}}), &["item_id", "preset"])},
        {"name": "remove_background", "description": "Cut the subject out of its background using Apple's on-device vision model (Mac only): the background becomes transparent, so whatever is on the tracks below shows through. mode 'subject' (default: people, animals, objects) or 'person'. Takes a few seconds per second of footage.", "input_schema": obj(json!({"item_id": item, "mode": {"type": "string", "enum": ["subject", "person"]}}), &["item_id"])},
        {"name": "attach_title_to_subject", "description": "Make a title (or shape layer) follow the main moving subject of a video clip, by tracking it with Apple's vision model (Mac only). Writes position keyframes on the title.", "input_schema": obj(json!({"clip_id": item, "title_id": item}), &["clip_id", "title_id"])},
        {"name": "reframe_sequence", "description": "Make a copy of the sequence in another aspect ratio (9:16 vertical, 1:1 square or 4:5 portrait): every video clip is scaled to fill the new frame and panned to keep its subject (a face if there is one) in view. Mac only; takes a few seconds per clip.", "input_schema": obj(json!({"aspect": {"type": "string", "enum": ["9:16", "1:1", "4:5"]}}), &["aspect"])},
        {"name": "stabilize", "description": "Steady shaky handheld footage: the part of the file the clip plays is analysed and re-rendered smoothed (a new, slightly zoomed copy of that stretch is made and the clip is pointed at it). smoothing is how many frames the camera path is averaged over (default 15; 30 for a walking shot). Takes about real time.", "input_schema": obj(json!({"item_id": item, "smoothing": {"type": "number"}}), &["item_id"])},
        {"name": "auto_color", "description": "Measure a few frames of a clip and set its exposure, contrast, saturation and colour temperature to a balanced look (fixes under- or over-exposure, a colour cast and flat colour; a gentle move, not a style). Replaces the clip's basic colour settings.", "input_schema": obj(json!({"item_id": item}), &["item_id"])},
        {"name": "match_color", "description": "Make a clip look like another one (same brightness, colour balance and colour) by setting its basic colour settings from a comparison of frames of both. item_id is the clip to change, reference_id the one to match.", "input_schema": obj(json!({"item_id": item, "reference_id": item}), &["item_id", "reference_id"])},
        {"name": "sync_by_audio", "description": "Line up clips shot of the same event by their sound. The first clip stays; the others are moved in time so their sound matches it. Clips with no clear match are left alone and reported.", "input_schema": obj(json!({"item_ids": {"type": "array", "items": {"type": "string"}}}), &["item_ids"])},
        {"name": "multicam_cut", "description": "Multicam switching: cut every camera-angle track at a time and show only one angle from there on. angle_tracks are the track names in angle order; angle is 1-based. Call it once at the start (angle 1 at the first clip's time) and then at each switch.", "input_schema": obj(json!({"angle_tracks": {"type": "array", "items": {"type": "string"}}, "angle": {"type": "number"}, "at_sec": {"type": "number"}}), &["angle_tracks", "angle", "at_sec"])},
        {"name": "measure_loudness", "description": "Measure loudness in LUFS (and loudness range, peaks) of one clip, or of the whole mix when item_id is left out. Read-only.", "input_schema": obj(json!({"item_id": item}), &[])},
        {"name": "learn_noise_reduction", "description": "Remove steady background noise (hiss, hum, room tone) from a clip by learning what the noise sounds like from a stretch of the clip that has only noise (at least 0.2 s, about 1 s is better), then adding Noise Reduction using that sample. from_sec and to_sec are seconds into the clip.", "input_schema": obj(json!({"item_id": item, "from_sec": {"type": "number"}, "to_sec": {"type": "number"}, "reduction_db": {"type": "number"}, "sensitivity": {"type": "number"}}), &["item_id", "from_sec", "to_sec"])},
        {"name": "normalize_loudness", "description": "Bring a clip (or, without item_id, the whole sequence by moving every track's level equally) to a target loudness in LUFS: -16 for web and podcasts, -14 for streaming music, -23 for broadcast (EBU R128). Default -16.", "input_schema": obj(json!({"item_id": item, "target_lufs": {"type": "number"}}), &[])},
        {"name": "add_title", "description": "Add a title on a video track (default: the top one). Templates: lower_third (name plate, uses subtitle), centered, caption, minimal (text with an accent line), boxed (text in a bordered plate), quote (large quotation mark, wrapped text, subtitle is the speaker).", "input_schema": obj(json!({"text": {"type": "string"}, "subtitle": {"type": "string"}, "template": {"type": "string", "enum": ["lower_third", "centered", "caption", "minimal", "boxed", "quote"]}, "at_sec": {"type": "number"}, "duration_sec": {"type": "number"}, "track": track}), &["text", "at_sec"])},
        {"name": "edit_title", "description": "Change a title's text, second line, template, size (px at 1080p), fades, outline / shadow / letter_spacing (px at 1080p), align (left, center, right) or font (default, mono, or a font file path).", "input_schema": obj(json!({"item_id": item, "outline": {"type": "number"}, "shadow": {"type": "number"}, "letter_spacing": {"type": "number"}, "align": {"type": "string"}, "font": {"type": "string"}, "text": {"type": "string"}, "subtitle": {"type": "string"}, "template": {"type": "string", "enum": ["lower_third", "centered", "caption", "minimal", "boxed", "quote"]}, "size": {"type": "number"}, "fade_in_sec": {"type": "number"}, "fade_out_sec": {"type": "number"}}), &["item_id"])},
        {"name": "remove_silences", "description": "Cut the silent stretches out of a clip (a jump-cut tightening), keeping keep_seconds of room either side. Later items on the track move earlier.", "input_schema": obj(json!({"item_id": item, "threshold_db": {"type": "number"}, "min_seconds": {"type": "number"}, "keep_seconds": {"type": "number"}}), &["item_id"])},
        {"name": "set_track", "description": "Hide/mute a track or change its volume.", "input_schema": obj(json!({"track": track, "enabled": {"type": "boolean"}, "volume_db": {"type": "number"}}), &["track"])},
    ])
}
