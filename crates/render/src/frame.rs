use rayon::prelude::*;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use crate::color::{to_linear, to_srgb8};

/// A decoded picture: 8-bit sRGB RGBA, straight (non-premultiplied) alpha, as it comes from the decoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame8 {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame8 {
    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Frame8 {
        Frame8 {
            width,
            height,
            rgba: rgba
                .iter()
                .copied()
                .cycle()
                .take((width * height * 4) as usize)
                .collect(),
        }
    }
}

/// The working image: linear light, premultiplied alpha, f32 RGBA.
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
}

impl Canvas {
    /// Opaque black.
    pub fn black(width: u32, height: u32) -> Canvas {
        let mut data = vec![0.0; (width * height * 4) as usize];
        data.par_chunks_mut(4 * 4096)
            .for_each(|run| run.chunks_exact_mut(4).for_each(|p| p[3] = 1.0));
        Canvas {
            width,
            height,
            data,
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [f32; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    /// Packed 8-bit sRGB RGB (alpha dropped: the canvas starts opaque black so it is always 1).
    pub fn to_rgb8(&self) -> Vec<u8> {
        let mut out = vec![0u8; (self.width * self.height * 3) as usize];
        out.par_chunks_mut(3 * 4096)
            .zip(self.data.par_chunks(4 * 4096))
            .for_each(|(o, d)| {
                for (o, p) in o.chunks_exact_mut(3).zip(d.chunks_exact(4)) {
                    o[0] = to_srgb8(p[0]);
                    o[1] = to_srgb8(p[1]);
                    o[2] = to_srgb8(p[2]);
                }
            });
        out
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = vec![255u8; self.data.len()];
        out.par_chunks_mut(4)
            .zip(self.data.par_chunks(4))
            .for_each(|(o, p)| {
                o[0] = to_srgb8(p[0]);
                o[1] = to_srgb8(p[1]);
                o[2] = to_srgb8(p[2]);
            });
        out
    }

    /// Mix an 8-bit picture over the canvas: `amount` (0..1) times the picture's own alpha says how much of
    /// it replaces what is there. Used by adjustment layers.
    pub fn blend_adjusted(&mut self, f: &Frame8, amount: f32) {
        self.data
            .par_chunks_mut(4 * 1024)
            .zip(f.rgba.par_chunks(4 * 1024))
            .for_each(|(dr, sr)| {
                for (d, s) in dr.chunks_exact_mut(4).zip(sr.chunks_exact(4)) {
                    let a = s[3] as f32 / 255.0 * amount;
                    if a <= 0.0 {
                        continue;
                    }
                    for c in 0..3 {
                        d[c] = d[c] * (1.0 - a) + to_linear(s[c]) * a;
                    }
                }
            });
    }

    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        save_rgb_png(path, self.width, self.height, &self.to_rgb8())
    }
}

/// Packed 8-bit RGB as PNG bytes in memory.
pub fn encode_rgb_png(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut bytes, width, height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(rgb).map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}

/// Pictures in a grid of `columns`, each with its label written in its top-left corner (on a dark band so it
/// reads on any picture). Returns the size and packed RGB. All pictures are scaled to the first one's width.
pub fn labelled_sheet(frames: &[(String, Frame8)], columns: usize) -> (u32, u32, Vec<u8>) {
    let Some((_, first)) = frames.first() else {
        return (0, 0, Vec::new());
    };
    let cols = columns.clamp(1, frames.len());
    let rows = frames.len().div_ceil(cols);
    let cell_w = first.width;
    let cell_h = frames
        .iter()
        .map(|(_, f)| (f.height as u64 * cell_w as u64 / f.width.max(1) as u64) as u32)
        .max()
        .unwrap_or(1);
    let gap = 4u32;
    let (w, h) = (
        cols as u32 * cell_w + (cols as u32 - 1) * gap,
        rows as u32 * cell_h + (rows as u32 - 1) * gap,
    );
    let mut rgb = vec![24u8; (w * h * 3) as usize];
    for (i, (label, f)) in frames.iter().enumerate() {
        let mut cell = if f.width == cell_w {
            f.clone()
        } else {
            f.thumbnail(cell_w.min(f.width))
        };
        let size = (cell_h as f32 / 12.0).clamp(10.0, 28.0);
        let band = (size * 1.4) as u32;
        for y in 0..band.min(cell.height) {
            for x in 0..(label.chars().count() as f32 * size * 0.62 + size) as u32 {
                if x < cell.width {
                    let k = ((y * cell.width + x) * 4) as usize;
                    for c in 0..3 {
                        cell.rgba[k + c] /= 3;
                    }
                }
            }
        }
        crate::text::draw_text(
            &mut cell,
            label,
            size * 0.4,
            size * 1.05,
            size,
            [255, 230, 80],
            1.0,
            false,
        );
        let (x0, y0) = (
            (i % cols) as u32 * (cell_w + gap),
            (i / cols) as u32 * (cell_h + gap),
        );
        for y in 0..cell.height.min(cell_h) {
            for x in 0..cell.width.min(cell_w) {
                let (s, d) = (
                    ((y * cell.width + x) * 4) as usize,
                    (((y0 + y) * w + x0 + x) * 3) as usize,
                );
                rgb[d..d + 3].copy_from_slice(&cell.rgba[s..s + 3]);
            }
        }
    }
    (w, h, rgb)
}

/// Write packed 8-bit RGB as a PNG.
pub fn save_rgb_png(path: &Path, width: u32, height: u32, rgb: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(rgb).map_err(|e| e.to_string())
}

impl Frame8 {
    /// Box-filter downscale to `width` (keeps the aspect ratio). Meant for thumbnails.
    pub fn thumbnail(&self, width: u32) -> Frame8 {
        let width = width.clamp(1, self.width);
        let height = ((self.height as u64 * width as u64) / self.width as u64).max(1) as u32;
        let span = |i: u32, n: u32, total: u32| {
            let a = i * total / n;
            (a, ((i + 1) * total / n).max(a + 1))
        };
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let (y0, y1) = span(y, height, self.height);
            for x in 0..width {
                let (x0, x1) = span(x, width, self.width);
                let mut sum = [0u32; 4];
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let i = ((sy * self.width + sx) * 4) as usize;
                        for (k, s) in sum.iter_mut().enumerate() {
                            *s += self.rgba[i + k] as u32;
                        }
                    }
                }
                let n = (y1 - y0) * (x1 - x0);
                rgba.extend(sum.map(|v| ((v + n / 2) / n) as u8));
            }
        }
        Frame8 {
            width,
            height,
            rgba,
        }
    }
}

