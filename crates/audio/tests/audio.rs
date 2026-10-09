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

#[test]
fn speed_reverse_and_freeze_reach_the_mix() {
    if !have_ffmpeg() {
        return;
    }
    // loud for the first 2 s of 4, then silence
    let p = scratch("step.wav");
    let src = format!("aevalsrc=if(lt(t\\,2)\\,0.5*sin(2*PI*440*t)\\,0):s={SR}:d=4:c=stereo");
    assert!(Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &src,
            "-c:a",
            "pcm_f32le"
        ])
        .arg(&p)
        .status()
        .unwrap()
        .success());
    let level = |out: &[f32], a: f64, b: f64| {
        let sr = SR as f64;
        rms(&out[(a * sr) as usize * 2..(b * sr) as usize * 2], 0)
    };
    let retime = |speed: f64, reverse: bool, freeze: bool| {
        let mut f = fx();
        let m = media(&mut f, &p);
        let id = place(&mut f, 0, 0, clip(&m, 0.0, 120));
        f.ed.apply(
            Author::User,
            Op::SetRetime {
                item: id,
                retime: Retime {
                    speed,
                    reverse,
                    freeze,
                },
            },
        )
        .unwrap();
        mix(&f)
    };
    // double speed: 2 s long, loud for the first second
    let out = retime(2.0, false, false);
    assert!(out.len() / 2 <= 2 * SR as usize + 100 && out.len() / 2 >= 2 * SR as usize - 100);
    assert!(
        level(&out, 0.1, 0.9) > 0.8 * HALF,
        "{}",
        level(&out, 0.1, 0.9)
    );
    assert!(level(&out, 1.1, 1.9) < 0.02);
    // reversed: silence first, then the loud half
    let out = retime(1.0, true, false);
    assert!(level(&out, 0.1, 1.9) < 0.02);
    assert!(level(&out, 2.1, 3.9) > 0.8 * HALF);
    // half speed: 8 s long, loud for 4 s
    let out = retime(0.5, false, false);
    assert!(level(&out, 0.2, 3.8) > 0.8 * HALF);
    assert!(level(&out, 4.2, 7.8) < 0.02);
    // a freeze has no sound
    let out = retime(1.0, false, true);
    assert!(out.iter().all(|s| s.abs() < 1e-6));
}

#[test]
fn a_transition_crossfades_the_sound_around_the_cut() {
    if !have_ffmpeg() {
        return;
    }
    let level = |path: &Path, v: f64| {
        let src = format!("aevalsrc={v}:s={SR}:d=6:c=stereo");
        assert!(Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &src,
                "-c:a",
                "pcm_f32le"
            ])
            .arg(path)
            .status()
            .unwrap()
            .success());
    };
    let (pa, pb) = (scratch("dc-a.wav"), scratch("dc-b.wav"));
    level(&pa, 0.5);
    level(&pb, 0.25);
    let mut f = fx();
    let (ma, mb) = (media(&mut f, &pa), media(&mut f, &pb));
    // 2 s of A (ticks 0..60), then 2 s of B taken from 2 s into its file
    place(&mut f, 0, 0, clip(&ma, 1.0, 60));
    let b = place(&mut f, 0, 60, clip(&mb, 2.0, 60));
    let at = |out: &[f32], secs: f64| out[(secs * SR as f64) as usize * 2];
    let hard = mix(&f);
    assert!((at(&hard, 1.9) - 0.5).abs() < 1e-3 && (at(&hard, 2.1) - 0.25).abs() < 1e-3);
    // a one-second crossfade covers 1.5 s .. 2.5 s
    f.ed.apply(
        Author::User,
        Op::SetTransition {
            item: b,
            transition: Some(Transition {
                kind: TransitionKind::Dissolve,
                duration: 30,
            }),
        },
    )
    .unwrap();
    let out = mix(&f);
    assert_eq!(out.len(), hard.len(), "the timeline does not get longer");
    assert!((at(&out, 1.4) - 0.5).abs() < 1e-3, "before: only A");
    assert!((at(&out, 2.6) - 0.25).abs() < 1e-3, "after: only B");
    assert!(
        (at(&out, 2.0) - 0.375).abs() < 0.01,
        "halfway: both at half"
    );
    assert!(at(&out, 1.75) > at(&out, 2.0) && at(&out, 2.0) > at(&out, 2.25));
}

