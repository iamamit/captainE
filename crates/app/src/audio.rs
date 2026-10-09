//! Audio output: a producer thread keeps a ring buffer filled from the mixer and a cpal stream drains it.
//! The number of frames actually played is the playback clock, so picture follows sound.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use captaine_audio::Mixer;
use captaine_model::{Id, Project};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

struct Shared {
    ring: Mutex<VecDeque<f32>>,
    played: AtomicI64,
    running: AtomicBool,
    generation: AtomicU64,
    /// Peak levels per timeline track plus the master (last), tagged with the first frame they cover
    meters: Mutex<VecDeque<(i64, Vec<f32>)>>,
}

pub struct AudioEngine {
    _stream: cpal::Stream,
    shared: Arc<Shared>,
    rate: u32,
    start_secs: f64,
}

/// Keep this much decoded audio queued ahead of the speakers.
const AHEAD_SECS: f64 = 1.0;
const BLOCK_FRAMES: usize = 4800;

impl AudioEngine {
    /// `None` when there is no usable output device; playback then runs on the wall clock, silently.
    pub fn new() -> Option<AudioEngine> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let config: cpal::StreamConfig = supported.into();
        let shared = Arc::new(Shared {
            ring: Mutex::new(VecDeque::new()),
            played: AtomicI64::new(0),
            running: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            meters: Mutex::new(VecDeque::new()),
        });
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, channels, shared.clone())?,
            cpal::SampleFormat::I16 => build::<i16>(&device, config, channels, shared.clone())?,
            _ => return None,
        };
        stream.play().ok()?;
        Some(AudioEngine {
            _stream: stream,
            shared,
            rate,
            start_secs: 0.0,
        })
    }

    /// Start playing the timeline's mix from `start_secs`.
    pub fn start(&mut self, project: &Project, timeline: Id, start_secs: f64) {
        self.stop();
        let generation = self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.start_secs = start_secs;
        self.shared.played.store(0, Ordering::SeqCst);
        self.shared.ring.lock().expect("ring").clear();
        self.shared.meters.lock().expect("meters").clear();
        self.shared.running.store(true, Ordering::SeqCst);
        let mut p = project.clone();
        p.settings.sample_rate = self.rate;
        let (shared, rate) = (self.shared.clone(), self.rate);
        std::thread::Builder::new()
            .name("audio-producer".into())
            .spawn(move || produce(shared, generation, p, timeline, start_secs, rate))
            .expect("spawn audio producer");
    }

    pub fn stop(&mut self) {
        self.shared.running.store(false, Ordering::SeqCst);
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        self.shared.ring.lock().expect("ring").clear();
    }

    /// Where playback has got to, in timeline seconds.
    pub fn position_secs(&self) -> f64 {
        self.start_secs + self.shared.played.load(Ordering::SeqCst) as f64 / self.rate as f64
    }

    /// Peak levels (per timeline track, then master) of the block being heard now.
    pub fn levels(&self) -> Option<Vec<f32>> {
        let played = self.shared.played.load(Ordering::SeqCst);
        let mut m = self.shared.meters.lock().ok()?;
        let mut latest = None;
        while m.front().is_some_and(|(at, _)| *at <= played) {
            latest = m.pop_front().map(|(_, v)| v);
        }
        latest
    }

    /// Frames of sound ready to play: false for the first moments after `start` while the mixer warms up.
    pub fn is_primed(&self) -> bool {
        self.shared.played.load(Ordering::SeqCst) > 0
    }
}

fn produce(
    shared: Arc<Shared>,
    generation: u64,
    project: Project,
    timeline: Id,
    start_secs: f64,
    rate: u32,
) {
    let mut mixer = Mixer::new(&project, timeline).ok();
    let total = mixer.as_ref().map_or(0, |m| m.total_samples());
    let mut pos = (start_secs * rate as f64).round() as i64;
    let first = pos;
    let want = (AHEAD_SECS * rate as f64) as usize * 2;
    while shared.generation.load(Ordering::SeqCst) == generation {
        let queued = shared.ring.lock().expect("ring").len();
        if queued >= want {
            std::thread::sleep(Duration::from_millis(8));
            continue;
        }
        // past the end (or nothing to mix) we keep the clock running with silence
        let mut peaks = Vec::new();
        let block = match (&mut mixer, pos < total) {
            (Some(m), true) => m
                .render_metered(pos, BLOCK_FRAMES, Some(&mut peaks))
                .unwrap_or_else(|_| vec![0.0; BLOCK_FRAMES * 2]),
            _ => vec![0.0; BLOCK_FRAMES * 2],
        };
        let master = block.iter().fold(0f32, |m, s| m.max(s.abs()));
        peaks.push(master);
        {
            let mut m = shared.meters.lock().expect("meters");
            m.push_back((pos - first, peaks));
            while m.len() > 64 {
                m.pop_front();
            }
        }
        pos += BLOCK_FRAMES as i64;
        if shared.generation.load(Ordering::SeqCst) != generation {
            break;
        }
        shared
            .ring
            .lock()
            .expect("ring")
            .extend(block.iter().map(|s| s.clamp(-1.0, 1.0)));
    }
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    shared: Arc<Shared>,
) -> Option<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let silence = T::from_sample(0.0f32);
                let mut frames = 0i64;
                let running = shared.running.load(Ordering::Relaxed);
                let mut ring = shared.ring.try_lock().ok();
                for frame in data.chunks_mut(channels.max(1)) {
                    let lr = match (&mut ring, running) {
                        (Some(r), true) if r.len() >= 2 => {
                            frames += 1;
                            Some((r.pop_front().unwrap_or(0.0), r.pop_front().unwrap_or(0.0)))
                        }
                        _ => None,
                    };
                    for (i, s) in frame.iter_mut().enumerate() {
                        *s = match (lr, i, channels) {
                            (Some((l, r)), _, 1) => T::from_sample((l + r) * 0.5),
                            (Some((l, _)), 0, _) => T::from_sample(l),
                            (Some((_, r)), 1, _) => T::from_sample(r),
                            _ => silence,
                        };
                    }
                }
                if frames > 0 {
                    shared.played.fetch_add(frames, Ordering::SeqCst);
                }
            },
            |_err| {},
            None,
        )
        .ok()
}
