//! Speech to text (docs/10). The recogniser is a local Whisper run through `mlx_whisper`: nothing is
//! uploaded. Audio is cut out of the media with FFmpeg, transcribed with word timings, and the result is
//! plain data: segments of words with start and end seconds relative to the start of the window asked for.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub words: Vec<Word>,
}

/// Where a Whisper command line tool can be found, if anywhere.
pub fn find_whisper() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CAPTAINE_WHISPER") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let home = std::env::var("HOME").ok().map(PathBuf::from)?;
    let candidates = [
        home.join(".captaine/whisper/bin/mlx_whisper"),
        home.join("codit/.venv/bin/mlx_whisper"),
        home.join("resolve-claude-mcp/.venv/bin/mlx_whisper"),
    ];
    if let Some(p) = candidates.into_iter().find(|p| p.exists()) {
        return Some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("mlx_whisper"))
        .find(|p| p.exists())
}

/// The best model already on this machine, so nothing is downloaded behind the user's back.
pub fn pick_model() -> Option<String> {
    if let Ok(m) = std::env::var("CAPTAINE_WHISPER_MODEL") {
        return Some(m);
    }
    let home = std::env::var("HOME").ok().map(PathBuf::from)?;
    let hub = home.join(".cache/huggingface/hub");
    [
        "whisper-large-v3-turbo",
        "whisper-large-v3-mlx",
        "whisper-medium",
        "whisper-small-mlx",
        "whisper-base-mlx",
    ]
    .into_iter()
    .find(|m| hub.join(format!("models--mlx-community--{m}")).exists())
    .map(|m| format!("mlx-community/{m}"))
}

/// Why speech to text cannot run here, or None when it can.
pub fn unavailable() -> Option<String> {
    if find_whisper().is_none() {
        return Some("No Whisper tool found. Install mlx-whisper (pip install mlx-whisper) or set CAPTAINE_WHISPER to its mlx_whisper program.".into());
    }
    if pick_model().is_none() {
        return Some("No Whisper model found in the Hugging Face cache. Run mlx_whisper once to download one, or set CAPTAINE_WHISPER_MODEL.".into());
    }
    None
}

