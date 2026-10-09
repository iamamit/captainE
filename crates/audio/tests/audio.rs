//! Mixer behaviour against analytic sine waves. Skipped when FFmpeg is not installed.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::process::Command;

use captaine_audio::*;
use captaine_media::media_ref;
use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

const SR: u32 = 48_000;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("captaine-audio-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// Float WAV: `amp * sin(2 pi freq t)`, stereo-identical, `secs` long.
fn sine(path: &Path, freq: f64, amp: f64, secs: f64) {
    let src = format!("aevalsrc={amp}*sin(2*PI*{freq}*t):s={SR}:d={secs}:c=stereo");
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &src,
            "-c:a",
            "pcm_f32le",
        ])
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

fn t(v: i64) -> RationalTime {
    RationalTime::new(v, Rational::FPS_30)
}

struct Fx {
    ed: Editor,
    tl: Id,
    a1: Id,
    a2: Id,
}

fn fx() -> Fx {
    let mut ed = Editor::new(Project::new(ProjectSettings {
        sample_rate: SR,
        ..ProjectSettings::default()
    }));
    let (tl, a1, a2) = (Id::new(), Id::new(), Id::new());
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "Main", Rational::FPS_30),
        },
    )
    .unwrap();
    for (i, id) in [a1, a2].into_iter().enumerate() {
        let tr = Track::new(id, format!("A{}", i + 1), TrackKind::Audio);
        ed.apply(
            Author::User,
            Op::AddTrack {
                timeline: tl,
                index: i,
                track: tr,
            },
        )
        .unwrap();
    }
    Fx { ed, tl, a1, a2 }
}

fn media(f: &mut Fx, p: &Path) -> MediaRef {
    let m = media_ref(p, Id::new()).unwrap();
    assert!(m.has_audio && !m.has_video);
    f.ed.apply(Author::User, Op::AddMedia { media: m.clone() })
        .unwrap();
    m
}

/// A clip whose source starts at `src_secs` seconds (audio-only media tick at the sample rate).
fn clip(m: &MediaRef, src_secs: f64, dur_ticks: i64) -> Clip {
    let src = RationalTime::new((src_secs * SR as f64) as i64, m.duration.unwrap().rate);
    Clip::new(Id::new(), "a", m.id, src, t(dur_ticks))
}

fn place(f: &mut Fx, track: usize, at: i64, c: Clip) -> Id {
    let id = c.id;
    let track = [f.a1, f.a2][track];
    let op = edit::overwrite(f.ed.project(), track, t(at), c).unwrap();
    f.ed.apply(Author::User, op).unwrap();
    id
}

