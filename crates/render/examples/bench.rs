//! Times the compositor and effects on a 4K canvas: `cargo run --release -p captaine-render --example bench`
use captaine_model::*;
use captaine_render::*;
use std::time::Instant;

fn main() {
    let (w, h) = (3840u32, 2160u32);
    let mut frame = Frame8::solid(w, h, [90, 120, 160, 255]);
    for (i, v) in frame.rgba.iter_mut().enumerate() {
        if i % 4 != 3 {
            *v = (i / 7 % 251) as u8;
        }
    }
    let props = VideoProps::default();
    let time = |name: &str, n: u32, mut f: Box<dyn FnMut()>| {
        f();
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("{name:<28} {ms:7.1} ms  ({:.0} fps)", 1000.0 / ms);
    };
    let (f1, p1) = (frame.clone(), props.clone());
    time(
        "one layer, 4K",
        5,
        Box::new(move || {
            let mut c = Canvas::black(w, h);
            draw_layer(&mut c, &f1, &p1);
        }),
    );
    let (f2, mut p2) = (frame.clone(), props.clone());
    p2.transform.rotation_deg = 4.0;
    p2.opacity = 0.6;
    time(
        "rotated, half opaque",
        5,
        Box::new(move || {
            let mut c = Canvas::black(w, h);
            draw_layer(&mut c, &f2, &p2);
        }),
    );
    let f3 = frame.clone();
    for kind in [
        EffectKind::Blur,
        EffectKind::Sharpen,
        EffectKind::Vignette,
        EffectKind::Glow,
        EffectKind::Grain,
    ] {
        let fx = vec![Effect::new(kind)];
        let f = f3.clone();
        time(
            &format!("effect {kind:?}"),
            3,
            Box::new(move || {
                let _ = apply_effects(&f, &fx);
            }),
        );
    }
    let f4 = frame.clone();
    let mut graded = props.clone();
    graded.color.exposure = 0.3;
    graded.color.saturation = 1.2;
    time(
        "graded layer",
        5,
        Box::new(move || {
            let mut c = Canvas::black(w, h);
            draw_layer(&mut c, &f4, &graded);
        }),
    );
    let c = Canvas::black(w, h);
    time(
        "canvas to rgba8",
        5,
        Box::new(move || {
            let _ = c.to_rgba8();
        }),
    );
}