#[test]
fn sound_effects_run_in_the_mix_and_tracks_are_metered() {
    if !have_ffmpeg() {
        return;
    }
    let p = scratch("loud.wav");
    sine(&p, 440.0, 0.9, 4.0);
    let mut f = fx();
    let m = media(&mut f, &p);
    let id = place(&mut f, 0, 0, clip(&m, 0.0, 90));
    place(&mut f, 1, 0, clip(&m, 0.0, 90));
    let peak = |s: &[f32]| s.iter().fold(0f32, |a, v| a.max(v.abs()));
    // two copies of 0.9 add up to 1.8
    let dry = mix(&f);
    assert!(peak(&dry) > 1.7);
    // a limiter on one clip takes that clip to -12 dB; the other is untouched
    let mut lim = Effect::new(EffectKind::Limiter);
    lim.params[0] = -12.0;
    f.ed.apply(
        Author::User,
        Op::SetEffects {
            item: id,
            effects: vec![lim],
        },
    )
    .unwrap();
    let mut mixer = Mixer::new(f.ed.project(), f.tl).unwrap();
    let mut peaks = Vec::new();
    let out = mixer
        .render_metered(SR as i64 / 2, 4800, Some(&mut peaks))
        .unwrap();
    assert_eq!(peaks.len(), 2, "one peak per track");
    assert!(peaks[0] < 0.26, "limited track {}", peaks[0]);
    assert!(peaks[1] > 0.85, "plain track {}", peaks[1]);
    assert!(peak(&out) < 1.2);
}

#[test]
fn music_ducks_under_dialogue_and_comes_back() {
    if !have_ffmpeg() {
        return;
    }
    // "dialogue": loud tone in seconds 1-2 of 4, silent otherwise; "music": steady quiet tone
    let voice = scratch("voice.wav");
    let src =
        format!("aevalsrc=if(between(t\\,1\\,2)\\,0.5*sin(2*PI*220*t)\\,0):s={SR}:d=4:c=stereo");
    assert!(Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &src,
            "-c:a",
            "pcm_f32le"
        ])
        .arg(&voice)
        .status()
        .unwrap()
        .success());
    let music = scratch("music.wav");
    sine(&music, 880.0, 0.2, 4.0);
    let mut f = fx();
    let (mv, mm) = (media(&mut f, &voice), media(&mut f, &music));
    place(&mut f, 0, 0, clip(&mv, 0.0, 120));
    let bed = place(&mut f, 1, 0, clip(&mm, 0.0, 120));
    let level = |out: &[f32], a: f64, b: f64| {
        let sr = SR as f64;
        // look at the music's own frequency by taking the whole mix; the voice is silent outside 1-2 s
        rms(&out[(a * sr) as usize * 2..(b * sr) as usize * 2], 0)
    };
    let plain = mix(&f);
    let before_plain = level(&plain, 0.2, 0.8);
    assert!(before_plain > 0.1);
    let mut duck = Effect::new(EffectKind::Ducking);
    duck.params = vec![-30.0, 20.0, 30.0, 200.0];
    f.ed.apply(
        Author::User,
        Op::SetEffects {
            item: bed,
            effects: vec![duck],
        },
    )
    .unwrap();
    let out = mix(&f);
    // away from the voice the music is at full level
    assert!((level(&out, 0.2, 0.8) - before_plain).abs() < 0.01);
    assert!(
        (level(&out, 3.2, 3.8) - before_plain).abs() < 0.02,
        "it recovers after the release"
    );
    // during the voice the music is far down: compare the 880 Hz part by removing the voice
    let mut only_voice = Vec::new();
    {
        let mut g = fx();
        let mv2 = media(&mut g, &voice);
        place(&mut g, 0, 0, clip(&mv2, 0.0, 120));
        only_voice.extend(mix(&g));
    }
    let a = (1.3 * SR as f64) as usize * 2;
    let b = (1.9 * SR as f64) as usize * 2;
    let residual: Vec<f32> = out[a..b]
        .iter()
        .zip(&only_voice[a..b])
        .map(|(m, v)| m - v)
        .collect();
    let music_during = rms(&residual, 0);
    assert!(
        music_during < before_plain * 0.2,
        "ducked: {music_during} vs {before_plain}"
    );
}