/// Lay thumbnails side by side; returns the size and packed RGB.
pub fn contact_sheet(frames: &[Frame8]) -> (u32, u32, Vec<u8>) {
    let h = frames.iter().map(|f| f.height).max().unwrap_or(0);
    let w: u32 = frames.iter().map(|f| f.width).sum();
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    let mut x0 = 0;
    for f in frames {
        for y in 0..f.height {
            for x in 0..f.width {
                let (s, d) = (
                    ((y * f.width + x) * 4) as usize,
                    ((y * w + x0 + x) * 3) as usize,
                );
                rgb[d..d + 3].copy_from_slice(&f.rgba[s..s + 3]);
            }
        }
        x0 += f.width;
    }
    (w, h, rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_averages_blocks() {
        let mut f = Frame8::solid(4, 2, [0, 0, 0, 255]);
        for y in 0..2 {
            for x in 0..2 {
                let i = ((y * 4 + x) * 4) as usize;
                f.rgba[i..i + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let t = f.thumbnail(2);
        assert_eq!((t.width, t.height), (2, 1));
        assert_eq!(&t.rgba[..4], &[255, 255, 255, 255]);
        assert_eq!(&t.rgba[4..8], &[0, 0, 0, 255]);
    }

    #[test]
    fn a_labelled_sheet_is_a_grid_and_encodes() {
        let frames: Vec<(String, Frame8)> = (0..5)
            .map(|i| {
                (
                    format!("{i}.0 s"),
                    Frame8::solid(320, 180, [0, 90, 200, 255]),
                )
            })
            .collect();
        let (w, h, rgb) = labelled_sheet(&frames, 3);
        assert_eq!((w, h), (3 * 320 + 2 * 4, 2 * 180 + 4));
        // the label is written in yellow on the first picture
        assert!(rgb
            .chunks(3)
            .take((w * 24) as usize)
            .any(|p| p[0] > 180 && p[2] < 120));
        let png = encode_rgb_png(w, h, &rgb).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn sheet_places_frames_in_a_row() {
        let a = Frame8::solid(2, 2, [255, 0, 0, 255]);
        let b = Frame8::solid(2, 2, [0, 0, 255, 255]);
        let (w, h, rgb) = contact_sheet(&[a, b]);
        assert_eq!((w, h), (4, 2));
        assert_eq!(&rgb[0..3], &[255, 0, 0]);
        assert_eq!(&rgb[6..9], &[0, 0, 255]);
    }
}
