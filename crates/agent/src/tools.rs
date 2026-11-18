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
    /// Pictures the last tool call made, handed to the model with its result (PNG)
    images: Vec<Vec<u8>>,
    /// Pictures sent so far in this request
    images_sent: usize,
    /// The user lets CoCaptain show frames of their video to Claude
    pub allow_frames: bool,
    /// A look was refused because the user has not allowed it yet: the run stops so the editor can ask them
    pub frames_refused: bool,
    /// Things for the editor itself to do once the reply is in (open a project, export...)
    pub app_actions: Vec<AppAction>,
}

/// What CoCaptain asks the editor to do outside the timeline. They happen after its reply, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum AppAction {
    /// Make an empty project and open it (the open one is saved first)
    NewProject {
        name: String,
        width: u32,
        height: u32,
        fps: String,
    },
    /// Open a project file
    OpenProject { path: String },
    /// Render the timeline to a video file
    Export { path: String },
}

/// Most pictures one request may send: enough to study a reference and check an edit, small enough to stay fast.
pub const IMAGE_BUDGET: usize = 40;

/// Most frames one look takes.
const MAX_FRAMES_PER_LOOK: usize = 16;

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

/// "#ff8800" or "ff8800" -> [r, g, b]
fn parse_hex(c: &str) -> Result<[u8; 3], String> {
    let hex = c.trim_start_matches('#');
    let bad = || format!("'{c}' is not a hex colour like #ff8800");
    if hex.len() != 6 {
        return Err(bad());
    }
    let v = u32::from_str_radix(hex, 16).map_err(|_| bad())?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
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
            images: Vec::new(),
            images_sent: 0,
            allow_frames: false,
            frames_refused: false,
            app_actions: Vec::new(),
        })
    }

    /// The pictures the last tool call made, for its result.
    pub fn take_images(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.images)
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
                effects_summary(o, &c.effects);
                keyframes_summary(o, &c.anim, self.fps());
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
                effects_summary(o, &t.effects);
                keyframes_summary(o, &t.anim, self.fps());
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
            // every colour setting that is not at its default, under the names set_clip takes
            let (c, n) = (&v.color, &d.color);
            let mut col = serde_json::Map::new();
            for (key, val, def) in [
                ("exposure", c.exposure, n.exposure),
                ("contrast", c.contrast, n.contrast),
                ("saturation", c.saturation, n.saturation),
                ("temperature_k", c.temperature, n.temperature),
                ("tint", c.tint, n.tint),
                ("highlights", c.highlights, n.highlights),
                ("shadows", c.shadows, n.shadows),
                ("vibrance", c.vibrance, n.vibrance),
            ] {
                if val != def {
                    col.insert(key.into(), json!(r2(val)));
                }
            }
            o.insert("color".into(), Value::Object(col));
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
            "look" => self.look(input),
            "import_media" => self.import_media(input),
            "new_project" => self.new_project(input),
            "open_project" => self.open_project(input),
            "export_video" => self.export_video(input),
            "look_media" => self.look_media(input),
            "list_media" => self.list_media(),
            "organize_media" => self.organize_media(input),
            "find_audio_plugins" => self.find_plugins(),
            "add_audio_plugin" => self.add_plugin(input),
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
            "edit_effect" => self.edit_effect(input),
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
            "add_track" => self.add_track(input),
            "copy_grade" => self.copy_grade(input),
            "reset_grade" => self.reset_grade(input),
            other => Err(format!("unknown tool '{other}'")),
        }
    }

    // ---- looking -----------------------------------------------------------------------------------

    /// Room to send `n` more pictures, or why not.
    fn picture_room(&mut self, n: usize) -> Result<(), String> {
        if !self.allow_frames {
            self.frames_refused = true;
            return Err("the user has not let CoCaptain see the video: tell them to turn on 'Let CoCaptain \
                        see the video' in the CoCaptain panel, then ask again"
                .into());
        }
        if self.images_sent + n > IMAGE_BUDGET {
            return Err(format!(
                "picture budget for this request is used up ({IMAGE_BUDGET}); work from what you have seen, \
                 or look at fewer frames on a contact sheet"
            ));
        }
        Ok(())
    }

    fn keep_pictures(&mut self, pngs: Vec<Vec<u8>>) {
        self.images_sent += pngs.len();
        self.images.extend(pngs);
    }

    fn look_times(input: &Value) -> Vec<f64> {
        input
            .get("times")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default()
    }

    /// The timeline's picture (with the edits made so far) at the given times or in the middle of each clip.
    fn look(&mut self, input: &Value) -> R {
        let tl = self.timeline()?.clone();
        let mut times = Self::look_times(input);
        if flag(input, "per_clip").unwrap_or(times.is_empty()) {
            for t in tl.tracks.iter().filter(|t| t.kind == TrackKind::Video) {
                for (it, st) in t.items.iter().zip(t.item_starts()) {
                    if matches!(it, Item::Clip(_)) {
                        times.push(self.secs(st + it.duration().value / 2));
                    }
                }
            }
            times.sort_by(f64::total_cmp);
            times.dedup_by(|a, b| (*a - *b).abs() < 0.2);
        }
        if times.is_empty() {
            return Err("give 'times' (seconds) or per_clip: true".into());
        }
        let skipped = times.len().saturating_sub(MAX_FRAMES_PER_LOOK);
        times.truncate(MAX_FRAMES_PER_LOOK);
        let sheet = flag(input, "sheet").unwrap_or(times.len() > 1);
        let width = num(input, "width").unwrap_or(if sheet { 480.0 } else { 960.0 }) as u32;
        self.picture_room(if sheet { 1 } else { times.len() })?;
        let frames = crate::look::timeline_frames(self.project(), tl.id, &times, width)?;
        let pngs = crate::look::pictures(&frames, sheet)?;
        // what each picture is made of, so the model can name what it sees
        let at: Vec<Value> = times
            .iter()
            .map(|&t| {
                let tick = self.ticks(t);
                let layers: Vec<String> = tl
                    .tracks
                    .iter()
                    .filter(|t| t.kind == TrackKind::Video)
                    .filter_map(|t| {
                        t.items
                            .iter()
                            .zip(t.item_starts())
                            .find(|(it, st)| {
                                it.is_content() && *st <= tick && tick < st + it.duration().value
                            })
                            .map(|(it, _)| match it {
                                Item::Clip(c) => format!("{} {}", t.name, c.name),
                                Item::Title(x) if x.adjustment => {
                                    format!("{} adjustment layer", t.name)
                                }
                                Item::Title(x) if x.text.trim().is_empty() => {
                                    format!("{} {}", t.name, x.name)
                                }
                                Item::Title(x) => format!("{} title '{}'", t.name, x.text),
                                Item::Gap(_) => String::new(),
                            })
                    })
                    .collect();
                json!({"sec": r2(t), "layers_bottom_to_top": layers})
            })
            .collect();
        let n = pngs.len();
        self.keep_pictures(pngs);
        Ok(json!({
            "pictures": n,
            "layout": if sheet { "one contact sheet, frames left to right then down, each labelled with its time" } else { "one picture per time" },
            "frames": at,
            "note": if skipped > 0 { format!("{skipped} more frames were left out: look at most {MAX_FRAMES_PER_LOOK} at a time") } else { String::new() },
        })
        .to_string())
    }

    /// Frames of a media file (in the pool, or any file by path) at the given times or spread evenly.
    fn look_media(&mut self, input: &Value) -> R {
        let path = match (text(input, "media_id"), text(input, "path")) {
            (Some(id), _) => self.find_media(id)?.path,
            (None, Some(p)) => p.to_string(),
            _ => return Err("give media_id (from list_media) or path".into()),
        };
        let mut times = Self::look_times(input);
        let sheet = flag(input, "sheet").unwrap_or(true);
        let width = num(input, "width").unwrap_or(if sheet { 480.0 } else { 960.0 }) as u32;
        self.picture_room(1)?;
        if times.is_empty() {
            // spread evenly over the file: probe its length first
            let (_, secs) = crate::look::media_frames(&path, &[0.0], 64)?;
            let n = num(input, "count")
                .unwrap_or(8.0)
                .clamp(1.0, MAX_FRAMES_PER_LOOK as f64) as usize;
            times = (0..n)
                .map(|i| r2((i as f64 + 0.5) / n as f64 * secs))
                .collect();
        }
        times.truncate(MAX_FRAMES_PER_LOOK);
        if !sheet {
            self.picture_room(times.len())?;
        }
        let (frames, secs) = crate::look::media_frames(&path, &times, width)?;
        let pngs = crate::look::pictures(&frames, sheet)?;
        let n = pngs.len();
        self.keep_pictures(pngs);
        Ok(json!({
            "file": path,
            "duration_sec": r2(secs),
            "pictures": n,
            "times_sec": times,
            "layout": if sheet { "one contact sheet, left to right then down, each labelled with its time in the file" } else { "one picture per time" },
        })
        .to_string())
    }

    // ---- the editor itself ---------------------------------------------------------------------------

    /// Bring video, audio or image files into the media pool (part of the suggested edit, so they can be placed
    /// on the timeline in the same request).
    fn import_media(&mut self, input: &Value) -> R {
        let paths: Vec<String> = input
            .get("paths")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if paths.is_empty() {
            return Err("give 'paths': files to import".into());
        }
        let mut added = Vec::new();
        for p in &paths {
            let path = std::path::Path::new(p);
            let canon = path.canonicalize().map_err(|e| format!("{p}: {e}"))?;
            if let Some(m) = self
                .project()
                .media
                .values()
                .find(|m| std::path::Path::new(&m.path) == canon)
            {
                added.push(
                    json!({"id": m.id.to_string(), "name": m.name, "note": "already in the pool"}),
                );
                continue;
            }
            let m =
                captaine_media::media_ref(&canon, Id::new()).map_err(|e| format!("{p}: {e}"))?;
            added.push(json!({"id": m.id.to_string(), "name": m.name, "duration_sec": m.duration.map(|d| r2(d.to_seconds_f64())), "picture": m.has_video, "sound": m.has_audio}));
            self.commit(Op::AddMedia { media: m })?;
        }
        Ok(json!({"imported": added}).to_string())
    }

    fn new_project(&mut self, input: &Value) -> R {
        let name = text(input, "name").unwrap_or("Untitled").trim().to_string();
        let width = num(input, "width").unwrap_or(1920.0) as u32;
        let height = num(input, "height").unwrap_or(1080.0) as u32;
        let fps = match input.get("fps") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => "30".into(),
        };
        if !(16..=8192).contains(&width) || !(16..=8192).contains(&height) {
            return Err("width and height must be 16 to 8192 pixels".into());
        }
        self.app_actions.push(AppAction::NewProject {
            name: name.clone(),
            width,
            height,
            fps: fps.clone(),
        });
        Ok(json!({"ok": true, "note": format!("'{name}' ({width}x{height}, {fps} fps) is created and opened when you finish replying; the project open now is saved first. Edits you make in this request still go to the project open now, so make this your last step.")}).to_string())
    }

    fn open_project(&mut self, input: &Value) -> R {
        let path = req_text(input, "path")?;
        if !std::path::Path::new(path).is_file() {
            return Err(format!("no project file at {path}"));
        }
        self.app_actions
            .push(AppAction::OpenProject { path: path.into() });
        Ok(json!({"ok": true, "note": "it opens when you finish replying (the project open now is saved first); make this your last step"}).to_string())
    }

    fn export_video(&mut self, input: &Value) -> R {
        let path = req_text(input, "path")?;
        if !path.ends_with(".mp4") && !path.ends_with(".mov") {
            return Err("path must end in .mp4 or .mov".into());
        }
        self.app_actions
            .push(AppAction::Export { path: path.into() });
        Ok(json!({"ok": true, "note": "the export starts when you finish replying, from the timeline as it is then (a suggestion the user has not applied yet is not in it); progress shows on the Deliver page"}).to_string())
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
                    "bin": self.project().meta.get(&m.id).map(|x| x.bin.clone()).filter(|b| !b.is_empty()),
                    "tags": self.project().meta.get(&m.id).map(|x| x.tags.clone()).filter(|t| !t.is_empty()),
                    "note": self.project().meta.get(&m.id).map(|x| x.note.clone()).filter(|n| !n.is_empty()),
                    "input_color": self.project().meta.get(&m.id).map(|x| x.input.key()),
                })
            })
            .collect();
        Ok(Value::Array(v).to_string())
    }

    fn find_plugins(&self) -> R {
        let found = captaine_plugins::scan(&[]);
        let v: Vec<Value> = found
            .iter()
            .map(|p| json!({"id": p.id, "name": p.name, "vendor": p.vendor}))
            .collect();
        Ok(json!({"plugins": v}).to_string())
    }

    /// Put a CLAP plugin on a clip's sound; `settings` maps control names to values.
    fn add_plugin(&mut self, input: &Value) -> R {
        let (clip, ..) = self.clip_with_media(req_text(input, "item_id")?)?;
        let want = req_text(input, "plugin")?.to_lowercase();
        let found = captaine_plugins::scan(&[]);
        let info = found
            .iter()
            .find(|p| p.id.to_lowercase() == want || p.name.to_lowercase() == want)
            .or_else(|| found.iter().find(|p| p.name.to_lowercase().contains(&want)))
            .ok_or_else(|| format!("no plugin '{want}'; call find_audio_plugins"))?;
        let mut r = captaine_plugins::PluginRef {
            id: info.id.clone(),
            path: info.path.clone(),
            params: vec![],
        };
        if let Some(obj) = input.get("settings").and_then(Value::as_object) {
            let controls = captaine_plugins::params(&r)?;
            for (name, v) in obj {
                let c = controls
                    .iter()
                    .find(|c| c.name.to_lowercase() == name.to_lowercase())
                    .ok_or_else(|| {
                        format!(
                            "{} has no control '{name}'; it has {}",
                            info.name,
                            controls
                                .iter()
                                .map(|c| c.name.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })?;
                let v = v.as_f64().ok_or("settings are numbers")?;
                r = r.with_value(c.id, v.clamp(c.min, c.max));
            }
        }
        let mut fx = Effect::new(EffectKind::Plugin);
        fx.text = r.to_text();
        if let Some(m) = num(input, "mix_percent") {
            fx.params[0] = m.clamp(0.0, 100.0);
        }
        let mut effects = clip.effects.clone();
        effects.push(fx);
        self.commit(Op::SetEffects {
            item: clip.id,
            effects,
        })?;
        self.describe(clip.id)
    }

    fn organize_media(&mut self, input: &Value) -> R {
        let m = self.find_media(req_text(input, "media")?)?;
        let mut meta = self.project().meta.get(&m.id).cloned().unwrap_or_default();
        if let Some(b) = text(input, "bin") {
            meta.bin = b.trim().to_string();
        }
        if let Some(tags) = input.get("tags").and_then(Value::as_array) {
            meta.tags = tags
                .iter()
                .filter_map(|t| t.as_str())
                .map(|t| t.trim().to_string())
                .collect();
        }
        if let Some(n) = text(input, "note") {
            meta.note = n.trim().to_string();
        }
        if let Some(k) = text(input, "input_color") {
            meta.input = InputSpace::from_key(k)
                .ok_or("input_color is one of auto, rec709, rec2020, hlg, pq")?;
        }
        self.commit(Op::SetMediaMeta {
            media: m.id,
            meta: Some(meta),
        })?;
        self.list_media()
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
        let (item, track, _) = self.find_item(req_text(input, "item_id")?)?;
        let ripple = flag(input, "ripple").unwrap_or(false);
        if ripple {
            self.commit(Op::RemoveItem { item: item.id() })?;
        } else {
            self.plan(|p| edit::lift(p, item.id()))?;
            // the gap left by the last item of a track would only stretch the timeline: drop it
            if let Ok(Some(op)) = edit::clean_gaps(self.ed.project(), track) {
                self.commit(op)?;
            }
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
        // A keyframed property ignores its fixed value; say so rather than report a change nobody will see.
        let keyed: Vec<&str> = [
            ("opacity", "opacity"),
            ("position_x", "position.x"),
            ("position_y", "position.y"),
            ("scale_percent", "scale"),
            ("rotation_deg", "rotation"),
            ("exposure", "exposure"),
            ("contrast", "contrast"),
            ("saturation", "saturation"),
            ("temperature_k", "temperature"),
            ("tint", "tint"),
            ("highlights", "highlights"),
            ("shadows", "shadows"),
            ("vibrance", "vibrance"),
            ("volume_db", "volume"),
            ("pan", "pan"),
        ]
        .into_iter()
        .filter(|(arg, prop)| input.get(*arg).is_some() && item_anim(&item).0.contains_key(*prop))
        .map(|(_, prop)| prop)
        .collect();
        self.commit(Op::Transaction {
            description: "CoCaptain: set properties".into(),
            ops,
        })?;
        let out = self.describe(id)?;
        if keyed.is_empty() {
            return Ok(out);
        }
        let mut v: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
        v["warning"] = json!(format!(
            "{} keyframed, so the fixed value has no effect. To shift the whole animation, send its keyframes (get_timeline lists them) to set_keyframes with each value changed; to use one fixed value, call set_keyframes with empty keys, then set_clip",
            keyed.join(", ") + if keyed.len() == 1 { " is" } else { " are" }
        ));
        Ok(v.to_string())
    }

    fn set_keyframes(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        let prop = req_text(input, "property")?.to_string();
        if !is_property(&prop) {
            return Err(format!(
                "unknown property '{prop}'; use one of {}, or fx.<kind>.<setting> for an effect on the item",
                PROPERTIES
                    .iter()
                    .map(|p| p.0)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        // an effect setting: the effect has to be there, and the values in its range
        let fx_range = match parse_fx_property(&prop) {
            Some((kind, nth, param)) => {
                let effects = match &item {
                    Item::Clip(c) => &c.effects,
                    Item::Title(t) => &t.effects,
                    Item::Gap(_) => return Err("a gap has no effects".into()),
                };
                let have = effects.iter().filter(|e| e.kind == kind).count();
                if have < nth {
                    return Err(format!(
                        "the item has {have} {} effect(s); add it first (edit_effect), then animate its settings",
                        kind.key()
                    ));
                }
                let d = &kind.params()[param];
                Some((d.min, d.max))
            }
            None => None,
        };
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
            if let Some((lo, hi)) = fx_range {
                if !(lo..=hi).contains(&v) {
                    return Err(format!("{prop} = {v} is outside {lo}..{hi}"));
                }
            }
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
        if let Some(p) = num(input, "ramp_to_percent") {
            rt.ramp_to = if p <= 0.0 { 0.0 } else { p / 100.0 };
        }
        if rt.speed < Retime::MIN_SPEED
            || rt.speed > Retime::MAX_SPEED
            || (rt.ramp_to != 0.0 && !(Retime::MIN_SPEED..=Retime::MAX_SPEED).contains(&rt.ramp_to))
        {
            return Err("speed must be between 5% and 2000%".into());
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
            other => Some(TransitionKind::from_key(other).ok_or_else(|| {
                format!(
                    "unknown transition '{other}'; use one of {}",
                    TransitionKind::ALL.map(|t| t.key()).join(", ")
                )
            })?),
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
        let mut notes = Vec::new();
        for e in input
            .get("effects")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let mut fx = effect_from_json(e, None)?;
            notes.extend(self.pick_key_colour(&item, &mut fx, e));
            list.push(fx);
        }
        self.commit(Op::SetEffects {
            item: item.id(),
            effects: list,
        })?;
        with_notes(self.describe(item.id())?, &notes)
    }

    /// A new chroma key takes its key colour from the clip itself, the way an eyedropper would: the most common
    /// strong colour near the one asked for (pure green when none is given), since a guessed colour rarely
    /// matches real footage.
    fn pick_key_colour(&self, item: &Item, fx: &mut Effect, input: &Value) -> Option<String> {
        if fx.kind != EffectKind::ChromaKey {
            return None;
        }
        let given = input.get("params").and_then(Value::as_object);
        let Item::Clip(c) = item else { return None };
        let media = self.project().media.get(&c.media)?;
        let asked = [fx.params[0], fx.params[1], fx.params[2]];
        let default_green = asked == [0.0, 255.0, 0.0];
        let hue = hue_of(asked);
        let reach = if default_green { 60.0 } else { 40.0 };
        match crate::color::key_colour(c, media, hue, reach) {
            Ok(Some(rgb)) => {
                for (i, v) in rgb.iter().enumerate() {
                    fx.params[i] = *v as f64;
                }
                // a dull backdrop is close to grey: scale the reach of the key to how colourful the backdrop
                // is (pure green is the yardstick), so it does not eat into the subject
                let [r, g, b] = rgb.map(|v| v as f64 / 255.0);
                let cb = -0.168736 * r - 0.331264 * g + 0.5 * b;
                let cr = 0.5 * r - 0.418688 * g - 0.081312 * b;
                let k = (cb.hypot(cr) / 0.53).clamp(0.2, 1.0);
                let has = |name: &str| given.is_some_and(|p| p.contains_key(name));
                if !has("tolerance") || k < 0.6 {
                    fx.params[3] = (35.0 * k).max(6.0).round();
                }
                if !has("softness") || k < 0.6 {
                    fx.params[4] = (15.0 * k).max(3.0).round();
                }
                Some(format!(
                    "key colour picked from the shot: red {}, green {}, blue {} (tolerance {}, softness {})",
                    rgb[0], rgb[1], rgb[2], fx.params[3], fx.params[4]
                ))
            }
            Ok(None) => Some(format!(
                "no backdrop near that colour covers much of the shot (hue {hue:.0}): keying red {}, green {}, blue {} as given; tell the user, or give the colour actually in the shot",
                asked[0], asked[1], asked[2]
            )),
            Err(_) => None,
        }
    }

    /// Change one effect of an item, leaving its other effects alone: the settings given are changed, the rest
    /// keep their current values. A missing effect is added (colour effects go with the other colour effects, in
    /// front; the rest at the end). `remove: true` takes it off.
    fn edit_effect(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        let mut effects = match &item {
            Item::Clip(c) => c.effects.clone(),
            Item::Title(t) => t.effects.clone(),
            Item::Gap(_) => return Err("effects go on clips and layers".into()),
        };
        let kind = effect_kind(req_text(input, "kind")?)?;
        let at = effects.iter().position(|e| e.kind == kind);
        let mut notes = Vec::new();
        let mut ops = Vec::new();
        if flag(input, "remove") == Some(true) {
            let Some(i) = at else {
                return Err(format!("this item has no {} effect", kind.key()));
            };
            effects.remove(i);
            // its keyframes go too, once no effect of that kind is left
            if !effects.iter().any(|e| e.kind == kind) {
                let anim = match &item {
                    Item::Clip(c) => &c.anim,
                    Item::Title(t) => &t.anim,
                    Item::Gap(_) => unreachable!(),
                };
                let (a, b) = (format!("fx.{}.", kind.key()), format!("fx.{}#", kind.key()));
                for prop in anim
                    .0
                    .keys()
                    .filter(|k| k.starts_with(&a) || k.starts_with(&b))
                {
                    ops.push(Op::SetAnimTrack {
                        item: item.id(),
                        prop: prop.clone(),
                        keys: Vec::new(),
                    });
                }
            }
        } else {
            let mut fx = effect_from_json(input, at.map(|i| effects[i].clone()))?;
            if at.is_none() {
                notes.extend(self.pick_key_colour(&item, &mut fx, input));
            }
            match at {
                Some(i) => effects[i] = fx,
                None if is_color_effect(kind) => {
                    let i = effects
                        .iter()
                        .take_while(|e| is_color_effect(e.kind))
                        .count();
                    effects.insert(i, fx);
                }
                None => effects.push(fx),
            }
        }
        ops.insert(
            0,
            Op::SetEffects {
                item: item.id(),
                effects,
            },
        );
        self.commit(if ops.len() == 1 {
            ops.remove(0)
        } else {
            Op::Transaction {
                description: format!("CoCaptain: remove {}", kind.key()),
                ops,
            }
        })?;
        with_notes(self.describe(item.id())?, &notes)
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
            description: "CoCaptain: split at scene changes".into(),
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
            "polygon" => Some(ShapeKind::Polygon),
            "star" => Some(ShapeKind::Star),
            "line" => Some(ShapeKind::Line),
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
                let [r, g, b] = parse_hex(c)?;
                sh.color = [r, g, b, 255];
            }
            if let Some(n) = num(input, "sides") {
                sh.sides = n.round().clamp(0.0, 99.0) as u32;
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
            description: "CoCaptain: remove filler words".into(),
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
            description: "CoCaptain: add captions".into(),
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
            description: format!("CoCaptain: {name} {}", if entering { "in" } else { "out" }),
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
            description: "CoCaptain: attach title to subject".into(),
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
            "3:4" => ("3:4", (3, 4)),
            "4:3" => ("4:3", (4, 3)),
            "3:2" => ("3:2", (3, 2)),
            "5:3" => ("5:3", (5, 3)),
            other => {
                return Err(format!(
                    "aspect is 9:16, 1:1, 4:5, 3:4, 4:3, 3:2 or 5:3, not '{other}'"
                ))
            }
        };
        let cur = self.timeline()?.clone();
        let s = &self.project().settings;
        let short = s.width.min(s.height).max(2);
        let even = |v: f64| ((v.round() as u32).max(2)) & !1;
        // portrait/square: the short side becomes the width; landscape (4:3): it stays the height
        let canvas = if ratio.0 > ratio.1 {
            (
                even(short as f64 * ratio.0 as f64 / ratio.1 as f64),
                even(short as f64),
            )
        } else {
            (
                even(short as f64),
                even(short as f64 * ratio.1 as f64 / ratio.0 as f64),
            )
        };
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
            description: format!("CoCaptain: reframe to {label}"),
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
            description: "CoCaptain: stabilize".into(),
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
            description: format!("CoCaptain: normalize to {target} LUFS"),
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

    /// Every video clip of the timeline, in playing order.
    fn video_clips(&self) -> Result<Vec<Clip>, String> {
        let tl = self.timeline()?;
        let mut v: Vec<(i64, Clip)> = tl
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Video)
            .flat_map(|t| {
                t.items
                    .iter()
                    .zip(t.item_starts())
                    .filter_map(|(it, st)| match it {
                        Item::Clip(c) => Some((st, c.clone())),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|(_, c)| {
                self.project()
                    .media
                    .get(&c.media)
                    .is_some_and(|m| m.has_video)
            })
            .collect();
        v.sort_by_key(|(st, _)| *st);
        Ok(v.into_iter().map(|(_, c)| c).collect())
    }

    fn copy_grade(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "from_item_id")?)?;
        let Item::Clip(from) = item else {
            return Err("from_item_id must be a video clip".into());
        };
        let grade = Grade::of(&from);
        let targets: Vec<Clip> = if input.get("all_shots").and_then(Value::as_bool) == Some(true) {
            self.video_clips()?
                .into_iter()
                .filter(|c| c.id != from.id)
                .collect()
        } else {
            let ids = input
                .get("to_item_ids")
                .and_then(Value::as_array)
                .ok_or("give to_item_ids (a list) or all_shots: true")?;
            let mut v = Vec::new();
            for id in ids {
                let (it, _, _) = self.find_item(id.as_str().ok_or("item ids are strings")?)?;
                match it {
                    Item::Clip(c) => v.push(c),
                    _ => return Err("grades go on video clips".into()),
                }
            }
            v
        };
        let ops: Vec<Op> = targets
            .iter()
            .flat_map(|c| captaine_ops::edit::set_grade(c, &grade))
            .collect();
        if ops.is_empty() {
            return Ok(
                json!({"ok": true, "changed": 0, "note": "those shots already have this grade"})
                    .to_string(),
            );
        }
        self.commit(Op::Transaction {
            description: format!("CoCaptain: copy grade to {} shot(s)", targets.len()),
            ops,
        })?;
        Ok(json!({"ok": true, "changed": targets.len()}).to_string())
    }

    fn reset_grade(&mut self, input: &Value) -> R {
        let (item, _, _) = self.find_item(req_text(input, "item_id")?)?;
        let Item::Clip(c) = item else {
            return Err("item_id must be a video clip".into());
        };
        let ops = captaine_ops::edit::set_grade(&c, &Grade::neutral());
        if ops.is_empty() {
            return Ok(json!({"ok": true, "note": "this shot has no grade"}).to_string());
        }
        self.commit(Op::Transaction {
            description: "CoCaptain: reset grade".into(),
            ops,
        })?;
        self.describe(c.id)
    }

    fn add_track(&mut self, input: &Value) -> R {
        let kind = match req_text(input, "kind")? {
            "video" => TrackKind::Video,
            "audio" => TrackKind::Audio,
            other => return Err(format!("kind is video or audio, not '{other}'")),
        };
        let count = num(input, "count").unwrap_or(1.0).round().clamp(1.0, 16.0) as usize;
        let tl = self.timeline()?.clone();
        let op = captaine_ops::edit::add_tracks(&tl, kind, count, text(input, "name"))
            .map_err(|e| e.to_string())?;
        let names: Vec<String> = match &op {
            Op::Transaction { ops, .. } => ops
                .iter()
                .filter_map(|o| match o {
                    Op::AddTrack { track, .. } => Some(track.name.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        self.commit(op)?;
        Ok(json!({"ok": true, "added": names}).to_string())
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
        if let Some(x) = num(input, "glow") {
            t.style.glow = x.max(0.0);
        }
        if let Some(x) = num(input, "letter_spacing") {
            t.style.tracking = x;
        }
        if let Some(c) = text(input, "color") {
            t.style.color = parse_hex(c)?;
        }
        if let Some(c) = text(input, "outline_color") {
            t.style.outline_color = parse_hex(c)?;
        }
        if let Some(c) = text(input, "accent_color") {
            t.style.accent = parse_hex(c)?;
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
            description: "CoCaptain: remove silences".into(),
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
    let mut tools = json!([
        {"name": "get_timeline", "description": "The whole timeline: tracks, every clip/title/gap with its id, start and duration in seconds, and non-default properties. Call this before editing.", "input_schema": obj(json!({}), &[])},
        {"name": "find_audio_plugins", "description": "List the CLAP audio plugins installed on this computer (id, name, vendor). Read-only.", "input_schema": obj(json!({}), &[])},
        {"name": "add_audio_plugin", "description": "Put an installed CLAP audio plugin on a clip's sound. plugin is its id or name (from find_audio_plugins). settings maps the plugin's control names to values, mix_percent blends the plugin's sound with the original (default 100).", "input_schema": obj(json!({"item_id": item, "plugin": {"type": "string"}, "settings": {"type": "object"}, "mix_percent": {"type": "number"}}), &["item_id", "plugin"])},
        {"name": "organize_media", "description": "File a media item in a bin of the media pool (a folder the user sees), give it tags and a note. Omitted fields stay as they are; bin \"\" takes it out of its bin; tags replaces the whole tag list. Returns the media list.", "input_schema": obj(json!({"media": {"type": "string"}, "bin": {"type": "string"}, "tags": {"type": "array", "items": {"type": "string"}}, "note": {"type": "string"}, "input_color": {"type": "string", "enum": ["auto", "rec709", "rec2020", "hlg", "pq"], "description": "how the footage is encoded; HDR (hlg, pq) and wide-gamut (rec2020) footage is converted to Rec.709 for editing and export. auto reads it from the file."}}), &["media"])},
        {"name": "list_media", "description": "The media pool: ids, names, lengths, whether each has picture and sound.", "input_schema": obj(json!({}), &[])},
        {"name": "import_media", "description": "Bring files (video, audio, pictures) into the media pool by their full paths; they can then be placed with add_clip in the same request. Part of your suggested edit.", "input_schema": obj(json!({"paths": {"type": "array", "items": {"type": "string"}}}), &["paths"])},
        {"name": "new_project", "description": "Make a new, empty project (tracks V1, V2, A1, A2) and open it in the editor when you finish replying; the project open now is saved first. Only when the user asks for a new or blank project. Default 1920x1080 at 30 fps; a vertical reel is 1080x1920.", "input_schema": obj(json!({"name": {"type": "string"}, "width": {"type": "number"}, "height": {"type": "number"}, "fps": {"type": "string", "description": "24, 25, 30, 60, 23.976, 29.97 or 59.94"}}), &[])},
        {"name": "open_project", "description": "Open another project file (.captaine.json) when you finish replying; the one open now is saved first. Only when the user asks.", "input_schema": obj(json!({"path": {"type": "string"}}), &["path"])},
        {"name": "export_video", "description": "Render the timeline to a video file (.mp4 H.264, or .mov ProRes) when you finish replying. Only when the user asks to export or render.", "input_schema": obj(json!({"path": {"type": "string"}}), &["path"])},
        {"name": "look", "description": "SEE the timeline: renders the picture at the given times (seconds), including every edit you have made so far, and shows it to you. With per_clip (the default when no times are given) you get the middle frame of every clip. Several frames come as one contact sheet, each labelled with its time; sheet: false gives one bigger picture per time. Up to 16 frames per call and 40 pictures per request, so pick the moments that matter. Use it to judge colour, framing, text placement and timing, and always to check your work after editing.", "input_schema": obj(json!({"times": {"type": "array", "items": {"type": "number"}}, "per_clip": {"type": "boolean"}, "sheet": {"type": "boolean"}, "width": {"type": "number", "description": "pixels per frame (default 480 on a sheet, 960 alone, at most 1024)"}}), &[])},
        {"name": "look_media", "description": "SEE a media file: frames at the given times (seconds into the file), or `count` frames spread evenly (default 8), as a labelled contact sheet. Works on media in the pool (media_id) or any video file on this Mac (path), e.g. a reference reel the user gave you or footage you are choosing between.", "input_schema": obj(json!({"media_id": {"type": "string"}, "path": {"type": "string"}, "times": {"type": "array", "items": {"type": "number"}}, "count": {"type": "number"}, "sheet": {"type": "boolean"}, "width": {"type": "number"}}), &[])},
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
        {"name": "set_keyframes", "description": "Animate one property of a clip or title over time. Replaces that property's keyframes (an empty list removes the animation). property: position.x, position.y (canvas px), scale (percent), rotation (deg), opacity (percent 0-100), exposure, contrast, saturation, temperature, tint, highlights, shadows, vibrance, volume (dB), pan; or any setting of an effect already on the item, as fx.<kind>.<setting> (e.g. fx.blur.radius, fx.zoom_blur.amount, fx.brightness_contrast.brightness; fx.<kind>#2.<setting> for the second effect of that kind), with values in that setting's range. Each key: at_sec from the item's start, value, optional interp (linear, hold, ease_in, ease_out, ease_in_out) used to reach the next key.", "input_schema": obj(json!({"item_id": item, "property": {"type": "string"}, "keys": {"type": "array", "items": {"type": "object", "properties": {"at_sec": {"type": "number"}, "value": {"type": "number"}, "interp": {"type": "string"}}, "required": ["at_sec", "value"]}}}), &["item_id", "property", "keys"])},
        {"name": "set_speed", "description": "Change a clip's speed (speed_percent: 100 normal, 200 twice as fast, 50 slow motion), play it backwards (reverse) or hold its first frame (freeze). The same stretch of source plays, so the clip gets shorter or longer and later items on its track move (ripple).", "input_schema": obj(json!({"item_id": item, "speed_percent": {"type": "number"}, "ramp_to_percent": {"type": "number", "description": "make a speed ramp: the speed changes steadily from speed_percent at the start to this at the end; 0 removes the ramp"}, "reverse": {"type": "boolean"}, "freeze": {"type": "boolean"}}), &["item_id"])},
        {"name": "set_transition", "description": "Put a transition on the cut between a clip and the clip right before it (item_id is the LATER clip). kind: dissolve, dip, wipe_right, wipe_left, wipe_down, wipe_up, push_left, push_right, slide_left, slide_right, iris, zoom_in, whip_left, whip_right (a fast camera whip with motion blur; 0.15-0.3 s), or none to remove. The overlap is centred on the cut and does not change any lengths; it cannot be longer than twice the shorter of the two clips.", "input_schema": obj(json!({"item_id": item, "kind": {"type": "string", "enum": ["dissolve", "dip", "wipe_right", "wipe_left", "wipe_down", "wipe_up", "push_left", "push_right", "slide_left", "slide_right", "iris", "zoom_in", "whip_left", "whip_right", "none"]}, "duration_sec": {"type": "number"}}), &["item_id"])},
        {"name": "edit_effect", "description": "Change ONE effect of a clip, shape or adjustment layer and keep all its other effects. Prefer this over set_effects for adjusting a grade: only the params given change, the rest keep their current values; a missing effect of that kind is added (colour effects — color_wheels, curves, hsl, lut — go in front with the other colour effects). remove: true takes that effect off. Same kinds, params, path and points as set_effects.", "input_schema": obj(json!({"item_id": item, "kind": {"type": "string"}, "params": {"type": "object"}, "path": {"type": "string"}, "enabled": {"type": "boolean"}, "remove": {"type": "boolean"}}), &["item_id", "kind"])},
        {"name": "set_effects", "description": "Replace ALL the picture effects of a clip, shape or adjustment layer with this list (an empty list removes them; effects not in the list are lost — to change or add a single effect use edit_effect). Effects run in order before the transform. kinds: blur (radius px at 1080p), sharpen (amount %), vignette (amount, softness %), mirror (vertical 0/1), crop (left, top, right, bottom in % of the frame), mask (shape 0 rectangle/1 ellipse/2 drawn polygon given as points [[x,y],...] in % of the frame, 3 to 64 of them, center_x, center_y, width, height in % of the frame, rotation deg, feather %, invert 0/1), chroma_key (key_red, key_green, key_blue 0-255 — default pure green —, tolerance, softness, spill_removal %, edge_shrink px (negative grows), matte_view 0/1), glow (threshold %, radius px, intensity %), grain (amount %, size px, colour %), aberration (amount px), duotone (shadow_red/green/blue, highlight_red/green/blue 0-255, strength %), invert (amount %), posterize (levels), letterbox (aspect = width/height left visible, e.g. 2.35 for cinema bars or 1 for a square; opacity %; put it on an adjustment layer so the bars stay still while clips move), pixelate (block_size px at 1080p), color_wheels (lift_/gamma_/gain_ + master, red, green, blue; each -1..1, 0 = unchanged; lift = shadows, gamma = mid-tones, gain = highlights; 0.03-0.15 is a subtle push, 0.3+ is strong), curves (master_0%, master_25%, master_50%, master_75%, master_100%, and the same five for red_, green_, blue_: the output level 0..1 at that input; untouched = 0, 0.25, 0.5, 0.75, 1), hsl (for each band red, yellow, green, cyan, blue, magenta: <band>_hue = shift in degrees -60..60 (0 = none), <band>_sat = saturation in percent 0..200 (100 = unchanged, 0 = grey), <band>_lum = lightness -50..50 (0 = unchanged)), lut (strength %; the .cube file goes in path next to kind, not in params); sound effects: eq (low_gain, low_freq, mid_1_gain, mid_1_freq, mid_1_q, mid_2_gain, mid_2_freq, mid_2_q, high_gain, high_freq; gains in dB), compressor (threshold_db, ratio, attack_ms, release_ms, makeup_db), limiter (ceiling_db, release_ms), de_esser (threshold_db, reduction_%), noise_reduction (reduction_db, sensitivity_%). Each entry: {kind, enabled?, params?: {name: number}}; unspecified params keep their defaults.", "input_schema": obj(json!({"item_id": item, "effects": {"type": "array", "items": {"type": "object", "properties": {"kind": {"type": "string"}, "enabled": {"type": "boolean"}, "params": {"type": "object"}, "path": {"type": "string", "description": "lut only: the .cube file"}, "points": {"type": "array"}}, "required": ["kind"]}}}), &["item_id", "effects"])},
        {"name": "split_at_scenes", "description": "Cut a clip wherever the picture changes abruptly (a hard cut in the footage). threshold 0.05-0.95 (default 0.3; lower finds more), min_seconds is the shortest piece to leave (default 0.5).", "input_schema": obj(json!({"item_id": item, "threshold": {"type": "number"}, "min_seconds": {"type": "number"}}), &["item_id"])},
        {"name": "add_layer", "description": "Add a shape, a solid colour or an adjustment layer on a video track (default: the top one). An adjustment layer draws nothing but runs its effects (set them with set_effects on its item_id) over everything on the tracks below it. kind: rectangle, ellipse, polygon (regular, sides 3-16), star (sides = points), line (a rounded bar: width_percent long, height_percent thick), solid, adjustment. color is hex like #ff8800; width_percent/height_percent size a rectangle or ellipse as a share of the frame.", "input_schema": obj(json!({"kind": {"type": "string", "enum": ["rectangle", "ellipse", "polygon", "star", "line", "solid", "adjustment"]}, "sides": {"type": "number"}, "at_sec": {"type": "number"}, "duration_sec": {"type": "number"}, "track": track, "color": {"type": "string"}, "width_percent": {"type": "number"}, "height_percent": {"type": "number"}}), &["kind", "at_sec"])},
        {"name": "transcribe", "description": "Speech to text for a clip, locally. Returns segments with start/end in timeline seconds. Read-only; takes a few seconds per minute of audio.", "input_schema": obj(json!({"item_id": item, "language": {"type": "string", "description": "ISO code like en; omit to detect"}}), &["item_id"])},
        {"name": "remove_filler_words", "description": "Transcribe a clip and cut out hesitations (um, uh, er, hmm); the rest closes up. strict=true also cuts words like 'like', 'basically', 'literally', 'actually', 'so'.", "input_schema": obj(json!({"item_id": item, "strict": {"type": "boolean"}, "language": {"type": "string"}}), &["item_id"])},
        {"name": "add_captions", "description": "Transcribe a clip and put the words on screen as caption titles on a 'Captions' track (made if needed), timed to the speech. max_chars limits a caption line (default 42).", "input_schema": obj(json!({"item_id": item, "max_chars": {"type": "number"}, "language": {"type": "string"}}), &["item_id"])},
        {"name": "add_marker", "description": "Pin a marker (a chapter, a note to self) on the timeline at a time. Optional note, duration_sec for a range, color (blue, green, yellow, red, purple, cyan).", "input_schema": obj(json!({"at_sec": {"type": "number"}, "name": {"type": "string"}, "note": {"type": "string"}, "duration_sec": {"type": "number"}, "color": {"type": "string"}}), &["at_sec", "name"])},
        {"name": "remove_marker", "description": "Remove a marker by its id (see get_timeline) or its exact name.", "input_schema": obj(json!({"marker_id": {"type": "string"}}), &["marker_id"])},
        {"name": "animate_title", "description": "Animate how a title or layer comes on (direction in) or goes off (out) with a preset: fade, slide_up, slide_down, slide_left, slide_right, pop, zoom, typewriter (in only). duration_sec is how long the move takes (default 0.6).", "input_schema": obj(json!({"item_id": item, "preset": {"type": "string"}, "direction": {"type": "string", "enum": ["in", "out"]}, "duration_sec": {"type": "number"}}), &["item_id", "preset"])},
        {"name": "remove_background", "description": "Cut the subject out of its background using Apple's on-device vision model (Mac only): the background becomes transparent, so whatever is on the tracks below shows through. mode 'subject' (default: people, animals, objects) or 'person'. Takes a few seconds per second of footage.", "input_schema": obj(json!({"item_id": item, "mode": {"type": "string", "enum": ["subject", "person"]}}), &["item_id"])},
        {"name": "attach_title_to_subject", "description": "Make a title (or shape layer) follow the main moving subject of a video clip, by tracking it with Apple's vision model (Mac only). Writes position keyframes on the title.", "input_schema": obj(json!({"clip_id": item, "title_id": item}), &["clip_id", "title_id"])},
        {"name": "reframe_sequence", "description": "Make a copy of the sequence in another aspect ratio (9:16 vertical, 1:1 square, 4:5 or 3:4 portrait, 4:3, 3:2 or 5:3 landscape): every video clip is scaled to fill the new frame and panned to keep its subject (a face if there is one) in view. Mac only; takes a few seconds per clip.", "input_schema": obj(json!({"aspect": {"type": "string", "enum": ["9:16", "1:1", "4:5", "3:4", "4:3", "3:2", "5:3"]}}), &["aspect"])},
        {"name": "stabilize", "description": "Steady shaky handheld footage: the part of the file the clip plays is analysed and re-rendered smoothed (a new, slightly zoomed copy of that stretch is made and the clip is pointed at it). smoothing is how many frames the camera path is averaged over (default 15; 30 for a walking shot). Takes about real time.", "input_schema": obj(json!({"item_id": item, "smoothing": {"type": "number"}}), &["item_id"])},
        {"name": "auto_color", "description": "Measure a few frames of a clip and set its exposure, contrast, saturation and colour temperature to a balanced look (fixes under- or over-exposure, a colour cast and flat colour; a gentle move, not a style). Replaces the clip's basic colour settings.", "input_schema": obj(json!({"item_id": item}), &["item_id"])},
        {"name": "match_color", "description": "Make a clip look like another one (same brightness, colour balance and colour) by setting its basic colour settings from a comparison of frames of both. item_id is the clip to change, reference_id the one to match. Its wheels, curves, HSL and LUT stay. This alone answers \"match the colour\"; do not follow it with copy_grade, which would replace the shot's own grade.", "input_schema": obj(json!({"item_id": item, "reference_id": item}), &["item_id", "reference_id"])},
        {"name": "sync_by_audio", "description": "Line up clips shot of the same event by their sound. The first clip stays; the others are moved in time so their sound matches it. Clips with no clear match are left alone and reported.", "input_schema": obj(json!({"item_ids": {"type": "array", "items": {"type": "string"}}}), &["item_ids"])},
        {"name": "multicam_cut", "description": "Multicam switching: cut every camera-angle track at a time and show only one angle from there on. angle_tracks are the track names in angle order; angle is 1-based. Call it once at the start (angle 1 at the first clip's time) and then at each switch.", "input_schema": obj(json!({"angle_tracks": {"type": "array", "items": {"type": "string"}}, "angle": {"type": "number"}, "at_sec": {"type": "number"}}), &["angle_tracks", "angle", "at_sec"])},
        {"name": "measure_loudness", "description": "Measure loudness in LUFS (and loudness range, peaks) of one clip, or of the whole mix when item_id is left out. Read-only.", "input_schema": obj(json!({"item_id": item}), &[])},
        {"name": "learn_noise_reduction", "description": "Remove steady background noise (hiss, hum, room tone) from a clip by learning what the noise sounds like from a stretch of the clip that has only noise (at least 0.2 s, about 1 s is better), then adding Noise Reduction using that sample. from_sec and to_sec are seconds into the clip.", "input_schema": obj(json!({"item_id": item, "from_sec": {"type": "number"}, "to_sec": {"type": "number"}, "reduction_db": {"type": "number"}, "sensitivity": {"type": "number"}}), &["item_id", "from_sec", "to_sec"])},
        {"name": "normalize_loudness", "description": "Bring a clip (or, without item_id, the whole sequence by moving every track's level equally) to a target loudness in LUFS: -16 for web and podcasts, -14 for streaming music, -23 for broadcast (EBU R128). Default -16.", "input_schema": obj(json!({"item_id": item, "target_lufs": {"type": "number"}}), &[])},
        {"name": "add_title", "description": "Add a title on a video track (default: the top one). Templates: lower_third (name plate, uses subtitle), centered, caption, minimal (text with an accent line), boxed (text in a bordered plate), quote (large quotation mark, wrapped text, subtitle is the speaker).", "input_schema": obj(json!({"text": {"type": "string"}, "subtitle": {"type": "string"}, "template": {"type": "string", "enum": ["lower_third", "centered", "caption", "minimal", "boxed", "quote"]}, "at_sec": {"type": "number"}, "duration_sec": {"type": "number"}, "track": track}), &["text", "at_sec"])},
        {"name": "edit_title", "description": "Change a title's text, second line, template, size (px at 1080p), fades, outline / shadow / glow (soft dark halo behind the letters, keeps text readable on bright pictures) / letter_spacing (px at 1080p), align (left, center, right), colours (color = the text, outline_color, accent_color = the plate or line of lower_third, minimal, boxed and quote; hex like #d90d12) or font (default, mono, or a font file path; for a font collection (.ttc) add '#' and the style name or face number, e.g. /System/Library/Fonts/Supplemental/Futura.ttc#Condensed ExtraBold).", "input_schema": obj(json!({"item_id": item, "outline": {"type": "number"}, "color": {"type": "string"}, "outline_color": {"type": "string"}, "accent_color": {"type": "string"}, "shadow": {"type": "number"}, "glow": {"type": "number"}, "letter_spacing": {"type": "number"}, "align": {"type": "string"}, "font": {"type": "string"}, "text": {"type": "string"}, "subtitle": {"type": "string"}, "template": {"type": "string", "enum": ["lower_third", "centered", "caption", "minimal", "boxed", "quote"]}, "size": {"type": "number"}, "fade_in_sec": {"type": "number"}, "fade_out_sec": {"type": "number"}}), &["item_id"])},
        {"name": "remove_silences", "description": "Cut the silent stretches out of a clip (a jump-cut tightening), keeping keep_seconds of room either side. Later items on the track move earlier.", "input_schema": obj(json!({"item_id": item, "threshold_db": {"type": "number"}, "min_seconds": {"type": "number"}, "keep_seconds": {"type": "number"}}), &["item_id"])},
        {"name": "copy_grade", "description": "Give other shots the same grade as one shot: its colour settings (exposure, contrast, saturation, white balance...) and colour effects (wheels, curves, HSL, LUT). This REPLACES their own grade (their wheels, curves, HSL, LUT and colour keyframes are lost): use it when asked for the same grade or look, not to match colour (that is match_color). Their other effects stay. to_item_ids lists the shots, or all_shots: true for every video clip.", "input_schema": obj(json!({"from_item_id": item, "to_item_ids": {"type": "array", "items": {"type": "string"}}, "all_shots": {"type": "boolean"}}), &["from_item_id"])},
        {"name": "reset_grade", "description": "Remove a shot's grade: neutral colour settings and no colour effects (wheels, curves, HSL, LUT); its other effects stay.", "input_schema": obj(json!({"item_id": item}), &["item_id"])},
        {"name": "add_track", "description": "Add new empty tracks: kind video (placed above the top video track) or audio (after the last audio track). count (default 1); name is optional (default V3, A3 and so on). Returns the names of the tracks added.", "input_schema": obj(json!({"kind": {"type": "string", "enum": ["video", "audio"]}, "count": {"type": "number"}, "name": {"type": "string"}}), &["kind"])},
        {"name": "set_track", "description": "Hide/mute a track or change its volume.", "input_schema": obj(json!({"track": track, "enabled": {"type": "boolean"}, "volume_db": {"type": "number"}}), &["track"])},
    ]);
    generated_parts(&mut tools);
    tools
}

/// Hue in degrees of an RGB colour (0-255 channels).
fn hue_of(c: [f64; 3]) -> f64 {
    let [r, g, b] = c.map(|v| v / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    if d <= 0.0 {
        return 120.0;
    }
    60.0 * if mx == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    }
}

/// A tool result with notes for the model added (when there are any).
fn with_notes(out: String, notes: &[String]) -> R {
    if notes.is_empty() {
        return Ok(out);
    }
    let mut v: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    v["note"] = json!(notes.join("; "));
    Ok(v.to_string())
}

/// Settings that need more than a name and a range to be used well.
fn effect_note(k: EffectKind) -> &'static str {
    match k {
        EffectKind::ColorWheels => "each -1..1, 0 = unchanged; lift = shadows, gamma = mid-tones, gain = highlights; 0.03-0.15 is a subtle push, 0.3+ is strong",
        EffectKind::Curves => "the output level 0..1 at that input; untouched = 0, 0.25, 0.5, 0.75, 1",
        EffectKind::Hsl => "<band>_hue = shift in degrees (0 = none), <band>_sat = percent (100 = unchanged, 0 = grey), <band>_lum = lightness (0 = unchanged)",
        EffectKind::Lut => "the .cube file goes in path next to kind, not in params",
        EffectKind::Mask => "shape 0 rectangle/1 ellipse/2 drawn polygon given as points [[x,y],...] in % of the frame, 3 to 64 of them",
        EffectKind::ChromaKey => "key colour 0-255 (default green); a new key is refined to the actual backdrop colour in the shot near the one given, like an eyedropper; edge_shrink px (negative grows)",
        EffectKind::Letterbox => "aspect = width/height left visible, e.g. 2.35; put it on an adjustment layer so the bars stay still",
        EffectKind::DropShadow => "distance and softness in px at 1080p; only shows around layers smaller than the frame or with transparency (titles, shapes, scaled clips)",
        EffectKind::CornerPin => "corners in % of the frame; untouched = 0,0 100,0 100,100 0,100",
        EffectKind::LowFrameRate => "frames per second actually shown",
        EffectKind::Gradient => "corner colours 0-255",
        _ => "",
    }
}

/// Every picture effect by library category with its settings and ranges, for the tool descriptions.
fn effect_catalog() -> String {
    let mut out = String::new();
    for cat in Category::ALL {
        let kinds: Vec<EffectKind> = EffectKind::ALL
            .into_iter()
            .filter(|k| k.category() == cat && k.category() != Category::Sound)
            .collect();
        if kinds.is_empty() {
            continue;
        }
        out += &format!("{}: ", cat.label());
        let entries: Vec<String> = kinds
            .iter()
            .map(|k| {
                let ps: Vec<String> = k
                    .params()
                    .iter()
                    .map(|d| format!("{} {}..{} ({})", param_key(d.name), d.min, d.max, d.default))
                    .collect();
                let note = effect_note(*k);
                format!(
                    "{} = {} [{}{}]",
                    k.key(),
                    k.about().to_lowercase(),
                    ps.join(", "),
                    if note.is_empty() {
                        String::new()
                    } else {
                        format!("; {note}")
                    }
                )
            })
            .collect();
        out += &entries.join("; ");
        out += ". ";
    }
    out
}

/// The parts of the tool list that come from the model's catalogues, so a new effect or transition is known to
/// CoCaptain the moment it exists.
fn generated_parts(tools: &mut Value) {
    let Some(list) = tools.as_array_mut() else {
        return;
    };
    for t in list {
        match t["name"].as_str() {
            Some("set_effects") => {
                t["description"] = json!(format!(
                    "Replace ALL the effects of a clip, shape or adjustment layer with this list (an empty list removes them; effects not in the list are lost — to change or add a single effect use edit_effect). Picture effects run in order before the transform. Each entry: {{kind, enabled?, params?: {{setting: number}}, path?, points?}}; unspecified settings keep their defaults. Settings are written as shown: name min..max (default). Picture effects — {}Sound effects: eq (low_gain, low_freq, mid_1_gain, mid_1_freq, mid_1_q, mid_2_gain, mid_2_freq, mid_2_q, high_gain, high_freq; gains in dB), compressor (threshold_db, ratio, attack_ms, release_ms, makeup_db), limiter (ceiling_db, release_ms), de_esser (threshold_db, reduction_%), noise_reduction (reduction_db, sensitivity_%), ducking (threshold_db, reduction_db, attack_ms, release_ms).",
                    effect_catalog()
                ));
            }
            Some("set_transition") => {
                let kinds: Vec<String> = TransitionKind::ALL
                    .iter()
                    .map(|k| format!("{} ({})", k.key(), k.about().to_lowercase()))
                    .collect();
                t["description"] = json!(format!(
                    "Put a transition on the cut between a clip and the clip right before it (item_id is the LATER clip). kind: {}; or none to remove. Whips and flashes work best short (0.15-0.4 s). The overlap is centred on the cut and does not change any lengths; it cannot be longer than twice the shorter of the two clips.",
                    kinds.join(", ")
                ));
                let mut keys: Vec<&str> = TransitionKind::ALL.iter().map(|k| k.key()).collect();
                keys.push("none");
                t["input_schema"]["properties"]["kind"]["enum"] = json!(keys);
            }
            _ => {}
        }
    }
}

/// The effects on an item, briefly: which kinds, and which are switched off.
fn effect_kind(name: &str) -> Result<EffectKind, String> {
    EffectKind::from_key(name).ok_or_else(|| {
        format!(
            "unknown effect '{name}'; use one of {}",
            EffectKind::ALL
                .iter()
                .map(|k| k.key())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

/// One effect from its JSON description (`kind`, `params`, `enabled`, `path`, `points`). Starting from `base`
/// (an effect already on the item) only the settings given change; without it, the rest are the defaults.
fn effect_from_json(e: &Value, base: Option<Effect>) -> Result<Effect, String> {
    let name = req_text(e, "kind")?;
    let kind = effect_kind(name)?;
    let mut fx = base
        .filter(|b| b.kind == kind)
        .unwrap_or_else(|| Effect::new(kind));
    if let Some(en) = flag(e, "enabled") {
        fx.enabled = en;
    }
    // The file path is accepted at the top level or, as models often send it, inside params.
    let in_params = e
        .get("params")
        .and_then(|p| p.get("path"))
        .and_then(Value::as_str);
    if let Some(path) = text(e, "path").or(in_params) {
        fx.text = path.to_string();
    }
    if let Some(pts) = e.get("points").and_then(Value::as_array) {
        let pts: Option<Vec<(f64, f64)>> = pts
            .iter()
            .map(|p| {
                let a = p.as_array()?;
                Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
            })
            .collect();
        fx.text = polygon_text(&pts.ok_or("points are [x, y] pairs in percent")?);
    }
    if let Some(ps) = e.get("params").and_then(Value::as_object) {
        for (k, v) in ps.iter().filter(|(k, _)| *k != "path") {
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
            let val = v.as_f64().ok_or("parameters are numbers")?;
            let d = &kind.params()[idx];
            if !(d.min..=d.max).contains(&val) {
                return Err(format!(
                    "{name} {k} = {val} is outside {}..{} (default {})",
                    d.min, d.max, d.default
                ));
            }
            fx.params[idx] = val;
        }
    }
    fx.check().map_err(|m| format!("{name}: {m}"))?;
    Ok(fx)
}

/// Animated properties with their keyframes, in the form set_keyframes takes, so they can be edited and sent
/// back: "keyframes": {"exposure": [{"at_sec": 0.0, "value": -1.0, "interp": "ease_out"}, ...]}. Long
/// animations (motion tracking) only give their count.
fn keyframes_summary(o: &mut serde_json::Map<String, Value>, anim: &Anim, fps: f64) {
    if anim.is_empty() {
        return;
    }
    let m: serde_json::Map<String, Value> = anim
        .0
        .iter()
        .map(|(k, keys)| {
            let v = if keys.len() > 12 {
                json!({"count": keys.len()})
            } else {
                Value::Array(
                    keys.iter()
                        .map(|key| {
                            let mut e =
                                json!({"at_sec": r2(key.t as f64 / fps), "value": r2(key.v)});
                            if key.interp != Interp::Linear {
                                e["interp"] = json!(interp_name(key.interp));
                            }
                            e
                        })
                        .collect(),
                )
            };
            (k.clone(), v)
        })
        .collect();
    o.insert("keyframes".into(), Value::Object(m));
}

fn interp_name(i: Interp) -> &'static str {
    match i {
        Interp::Hold => "hold",
        Interp::Linear => "linear",
        Interp::EaseIn => "ease_in",
        Interp::EaseOut => "ease_out",
        Interp::EaseInOut => "ease_in_out",
        // a curve drawn in the editor; ease_in_out is the nearest shape set_keyframes can send back
        Interp::Bezier(..) => "ease_in_out",
    }
}

fn item_anim(item: &Item) -> &Anim {
    static NONE: std::sync::OnceLock<Anim> = std::sync::OnceLock::new();
    match item {
        Item::Clip(c) => &c.anim,
        Item::Title(t) => &t.anim,
        Item::Gap(_) => NONE.get_or_init(Anim::default),
    }
}

fn effects_summary(o: &mut serde_json::Map<String, Value>, effects: &[Effect]) {
    if effects.is_empty() {
        return;
    }
    let list: Vec<Value> = effects
        .iter()
        .map(|e| {
            let mut v = json!({"kind": e.kind.key()});
            if !e.enabled {
                v["enabled"] = json!(false);
            }
            // the settings that differ from the defaults, under the names set_effects takes
            let mut params = serde_json::Map::new();
            for (d, val) in e.kind.params().iter().zip(&e.params) {
                if (val - d.default).abs() > 1e-9 {
                    params.insert(d.name.to_lowercase().replace(' ', "_"), json!(r2(*val)));
                }
            }
            if !params.is_empty() {
                v["params"] = Value::Object(params);
            }
            if e.kind == EffectKind::Lut && !e.text.is_empty() {
                v["path"] = json!(e.text);
            }
            if let Some(p) = captaine_plugins::PluginRef::parse(&e.text) {
                v["plugin"] = json!(p.id);
            }
            v
        })
        .collect();
    o.insert("effects".into(), Value::Array(list));
}
