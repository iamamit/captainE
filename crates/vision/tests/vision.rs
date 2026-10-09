//! Real Vision runs on synthetic footage: a bright ball drifting over a dull gradient.
//! Skipped when the Mac's Vision framework is not reachable from Python.

use std::path::Path;
use std::process::Command;

use captaine_vision::*;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

/// 12 frames at 6 fps of an orange ball moving left to right, as a video.
fn ball_video(path: &Path) {
    let filter = "gradients=s=640x360:c0=0x203040:c1=0x405060:seed=3:r=6:d=2,format=rgb24,\
        geq=r='if(lt(hypot(X-(160+25*N)\\,Y-190)\\,80)\\,235\\,r(X\\,Y))':\
        g='if(lt(hypot(X-(160+25*N)\\,Y-190)\\,80)\\,120\\,g(X\\,Y))':\
        b='if(lt(hypot(X-(160+25*N)\\,Y-190)\\,80)\\,30\\,b(X\\,Y))'";
    assert!(Command::new("ffmpeg")
        .args([
            "-y", "-v", "error", "-f", "lavfi", "-i", filter, "-c:v", "libx264", "-pix_fmt",
            "yuv420p", "-crf", "10"
        ])
        .arg(path)
        .status()
        .unwrap()
        .success());
}

fn gray_at(path: &Path, frame: usize, x: u32, y: u32) -> u8 {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("select=eq(n\\,{frame}),crop=1:1:{x}:{y}"),
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            "-",
        ])
        .output()
        .unwrap();
    out.stdout.first().copied().unwrap_or(0)
}

#[test]
fn lifts_a_moving_subject_and_follows_it() {
    if unavailable().is_some() || !have_ffmpeg() {
        eprintln!("skipped: {:?}", unavailable());
        return;
    }
    let dir = Scratch::new("vision-test").unwrap();
    let video = dir.0.join("ball.mp4");
    ball_video(&video);
    let video_s = video.to_str().unwrap();

    // a matte video: white on the ball, black elsewhere, frame by frame
    let matte = dir.0.join("matte.mkv");
    matte_video(video_s, 0.0, 2.0, 6.0, MatteMode::Subject, &matte).unwrap();
    for (frame, cx) in [(0usize, 160u32), (6, 310), (11, 435)] {
        // the matte is 640 wide like the source here
        assert!(
            gray_at(&matte, frame, cx, 190) > 200,
            "frame {frame}: the ball is white"
        );
        assert!(
            gray_at(&matte, frame, 30, 30) < 40,
            "frame {frame}: the background is black"
        );
    }

    // follow the ball from its first position
    let frames = dir.0.join("frames");
    extract_frames(video_s, 0.0, 12, 6.0, 640, &frames).unwrap();
    let start = [
        (160.0 - 80.0) / 640.0,
        (190.0 - 80.0) / 360.0,
        160.0 / 640.0,
        160.0 / 360.0,
    ];
    let path = track(&frames, start).unwrap();
    assert_eq!(path.len(), 12);
    let centre = |b: &Rect| b[0] + b[2] / 2.0;
    let (first, last) = (centre(&path[0].0), centre(&path[11].0));
    assert!(
        last - first > 0.25,
        "it moved right by about 275 px of 640: {first} -> {last}"
    );
    assert!(path.iter().all(|(_, c)| *c > 0.0));
    // attention goes to the ball
    let sal = saliency(&frames).unwrap();
    let sx = sal[11]
        .iter()
        .map(|b| b[0] + b[2] / 2.0)
        .next()
        .expect("a salient box");
    assert!(
        sx > 0.5,
        "late in the shot the salient thing is on the right: {sx}"
    );
    // no faces in a ball
    assert!(faces(&frames).unwrap().iter().all(Vec::is_empty));
}
