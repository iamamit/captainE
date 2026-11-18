use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;

use captaine_model::{Id, InputSpace};
use captaine_time::Rational;

use crate::Frame8;

/// Sequential decoder: one `ffmpeg` process streaming raw RGBA frames from a start position.
///
/// Frames come out at the display size (rotation applied) scaled to `width` x `height`, so reading a
/// low-resolution proxy still yields frames the size of the original. Asking for the next frame is cheap;
/// jumping far or backwards restarts the process at the new position.
pub struct VideoReader {
    /// The FFmpeg process; taken out when the reader is dropped
    child: Option<Child>,
    /// Frames the reading thread has already pulled out of FFmpeg, in order
    frames: std::sync::mpsc::Receiver<Vec<u8>>,
    width: u32,
    height: u32,
    fps: Rational,
    path: String,
    hw: bool,
    space: InputSpace,
    /// index of the frame the stream will deliver next
    next: i64,
    last: Option<Arc<Frame8>>,
    eof: bool,
}

/// Skip forward through the stream instead of restarting when the target is this close.
const MAX_SKIP: i64 = 90;

impl VideoReader {
    pub fn open(
        path: &str,
        width: u32,
        height: u32,
        fps: Rational,
        start: i64,
    ) -> Result<VideoReader, String> {
        VideoReader::open_with(path, width, height, fps, start, false)
    }

    /// Like [`open`](Self::open); `hw` asks FFmpeg for the platform's hardware decoder (macOS VideoToolbox),
    /// which it quietly replaces with software decoding for formats the hardware cannot do.
    pub fn open_with(
        path: &str,
        width: u32,
        height: u32,
        fps: Rational,
        start: i64,
        hw: bool,
    ) -> Result<VideoReader, String> {
        VideoReader::open_in(path, width, height, fps, start, hw, InputSpace::Rec709)
    }

