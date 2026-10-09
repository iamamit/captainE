//! Phase 1 exit criterion (docs/12): a project with 50 clips, 4 video + 4 audio tracks, exports identically
//! every time. Skipped when FFmpeg is not installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use captaine_export::*;
use captaine_media::{media_ref, probe};
use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("captaine-exit-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn ffmpeg(args: &[&str], out: &Path) {
    assert!(Command::new("ffmpeg")
        .args(["-y", "-v", "error"])
        .args(args)
        .arg(out)
        .status()
        .unwrap()
        .success());
}

fn t(v: i64) -> RationalTime {
    RationalTime::new(v, Rational::FPS_30)
}

/// Tiny deterministic generator so the project is the same on every run.
struct Rng(u64);
impl Rng {
    fn below(&mut self, n: i64) -> i64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as i64
    }
}

#[test]
fn fifty_clips_on_four_plus_four_tracks_export_identically() {
    if !have_ffmpeg() {
        return;
    }
    let (a, b, c) = (scratch("av.mp4"), scratch("v.mp4"), scratch("music.wav"));
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x180:rate=30:duration=6",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:duration=6",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ],
        &a,
    );
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=240x320:rate=25:duration=6",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ],
        &b,
    );
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=660:duration=6",
            "-c:a",
            "pcm_s16le",
        ],
        &c,
    );

    let settings = ProjectSettings {
        frame_rate: Rational::FPS_30,
        width: 320,
        height: 180,
        sample_rate: 48_000,
    };
    let mut ed = Editor::new(Project::new(settings));
    let media: Vec<MediaRef> = [&a, &b, &c]
        .iter()
        .map(|p| media_ref(p, Id::new()).unwrap())
        .collect();
    for m in &media {
        ed.apply(Author::User, Op::AddMedia { media: m.clone() })
            .unwrap();
    }
    let tl = Id::new();
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "Main", Rational::FPS_30),
        },
    )
    .unwrap();
    let mut video = Vec::new();
    let mut audio = Vec::new();
    for i in 0..4 {
        let (v, au) = (Id::new(), Id::new());
        ed.apply(
            Author::User,
            Op::AddTrack {
                timeline: tl,
                index: ed.project().timeline(tl).unwrap().tracks.len(),
                track: Track::new(v, format!("V{}", i + 1), TrackKind::Video),
            },
        )
        .unwrap();
        video.push(v);
        audio.push(au);
    }
    for (i, au) in audio.iter().enumerate() {
        ed.apply(
            Author::User,
            Op::AddTrack {
                timeline: tl,
                index: ed.project().timeline(tl).unwrap().tracks.len(),
                track: Track::new(*au, format!("A{}", i + 1), TrackKind::Audio),
            },
        )
        .unwrap();
    }

    let mut rng = Rng(0x00C0_FFEE);
    let mut placed = 0;
    let mut cursor = [[0i64; 4]; 2]; // next free tick on each [audio, video] track
                                     // 26 video clips (media 0 and 1), 24 audio clips (media 0 with audio, and the music)
    for n in 0..50 {
        let is_video = n < 26;
        let (m, rate) = if is_video {
            let m = &media[rng.below(2) as usize];
            (m, m.duration.unwrap().rate)
        } else if rng.below(2) == 0 {
            (&media[0], media[0].duration.unwrap().rate)
        } else {
            (&media[2], media[2].duration.unwrap().rate)
        };
        let dur = 20 + rng.below(45); // 0.7 .. 2.2 s
        let src_secs = rng.below(2) as f64; // stay well inside the 6 s sources
        let src = RationalTime::new((src_secs * rate.as_f64()) as i64, rate);
        let mut clip = Clip::new(Id::new(), format!("c{n}"), m.id, src, t(dur));
        clip.video.opacity = 0.5 + rng.below(50) as f64 / 100.0;
        clip.video.transform.scale = (0.5 + rng.below(100) as f64 / 100.0, 0.0);
        clip.video.transform.scale.1 = clip.video.transform.scale.0;
        clip.video.transform.position = (rng.below(120) as f64 - 60.0, rng.below(60) as f64 - 30.0);
        clip.video.transform.rotation_deg = rng.below(40) as f64 - 20.0;
        clip.audio.gain_db = -(rng.below(12) as f64);
        clip.audio.pan = (rng.below(21) as f64 - 10.0) / 10.0;
        clip.audio.fade_in = rng.below(10);
        clip.audio.fade_out = rng.below(10);
        let lane = (n % 4) as usize;
        let track = if is_video { video[lane] } else { audio[lane] };
        let at = cursor[is_video as usize][lane] + rng.below(20);
        cursor[is_video as usize][lane] = at + dur;
        let op = edit::overwrite(ed.project(), track, t(at), clip).unwrap();
        ed.apply(Author::User, op).unwrap();
        placed += 1;
    }
    assert_eq!(placed, 50);
    ed.project().validate().unwrap();
    let clips: usize = ed
        .project()
        .timeline(tl)
        .unwrap()
        .tracks
        .iter()
        .map(|t| t.items.iter().filter(|i| i.as_clip().is_some()).count())
        .sum();
    assert!(
        clips >= 50,
        "overwrites may split clips but never remove this many: {clips}"
    );

    let total = ed.project().timeline(tl).unwrap().duration().value;
    assert!(total > 150, "a timeline with some length: {total}");
    let (o1, o2) = (scratch("o1.mp4"), scratch("o2.mp4"));
    let opts = ExportOptions {
        preset: "veryfast".into(),
        ..Default::default()
    };
    export(ed.project(), tl, &o1, &opts, &mut |_, _| {}).unwrap();
    // a different Editor state reached by undo + redo must render the same too
    assert!(ed.undo() && ed.redo());
    export(ed.project(), tl, &o2, &opts, &mut |_, _| {}).unwrap();

    let info = probe(&o1).unwrap();
    assert_eq!(info.length, total);
    assert!(info.has_audio);
    assert_eq!(
        std::fs::read(&o1).unwrap(),
        std::fs::read(&o2).unwrap(),
        "exports must be byte-identical"
    );
}
