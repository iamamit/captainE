use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;

use captaine_model::Id;
use captaine_time::Rational;

use crate::Frame8;

/// Sequential decoder: one `ffmpeg` process streaming raw RGBA frames from a start position.
///
/// Frames come out at the display size (rotation applied) scaled to `width` x `height`, so reading a
/// low-resolution proxy still yields frames the size of the original. Asking for the next frame is cheap;
/// jumping far or backwards restarts the process at the new position.
pub struct VideoReader {
    child: Child,
    out: ChildStdout,
    width: u32,
    height: u32,
    fps: Rational,
    path: String,
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
        if !Path::new(path).exists() {
            return Err(format!("media file not found: {path}"));
        }
        // half a frame early so accurate seeking lands on frame `start`
        let secs = ((start as f64 - 0.5) / fps.as_f64()).max(0.0);
        let mut child = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-nostdin",
                "-ss",
                &format!("{secs:.6}"),
                "-i",
                path,
            ])
            .args([
                "-an",
                "-sn",
                "-vf",
                &format!("scale={width}:{height}:flags=bicubic"),
            ])
            .args(["-pix_fmt", "rgba", "-f", "rawvideo", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot run ffmpeg (is FFmpeg installed?): {e}"))?;
        let out = child.stdout.take().expect("piped");
        Ok(VideoReader {
            child,
            out,
            width,
            height,
            fps,
            path: path.to_string(),
            next: start,
            last: None,
            eof: false,
        })
    }

    fn frame_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    /// Read one frame into `buf`; false at end of stream.
    fn read_into(&mut self, buf: &mut [u8]) -> Result<bool, String> {
        match self.out.read_exact(buf) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => Ok(false),
            Err(e) => Err(format!("reading {}: {e}", self.path)),
        }
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
            *self = VideoReader::open(&self.path, self.width, self.height, self.fps, index)?;
        }
        let mut scratch = Vec::new();
        while self.next <= index && !self.eof {
            let mut buf = vec![0u8; self.frame_bytes()];
            if self.read_into(&mut buf)? {
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
        let _ = self.child.kill();
        let _ = self.child.wait();
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
    fn replacing_an_entry_keeps_the_count_right() {
        let m = Id::from_u128(1);
        let mut c = FrameCache::new(1000);
        c.insert(m, 0, f(10));
        c.insert(m, 0, f(30));
        assert_eq!((c.len(), c.bytes()), (1, 30));
    }
}