#[test]
fn a_nested_sequence_brings_its_sound_with_it() {
    if !have_ffmpeg() {
        return;
    }
    let p = scratch("nested-tone.wav");
    sine(&p, 440.0, 0.5, 6.0);
    let mut f = fx();
    let m = media(&mut f, &p);
    // the inner sequence: a second timeline holding 2 s of the tone, starting 1 s into it
    let inner = Id::new();
    f.ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(inner, "Inner", Rational::FPS_30),
        },
    )
    .unwrap();
    let it = Id::new();
    f.ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: inner,
            index: 0,
            track: Track::new(it, "A1", TrackKind::Audio),
        },
    )
    .unwrap();
    let c = clip(&m, 0.0, 60);
    let op = edit::overwrite(f.ed.project(), it, t(30), c).unwrap(); // silence for 1 s, then the tone for 2 s
    f.ed.apply(Author::User, op).unwrap();
    // the outer one plays the inner from its start, placed at 1 s
    let nested = MediaRef::nested(f.ed.project().timeline(inner).unwrap(), (0, 0));
    f.ed.apply(
        Author::User,
        Op::AddMedia {
            media: nested.clone(),
        },
    )
    .unwrap();
    let plain = Clip::new(Id::new(), "nested", nested.id, t(0), t(90));
    let op = edit::overwrite(f.ed.project(), f.a1, t(30), plain).unwrap();
    f.ed.apply(Author::User, op).unwrap();
    let out = mix_of(&f, f.tl);
    let level = |a: f64, b: f64| {
        rms(
            &out[(a * SR as f64) as usize * 2..(b * SR as f64) as usize * 2],
            0,
        )
    };
    assert!(level(0.1, 0.9) < 0.01, "before the nested clip: silence");
    assert!(
        level(1.2, 1.9) < 0.01,
        "inside it, but the inner sequence is silent for its first second"
    );
    assert!(
        (level(2.3, 3.8) - HALF).abs() < 0.02,
        "then the inner tone: {}",
        level(2.3, 3.8)
    );
    assert!(
        out.len() / 2 <= 4 * SR as usize + 10,
        "the mix ends where the nested clip ends"
    );
}

fn mix_of(f: &Fx, tl: Id) -> Vec<f32> {
    let mut m = Mixer::new(f.ed.project(), tl).unwrap();
    let n = m.total_samples() as usize;
    m.render(0, n).unwrap()
}

#[test]
fn recordings_of_one_event_are_lined_up_by_their_sound() {
    if !have_ffmpeg() {
        return;
    }
    let dir = scratch("sync-src.wav");
    // 30 s of gated noise: bursts at irregular times, a signature no other stretch shares
    let gate = "if(gt(sin(t*3.1)+sin(t*7.3)+sin(t*1.7)+sin(t*0.53)\\,1.1)\\,1\\,0.04)";
    let src = format!("anoisesrc=d=30:c=white:a=0.6:s=7,volume='{gate}':eval=frame");
    assert!(Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &src,
            "-ar",
            "48000",
            "-c:a",
            "pcm_s16le"
        ])
        .arg(&dir)
        .status()
        .unwrap()
        .success());
    let cut = |name: &str, from: f64, noise: f64| {
        let out = scratch(name);
        let filter = format!("[0:a]atrim=start={from}:duration=22,asetpts=PTS-STARTPTS[x];anoisesrc=d=22:c=pink:a={noise}:s=3[n];[x][n]amix=inputs=2:normalize=0");
        assert!(Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i"])
            .arg(&dir)
            .args([
                "-filter_complex",
                &filter,
                "-ar",
                "48000",
                "-c:a",
                "pcm_s16le"
            ])
            .arg(&out)
            .status()
            .unwrap()
            .success());
        out
    };
    let a = cut("sync-a.wav", 0.0, 0.01);
    let b = cut("sync-b.wav", 2.37, 0.05); // started 2.37 s into the action, and with more background noise
    let al = find_offset(
        a.to_str().unwrap(),
        0.0,
        b.to_str().unwrap(),
        0.0,
        18.0,
        10.0,
    )
    .unwrap();
    assert!(
        (al.offset_secs - 2.37).abs() < 0.02,
        "b has to move 2.37 s later: {al:?}"
    );
    assert!(al.confidence > 4.0, "{al:?}");
    // the other way round, the sign flips
    let back = find_offset(
        b.to_str().unwrap(),
        0.0,
        a.to_str().unwrap(),
        0.0,
        18.0,
        10.0,
    )
    .unwrap();
    assert!((back.offset_secs + 2.37).abs() < 0.02, "{back:?}");
    // unrelated sound is a weak match
    let other = scratch("sync-other.wav");
    assert!(Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "anoisesrc=d=22:c=brown:a=0.4:s=99",
            "-ar",
            "48000",
            "-c:a",
            "pcm_s16le"
        ])
        .arg(&other)
        .status()
        .unwrap()
        .success());
    let weak = find_offset(
        a.to_str().unwrap(),
        0.0,
        other.to_str().unwrap(),
        0.0,
        18.0,
        10.0,
    )
    .unwrap();
    assert!(weak.confidence < al.confidence * 0.8, "{weak:?} vs {al:?}");
}