    /// Like [`open_with`](Self::open_with), bringing footage in `space` into Rec.709 SDR (HDR is tone mapped).
    pub fn open_in(
        path: &str,
        width: u32,
        height: u32,
        fps: Rational,
        start: i64,
        hw: bool,
        space: InputSpace,
    ) -> Result<VideoReader, String> {
        if !Path::new(path).exists() {
            return Err(format!("media file not found: {path}"));
        }
        // half a frame early so accurate seeking lands on frame `start`
        let secs = ((start as f64 - 0.5) / fps.as_f64()).max(0.0);
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin"]);
        if hw && cfg!(target_os = "macos") {
            cmd.args(["-hwaccel", "videotoolbox"]);
        }
        let source = captaine_media::decodable_path(path);
        // a picture is one frame: loop it so any frame of the clip can be read
        if captaine_media::is_tiled_picture(Path::new(&source)) {
            cmd.args(["-stream_loop", "-1"]); // a HEIC that could not be converted
        } else if captaine_media::is_still_image(&source) {
            cmd.args(["-loop", "1"]);
        }
        let mut child = cmd
            .args(["-ss", &format!("{secs:.6}"), "-i", &source])
            // one output frame per decoded frame: the default would duplicate the first one when the seek
            // lands exactly half a frame early, shifting every later frame by one
            .args(["-fps_mode", "passthrough"])
            .args([
                "-an",
                "-sn",
                "-vf",
                &format!(
                    // Frames are addressed by index (frame N = N / fps seconds), so the stream must be constant
                    // rate: phone footage and re-exported edits are often variable frame rate (here 24 to 96 fps in
                    // one file), and counting their raw frames drifts the picture away from the sound. The fps
                    // filter puts every frame on the fps grid, starting at the first frame after the seek, picking
                    // the source frame nearest each slot; a constant-rate file passes through unchanged.
                    // Lanczos keeps the fine detail of camera originals when they are scaled down to the size
                    // they are shown at (bicubic softens it noticeably at 2:1 and beyond)
                    "fps=fps={:.9}:round=near,scale={width}:{height}:flags=lanczos{}",
                    fps.as_f64(),
                    captaine_media::to_rec709(space)
                ),
            ])
            .args(["-pix_fmt", "rgba", "-f", "rawvideo", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot run ffmpeg (is FFmpeg installed?): {e}"))?;
        let mut out: ChildStdout = child.stdout.take().expect("piped");
        // Frames are read ahead on their own thread, so decoding overlaps with compositing instead of
        // waiting for it (FFmpeg would otherwise stall on a full pipe after every frame).
        let bytes = width as usize * height as usize * 4;
        let ahead = (96_000_000 / bytes.max(1)).clamp(2, 12);
        let (tx, frames) = std::sync::mpsc::sync_channel::<Vec<u8>>(ahead);
        std::thread::Builder::new()
            .name("frame-reader".into())
            .spawn(move || loop {
                let mut buf = vec![0u8; bytes];
                if out.read_exact(&mut buf).is_err() || tx.send(buf).is_err() {
                    break;
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(VideoReader {
            child: Some(child),
            frames,
            width,
            height,
            fps,
            path: path.to_string(),
            hw,
            space,
            next: start,
            last: None,
            eof: false,
        })
    }

    /// The next frame's bytes; None at the end of the stream.
    fn next_bytes(&mut self) -> Option<Vec<u8>> {
        self.frames.recv().ok()
    }

    /// Frame `index` of the source. Past the end of the file the last frame is held.
    pub fn frame(&mut self, index: i64) -> Result<Arc<Frame8>, String> {
        if index < self.next || index > self.next + MAX_SKIP {
            if let Some(last) = &self.last {
                // already holding the final frame of a finished stream
                if self.eof && index >= self.next {
                    return Ok(last.clone());
                }
            }
            *self = VideoReader::open_in(
                &self.path,
                self.width,
                self.height,
                self.fps,
                index,
                self.hw,
                self.space,
            )?;
        }
        let mut scratch = Vec::new();
        while self.next <= index && !self.eof {
            if let Some(buf) = self.next_bytes() {
                let wanted = self.next == index;
                self.next += 1;
                if wanted || self.last.is_none() || self.next > index {
                    self.last = Some(Arc::new(Frame8 {
                        width: self.width,
                        height: self.height,
                        rgba: buf,
                    }));
                } else {
                    scratch = buf; // discarded frame
                }
            } else {
                self.eof = true;
            }
        }
        drop(scratch);
        self.last
            .clone()
            .ok_or_else(|| format!("no frames decoded from {}", self.path))
    }
}

impl Drop for VideoReader {
    fn drop(&mut self) {
        // stopping FFmpeg means waiting for it to exit; do that off the thread that is rendering, so the frame
        // at a cut does not wait for the previous clip's decoder to shut down
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let reap = move || {
                let _ = child.wait();
            };
            if std::thread::Builder::new()
                .name("reap-decoder".into())
                .spawn(reap)
                .is_err()
            {
                // no thread to spare: the process was killed already, it is reaped when we exit
            }
        }
    }
}

/// Decoded-frame cache with a byte budget; least recently used frames go first.
pub struct FrameCache {
    map: HashMap<(Id, i64), (Arc<Frame8>, u64)>,
    bytes: usize,
    budget: usize,
    clock: u64,
}

impl FrameCache {
    pub fn new(budget_bytes: usize) -> FrameCache {
        FrameCache {
            map: HashMap::new(),
            bytes: 0,
            budget: budget_bytes,
            clock: 0,
        }
    }

    pub fn get(&mut self, media: Id, frame: i64) -> Option<Arc<Frame8>> {
        self.clock += 1;
        let clock = self.clock;
        self.map.get_mut(&(media, frame)).map(|e| {
            e.1 = clock;
            e.0.clone()
        })
    }

    pub fn insert(&mut self, media: Id, frame: i64, f: Arc<Frame8>) {
        self.clock += 1;
        self.bytes += f.rgba.len();
        if let Some((old, _)) = self.map.insert((media, frame), (f, self.clock)) {
            self.bytes -= old.rgba.len();
        }
        while self.bytes > self.budget && self.map.len() > 1 {
            let oldest = *self
                .map
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| k)
                .expect("non-empty");
            if let Some((f, _)) = self.map.remove(&oldest) {
                self.bytes -= f.rgba.len();
            }
        }
    }

    /// Drop every picture of `media` (its file changed on disk).
    pub fn forget(&mut self, media: Id) {
        let bytes = &mut self.bytes;
        self.map.retain(|(m, _), (f, _)| {
            let keep = *m != media;
            if !keep {
                *bytes -= f.rgba.len();
            }
            keep
        });
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(n: usize) -> Arc<Frame8> {
        Arc::new(Frame8 {
            width: 1,
            height: 1,
            rgba: vec![0; n],
        })
    }

    #[test]
    fn evicts_least_recently_used_within_budget() {
        let m = Id::from_u128(1);
        let mut c = FrameCache::new(100);
        c.insert(m, 0, f(40));
        c.insert(m, 1, f(40));
        assert!(c.get(m, 0).is_some()); // touch 0 so 1 is the oldest
        c.insert(m, 2, f(40));
        assert!(c.get(m, 1).is_none());
        assert!(c.get(m, 0).is_some() && c.get(m, 2).is_some());
        assert_eq!(c.bytes(), 80);
    }

    #[test]
    fn forgetting_a_media_drops_only_its_pictures() {
        let mut c = FrameCache::new(1 << 30);
        let (a, b) = (Id::new(), Id::new());
        c.insert(a, 0, f(10));
        c.insert(a, 1, f(10));
        c.insert(b, 0, f(10));
        c.forget(a);
        assert_eq!(c.len(), 1);
        assert!(c.get(b, 0).is_some() && c.get(a, 0).is_none());
        assert_eq!(c.bytes(), f(10).rgba.len());
    }

    #[test]
    fn replacing_an_entry_keeps_the_count_right() {
        let m = Id::from_u128(1);
        let mut c = FrameCache::new(1000);
        c.insert(m, 0, f(10));
        c.insert(m, 0, f(30));
        assert_eq!((c.len(), c.bytes()), (1, 30));
    }
}
