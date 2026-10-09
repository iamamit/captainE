use std::collections::HashMap;
use std::sync::Arc;

use captaine_model::{value_at, Clip, Id, Item, Keyframe, Project};
use captaine_time::{Rational, RationalTime, Rounding};

use crate::decode::decode_filtered;
use crate::AudioError;

/// Decode this much of a clip at a time so long recordings never sit in memory whole.
const WINDOW_SECS: usize = 30;

struct MixClip {
    id: Id,
    path: String,
    /// where the clip starts and how long it is, in output samples
    start: i64,
    len: i64,
    /// seconds into the source file where the clip's window begins, and how long the window is
    src_secs: f64,
    span_secs: f64,
    speed: f64,
    reverse: bool,
    gain_db: f64,
    track_db: f64,
    pan: f64,
    /// Keyframes of volume (dB) and pan, times in output samples from the start of the clip
    vol_keys: Vec<Keyframe>,
    pan_keys: Vec<Keyframe>,
    fade_in: i64,
    fade_out: i64,
}

/// Mixes every audio-bearing clip of a timeline into stereo f32 at the project sample rate.
///
/// Clip boundaries are rounded once from cumulative timeline time, so neighbouring clips never overlap
/// or leave a one-sample hole.
pub struct Mixer {
    sample_rate: u32,
    clips: Vec<MixClip>,
    total: i64,
    windows: HashMap<(Id, i64), Arc<Vec<f32>>>,
}

/// Left and right gain for a clip at `gain`, balanced by `pan` (-1 left .. 1 right): unity in the middle.
fn left_right(gain: f32, pan: f64) -> (f32, f32) {
    (
        gain * if pan > 0.0 {
            (pan * std::f64::consts::FRAC_PI_2).cos() as f32
        } else {
            1.0
        },
        gain * if pan < 0.0 {
            (-pan * std::f64::consts::FRAC_PI_2).cos() as f32
        } else {
            1.0
        },
    )
}

fn db_to_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

impl Mixer {
    pub fn new(project: &Project, timeline: Id) -> Result<Mixer, AudioError> {
        let tl = project.timeline(timeline).ok_or(AudioError::NoTimeline)?;
        let sr = project.settings.sample_rate;
        let sr_rate = Rational::new(sr as i64, 1).expect("positive sample rate");
        let to_samples = |ticks: i64| -> i64 {
            RationalTime::new(ticks, tl.rate)
                .rescale(sr_rate, Rounding::Nearest)
                .map_or(0, |t| t.value)
        };
        let mut clips = Vec::new();
        for track in tl.tracks.iter().filter(|t| t.enabled) {
            // the clip just before, if it made sound: (item index, index in `clips`)
            let mut prev: Option<(usize, usize)> = None;
            for (ii, (item, start)) in track.items.iter().zip(track.item_starts()).enumerate() {
                let Item::Clip(c) = item else { continue };
                let Some(m) = project.media.get(&c.media) else {
                    continue;
                };
                if !c.enabled || c.audio.muted || !m.has_audio || c.retime.freeze {
                    prev = None;
                    continue;
                }
                let end = start + c.duration.value;
                let (s, e) = (to_samples(start), to_samples(end));
                clips.push(MixClip {
                    id: c.id,
                    path: m.path.clone(),
                    start: s,
                    len: e - s,
                    src_secs: c.source_start.to_seconds_f64(),
                    span_secs: c.source_span() as f64 / c.source_start.rate.as_f64(),
                    speed: c.retime.speed,
                    reverse: c.retime.reverse,
                    gain_db: c.audio.gain_db,
                    track_db: track.gain_db,
                    pan: c.audio.pan,
                    vol_keys: c
                        .anim
                        .keys("volume")
                        .iter()
                        .map(|k| Keyframe {
                            t: to_samples(k.t),
                            ..*k
                        })
                        .collect(),
                    pan_keys: c
                        .anim
                        .keys("pan")
                        .iter()
                        .map(|k| Keyframe {
                            t: to_samples(k.t),
                            ..*k
                        })
                        .collect(),
                    fade_in: to_samples(c.audio.fade_in),
                    fade_out: to_samples(c.audio.fade_out),
                });
                let this = clips.len() - 1;
                // a transition from the previous clip: both play through the overlap, one fading out
                // as the other fades in
                if let (Some(t), Some((pi, pc))) = (c.transition, prev) {
                    if pi + 1 == ii {
                        if let Item::Clip(pclip) = &track.items[pi] {
                            let d = t
                                .duration
                                .min(2 * pclip.duration.value.min(c.duration.value));
                            let (a, b) = (
                                to_samples(start) - to_samples(start - d / 2),
                                to_samples(start + (d - d / 2)) - to_samples(start),
                            );
                            let simple = |x: &Clip| !x.retime.reverse && x.retime.speed == 1.0;
                            if d >= 2 && simple(c) && simple(pclip) {
                                // the incoming clip can only start earlier if its source has room
                                let room = (clips[this].src_secs * sr as f64).floor() as i64;
                                let a = a.min(room.max(0));
                                {
                                    let out = &mut clips[pc];
                                    out.len += b;
                                    out.fade_out = out.fade_out.max(a + b);
                                }
                                let inc = &mut clips[this];
                                inc.start -= a;
                                inc.len += a;
                                inc.src_secs -= a as f64 / sr as f64;
                                inc.fade_in = inc.fade_in.max(a + b);
                                for k in inc.vol_keys.iter_mut().chain(inc.pan_keys.iter_mut()) {
                                    k.t += a;
                                }
                            }
                        }
                    }
                }
                prev = Some((ii, this));
            }
        }
        let total = to_samples(tl.duration().value);
        Ok(Mixer {
            sample_rate: sr,
            clips,
            total,
            windows: HashMap::new(),
        })
    }

