//! Video scopes drawn from the picture on screen: waveform, RGB parade, vectorscope and histogram.
//! Each returns an image `SIZE` wide and `SIZE` tall.

use egui::{Color32, ColorImage};

pub const SIZE: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    Waveform,
    Parade,
    Vectorscope,
    Histogram,
}

impl ScopeKind {
    pub const ALL: [ScopeKind; 4] = [
        ScopeKind::Waveform,
        ScopeKind::Parade,
        ScopeKind::Vectorscope,
        ScopeKind::Histogram,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ScopeKind::Waveform => "Waveform",
            ScopeKind::Parade => "Parade",
            ScopeKind::Vectorscope => "Vectorscope",
            ScopeKind::Histogram => "Histogram",
        }
    }
}

pub fn render(kind: ScopeKind, src: &ColorImage) -> ColorImage {
    let acc = match kind {
        ScopeKind::Waveform => waveform(src, false),
        ScopeKind::Parade => waveform(src, true),
        ScopeKind::Vectorscope => vectorscope(src),
        ScopeKind::Histogram => histogram(src),
    };
    to_image(acc, kind)
}

/// Per-pixel RGB counts: `SIZE * SIZE` cells of three channels.
type Acc = Vec<[f32; 3]>;

fn waveform(src: &ColorImage, parade: bool) -> Acc {
    let mut acc = vec![[0f32; 3]; SIZE * SIZE];
    let (w, h) = (src.size[0], src.size[1]);
    if w == 0 || h == 0 {
        return acc;
    }
    // sample at most ~one pixel per output column per row
    let step_y = (h / 128).max(1);
    for y in (0..h).step_by(step_y) {
        for x in 0..w {
            let p = src.pixels[y * w + x];
            let rgb = [p.r(), p.g(), p.b()];
            if parade {
                let third = SIZE / 3;
                let col = x * third / w;
                for (c, v) in rgb.iter().enumerate() {
                    let row = SIZE - 1 - *v as usize;
                    acc[row * SIZE + c * third + col][c] += 1.0;
                }
            } else {
                let col = x * SIZE / w;
                let luma = (0.2126 * rgb[0] as f32
                    + 0.7152 * rgb[1] as f32
                    + 0.0722 * rgb[2] as f32) as usize;
                let row = SIZE - 1 - luma.min(255);
                let cell = &mut acc[row * SIZE + col];
                for c in 0..3 {
                    cell[c] += rgb[c] as f32 / 255.0 + 0.02;
                }
            }
        }
    }
    acc
}

fn vectorscope(src: &ColorImage) -> Acc {
    let mut acc = vec![[0f32; 3]; SIZE * SIZE];
    let step = (src.pixels.len() / 40_000).max(1);
    for p in src.pixels.iter().step_by(step) {
        let (r, g, b) = (
            p.r() as f32 / 255.0,
            p.g() as f32 / 255.0,
            p.b() as f32 / 255.0,
        );
        let cb = -0.168736 * r - 0.331264 * g + 0.5 * b;
        let cr = 0.5 * r - 0.418688 * g - 0.081312 * b;
        let x = ((cb + 0.5) * (SIZE - 1) as f32).round() as usize;
        let y = ((0.5 - cr) * (SIZE - 1) as f32).round() as usize;
        let cell = &mut acc[y.min(SIZE - 1) * SIZE + x.min(SIZE - 1)];
        cell[0] += r + 0.02;
        cell[1] += g + 0.02;
        cell[2] += b + 0.02;
    }
    acc
}

fn histogram(src: &ColorImage) -> Acc {
    let mut counts = [[0u32; 256]; 3];
    for p in &src.pixels {
        counts[0][p.r() as usize] += 1;
        counts[1][p.g() as usize] += 1;
        counts[2][p.b() as usize] += 1;
    }
    let peak = counts.iter().flatten().copied().max().unwrap_or(1).max(1) as f32;
    let mut acc = vec![[0f32; 3]; SIZE * SIZE];
    for (c, row) in counts.iter().enumerate() {
        for (x, &n) in row.iter().enumerate() {
            let height = ((n as f32 / peak).sqrt() * (SIZE - 1) as f32) as usize;
            for y in 0..height {
                acc[(SIZE - 1 - y) * SIZE + x][c] = 1.0;
            }
        }
    }
    acc
}