fn rms(s: &[f32], ch: usize) -> f64 {
    let v: Vec<f64> = s.chunks(2).map(|f| f[ch] as f64).collect();
    (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt()
}

fn mix(f: &Fx) -> Vec<f32> {
    let mut m = Mixer::new(f.ed.project(), f.tl).unwrap();
    let n = m.total_samples() as usize;
    m.render(0, n).unwrap()
}

const HALF: f64 = 0.5 / std::f64::consts::SQRT_2; // rms of a 0.5-amplitude sine

#[test]
fn places_audio_at_the_right_samples() {
    if !have_ffmpeg() {
        return;
    }
    let p = scratch("s440.wav");
    sine(&p, 440.0, 0.5, 4.0);
    let mut f = fx();
    let m = media(&mut f, &p);
    // 2 s of the sine starting 0.5 s into the file, placed at 1 s (tick 30..90)
    place(&mut f, 0, 30, clip(&m, 0.5, 60));
    let out = mix(&f);
    assert_eq!(out.len(), 3 * SR as usize * 2);
    assert!(
        out[..SR as usize * 2].iter().all(|s| *s == 0.0),
        "silence before the clip"
    );
    // sample-accurate: output sample n (n >= SR) is the source at 0.5 s + (n - SR)
    for n in [
        SR as usize,
        SR as usize + 1,
        2 * SR as usize + 12345,
        3 * SR as usize - 1,
    ] {
        let src_n = n - SR as usize + (0.5 * SR as f64) as usize;
        let want = 0.5 * (2.0 * PI * 440.0 * src_n as f64 / SR as f64).sin();
        assert!(
            (out[n * 2] as f64 - want).abs() < 1e-4,
            "sample {n}: {} vs {want}",
            out[n * 2]
        );
        assert_eq!(out[n * 2], out[n * 2 + 1]);
    }
}

#[test]
fn gain_pan_fades_mute() {
    if !have_ffmpeg() {
        return;
    }
    let p = scratch("s440b.wav");
    sine(&p, 440.0, 0.5, 4.0);
    let mut f = fx();
    let m = media(&mut f, &p);
    let c = clip(&m, 0.0, 90); // 3 s
    let id = place(&mut f, 0, 0, c);
    let base = rms(&mix(&f), 0);
    assert!((base - HALF).abs() < 0.002, "{base}");

    let set = |f: &mut Fx, audio: AudioProps| {
        f.ed.apply(Author::User, Op::SetAudio { item: id, audio })
            .unwrap()
    };
    let flat = AudioProps::default();

    set(
        &mut f,
        AudioProps {
            gain_db: -6.0206,
            ..flat
        },
    );
    assert!((rms(&mix(&f), 0) / base - 0.5).abs() < 0.002);

    set(&mut f, AudioProps { pan: 1.0, ..flat });
    let out = mix(&f);
    assert!(
        rms(&out, 0) < 1e-4 && (rms(&out, 1) - base).abs() < 0.002,
        "hard right"
    );
    set(&mut f, AudioProps { pan: -0.5, ..flat });
    let out = mix(&f);
    assert!((rms(&out, 0) - base).abs() < 0.002);
    assert!((rms(&out, 1) / base - (0.5 * PI / 2.0).cos()).abs() < 0.005);

    // 1 s fade in, 1 s fade out
    set(
        &mut f,
        AudioProps {
            fade_in: 30,
            fade_out: 30,
            ..flat
        },
    );
    let out = mix(&f);
    let sr = SR as usize;
    let peak = |a: usize, b: usize| out[a * 2..b * 2].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak(0, sr / 10) < 0.5 * 0.1 + 0.01, "first 0.1 s is quiet");
    assert!(peak(sr + 100, 2 * sr - 100) > 0.49, "middle is full level");
    assert!(
        peak(3 * sr - sr / 10, 3 * sr) < 0.5 * 0.1 + 0.01,
        "last 0.1 s is quiet"
    );
    assert!(out[0] == 0.0);

    set(
        &mut f,
        AudioProps {
            muted: true,
            ..flat
        },
    );
    assert!(mix(&f).iter().all(|s| *s == 0.0));
    set(&mut f, flat);
    f.ed.apply(
        Author::User,
        Op::SetTrack {
            track: f.a1,
            enabled: false,
            gain_db: 0.0,
        },
    )
    .unwrap();
    assert!(mix(&f).iter().all(|s| *s == 0.0));
    f.ed.apply(
        Author::User,
        Op::SetTrack {
            track: f.a1,
            enabled: true,
            gain_db: -6.0206,
        },
    )
    .unwrap();
    assert!(
        (rms(&mix(&f), 0) / base - 0.5).abs() < 0.002,
        "track gain applies"
    );
}

#[test]
fn tracks_sum() {
    if !have_ffmpeg() {
        return;
    }
    let p = scratch("s440c.wav");
    sine(&p, 440.0, 0.25, 2.0);
    let mut f = fx();
    let m = media(&mut f, &p);
    place(&mut f, 0, 0, clip(&m, 0.0, 60));
    let one = rms(&mix(&f), 0);
    place(&mut f, 1, 0, clip(&m, 0.0, 60));
    let two = rms(&mix(&f), 0);
    assert!((two / one - 2.0).abs() < 0.001, "{one} -> {two}");
}

#[test]
fn long_clips_are_seamless_across_decode_windows() {
    if !have_ffmpeg() {
        return;
    }
    let p = scratch("long.wav");
    sine(&p, 1000.0, 0.5, 70.0);
    let mut f = fx();
    let m = media(&mut f, &p);
    place(&mut f, 0, 0, clip(&m, 0.0, 70 * 30));
    let mut mixer = Mixer::new(f.ed.project(), f.tl).unwrap();
    assert_eq!(mixer.total_samples(), 70 * SR as i64);
    // render in uneven blocks that straddle the 30 s and 60 s window seams and check every sample
    let mut at = 0i64;
    let mut worst = 0.0f64;
    for block in [48_000 * 29 + 17, 4_000, 48_000 * 30, 99_999, 48_000 * 8] {
        let out = mixer.render(at, block).unwrap();
        for i in (0..block).step_by(7) {
            let n = at + i as i64;
            let want = 0.5 * (2.0 * PI * 1000.0 * n as f64 / SR as f64).sin();
            worst = worst.max((out[i * 2] as f64 - want).abs());
        }
        at += block as i64;
    }
    assert!(worst < 1e-4, "worst error {worst}");
}

