//! Times each step of an adjustment layer's grade at preview and export size:
//! `cargo run --release -p captaine-render --example adjust_bench`
use captaine_model::*;
use captaine_render::*;
use std::time::Instant;

fn main() {
    for (w, h) in [(960u32, 540u32), (3840, 2160)] {
        let mut frame = Frame8::solid(w, h, [90, 120, 160, 255]);
        for (i, v) in frame.rgba.iter_mut().enumerate() {
            if i % 4 != 3 {
                *v = (i / 7 % 251) as u8;
            }
        }
        let canvas = Canvas::black(w, h);
        let time = |name: &str, f: &mut dyn FnMut()| {
            f();
            let n = if w > 1000 { 5 } else { 30 };
            let t = Instant::now();
            for _ in 0..n {
                f();
            }
            println!(
                "{w}x{h} {name:<22} {:7.2} ms",
                t.elapsed().as_secs_f64() * 1000.0 / n as f64
            );
        };
        let fx = |k: EffectKind, ps: &[(usize, f64)]| {
            let mut e = Effect::new(k);
            for &(i, v) in ps {
                e.params[i] = v;
            }
            e
        };
        let all = vec![
            fx(EffectKind::ColorWheels, &[(1, -0.04), (3, 0.045)]),
            fx(EffectKind::Sharpen, &[(0, 35.0)]),
            fx(EffectKind::Glow, &[(0, 62.0), (1, 18.0), (2, 45.0)]),
            fx(EffectKind::Vignette, &[(0, 25.0), (1, 50.0)]),
            fx(EffectKind::Letterbox, &[(0, 2.39)]),
        ];
        time("canvas -> rgba8", &mut || {
            std::hint::black_box(canvas.to_rgba8());
        });
        for e in &all {
            let one = vec![e.clone()];
            time(e.kind.label(), &mut || {
                std::hint::black_box(apply_effects(&frame, &one));
            });
        }
        time("all five", &mut || {
            std::hint::black_box(apply_effects(&frame, &all));
        });
        let mut c = canvas.clone();
        time("blend back", &mut || c.blend_adjusted(&frame, 1.0));
    }
}