fn to_image(acc: Acc, kind: ScopeKind) -> ColorImage {
    let peak = acc
        .iter()
        .flat_map(|c| c.iter())
        .fold(0f32, |m, v| m.max(*v))
        .max(1.0);
    let gain = match kind {
        // density scopes brighten sparse traces
        ScopeKind::Histogram => 1.0,
        _ => 6.0 / peak.sqrt().max(1.0),
    };
    let mut pixels = Vec::with_capacity(SIZE * SIZE);
    for (i, c) in acc.iter().enumerate() {
        let (x, y) = (i % SIZE, i / SIZE);
        let mut px = [14.0f32, 14.0, 18.0];
        // graticule: lines at 0, 25, 50, 75, 100 %
        let grid = match kind {
            ScopeKind::Vectorscope => (x as i32 - 128).abs() < 1 || (y as i32 - 128).abs() < 1,
            _ => y % 64 == 0 || (kind == ScopeKind::Histogram && x % 64 == 0),
        };
        if grid {
            px = [40.0, 40.0, 48.0];
        }
        let m = c[0].max(c[1]).max(c[2]);
        for ch in 0..3 {
            let v = if kind == ScopeKind::Histogram {
                c[ch] * 0.7
            } else if m > 0.0 {
                // the trace keeps the colour of the pixels that made it; how bright depends on how many
                c[ch] / m * (m * gain).min(1.0)
            } else {
                0.0
            };
            px[ch] = (px[ch] + v * 255.0).min(255.0);
        }
        // parade and histogram colour their traces; the waveform stays the picture's colour
        if kind == ScopeKind::Parade {
            let third = x * 3 / SIZE;
            for (ch, v) in px.iter_mut().enumerate() {
                if ch != third.min(2) {
                    *v = v.min(60.0);
                }
            }
        }
        pixels.push(Color32::from_rgb(px[0] as u8, px[1] as u8, px[2] as u8));
    }
    ColorImage {
        size: [SIZE, SIZE],
        pixels,
        source_size: egui::vec2(SIZE as f32, SIZE as f32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, c: Color32) -> ColorImage {
        ColorImage::new([w, h], vec![c; w * h])
    }

    fn lit(img: &ColorImage, x: usize, y: usize) -> bool {
        let p = img.pixels[y * SIZE + x];
        p.r().max(p.g()).max(p.b()) > 100
    }

    #[test]
    fn waveform_puts_brightness_on_the_vertical_axis() {
        let dark = render(ScopeKind::Waveform, &solid(64, 36, Color32::from_gray(20)));
        let bright = render(ScopeKind::Waveform, &solid(64, 36, Color32::from_gray(230)));
        assert!(lit(&dark, 100, SIZE - 1 - 20) && !lit(&dark, 100, 25));
        assert!(lit(&bright, 100, SIZE - 1 - 230) && !lit(&bright, 100, 200));
    }

    #[test]
    fn vectorscope_places_red_right_of_and_above_the_centre() {
        let img = render(
            ScopeKind::Vectorscope,
            &solid(32, 32, Color32::from_rgb(255, 0, 0)),
        );
        // red: Cb negative (left), Cr positive (up)
        let mut found = None;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let p = img.pixels[y * SIZE + x];
                if p.r() > 200 && p.g() < 100 {
                    found = Some((x, y));
                }
            }
        }
        let (x, y) = found.expect("a red dot");
        assert!(x < 128 && y < 128, "({x}, {y})");
        // grey sits dead centre
        let g = render(
            ScopeKind::Vectorscope,
            &solid(32, 32, Color32::from_gray(128)),
        );
        assert!(lit(&g, 128, 128) || lit(&g, 127, 127) || lit(&g, 128, 127) || lit(&g, 127, 128));
    }

    #[test]
    fn histogram_and_parade_separate_the_channels() {
        let red = solid(32, 32, Color32::from_rgb(255, 0, 0));
        let h = render(ScopeKind::Histogram, &red);
        assert!(
            h.pixels[(SIZE - 10) * SIZE + 255].r() > 100,
            "red spike at the right"
        );
        assert!(h.pixels[(SIZE - 10) * SIZE + 255].g() < 60);
        assert!(
            h.pixels[(SIZE - 10) * SIZE].g() > 100,
            "green spike at the left"
        );
        let p = render(ScopeKind::Parade, &red);
        // red trace in the first third near the top, green in the middle third at the bottom
        let mut top_red = false;
        for x in 0..SIZE / 3 {
            top_red |= p.pixels[x].r() > 100;
        }
        assert!(top_red);
        let mut mid_green_bottom = false;
        for x in SIZE / 3..2 * SIZE / 3 {
            mid_green_bottom |= p.pixels[(SIZE - 1) * SIZE + x].g() > 100;
        }
        assert!(mid_green_bottom);
    }

    #[test]
    fn an_empty_picture_does_not_panic() {
        for k in ScopeKind::ALL {
            let img = render(k, &ColorImage::new([0, 0], vec![]));
            assert_eq!(img.size, [SIZE, SIZE]);
        }
    }
}
