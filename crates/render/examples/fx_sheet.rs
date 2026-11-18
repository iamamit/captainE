//! A contact sheet of every picture effect (showcase settings) on one frame of a video, rows of eight:
//! `cargo run --release -p captaine-render --example fx_sheet -- <video> <out.png> [seconds]`.
use captaine_model::{Effect, EffectKind};
use captaine_render::{apply_effects_at, save_rgb_png, Frame8, FxTime, VideoReader};
use captaine_time::Rational;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let (src, out) = (a.get(1).ok_or("video")?, a.get(2).ok_or("out.png")?);
    let secs: f64 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(2.0);
    let fps = Rational::FPS_30;
    let at = (secs * 30.0) as i64;
    let frame = VideoReader::open(src, 288, 162, fps, at)?.frame(at)?;
    let mut tiles: Vec<Frame8> = Vec::new();
    for kind in EffectKind::ALL.into_iter().filter(|k| !k.is_audio()) {
        if matches!(kind, EffectKind::Matte | EffectKind::Lut) {
            continue;
        }
        let mut e = Effect::new(kind);
        // neutral-by-default effects get a typical setting
        match kind {
            EffectKind::BrightnessContrast => e.params = vec![25.0, 30.0],
            EffectKind::Levels => e.params[0] = 40.0,
            EffectKind::ChannelMixer => e.params[1] = 50.0,
            EffectKind::CornerPin => e.params[0] = 15.0,
            EffectKind::Offset => e.params[0] = 30.0,
            _ => {}
        }
        let f = apply_effects_at(&frame, &[e], &[], FxTime::new(9, 0.6)).unwrap();
        // effects that leave transparency show on grey
        let mut g = f.clone();
        for p in g.rgba.chunks_exact_mut(4) {
            let k = p[3] as u32;
            for v in p.iter_mut().take(3) {
                *v = ((*v as u32 * k + 60 * (255 - k)) / 255) as u8;
            }
            p[3] = 255;
        }
        tiles.push(g);
    }
    let (tw, th) = (frame.width, frame.height);
    let cols = 8u32;
    let rows = (tiles.len() as u32).div_ceil(cols);
    let (w, h) = (tw * cols, th * rows);
    let mut rgb = vec![30u8; (w * h * 3) as usize];
    for (i, t) in tiles.iter().enumerate() {
        let (ox, oy) = ((i as u32 % cols) * tw, (i as u32 / cols) * th);
        for y in 0..th {
            for x in 0..tw {
                let s = ((y * tw + x) * 4) as usize;
                let d = (((oy + y) * w + ox + x) * 3) as usize;
                rgb[d..d + 3].copy_from_slice(&t.rgba[s..s + 3]);
            }
        }
    }
    save_rgb_png(std::path::Path::new(out), w, h, &rgb)?;
    let names: Vec<&str> = EffectKind::ALL
        .into_iter()
        .filter(|k| !k.is_audio() && !matches!(k, EffectKind::Matte | EffectKind::Lut))
        .map(|k| k.key())
        .collect();
    for (r, chunk) in names.chunks(cols as usize).enumerate() {
        println!("row {}: {}", r + 1, chunk.join(", "));
    }
    Ok(())
}