#[test]
fn track_pan_and_solo_shape_the_mix() {
    if !have_ffmpeg() {
        return;
    }
    let (pa, pb) = (scratch("pan-a.wav"), scratch("pan-b.wav"));
    sine(&pa, 440.0, 0.5, 4.0);
    sine(&pb, 880.0, 0.25, 4.0);
    let mut f = fx();
    let (ma, mb) = (media(&mut f, &pa), media(&mut f, &pb));
    place(&mut f, 0, 0, clip(&ma, 0.0, 60));
    place(&mut f, 1, 0, clip(&mb, 0.0, 60));
    let both = mix(&f);
    let n = SR as usize;
    let (l, r) = (rms(&both[n..2 * n], 0), rms(&both[n..2 * n], 1));
    assert!((l - r).abs() < 0.01 && l > 0.3, "centred: {l} {r}");
    // track 1 panned hard left: its tone leaves the right channel
    f.ed.apply(
        Author::User,
        Op::SetTrackMix {
            track: f.a1,
            pan: -1.0,
            solo: false,
            locked: false,
        },
    )
    .unwrap();
    let panned = mix(&f);
    let (l, r) = (rms(&panned[n..2 * n], 0), rms(&panned[n..2 * n], 1));
    assert!(l > r * 1.5, "left is louder: {l} {r}");
    // solo track 2: only its tone is left, and it sits in the middle
    f.ed.apply(
        Author::User,
        Op::SetTrackMix {
            track: f.a2,
            pan: 0.0,
            solo: true,
            locked: false,
        },
    )
    .unwrap();
    let solo = mix(&f);
    let (l, r) = (rms(&solo[n..2 * n], 0), rms(&solo[n..2 * n], 1));
    assert!(
        (l - 0.25 / std::f64::consts::SQRT_2).abs() < 0.02 && (l - r).abs() < 0.01,
        "{l} {r}"
    );
    // soloing both brings both back
    f.ed.apply(
        Author::User,
        Op::SetTrackMix {
            track: f.a1,
            pan: -1.0,
            solo: true,
            locked: false,
        },
    )
    .unwrap();
    let again = mix(&f);
    assert!(rms(&again[n..2 * n], 0) > l * 1.5);
}

#[test]
fn locked_tracks_refuse_edits_but_not_undo_or_unlock() {
    if !have_ffmpeg() {
        return;
    }
    let p = scratch("lock.wav");
    sine(&p, 440.0, 0.5, 4.0);
    let mut f = fx();
    let m = media(&mut f, &p);
    let id = place(&mut f, 0, 0, clip(&m, 0.0, 60));
    let before = f.ed.snapshot();
    let steps = f.ed.history().len();
    f.ed.apply(
        Author::User,
        Op::SetTrackMix {
            track: f.a1,
            pan: 0.0,
            solo: false,
            locked: true,
        },
    )
    .unwrap();
    // a change of the clip, an insert and a transaction containing either are all refused
    let e =
        f.ed.apply(
            Author::User,
            Op::SetName {
                item: id,
                name: "x".into(),
            },
        )
        .unwrap_err();
    assert!(e.to_string().contains("locked"), "{e}");
    assert!(f
        .ed
        .apply(Author::User, Op::RemoveItem { item: id })
        .is_err());
    let again = clip(&m, 0.0, 30);
    assert!(f
        .ed
        .apply(
            Author::User,
            edit::append(f.ed.project(), f.a1, again.clone())
        )
        .is_err());
    assert!(f
        .ed
        .apply(
            Author::User,
            Op::Transaction {
                description: "t".into(),
                ops: vec![Op::SetName {
                    item: id,
                    name: "y".into()
                }]
            }
        )
        .is_err());
    // the other track and the track's own settings are fine
    f.ed.apply(Author::User, edit::append(f.ed.project(), f.a2, again))
        .unwrap();
    f.ed.apply(
        Author::User,
        Op::SetTrack {
            track: f.a1,
            enabled: false,
            gain_db: -3.0,
        },
    )
    .unwrap();
    // unlock, edit, and undo all the way back
    f.ed.apply(
        Author::User,
        Op::SetTrackMix {
            track: f.a1,
            pan: 0.0,
            solo: false,
            locked: false,
        },
    )
    .unwrap();
    f.ed.apply(
        Author::User,
        Op::SetName {
            item: id,
            name: "ok".into(),
        },
    )
    .unwrap();
    f.ed.apply(
        Author::User,
        Op::SetTrackMix {
            track: f.a1,
            pan: 0.0,
            solo: false,
            locked: true,
        },
    )
    .unwrap();
    while f.ed.history().len() > steps {
        assert!(f.ed.undo());
    }
    assert_eq!(f.ed.project(), &before, "undo works even through a lock");
}