    /// Length of the timeline in samples.
    pub fn total_samples(&self) -> i64 {
        self.total
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn window(&mut self, ci: usize, w: i64) -> Result<Arc<Vec<f32>>, AudioError> {
        let id = self.clips[ci].id;
        if let Some(v) = self.windows.get(&(id, w)) {
            return Ok(v.clone());
        }
        let wl = (WINDOW_SECS * self.sample_rate as usize) as i64;
        let c = &self.clips[ci];
        let from = w * wl;
        let n = (c.len - from).min(wl).max(0) as usize;
        let sr = self.sample_rate as f64;
        let secs = if c.reverse {
            // output runs from the top of the source window downwards
            c.src_secs + c.span_secs - (from + n as i64) as f64 * c.speed / sr
        } else {
            c.src_secs + from as f64 * c.speed / sr
        };
        let pcm = Arc::new(decode_filtered(
            &c.path,
            secs,
            n,
            self.sample_rate,
            2,
            c.speed,
            c.reverse,
        )?);
        self.windows.insert((id, w), pcm.clone());
        Ok(pcm)
    }

    /// `count` stereo frames starting at output sample `start`, interleaved. Not clamped.
    pub fn render(&mut self, start: i64, count: usize) -> Result<Vec<f32>, AudioError> {
        let end = start + count as i64;
        let mut out = vec![0.0f32; count * 2];
        let wl = (WINDOW_SECS * self.sample_rate as usize) as i64;
        for ci in 0..self.clips.len() {
            let (cs, cl) = (self.clips[ci].start, self.clips[ci].len);
            let (a, b) = (start.max(cs), end.min(cs + cl));
            if a >= b {
                continue;
            }
            let (fi, fo) = (self.clips[ci].fade_in, self.clips[ci].fade_out);
            let animated =
                !self.clips[ci].vol_keys.is_empty() || !self.clips[ci].pan_keys.is_empty();
            let (mut gl, mut gr) = {
                let c = &self.clips[ci];
                left_right(db_to_gain(c.gain_db + c.track_db), c.pan)
            };
            let mut pos = a - cs; // clip-relative
            let stop = b - cs;
            while pos < stop {
                let w = pos / wl;
                let win = self.window(ci, w)?;
                let w_end = ((w + 1) * wl).min(stop);
                for p in pos..w_end {
                    if animated {
                        let c = &self.clips[ci];
                        let db = value_at(&c.vol_keys, p).unwrap_or(c.gain_db);
                        let pan = value_at(&c.pan_keys, p).unwrap_or(c.pan).clamp(-1.0, 1.0);
                        (gl, gr) = left_right(db_to_gain(db + c.track_db), pan);
                    }
                    let mut g = 1.0f32;
                    if p < fi {
                        g = p as f32 / fi as f32;
                    }
                    if p >= cl - fo {
                        g = g.min((cl - p) as f32 / fo as f32);
                    }
                    let si = ((p - w * wl) * 2) as usize;
                    let oi = ((cs + p - start) * 2) as usize;
                    out[oi] += win[si] * gl * g;
                    out[oi + 1] += win[si + 1] * gr * g;
                }
                pos = w_end;
            }
        }
        // keep memory bounded: forget windows that lie entirely before this block
        let sr = self.sample_rate as i64;
        let clips = &self.clips;
        self.windows.retain(|(id, w), _| {
            clips.iter().find(|c| c.id == *id).is_some_and(|c| {
                c.start + (w + 1) * wl > start && c.start + w * wl < end + sr * WINDOW_SECS as i64
            })
        });
        Ok(out)
    }
}
