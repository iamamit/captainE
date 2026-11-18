//! Text and simple shapes drawn into an RGBA picture with straight alpha, using the bundled font.
//! Pure CPU and table-free arithmetic, so the same title draws identically everywhere.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};

use crate::Frame8;

fn default_font() -> FontArc {
    static F: OnceLock<FontArc> = OnceLock::new();
    F.get_or_init(|| {
        FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("bundled font parses")
    })
    .clone()
}

/// A typeface by name: `builtin:mono` or the path of a font file. `None` if it cannot be read.
///
/// A font collection (`.ttc`/`.otc`, several styles in one file) takes a face after `#`: its number
/// (`Futura.ttc#4`) or its style name (`Futura.ttc#Condensed ExtraBold`, matched without regard to case
/// against the style and full names). Without `#` the first face is used.
pub fn load_font(spec: &str) -> Option<FontArc> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<FontArc>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut map = cache.lock().ok()?;
    map.entry(spec.to_string())
        .or_insert_with(|| match spec {
            "builtin:mono" => FontArc::try_from_slice(epaint_default_fonts::HACK_REGULAR).ok(),
            spec => {
                let (path, face) = split_face(spec);
                let bytes = std::fs::read(path).ok()?;
                let index = match face {
                    None => 0,
                    Some(f) => face_index(&bytes, f)?,
                };
                ab_glyph::FontVec::try_from_vec_and_index(bytes, index)
                    .ok()
                    .map(FontArc::new)
            }
        })
        .clone()
}

/// `path#face` -> (path, Some(face)); a `#` only counts when the part before it is an existing file,
/// so a path that itself contains `#` still works.
fn split_face(spec: &str) -> (&str, Option<&str>) {
    if let Some(i) = spec.rfind('#') {
        let (path, face) = (&spec[..i], &spec[i + 1..]);
        if !face.is_empty() && std::path::Path::new(path).is_file() {
            return (path, Some(face));
        }
    }
    (spec, None)
}

fn face_index(bytes: &[u8], face: &str) -> Option<u32> {
    if let Ok(n) = face.trim().parse::<u32>() {
        return (n < face_count(bytes).max(1)).then_some(n);
    }
    let want = face.trim().to_lowercase();
    font_faces(bytes)
        .into_iter()
        .position(|names| names.iter().any(|n| n.to_lowercase() == want))
        .map(|i| i as u32)
}

