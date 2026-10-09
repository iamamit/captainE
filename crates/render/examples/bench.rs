//! Preview speed: `cargo run --release -p captaine-render --example bench -- clip.mp4 [scale] [hw]`
//! Renders 120 consecutive frames of one clip on a 1920x1080 canvas and prints frames per second.

use std::sync::Arc;
use std::time::Instant;

use captaine_model::*;
use captaine_render::*;
use captaine_time::{Rational, RationalTime};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: bench clip.mp4 [scale] [hw]");
    let scale: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let hw = args.get(3).is_some_and(|s| s == "hw");
    let m = captaine_media::media_ref(std::path::Path::new(path), Id::new()).expect("probe");
    let mut p = Project::new(ProjectSettings {
        frame_rate: Rational::FPS_30,
        width: 1920,
        height: 1080,
        sample_rate: 48_000,
    });
    let media_id = m.id;
    let rate = m.duration.unwrap().rate;
    p.media.insert(media_id, m);
    let mut tl = Timeline::new(Id::new(), "t", Rational::FPS_30);
    let mut v1 = Track::new(Id::new(), "V1", TrackKind::Video);
    v1.items.push(Item::Clip(Clip::new(
        Id::new(),
        "c",
        media_id,
        RationalTime::new(0, rate),
        RationalTime::new(150, Rational::FPS_30),
    )));
    tl.tracks.push(Arc::new(v1));
    let tl_id = tl.id;
    p.timelines.insert(tl_id, Arc::new(tl));
    let shown = preview_project(&p, scale);
    let opts = RenderOptions {
        decode_scale: scale,
        hw_decode: hw,
        ..RenderOptions::default()
    };
    let mut r = Renderer::new(&shown, tl_id, opts).unwrap();
    r.render(0).unwrap(); // warm up (process start, first frame)
    let t = Instant::now();
    for tick in 1..=120 {
        r.render(tick).unwrap();
    }
    let secs = t.elapsed().as_secs_f64();
    println!(
        "{:.1} fps  (canvas {}x{}, decode scale {scale}, hw {hw})",
        120.0 / secs,
        shown.settings.width,
        shown.settings.height
    );
}