/// Parse Whisper's JSON output.
pub fn parse_whisper_json(text: &str) -> Result<Vec<Segment>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("unreadable transcript: {e}"))?;
    let segs = v
        .get("segments")
        .and_then(Value::as_array)
        .ok_or("the transcript has no segments")?;
    let mut out = Vec::new();
    for s in segs {
        let num = |x: &Value, k: &str| x.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let words: Vec<Word> = s
            .get("words")
            .and_then(Value::as_array)
            .map(|ws| {
                ws.iter()
                    .map(|w| Word {
                        text: w
                            .get("word")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                        start: num(w, "start"),
                        end: num(w, "end"),
                    })
                    .filter(|w| !w.text.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let text = s
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() && words.is_empty() {
            continue;
        }
        out.push(Segment {
            text,
            start: num(s, "start"),
            end: num(s, "end"),
            words,
        });
    }
    Ok(out)
}

/// Transcribe `dur_secs` of `media`'s sound starting `from_secs` into the file. Times in the result count
/// from `from_secs`.
pub fn transcribe(
    media: &str,
    from_secs: f64,
    dur_secs: f64,
    language: Option<&str>,
) -> Result<Vec<Segment>, String> {
    if let Some(why) = unavailable() {
        return Err(why);
    }
    let tool = find_whisper().expect("checked");
    let model = pick_model().expect("checked");
    if !Path::new(media).exists() {
        return Err(format!("media file not found: {media}"));
    }
    let dir = std::env::temp_dir().join(format!(
        "captaine-speech-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos())
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        let wav = dir.join("speech.wav");
        let ff = Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-nostdin", "-ss"])
            .arg(format!("{:.6}", from_secs.max(0.0)))
            .arg("-t")
            .arg(format!("{:.6}", dur_secs.max(0.1)))
            .arg("-i")
            .arg(media)
            .args(["-vn", "-ac", "1", "-ar", "16000"])
            .arg(&wav)
            .output()
            .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
        if !ff.status.success() {
            return Err(String::from_utf8_lossy(&ff.stderr).trim().to_string());
        }
        let mut cmd = Command::new(&tool);
        cmd.arg(&wav)
            .args([
                "--model",
                &model,
                "--output-format",
                "json",
                "--word-timestamps",
                "True",
            ])
            .arg("--output-dir")
            .arg(&dir)
            .args(["--output-name", "out", "--verbose", "False"])
            .env("HF_HUB_OFFLINE", "1");
        if let Some(l) = language {
            cmd.args(["--language", l]);
        }
        let out = cmd
            .output()
            .map_err(|e| format!("cannot run {}: {e}", tool.display()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "Whisper failed: {}",
                err.lines().last().unwrap_or("")
            ));
        }
        let json = std::fs::read_to_string(dir.join("out.json"))
            .map_err(|e| format!("no transcript was written: {e}"))?;
        parse_whisper_json(&json)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Words that add nothing when spoken: hesitations. `strict` also takes "like", "you know" and the like
/// when they are single words.
pub fn is_filler(word: &str, strict: bool) -> bool {
    let w: String = word
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    const HESITATIONS: [&str; 10] = [
        "um", "umm", "uh", "uhh", "uhm", "er", "erm", "hmm", "mm", "ah",
    ];
    const WEAK: [&str; 5] = ["like", "basically", "literally", "actually", "so"];
    HESITATIONS.contains(&w.as_str()) || (strict && WEAK.contains(&w.as_str()))
}

/// Time ranges (seconds) of the filler words, each widened by `pad` and merged when they touch.
pub fn filler_ranges(segments: &[Segment], strict: bool, pad: f64) -> Vec<(f64, f64)> {
    let mut ranges: Vec<(f64, f64)> = segments
        .iter()
        .flat_map(|s| s.words.iter())
        .filter(|w| is_filler(&w.text, strict))
        .map(|w| ((w.start - pad).max(0.0), w.end + pad))
        .collect();
    ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.0 <= last.1 => last.1 = last.1.max(r.1),
            _ => merged.push(r),
        }
    }
    merged
}

/// Break the words into caption lines: a line ends at sentence punctuation, after a pause, or when it would
/// get longer than `max_chars` or `max_secs`. Returns `(start, end, text)`.
pub fn caption_lines(
    segments: &[Segment],
    max_chars: usize,
    max_secs: f64,
) -> Vec<(f64, f64, String)> {
    let mut lines = Vec::new();
    let mut cur: Vec<&Word> = Vec::new();
    let flush = |cur: &mut Vec<&Word>, lines: &mut Vec<(f64, f64, String)>| {
        if let (Some(f), Some(l)) = (cur.first(), cur.last()) {
            let text = cur
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            lines.push((f.start, l.end, text));
        }
        cur.clear();
    };
    for w in segments.iter().flat_map(|s| s.words.iter()) {
        if let Some(last) = cur.last() {
            let len: usize = cur
                .iter()
                .map(|x| x.text.chars().count() + 1)
                .sum::<usize>()
                + w.text.chars().count();
            let ends_sentence = last.text.ends_with(['.', '!', '?']);
            if ends_sentence
                || len > max_chars
                || w.end - cur[0].start > max_secs
                || w.start - last.end > 0.8
            {
                flush(&mut cur, &mut lines);
            }
        }
        cur.push(w);
    }
    flush(&mut cur, &mut lines);
    lines
}

/// `HH:MM:SS,mmm`
pub fn srt_time(secs: f64) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

pub fn parse_srt_time(s: &str) -> Option<f64> {
    let s = s.trim().replace('.', ",");
    let (hms, ms) = s.split_once(',')?;
    let mut p = hms.split(':').map(|x| x.parse::<f64>().ok());
    let (h, m, sec) = (p.next()??, p.next()??, p.next()??);
    Some(h * 3600.0 + m * 60.0 + sec + ms.parse::<f64>().ok()? / 1000.0)
}

/// Read an SRT file into `(start, end, text)` cues.
pub fn parse_srt(text: &str) -> Result<Vec<(f64, f64, String)>, String> {
    let text = text.replace("\r\n", "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let Some(ix) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let (a, b) = lines[ix].split_once("-->").ok_or("bad cue")?;
        let (start, end) = (
            parse_srt_time(a).ok_or_else(|| format!("bad time '{}'", a.trim()))?,
            parse_srt_time(b.split_whitespace().next().unwrap_or(""))
                .ok_or_else(|| format!("bad time '{}'", b.trim()))?,
        );
        let body = lines[ix + 1..].join("\n");
        if !body.is_empty() && end > start {
            cues.push((start, end, body));
        }
    }
    if cues.is_empty() {
        return Err("no subtitles found in the file".into());
    }
    Ok(cues)
}

pub fn write_srt(cues: &[(f64, f64, String)]) -> String {
    cues.iter()
        .enumerate()
        .map(|(i, (a, b, t))| format!("{}\n{} --> {}\n{}\n", i + 1, srt_time(*a), srt_time(*b), t))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"text":" Hello everyone. Um, this is a test.","segments":[
      {"id":0,"start":0.0,"end":0.84,"text":" Hello everyone.","words":[
        {"word":" Hello","start":0.0,"end":0.3,"probability":0.8},{"word":" everyone.","start":0.3,"end":0.84,"probability":0.9}]},
      {"id":1,"start":1.28,"end":4.3,"text":" Um, this is a test.","words":[
        {"word":" Um,","start":1.28,"end":1.6},{"word":" this","start":1.86,"end":2.14},{"word":" is","start":2.14,"end":2.28},
        {"word":" a","start":2.28,"end":2.38},{"word":" test.","start":2.38,"end":2.68}]}]}"#;

    #[test]
    fn parses_whisper_output() {
        let s = parse_whisper_json(JSON).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[1].words[0],
            Word {
                text: "Um,".into(),
                start: 1.28,
                end: 1.6
            }
        );
        assert!(parse_whisper_json("{}").is_err());
        assert!(parse_whisper_json("nope").is_err());
    }

    #[test]
    fn finds_filler_words_and_merges_neighbours() {
        let s = parse_whisper_json(JSON).unwrap();
        let r = filler_ranges(&s, false, 0.05);
        assert_eq!(r.len(), 1);
        assert!((r[0].0 - 1.23).abs() < 1e-9 && (r[0].1 - 1.65).abs() < 1e-9);
        assert!(is_filler("Uh,", false) && is_filler("UM", false) && !is_filler("umbrella", false));
        assert!(!is_filler("like", false) && is_filler("like", true));
        // two fillers that touch become one range
        let seg = Segment {
            text: "".into(),
            start: 0.0,
            end: 2.0,
            words: vec![
                Word {
                    text: "um".into(),
                    start: 0.0,
                    end: 0.3,
                },
                Word {
                    text: "uh".into(),
                    start: 0.32,
                    end: 0.6,
                },
                Word {
                    text: "yes".into(),
                    start: 1.0,
                    end: 1.3,
                },
            ],
        };
        assert_eq!(filler_ranges(&[seg], false, 0.05), vec![(0.0, 0.65)]);
    }

    #[test]
    fn caption_lines_break_at_sentences_pauses_and_length() {
        let s = parse_whisper_json(JSON).unwrap();
        let lines = caption_lines(&s, 42, 6.0);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].2, "Hello everyone.");
        assert_eq!(lines[1], (1.28, 2.68, "Um, this is a test.".to_string()));
        // a short length limit splits a sentence
        let split = caption_lines(&s, 12, 6.0);
        assert!(split.len() > 2, "{split:?}");
        assert!(
            split.iter().all(|(_, _, t)| t.chars().count() <= 14),
            "{split:?}"
        );
    }

    #[test]
    fn srt_round_trip() {
        let cues = vec![
            (0.0, 0.84, "Hello everyone.".to_string()),
            (61.5, 3725.25, "Two\nlines".to_string()),
        ];
        let text = write_srt(&cues);
        assert!(text.starts_with("1\n00:00:00,000 --> 00:00:00,840\nHello everyone.\n\n2\n00:01:01,500 --> 01:02:05,250\nTwo\nlines"));
        assert_eq!(parse_srt(&text).unwrap(), cues);
        assert_eq!(
            parse_srt("1\r\n00:00:01.000 --> 00:00:02.000 X1:10\r\nHi\r\n").unwrap(),
            vec![(1.0, 2.0, "Hi".to_string())]
        );
        assert!(parse_srt("nothing here").is_err());
        assert!(parse_srt("1\n00:00:05,000 --> 00:00:01,000\nbackwards").is_err());
    }
}
