//! Time every picture effect on one frame: `cargo run --release -p captaine-render --example fx_bench [height]`.
use captaine_model::{Effect, EffectKind};
use captaine_render::{apply_effects_at, Frame8, FxTime};

fn main() {
    let h: u32 = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1080);
    let w = h * 16 / 9;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255]);
        }
    }
    let frame = Frame8 {
        width: w,
        height: h,
        rgba,
    };
    let mut rows = Vec::new();
    for kind in EffectKind::ALL.into_iter().filter(|k| !k.is_audio()) {
        if matches!(kind, EffectKind::Matte | EffectKind::Lut) {
            continue;
        }
        let fx = [Effect::new(kind)];
        let t = std::time::Instant::now();
        for _ in 0..3 {
            std::hint::black_box(apply_effects_at(&frame, &fx, &[], FxTime::new(5, 0.4)));
        }
        rows.push((t.elapsed().as_secs_f64() / 3.0 * 1000.0, kind.key()));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (ms, k) in rows {
        println!("{ms:8.1} ms  {k}");
    }
}