fn be16(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn be32(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// How many faces a font file holds (1 for a plain .ttf/.otf, 0 if it is not a font).
pub fn face_count(bytes: &[u8]) -> u32 {
    match bytes.get(..4) {
        Some(b"ttcf") => be32(bytes, 8).unwrap_or(0) as u32,
        Some(_) if font_names(bytes, 0).is_some() => 1,
        _ => 0,
    }
}

/// The names of every face in a font file, in face order: for each face its style name
/// (e.g. "Condensed ExtraBold"), full name and family, whichever the file provides.
pub fn font_faces(bytes: &[u8]) -> Vec<Vec<String>> {
    let offsets: Vec<usize> = if bytes.get(..4) == Some(b"ttcf") {
        let n = be32(bytes, 8).unwrap_or(0).min(256);
        (0..n).filter_map(|i| be32(bytes, 12 + 4 * i)).collect()
    } else {
        vec![0]
    };
    offsets
        .into_iter()
        .map(|o| font_names(bytes, o).unwrap_or_default())
        .collect()
}

/// Style (2, 17), full (4) and family (1, 16) names from the `name` table of the face at `offset`.
fn font_names(b: &[u8], offset: usize) -> Option<Vec<String>> {
    let tables = be16(b, offset + 4)?;
    let name = (0..tables).find_map(|i| {
        let rec = offset + 12 + 16 * i;
        (b.get(rec..rec + 4)? == b"name").then(|| be32(b, rec + 8))?
    })?;
    let (count, strings) = (be16(b, name + 2)?, name + be16(b, name + 4)?);
    let mut found: Vec<(usize, String)> = Vec::new();
    for i in 0..count.min(4096) {
        let r = name + 6 + 12 * i;
        let (platform, language, id) = (be16(b, r)?, be16(b, r + 4)?, be16(b, r + 6)?);
        let (len, at) = (be16(b, r + 8)?, strings + be16(b, r + 10)?);
        let raw = b.get(at..at + len)?;
        let text = match (platform, language) {
            (3, 0x409) | (0, _) => String::from_utf16_lossy(
                &raw.chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect::<Vec<_>>(),
            ),
            (1, 0) => raw.iter().map(|&c| c as char).collect(),
            _ => continue,
        };
        let rank = match id {
            17 => 0,
            2 => 1,
            4 => 2,
            16 => 3,
            1 => 4,
            _ => continue,
        };
        if !found.iter().any(|(_, t)| *t == text) {
            found.push((rank, text));
        }
    }
    found.sort_by_key(|(r, _)| *r);
    Some(found.into_iter().map(|(_, t)| t).collect())
}

/// How text is drawn right now: typeface and decoration. Set around a title's drawing with [`with_look`].
#[derive(Clone)]
pub struct Look {
    pub font: Option<FontArc>,
    /// Extra pixels between letters
    pub tracking: f32,
    /// Outline thickness in pixels (0 = none) and its colour
    pub outline: f32,
    pub outline_rgb: [u8; 3],
    /// Drop shadow distance in pixels (0 = none)
    pub shadow: f32,
    /// Centred soft halo blur in pixels (0 = none)
    pub glow: f32,
}

impl Default for Look {
    fn default() -> Look {
        Look {
            font: None,
            tracking: 0.0,
            outline: 0.0,
            outline_rgb: [0, 0, 0],
            shadow: 0.0,
            glow: 0.0,
        }
    }
}

thread_local! {
    static LOOK: RefCell<Look> = RefCell::new(Look::default());
}

/// Run `f` with `look` in force for text drawing on this thread.
pub fn with_look<R>(look: Look, f: impl FnOnce() -> R) -> R {
    let previous = LOOK.with(|l| std::mem::replace(&mut *l.borrow_mut(), look));
    let r = f();
    LOOK.with(|l| *l.borrow_mut() = previous);
    r
}

fn font() -> FontArc {
    LOOK.with(|l| l.borrow().font.clone())
        .unwrap_or_else(default_font)
}

fn look() -> Look {
    LOOK.with(|l| l.borrow().clone())
}

/// Distance the faux-bold second pass is shifted, as a fraction of the text size.
const BOLD_SHIFT: f32 = 0.034;

/// Whether bold text is thickened with shifted extra passes. Only the bundled font needs it; a chosen font file
/// already has the weight the user picked, and extra passes turn a thin face into parallel strokes.
fn faux_bold() -> bool {
    LOOK.with(|l| l.borrow().font.is_none())
}

/// Width of `text` in pixels at `size` pixels tall.
pub fn text_width(text: &str, size: f32, bold: bool) -> f32 {
    let font = font();
    let f = font.as_scaled(PxScale::from(size));
    let tracking = look().tracking;
    let bold = bold && faux_bold();
    let mut w = 0.0;
    let mut prev = None;
    let mut n = 0;
    for c in text.chars() {
        let id = f.glyph_id(c);
        if let Some(p) = prev {
            w += f.kern(p, id);
        }
        w += f.h_advance(id);
        prev = Some(id);
        n += 1;
    }
    if n > 1 {
        w += tracking * (n - 1) as f32;
    }
    if bold {
        w += size * BOLD_SHIFT * 2.0;
    }
    w
}

/// Pixels from the baseline up to the top of a capital letter, and down to the lowest descender.
pub fn ascent_descent(size: f32) -> (f32, f32) {
    let font = font();
    let f = font.as_scaled(PxScale::from(size));
    (f.ascent(), -f.descent())
}

/// Composite one source pixel over a straight-alpha destination.
fn over(dst: &mut [u8], rgb: [u8; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = a + da * (1.0 - a);
    for k in 0..3 {
        let v = (rgb[k] as f32 * a + dst[k] as f32 * da * (1.0 - a)) / out_a;
        dst[k] = v.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

#[allow(clippy::too_many_arguments)]
/// Draw `text` with its left end at `x` and its baseline at `baseline`, with the shadow and outline of the
/// current [`Look`].
pub fn draw_text(
    frame: &mut Frame8,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    rgb: [u8; 3],
    alpha: f32,
    bold: bool,
) {
    let l = look();
    if l.glow > 0.0 {
        dark_halo(
            frame,
            text,
            x,
            baseline,
            size,
            alpha,
            bold,
            l.glow * 0.5,
            0,
            0.8,
        );
    }
    if l.shadow > 0.0 {
        draw_soft_shadow(frame, text, x, baseline, size, alpha, bold, l.shadow);
    }
    if l.outline > 0.0 {
        let steps = 16;
        for i in 0..steps {
            let a = i as f32 / steps as f32 * std::f32::consts::TAU;
            draw_text_raw(
                frame,
                text,
                x + a.cos() * l.outline,
                baseline + a.sin() * l.outline,
                size,
                l.outline_rgb,
                alpha,
                bold,
            );
        }
    }
    draw_text_raw(frame, text, x, baseline, size, rgb, alpha, bold);
}

#[allow(clippy::too_many_arguments)]
/// A blurred drop shadow under `text`: the glyphs drawn into a small box around the text, softened with
/// `shadow / 2` pixels of blur and laid down `shadow / 2` pixels lower and to the right. Two hard offset copies
/// read as stacked outlines on thin type; a blurred one reads as a shadow.
fn draw_soft_shadow(
    frame: &mut Frame8,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    alpha: f32,
    bold: bool,
    shadow: f32,
) {
    let off = (shadow * 0.5).round() as i32;
    dark_halo(
        frame,
        text,
        x,
        baseline,
        size,
        alpha,
        bold,
        shadow * 0.5,
        off,
        0.75,
    );
}

/// The glyphs blurred by `blur` pixels, laid down `off` pixels lower and to the right in black at `strength`.
#[allow(clippy::too_many_arguments)]
fn dark_halo(
    frame: &mut Frame8,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    alpha: f32,
    bold: bool,
    blur: f32,
    off: i32,
    strength: f32,
) {
    let shadow = blur * 2.0;
    let (ascent, descent) = ascent_descent(size);
    let pad = (shadow * 1.5 + 2.0).ceil();
    let x0 = (x - pad).floor();
    let y0 = (baseline - ascent - pad).floor();
    let bw = (text_width(text, size, bold) + 2.0 * pad).ceil().max(1.0) as usize;
    let bh = (ascent + descent + 2.0 * pad).ceil().max(1.0) as usize;
    let mut glyphs = Frame8::solid(bw as u32, bh as u32, [0, 0, 0, 0]);
    draw_text_raw(
        &mut glyphs,
        text,
        x - x0,
        baseline - y0,
        size,
        [0, 0, 0],
        1.0,
        bold,
    );
    let plane: Vec<f32> = glyphs.rgba.chunks(4).map(|p| p[3] as f32 / 255.0).collect();
    let soft = crate::effects::blur_plane(&plane, bw, bh, blur as f64);
    // a thin face spreads to almost nothing under the blur; scale back up so its darkest point is as dark as a
    // heavy face's
    let peak = soft.iter().cloned().fold(0.0f32, f32::max);
    let gain = if peak > 0.0 { 1.0 / peak } else { 0.0 };
    let (w, h) = (frame.width as i32, frame.height as i32);
    for (j, row) in soft.chunks(bw).enumerate() {
        let py = y0 as i32 + j as i32 + off;
        if py < 0 || py >= h {
            continue;
        }
        for (i, &a) in row.iter().enumerate() {
            let px = x0 as i32 + i as i32 + off;
            if px >= 0 && px < w && a > 0.0 {
                let k = ((py * w + px) * 4) as usize;
                over(
                    &mut frame.rgba[k..k + 4],
                    [0, 0, 0],
                    (a * gain * strength * alpha).min(1.0),
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text_raw(
    frame: &mut Frame8,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    rgb: [u8; 3],
    alpha: f32,
    bold: bool,
) {
    let scale = PxScale::from(size);
    let font = font();
    let f = font.as_scaled(scale);
    let tracking = look().tracking;
    let passes: &[f32] = if bold && faux_bold() {
        &[0.0, size * BOLD_SHIFT, size * BOLD_SHIFT * 2.0]
    } else {
        &[0.0]
    };
    let (w, h) = (frame.width as i32, frame.height as i32);
    for &shift in passes {
        let mut caret = x + shift;
        let mut prev = None;
        for c in text.chars() {
            let id = f.glyph_id(c);
            if let Some(p) = prev {
                caret += f.kern(p, id);
            }
            let glyph = id.with_scale_and_position(scale, point(caret, baseline));
            if let Some(outlined) = font.outline_glyph(glyph) {
                let b = outlined.px_bounds();
                outlined.draw(|gx, gy, cov| {
                    let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && px < w && py < h && cov > 0.0 {
                        let i = ((py * w + px) * 4) as usize;
                        over(&mut frame.rgba[i..i + 4], rgb, cov * alpha);
                    }
                });
            }
            caret += f.h_advance(id) + tracking;
            prev = Some(id);
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Fill the rectangle `[x0,x1) x [y0,y1)` with rounded corners of `radius`, anti-aliased.
pub fn fill_rounded_rect(
    frame: &mut Frame8,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
    rgb: [u8; 3],
    alpha: f32,
) {
    let (w, h) = (frame.width as i32, frame.height as i32);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (hx, hy) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let r = radius.min(hx).min(hy).max(0.0);
    for py in (y0.floor() as i32).max(0)..(y1.ceil() as i32).min(h) {
        for px in (x0.floor() as i32).max(0)..(x1.ceil() as i32).min(w) {
            // signed distance to the rounded box, measured from the pixel centre
            let (dx, dy) = (
                (px as f32 + 0.5 - cx).abs() - (hx - r),
                (py as f32 + 0.5 - cy).abs() - (hy - r),
            );
            let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
            let d = outside + dx.max(dy).min(0.0) - r;
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov > 0.0 {
                let i = ((py * w + px) * 4) as usize;
                over(&mut frame.rgba[i..i + 4], rgb, cov * alpha);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(w: u32, h: u32) -> Frame8 {
        Frame8::solid(w, h, [0, 0, 0, 0])
    }

    #[test]
    fn glow_darkens_all_round_the_letters() {
        let mut f = Frame8::solid(200, 100, [255, 255, 255, 255]);
        let look = Look {
            glow: 16.0,
            ..Look::default()
        };
        with_look(look, || {
            draw_text(&mut f, "I", 90.0, 70.0, 50.0, [255, 255, 255], 1.0, true)
        });
        let px = |x: usize, y: usize| f.rgba[(y * 200 + x) * 4];
        // find the letter's columns, then look just outside them on both sides and above
        let row = 50;
        let ink: Vec<usize> = (0..200).filter(|&x| px(x, row) < 250).collect();
        assert!(!ink.is_empty(), "the halo leaves some grey");
        let (l, r) = (*ink.first().unwrap(), *ink.last().unwrap());
        assert!(l < 90, "halo reaches left of the letter");
        assert!(r > 95, "and right of it");
        let mid = (l + r) / 2;
        let top = (0..100).find(|&y| px(mid, y) < 250).unwrap();
        assert!(top < 30, "and above it (first grey row {top})");
    }

    #[test]
    fn shadow_is_blurred_not_a_hard_copy() {
        let mut f = Frame8::solid(200, 80, [255, 255, 255, 255]);
        let look = Look {
            shadow: 12.0,
            ..Look::default()
        };
        with_look(look, || {
            draw_text(&mut f, "I", 40.0, 60.0, 50.0, [255, 255, 255], 1.0, false)
        });
        // a soft shadow leaves many grey levels between the white page and its darkest point
        let mut greys: Vec<u8> = f
            .rgba
            .chunks(4)
            .map(|p| p[0])
            .filter(|&v| v < 250)
            .collect();
        greys.sort_unstable();
        greys.dedup();
        assert!(greys.len() > 30, "only {} grey levels", greys.len());
    }

    #[test]
    fn width_grows_with_text_and_size() {
        assert!(text_width("WWWW", 40.0, false) > text_width("iiii", 40.0, false));
        assert!(text_width("Hello", 80.0, false) > text_width("Hello", 40.0, false) * 1.9);
        assert!(text_width("Hello", 40.0, true) > text_width("Hello", 40.0, false));
        assert_eq!(text_width("", 40.0, false), 0.0);
    }

    #[test]
    fn text_lands_between_baseline_and_ascent() {
        let mut f = blank(300, 100);
        draw_text(&mut f, "HE", 10.0, 70.0, 40.0, [255, 255, 255], 1.0, false);
        let inked = |y: u32| (0..300u32).any(|x| f.rgba[((y * 300 + x) * 4 + 3) as usize] > 128);
        assert!(inked(60), "the letters stand on the baseline");
        assert!(
            !inked(20) && !inked(90),
            "and nothing is far above or below"
        );
        // white ink: rgb stays white wherever there is any alpha
        assert!(f.rgba.chunks(4).filter(|p| p[3] > 0).all(|p| p[0] >= 250));
    }

    #[test]
    fn bold_is_heavier_and_drawing_is_deterministic() {
        let ink = |bold: bool| {
            let mut f = blank(200, 80);
            draw_text(&mut f, "Hi", 5.0, 60.0, 50.0, [255; 3], 1.0, bold);
            f
        };
        let (a, b) = (ink(false), ink(true));
        let sum = |f: &Frame8| f.rgba.chunks(4).map(|p| p[3] as u32).sum::<u32>();
        assert!(sum(&b) > sum(&a));
        assert_eq!(ink(true), ink(true));
    }

    #[test]
    fn clipping_at_the_edges_is_safe() {
        let mut f = blank(40, 20);
        draw_text(
            &mut f,
            "overflowing text",
            -30.0,
            10.0,
            40.0,
            [255; 3],
            1.0,
            true,
        );
        fill_rounded_rect(&mut f, -10.0, -10.0, 100.0, 100.0, 8.0, [9, 9, 9], 1.0);
        assert_eq!(f.rgba.len(), 40 * 20 * 4);
    }

    #[test]
    fn rounded_rect_has_soft_round_corners() {
        let mut f = blank(40, 40);
        fill_rounded_rect(&mut f, 4.0, 4.0, 36.0, 36.0, 10.0, [200, 100, 50], 1.0);
        let a = |x: u32, y: u32| f.rgba[((y * 40 + x) * 4 + 3) as usize];
        assert_eq!(a(20, 20), 255);
        assert_eq!(a(5, 5), 0, "the very corner is cut away");
        assert!(a(20, 4) > 200 && a(2, 20) == 0);
        assert_eq!(
            &f.rgba[((20 * 40 + 20) * 4) as usize..][..3],
            &[200, 100, 50]
        );
    }

    #[test]
    fn translucent_text_over_a_box_blends() {
        let mut f = Frame8::solid(10, 10, [255, 0, 0, 255]);
        over(&mut f.rgba[0..4], [0, 0, 255], 0.5);
        assert_eq!(&f.rgba[0..4], &[128, 0, 128, 255]);
    }
}

#[cfg(test)]
mod collection_tests {
    use super::*;

    const FUTURA: &str = "/System/Library/Fonts/Supplemental/Futura.ttc";

    #[test]
    fn plain_font_has_one_face() {
        assert_eq!(face_count(epaint_default_fonts::UBUNTU_LIGHT), 1);
        assert!(!font_faces(epaint_default_fonts::UBUNTU_LIGHT)[0].is_empty());
    }

    #[test]
    fn not_a_font_has_no_faces() {
        assert_eq!(face_count(b"hello world, not a font"), 0);
    }

    #[test]
    fn split_face_only_when_the_file_exists() {
        assert_eq!(
            split_face("/no/such/file.ttc#Bold"),
            ("/no/such/file.ttc#Bold", None)
        );
    }

    #[test]
    fn collection_faces_by_number_and_name() {
        let Ok(bytes) = std::fs::read(FUTURA) else {
            return;
        }; // macOS system font; skip elsewhere
        assert!(face_count(&bytes) > 1);
        let faces = font_faces(&bytes);
        let i = faces
            .iter()
            .position(|n| n.iter().any(|s| s == "Condensed ExtraBold"))
            .unwrap();
        assert_eq!(face_index(&bytes, "condensed extrabold"), Some(i as u32));
        assert_eq!(face_index(&bytes, &i.to_string()), Some(i as u32));
        assert_eq!(face_index(&bytes, "No Such Style"), None);
        assert_eq!(face_index(&bytes, "999"), None);
        let named = load_font(&format!("{FUTURA}#Condensed ExtraBold")).unwrap();
        let first = load_font(FUTURA).unwrap();
        // the condensed face draws an M narrower than the first (regular-width) face
        let w = |f: &FontArc| f.as_scaled(100.0).h_advance(f.glyph_id('M'));
        assert!(w(&named) < w(&first), "{} vs {}", w(&named), w(&first));
    }
}
