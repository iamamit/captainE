//! Runs the real recogniser on speech made by the Mac's own voice. Skipped when the tools are missing.

use std::process::Command;

use captaine_speech::*;

fn have(cmd: &str) -> bool {
    Command::new(cmd).arg("-version").output().is_ok()
        || Command::new("which")
            .arg(cmd)
            .output()
            .is_ok_and(|o| o.status.success())
}

#[test]
fn transcribes_synthetic_speech_with_word_times() {
    if unavailable().is_some() || !have("say") || !have("ffmpeg") {
        eprintln!("skipped: {:?}", unavailable());
        return;
    }
    let dir = std::env::temp_dir().join(format!("captaine-speech-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let aiff = dir.join("s.aiff");
    assert!(Command::new("say")
        .args(["-o"])
        .arg(&aiff)
        .arg("Hello everyone. Um, this is a test of the video editor. Uh, thank you for watching.")
        .status()
        .unwrap()
        .success());
    let segs = transcribe(aiff.to_str().unwrap(), 0.0, 30.0, Some("en")).unwrap();
    let all: String = segs
        .iter()
        .map(|s| s.text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        all.contains("hello everyone") && all.contains("video editor") && all.contains("thank you"),
        "{all}"
    );
    let words: Vec<&Word> = segs.iter().flat_map(|s| s.words.iter()).collect();
    assert!(words.len() >= 12);
    assert!(
        words.windows(2).all(|w| w[0].start <= w[1].start + 1e-6),
        "word times only go forward"
    );
    // the fillers are found at believable times (after the first sentence)
    let fillers = filler_ranges(&segs, false, 0.05);
    assert!(!fillers.is_empty(), "{all}");
    assert!(fillers[0].0 > 0.5);
    // a window starting later reports times from its own start
    let later = transcribe(aiff.to_str().unwrap(), 2.5, 30.0, Some("en")).unwrap();
    let first = later.iter().flat_map(|s| s.words.iter()).next().unwrap();
    assert!(first.start < 1.5, "relative to the window: {first:?}");
    let _ = std::fs::remove_dir_all(dir);
}
