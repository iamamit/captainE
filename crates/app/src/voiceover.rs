//! Voice-over: record the microphone while the timeline plays, then drop the take on an audio track.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use captaine_model::{Id, TrackKind};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::app::App;

/// A running microphone capture (mono, at the device's rate).
pub struct Recorder {
    _stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    peak: Arc<AtomicU32>,
    rate: u32,
    /// The timeline frame the take starts at.
    pub at: i64,
}

impl Recorder {
    pub fn start(at: i64) -> Result<Recorder, String> {
        let device = cpal::default_host().default_input_device().ok_or(
            "there is no microphone; allow captainE to use it in System Settings > Privacy",
        )?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let config: cpal::StreamConfig = supported.into();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let peak = Arc::new(AtomicU32::new(0));
        let stream = match cpal::default_host()
            .default_input_device()
            .and_then(|d| d.default_input_config().ok())
            .map(|c| c.sample_format())
        {
            Some(cpal::SampleFormat::F32) => {
                build::<f32>(&device, config, channels, samples.clone(), peak.clone())
            }
            Some(cpal::SampleFormat::I16) => {
                build::<i16>(&device, config, channels, samples.clone(), peak.clone())
            }
            _ => Err("this microphone's sample format is not supported".into()),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Recorder {
            _stream: stream,
            samples,
            peak,
            rate,
            at,
        })
    }

    pub fn seconds(&self) -> f64 {
        self.samples.lock().map_or(0, |s| s.len()) as f64 / self.rate as f64
    }

    /// The loudest sample since the last call, 0..1, for the meter.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    pub fn finish(self) -> (Vec<f32>, u32) {
        let s = self.samples.lock().map(|s| s.clone()).unwrap_or_default();
        (s, self.rate)
    }
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    out: Arc<Mutex<Vec<f32>>>,
    peak: Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut p = f32::from_bits(peak.load(Ordering::Relaxed));
                if let Ok(mut buf) = out.lock() {
                    for frame in data.chunks(channels.max(1)) {
                        let m = frame
                            .iter()
                            .map(|s| <f32 as cpal::FromSample<T>>::from_sample_(*s))
                            .sum::<f32>()
                            / frame.len() as f32;
                        p = p.max(m.abs());
                        buf.push(m);
                    }
                }
                peak.store(p.to_bits(), Ordering::Relaxed);
            },
            |_err| {},
            None,
        )
        .map_err(|e| e.to_string())
}

/// A 16-bit mono WAV file.
pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b)
}

/// Where takes are kept: next to the project when it is saved, otherwise in Music.
fn takes_dir(project_file: Option<&Path>) -> PathBuf {
    match project_file.and_then(|p| p.parent()) {
        Some(d) if !d.as_os_str().is_empty() => d.join("Voice-overs"),
        _ => std::env::var_os("HOME")
            .map_or_else(std::env::temp_dir, PathBuf::from)
            .join("Music")
            .join("captainE voice-overs"),
    }
}

impl App {
    pub fn voiceover_toggle(&mut self) {
        match self.voiceover.take() {
            None => {
                let at = self.playhead;
                match Recorder::start(at) {
                    Ok(r) => {
                        self.voiceover = Some(r);
                        if !self.playing {
                            self.toggle_play();
                        }
                    }
                    Err(e) => self.toast(e, true),
                }
            }
            Some(r) => {
                if self.playing {
                    self.toggle_play();
                }
                let (at, (samples, rate)) = (r.at, r.finish());
                self.place_take(&samples, rate, at);
            }
        }
    }

    /// Save a take as a WAV, import it and put it on an audio track at `at` (a new track if all are taken).
    pub fn place_take(&mut self, samples: &[f32], rate: u32, at: i64) {
        if samples.len() < rate as usize / 10 {
            self.toast("That take was too short to keep", true);
            return;
        }
        let dir = takes_dir(self.path.as_deref());
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let file = dir.join(format!("voiceover-{stamp}.wav"));
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| write_wav(&file, samples, rate))
        {
            self.toast(format!("Could not save the take: {e}"), true);
            return;
        }
        let m = match captaine_media::media_ref(&file, Id::new()) {
            Ok(m) => m,
            Err(e) => {
                self.toast(format!("Could not read the take: {e}"), true);
                return;
            }
        };
        let id = m.id;
        if !self.do_op(captaine_ops::Op::AddMedia { media: m }) {
            return;
        }
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        let clear = |t: &captaine_model::Track| {
            t.kind == TrackKind::Audio && !t.locked && t.duration_ticks() <= at
        };
        let track = tl.tracks.iter().rev().find(|t| clear(t)).map(|t| t.id);
        let track = match track {
            Some(t) => t,
            None => {
                let t = captaine_model::Track::new(
                    Id::new(),
                    format!("Voice-over {}", tl.tracks.len() + 1),
                    TrackKind::Audio,
                );
                let tid = t.id;
                if !self.do_op(captaine_ops::Op::AddTrack {
                    timeline: tl.id,
                    index: tl.tracks.len(),
                    track: t,
                }) {
                    return;
                }
                tid
            }
        };
        self.place_media(id, Some(track), Some(at));
        self.toast("Voice-over added", false);
    }
}