#[test]
fn loudness_matches_the_reference_tone() {
    if !have_ffmpeg() {
        return;
    }
    let p = scratch("k1000.wav");
    sine(&p, 1000.0, 0.5, 6.0);
    let mut f = fx();
    let m = media(&mut f, &p);
    place(&mut f, 0, 0, clip(&m, 0.0, 180));
    let l = measure(&mix(&f), SR).unwrap();
    // EBU Tech 3341: a stereo 1 kHz sine at -6.02 dBFS reads about -6.0 LUFS
    assert!((l.integrated_lufs + 6.02).abs() < 0.15, "{l:?}");
    assert!((l.sample_peak_db + 6.02).abs() < 0.05, "{l:?}");
    assert!(l.true_peak_db > -6.1 && l.true_peak_db < -5.9, "{l:?}");
}

#[test]
fn waveform_peaks_of_a_file() {
    if !have_ffmpeg() {
        return;
    }
    let p = scratch("wave.wav");
    sine(&p, 200.0, 0.5, 2.0);
    let pk = media_peaks(p.to_str().unwrap(), 2.0, 50).unwrap();
    assert_eq!(pk.len(), 100);
    let bad: Vec<_> = pk
        .iter()
        .enumerate()
        .filter(|(_, p)| !(p.max > 0.45 && p.max < 0.55 && p.min < -0.45))
        .collect();
    assert!(bad.is_empty(), "{bad:?}");
    assert!(pk
        .iter()
        .all(|p| p.max > 0.45 && p.max < 0.55 && p.min < -0.45));
}

#[test]
fn animated_volume_and_pan_sweep_through_the_mix() {
    if !have_ffmpeg() {
        return;
    }
    let p = scratch("sweep.wav");
    sine(&p, 440.0, 0.5, 6.0);
    let mut f = fx();
    let m = media(&mut f, &p);
    let id = place(&mut f, 0, 0, clip(&m, 0.0, 120)); // 4 s at 30 fps
    let k = |t: i64, v: f64| Keyframe::new(t, v);
    // volume: -60 dB up to 0 dB over the clip; pan: hard left to hard right
    f.ed.apply(
        Author::User,
        Op::SetAnimTrack {
            item: id,
            prop: "volume".into(),
            keys: vec![k(0, -60.0), k(120, 0.0)],
        },
    )
    .unwrap();
    f.ed.apply(
        Author::User,
        Op::SetAnimTrack {
            item: id,
            prop: "pan".into(),
            keys: vec![k(0, -1.0), k(120, 1.0)],
        },
    )
    .unwrap();
    let out = mix(&f);
    let sr = SR as usize;
    let slice = |a: f64, b: f64| &out[(a * sr as f64) as usize * 2..(b * sr as f64) as usize * 2];
    let (early, late) = (slice(0.05, 0.25), slice(3.75, 3.95));
    assert!(
        rms(late, 0) + rms(late, 1) > 20.0 * (rms(early, 0) + rms(early, 1)),
        "it gets much louder"
    );
    assert!(
        rms(early, 0) > 5.0 * rms(early, 1),
        "early on it is in the left channel"
    );
    assert!(
        rms(late, 1) > 5.0 * rms(late, 0),
        "and ends up in the right"
    );
    // half way the volume is -30 dB: about 3% of full level
    let mid = slice(1.9, 2.1);
    let full = HALF; // rms of the unprocessed 0.5-amplitude sine
    let loud = rms(mid, 0).max(rms(mid, 1));
    assert!(
        loud > full * 0.02 && loud < full * 0.06,
        "mid level {loud} vs {full}"
    );
    // with the keys removed it is back to a flat, centred clip
    f.ed.apply(
        Author::User,
        Op::SetAnimTrack {
            item: id,
            prop: "volume".into(),
            keys: vec![],
        },
    )
    .unwrap();
    f.ed.apply(
        Author::User,
        Op::SetAnimTrack {
            item: id,
            prop: "pan".into(),
            keys: vec![],
        },
    )
    .unwrap();
    let flat = mix(&f);
    assert!((rms(&flat, 0) - HALF).abs() < 0.003 && (rms(&flat, 1) - HALF).abs() < 0.003);
}
