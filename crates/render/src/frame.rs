use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use crate::color::to_srgb8;

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
        data.chunks_exact_mut(4).for_each(|p| p[3] = 1.0);
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
        let mut out = Vec::with_capacity((self.width * self.height * 3) as usize);
        for p in self.data.chunks_exact(4) {
            out.extend([to_srgb8(p[0]), to_srgb8(p[1]), to_srgb8(p[2])]);
        }
        out
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.data.len());
        for p in self.data.chunks_exact(4) {
            out.extend([to_srgb8(p[0]), to_srgb8(p[1]), to_srgb8(p[2]), 255]);
        }
        out
    }

    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        save_rgb_png(path, self.width, self.height, &self.to_rgb8())
    }
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
    fn sheet_places_frames_in_a_row() {
        let a = Frame8::solid(2, 2, [255, 0, 0, 255]);
        let b = Frame8::solid(2, 2, [0, 0, 255, 255]);
        let (w, h, rgb) = contact_sheet(&[a, b]);
        assert_eq!((w, h), (4, 2));
        assert_eq!(&rgb[0..3], &[255, 0, 0]);
        assert_eq!(&rgb[6..9], &[0, 0, 255]);
    }
}
