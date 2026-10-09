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
        if t == old {
            return Err("nothing to change".into());
        }
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
    }
}

fn parse_template(s: &str) -> Result<TitleTemplate, String> {
    match s {
        "lower_third" => Ok(TitleTemplate::LowerThird),
        "centered" => Ok(TitleTemplate::Centered),
        "caption" => Ok(TitleTemplate::Caption),
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
        {"name": "set_clip", "description": "Change properties of a clip or title: opacity 0-1, position_x/position_y in canvas pixels from centre, scale_percent, rotation_deg, exposure (stops), contrast (1 = unchanged), saturation (100 = unchanged), temperature_k (6500 neutral, higher warmer), volume_db, pan -1..1, muted, fade_in_sec, fade_out_sec (audio), name, enabled.", "input_schema": obj(json!({
            "item_id": item, "opacity": {"type": "number"}, "position_x": {"type": "number"}, "position_y": {"type": "number"}, "scale_percent": {"type": "number"}, "rotation_deg": {"type": "number"},
            "exposure": {"type": "number"}, "contrast": {"type": "number"}, "saturation": {"type": "number"}, "temperature_k": {"type": "number"},
            "volume_db": {"type": "number"}, "pan": {"type": "number"}, "muted": {"type": "boolean"}, "fade_in_sec": {"type": "number"}, "fade_out_sec": {"type": "number"},
            "name": {"type": "string"}, "enabled": {"type": "boolean"}}), &["item_id"])},
        {"name": "set_keyframes", "description": "Animate one property of a clip or title over time. Replaces that property's keyframes (an empty list removes the animation). property: position.x, position.y (canvas px), scale (percent), rotation (deg), opacity (percent 0-100), exposure, contrast, saturation, temperature, volume (dB), pan. Each key: at_sec from the item's start, value, optional interp (linear, hold, ease_in, ease_out, ease_in_out) used to reach the next key.", "input_schema": obj(json!({"item_id": item, "property": {"type": "string"}, "keys": {"type": "array", "items": {"type": "object", "properties": {"at_sec": {"type": "number"}, "value": {"type": "number"}, "interp": {"type": "string"}}, "required": ["at_sec", "value"]}}}), &["item_id", "property", "keys"])},
        {"name": "set_speed", "description": "Change a clip's speed (speed_percent: 100 normal, 200 twice as fast, 50 slow motion), play it backwards (reverse) or hold its first frame (freeze). The same stretch of source plays, so the clip gets shorter or longer and later items on its track move (ripple).", "input_schema": obj(json!({"item_id": item, "speed_percent": {"type": "number"}, "reverse": {"type": "boolean"}, "freeze": {"type": "boolean"}}), &["item_id"])},
        {"name": "set_transition", "description": "Put a transition on the cut between a clip and the clip right before it (item_id is the LATER clip). kind: dissolve, dip, wipe_right, wipe_left, push_left, push_right, or none to remove. The overlap is centred on the cut and does not change any lengths; it cannot be longer than twice the shorter of the two clips.", "input_schema": obj(json!({"item_id": item, "kind": {"type": "string", "enum": ["dissolve", "dip", "wipe_right", "wipe_left", "push_left", "push_right", "none"]}, "duration_sec": {"type": "number"}}), &["item_id"])},
        {"name": "add_title", "description": "Add a title on a video track (default: the top one). Templates: lower_third (name plate, uses subtitle), centered, caption.", "input_schema": obj(json!({"text": {"type": "string"}, "subtitle": {"type": "string"}, "template": {"type": "string", "enum": ["lower_third", "centered", "caption"]}, "at_sec": {"type": "number"}, "duration_sec": {"type": "number"}, "track": track}), &["text", "at_sec"])},
        {"name": "edit_title", "description": "Change a title's text, second line, template, size (px at 1080p) or fades.", "input_schema": obj(json!({"item_id": item, "text": {"type": "string"}, "subtitle": {"type": "string"}, "template": {"type": "string", "enum": ["lower_third", "centered", "caption"]}, "size": {"type": "number"}, "fade_in_sec": {"type": "number"}, "fade_out_sec": {"type": "number"}}), &["item_id"])},
        {"name": "remove_silences", "description": "Cut the silent stretches out of a clip (a jump-cut tightening), keeping keep_seconds of room either side. Later items on the track move earlier.", "input_schema": obj(json!({"item_id": item, "threshold_db": {"type": "number"}, "min_seconds": {"type": "number"}, "keep_seconds": {"type": "number"}}), &["item_id"])},
        {"name": "set_track", "description": "Hide/mute a track or change its volume.", "input_schema": obj(json!({"track": track, "enabled": {"type": "boolean"}, "volume_db": {"type": "number"}}), &["track"])},
    ])
}