#[test]
fn ducking_can_listen_to_one_chosen_track() {
    if !have_ffmpeg() {
        return;
    }
    let voice = scratch("duck-voice.wav");
    let src =
        format!("aevalsrc=if(between(t\\,1\\,2)\\,0.5*sin(2*PI*220*t)\\,0):s={SR}:d=4:c=stereo");
    assert!(Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &src,
            "-c:a",
            "pcm_f32le"
        ])
        .arg(&voice)
        .status()
        .unwrap()
        .success());
    let (music, hum) = (scratch("duck-music.wav"), scratch("duck-hum.wav"));
    sine(&music, 880.0, 0.2, 4.0);
    sine(&hum, 110.0, 0.5, 4.0); // loud, constant: a room tone the music should not duck for
    let mut f = fx();
    let a3 = Id::new();
    let tl = f.tl;
    f.ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: tl,
            index: 2,
            track: Track::new(a3, "A3", TrackKind::Audio),
        },
    )
    .unwrap();
    let (mv, mm, mh) = (
        media(&mut f, &voice),
        media(&mut f, &music),
        media(&mut f, &hum),
    );
    place(&mut f, 0, 0, clip(&mv, 0.0, 120)); // A1: the voice
    let bed = place(&mut f, 1, 0, clip(&mm, 0.0, 120)); // A2: the music
    let op = edit::overwrite(f.ed.project(), a3, t(0), clip(&mh, 0.0, 120)).unwrap(); // A3: the hum
    f.ed.apply(Author::User, op).unwrap();
    let level = |out: &[f32], a: f64, b: f64| {
        rms(
            &out[(a * SR as f64) as usize * 2..(b * SR as f64) as usize * 2],
            0,
        )
    };
    let mut duck = Effect::new(EffectKind::Ducking);
    duck.params = vec![-30.0, 24.0, 30.0, 200.0];
    // listening to everything: the constant hum keeps the music ducked even when the voice is quiet
    f.ed.apply(
        Author::User,
        Op::SetEffects {
            item: bed,
            effects: vec![duck.clone()],
        },
    )
    .unwrap();
    let all = mix(&f);
    let quiet_part_all = level(&all, 3.2, 3.8);
    // listening to the voice track only: the music is back at full level away from the voice
    duck.text = "A1".into();
    f.ed.apply(
        Author::User,
        Op::SetEffects {
            item: bed,
            effects: vec![duck],
        },
    )
    .unwrap();
    let voice_only = mix(&f);
    let quiet_part_voice = level(&voice_only, 3.2, 3.8);
    assert!(
        quiet_part_voice > quiet_part_all,
        "{quiet_part_voice} vs {quiet_part_all}"
    );
    // and under the voice the music does duck
    let during = level(&voice_only, 1.3, 1.9);
    let plain = {
        let mut g = fx();
        let (mv2, mm2, mh2) = (
            media(&mut g, &voice),
            media(&mut g, &music),
            media(&mut g, &hum),
        );
        place(&mut g, 0, 0, clip(&mv2, 0.0, 120));
        place(&mut g, 1, 0, clip(&mm2, 0.0, 120));
        let a3b = Id::new();
        let tlb = g.tl;
        g.ed.apply(
            Author::User,
            Op::AddTrack {
                timeline: tlb,
                index: 2,
                track: Track::new(a3b, "A3", TrackKind::Audio),
            },
        )
        .unwrap();
        let op = edit::overwrite(g.ed.project(), a3b, t(0), clip(&mh2, 0.0, 120)).unwrap();
        g.ed.apply(Author::User, op).unwrap();
        level(&mix(&g), 1.3, 1.9)
    };
    assert!(
        during < plain * 0.97,
        "the music dipped under the voice: {during} vs {plain}"
    );
}
