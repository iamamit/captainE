use std::collections::HashMap;
use std::sync::Arc;

use captaine_model::{Id, Item, Project};
use captaine_time::{Rational, RationalTime, Rounding};

use crate::decode::decode_pcm;
use crate::AudioError;

/// Decode this much of a clip at a time so long recordings never sit in memory whole.
const WINDOW_SECS: usize = 30;

struct MixClip {
    id: Id,
    path: String,
    /// where the clip starts and how long it is, in output samples
    start: i64,
    len: i64,
    /// seconds into the source file at the clip's first sample
    src_secs: f64,
    gain: f32,
    pan: f64,
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
            for (item, start) in track.items.iter().zip(track.item_starts()) {
                let Item::Clip(c) = item else { continue };
                let Some(m) = project.media.get(&c.media) else {
                    continue;
                };
                if !c.enabled || c.audio.muted || !m.has_audio {
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
                    gain: db_to_gain(c.audio.gain_db + track.gain_db),
                    pan: c.audio.pan,
                    fade_in: to_samples(c.audio.fade_in),
                    fade_out: to_samples(c.audio.fade_out),
                });
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
        let secs = c.src_secs + from as f64 / self.sample_rate as f64;
        let pcm = Arc::new(decode_pcm(&c.path, secs, n, self.sample_rate)?);
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
            let (gain, pan, fi, fo) = {
                let c = &self.clips[ci];
                (c.gain, c.pan, c.fade_in, c.fade_out)
            };
            let (gl, gr) = (
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
            );
            let mut pos = a - cs; // clip-relative
            let stop = b - cs;
            while pos < stop {
                let w = pos / wl;
                let win = self.window(ci, w)?;
                let w_end = ((w + 1) * wl).min(stop);
                for p in pos..w_end {
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
