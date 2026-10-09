//! Apple's Vision framework, used for what a video editor wants from machine vision: lifting a subject out
//! of its background (a matte per frame), finding faces, finding what the eye is drawn to, and following an
//! object. It runs on this Mac; nothing is uploaded. The work is done by `vision.py` through PyObjC.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const SCRIPT: &str = include_str!("vision.py");

/// A box in fractions of the frame, origin top-left: `[x, y, width, height]`.
pub type Rect = [f64; 4];

/// A Python that can `import Vision`.
pub fn find_python() -> Option<PathBuf> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let home = std::env::var("HOME").ok().map(PathBuf::from);
            let mut candidates: Vec<PathBuf> = Vec::new();
            if let Ok(p) = std::env::var("CAPTAINE_PYTHON") {
                candidates.push(p.into());
            }
            if let Some(h) = &home {
                candidates.push(h.join(".captaine/vision/bin/python"));
                candidates.push(h.join("codit/.venv/bin/python"));
                candidates.push(h.join("resolve-claude-mcp/.venv/bin/python"));
            }
            candidates.push("python3".into());
            candidates.into_iter().find(|p| {
                Command::new(p)
                    .args(["-c", "import Vision, Quartz"])
                    .output()
                    .is_ok_and(|o| o.status.success())
            })
        })
        .clone()
}

/// Why the vision tools cannot run here, or None when they can.
pub fn unavailable() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return Some(
            "Subject detection uses Apple's Vision framework, which is only on a Mac.".into(),
        );
    }
    if find_python().is_none() {
        return Some("No Python with PyObjC found. Install pyobjc-framework-Vision (pip install pyobjc-framework-Vision) or set CAPTAINE_PYTHON.".into());
    }
    None
}

fn script_path() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("captaine-vision-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("vision.py");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(SCRIPT) {
        std::fs::write(&path, SCRIPT).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

fn run(args: &[String]) -> Result<String, String> {
    if let Some(why) = unavailable() {
        return Err(why);
    }
    let python = find_python().expect("checked");
    let out = Command::new(python)
        .arg(script_path()?)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run Python: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "Vision failed: {}",
            err.lines().last().unwrap_or("")
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Cut `count` frames starting `from_secs` into the media, scaled to at most `max_w` wide, as numbered PNGs
/// at `fps` frames per second. Returns how many files were written.
pub fn extract_frames(
    media: &str,
    from_secs: f64,
    count: usize,
    fps: f64,
    max_w: u32,
    dir: &Path,
) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let out = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-nostdin", "-ss"])
        .arg(format!("{:.6}", from_secs.max(0.0)))
        .arg("-i")
        .arg(media)
        .args(["-an", "-sn", "-vf"])
        .arg(format!("fps={fps},scale='min({max_w},iw)':-2"))
        .args(["-frames:v", &count.to_string()])
        .arg(dir.join("f_%06d.png"))
        .output()
        .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(std::fs::read_dir(dir).map_or(0, |d| {
        d.filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
            .count()
    }))
}

/// How the subject is found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatteMode {
    /// Whatever stands out as the subject (people, animals, objects)
    Subject,
    /// People only
    Person,
}

/// Write one grey matte PNG per frame of `frames` into `out` (same names). White is subject.
pub fn matte_frames(frames: &Path, out: &Path, mode: MatteMode) -> Result<(), String> {
    run(&[
        "matte".into(),
        frames.to_string_lossy().into(),
        out.to_string_lossy().into(),
        match mode {
            MatteMode::Subject => "foreground".into(),
            MatteMode::Person => "person".into(),
        },
    ])
    .map(|_| ())
}

