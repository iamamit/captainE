//! Pictures for CoCaptain: frames of the timeline (with the edits it is making) and of any footage, small and
//! labelled, so the model can look before it judges and look again after it edits.

use captaine_model::{Id, Project};
use captaine_render::{
    encode_rgb_png, labelled_sheet, preview_project, Frame8, RenderOptions, Renderer, VideoReader,
};

/// Widest picture sent (pixels); the model sees detail well enough at this size and it keeps requests small.
pub const MAX_WIDTH: u32 = 1024;

/// Standard base64 (RFC 4648 §4) with padding, as image blocks carry it.
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let v = (chunk[0] as u32) << 16
            | (chunk.get(1).copied().unwrap_or(0) as u32) << 8
            | chunk.get(2).copied().unwrap_or(0) as u32;
        for i in 0..4 {
            if i <= n {
                out.push(ABC[(v >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// "1.20 s"
fn label(secs: f64) -> String {
    format!("{secs:.2} s")
}

/// The timeline's picture at each time (seconds), `width` pixels wide, labelled with the time.
pub fn timeline_frames(
    project: &Project,
    timeline: Id,
    times: &[f64],
    width: u32,
) -> Result<Vec<(String, Frame8)>, String> {
    let tl = project
        .timeline(timeline)
        .ok_or("the timeline is missing")?;
    let rate = tl.rate.as_f64();
    let (cw, _) = project.canvas(timeline);
    let scale = (width.min(MAX_WIDTH) as f64 / cw.max(1) as f64).min(1.0);
    let small = preview_project(project, scale);
    let opts = RenderOptions {
        decode_scale: scale,
        hw_decode: true,
        ..RenderOptions::default()
    };
    let mut r = Renderer::new(&small, timeline, opts).map_err(|e| e.to_string())?;
    let last = (tl.duration().value - 1).max(0);
    let mut out = Vec::new();
    for &t in times {
        let tick = ((t * rate).round() as i64).clamp(0, last);
        let c = r.render(tick).map_err(|e| e.to_string())?;
        out.push((
            label(t),
            Frame8 {
                width: c.width,
                height: c.height,
                rgba: c.to_rgba8(),
            },
        ));
    }
    Ok(out)
}

/// Frames of a media file at each time (seconds into the file), `width` pixels wide. Also returns the file's
/// length in seconds.
pub fn media_frames(
    path: &str,
    times: &[f64],
    width: u32,
) -> Result<(Vec<(String, Frame8)>, f64), String> {
    let m = captaine_media::media_ref(std::path::Path::new(path), Id::new())
        .map_err(|e| e.to_string())?;
    if !m.has_video {
        return Err(format!("{} has no picture", m.name));
    }
    let dur = m.duration.ok_or("the file's length is unknown")?;
    let (rate, secs) = (dur.rate, dur.to_seconds_f64());
    let w = (width.min(MAX_WIDTH).min(m.width.max(2))) & !1;
    let h = ((m.height as f64 * w as f64 / m.width.max(1) as f64).round() as u32).max(2) & !1;
    let mut out = Vec::new();
    for &t in times {
        let f = ((t.clamp(0.0, secs) * rate.as_f64()).round() as i64)
            .min(dur.value - 1)
            .max(0);
        let mut r = VideoReader::open(path, w, h, rate, f)?;
        out.push((label(t), r.frame(f)?.as_ref().clone()));
    }
    Ok((out, secs))
}

/// PNG pictures of the frames: one contact sheet (a grid, labels in the corners) or one picture per frame.
pub fn pictures(frames: &[(String, Frame8)], sheet: bool) -> Result<Vec<Vec<u8>>, String> {
    if sheet && frames.len() > 1 {
        let cols = match frames.len() {
            2 => 2,
            3 | 5 | 6 | 9 => 3,
            _ => 4,
        };
        let (w, h, rgb) = labelled_sheet(frames, cols);
        return Ok(vec![encode_rgb_png(w, h, &rgb)?]);
    }
    frames
        .iter()
        .map(|f| {
            let (w, h, rgb) = labelled_sheet(std::slice::from_ref(f), 1);
            encode_rgb_png(w, h, &rgb)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_examples() {
        // RFC 4648 §10
        for (raw, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
    }
}
