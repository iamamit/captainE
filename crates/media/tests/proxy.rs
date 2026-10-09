use std::process::Command;

use captaine_media::{make_proxy, probe};

#[test]
fn proxy_is_smaller_intra_only_and_frame_exact() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("captaine-proxy-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (src, dst) = (dir.join("src.mp4"), dir.join("cache/src.proxy.mp4"));
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=1280x720:rate=30:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=duration=2",
        ])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
        .arg(&src)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    make_proxy(&src, &dst, 360).unwrap();
    let (a, b) = (probe(&src).unwrap(), probe(&dst).unwrap());
    assert_eq!((b.width, b.height), (640, 360));
    assert_eq!((a.length, b.length), (60, 60));
    assert!(!b.has_audio);
    assert!(!dst.with_extension("partial.mp4").exists());
    // never upscales
    let small = dir.join("small.mp4");
    make_proxy(&src, &small, 4000).unwrap();
    assert_eq!(probe(&small).unwrap().height, 720);
    let _ = std::fs::remove_dir_all(&dir);
}