fn parse_boxes(text: &str) -> Result<Vec<Vec<Rect>>, String> {
    let v: serde_json::Value =
        serde_json::from_str(text.trim()).map_err(|e| format!("unreadable Vision output: {e}"))?;
    let frames = v.as_array().ok_or("Vision output is not a list")?;
    Ok(frames
        .iter()
        .map(|f| {
            f.as_array()
                .map(|boxes| {
                    boxes
                        .iter()
                        .filter_map(|b| {
                            let a = b.as_array()?;
                            Some([
                                a.first()?.as_f64()?,
                                a.get(1)?.as_f64()?,
                                a.get(2)?.as_f64()?,
                                a.get(3)?.as_f64()?,
                            ])
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect())
}

/// Face boxes in each frame.
pub fn faces(frames: &Path) -> Result<Vec<Vec<Rect>>, String> {
    parse_boxes(&run(&["faces".into(), frames.to_string_lossy().into()])?)
}

/// Where attention goes in each frame (usually one box).
pub fn saliency(frames: &Path) -> Result<Vec<Vec<Rect>>, String> {
    parse_boxes(&run(&["saliency".into(), frames.to_string_lossy().into()])?)
}

/// Follow the object starting in `start` through the frames; one box and its confidence per frame.
pub fn track(frames: &Path, start: Rect) -> Result<Vec<(Rect, f64)>, String> {
    let out = run(&[
        "track".into(),
        frames.to_string_lossy().into(),
        start[0].to_string(),
        start[1].to_string(),
        start[2].to_string(),
        start[3].to_string(),
    ])?;
    let v: serde_json::Value =
        serde_json::from_str(out.trim()).map_err(|e| format!("unreadable Vision output: {e}"))?;
    Ok(v.as_array()
        .ok_or("Vision output is not a list")?
        .iter()
        .filter_map(|b| {
            let a = b.as_array()?;
            Some((
                [
                    a.first()?.as_f64()?,
                    a.get(1)?.as_f64()?,
                    a.get(2)?.as_f64()?,
                    a.get(3)?.as_f64()?,
                ],
                a.get(4)?.as_f64()?,
            ))
        })
        .collect())
}

/// A scratch directory that is removed when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Result<Scratch, String> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let p = std::env::temp_dir().join(format!("captaine-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
        Ok(Scratch(p))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A grey matte video for `dur_secs` of `media` from `from_secs`: white where the subject is, one frame
/// per frame of the media at `fps`, stored losslessly at `out` (an .mkv).
pub fn matte_video(
    media: &str,
    from_secs: f64,
    dur_secs: f64,
    fps: f64,
    mode: MatteMode,
    out: &Path,
) -> Result<(), String> {
    let n = (dur_secs * fps).round().max(1.0) as usize;
    let scratch = Scratch::new("matte")?;
    let (frames, mattes) = (scratch.0.join("in"), scratch.0.join("out"));
    let got = extract_frames(media, from_secs, n, fps, 640, &frames)?;
    if got == 0 {
        return Err("no frames could be read from the media".into());
    }
    matte_frames(&frames, &mattes, mode)?;
    let _ = std::fs::remove_file(out);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let enc = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-framerate"])
        .arg(format!("{fps}"))
        .arg("-i")
        .arg(mattes.join("f_%06d.png"))
        .args(["-vf", "format=gray", "-c:v", "ffv1"])
        .arg(out)
        .output()
        .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    if !enc.status.success() {
        return Err(String::from_utf8_lossy(&enc.stderr).trim().to_string());
    }
    Ok(())
}

/// The box (fractions of the frame) around everything the matte calls subject, or None when the picture is empty.
fn matte_box(path: &Path) -> Option<Rect> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = info.color_type.samples();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            if buf[(y * w + x) * channels] > 128 {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    (x1 >= x0 && y1 >= y0).then(|| {
        [
            x0 as f64 / w as f64,
            y0 as f64 / h as f64,
            (x1 - x0 + 1) as f64 / w as f64,
            (y1 - y0 + 1) as f64 / h as f64,
        ]
    })
}

/// The box around the main subject in each frame, found by lifting it out of the background.
pub fn subjects(frames: &Path) -> Result<Vec<Option<Rect>>, String> {
    let scratch = Scratch::new("subjects")?;
    matte_frames(frames, &scratch.0, MatteMode::Subject)?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&scratch.0)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    files.sort();
    Ok(files.iter().map(|f| matte_box(f)).collect())
}
