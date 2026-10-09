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
        let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut enc = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&self.to_rgb8())
            .map_err(|e| e.to_string())
    }
}
