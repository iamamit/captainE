//! How fast does a 4K timeline render? `cargo run --release -p captaine-render --example playback -- clip.mp4`
#![allow(clippy::field_reassign_with_default)]
use std::sync::Arc;
use std::time::Instant;

use captaine_media::media_ref;
use captaine_model::*;
use captaine_ops::*;
use captaine_render::*;
use captaine_time::{Rational, RationalTime};

fn main() {
    let path = std::env::args().nth(1).expect("a video file");
    let rate = Rational::FPS_30;
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: rate,
        width: 3840,
        height: 2160,
        sample_rate: 48_000,
    }));
    let m = media_ref(std::path::Path::new(&path), Id::new()).unwrap();
    ed.apply(Author::User, Op::AddMedia { media: m.clone() })
        .unwrap();
    let (tl, v1, v2) = (Id::new(), Id::new(), Id::new());
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "t", rate),
        },
    )
    .unwrap();
    for (i, id) in [v1, v2].into_iter().enumerate() {
        ed.apply(
            Author::User,
            Op::AddTrack {
                timeline: tl,
                index: i,
                track: Track::new(id, format!("V{}", i + 1), TrackKind::Video),
            },
        )
        .unwrap();
    }
    let n = 120;
    for (track, name) in [(v1, "a"), (v2, "b")] {
        let c = Clip::new(
            Id::new(),
            name,
            m.id,
            RationalTime::new(0, m.duration.unwrap().rate),
            RationalTime::new(n, rate),
        );
        ed.apply(Author::User, edit::append(ed.project(), track, c))
            .unwrap();
    }
    // the top layer is half size, half opaque, with a grade
    let top = match &ed.project().timeline(tl).unwrap().tracks[1].items[0] {
        Item::Clip(c) => c.id,
        _ => unreachable!(),
    };
    let mut v = VideoProps::default();
    v.opacity = 0.7;
    v.transform.scale = (0.5, 0.5);
    v.color.saturation = 1.3;
    ed.apply(
        Author::User,
        Op::SetVideo {
            item: top,
            video: v,
        },
    )
    .unwrap();
    // what the editor's preview does: a quarter-size project, pictures decoded at that size
    {
        let small = preview_project(ed.project(), 0.25);
        let opts = RenderOptions {
            use_proxy: true,
            decode_scale: 0.25,
            hw_decode: true,
            ..RenderOptions::default()
        };
        let mut r = Renderer::new(&small, tl, opts).unwrap();
        r.render(0).unwrap();
        let t = Instant::now();
        for f in 1..=60 {
            let c = r.render(f).unwrap();
            let _ = c.to_rgba8();
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / 60.0;
        println!(
            "{:<16} {ms:6.1} ms/frame  ({:.0} fps)",
            "editor preview",
            1000.0 / ms
        );
    }
    for (name, opts) in [
        (
            "full size",
            RenderOptions {
                hw_decode: true,
                ..RenderOptions::default()
            },
        ),
        (
            "preview at 1/2",
            RenderOptions {
                hw_decode: true,
                decode_scale: 0.5,
                ..RenderOptions::default()
            },
        ),
        (
            "preview at 1/4",
            RenderOptions {
                hw_decode: true,
                decode_scale: 0.25,
                ..RenderOptions::default()
            },
        ),
    ] {
        let mut r = Renderer::new(ed.project(), tl, opts).unwrap();
        r.render(0).unwrap();
        let t = Instant::now();
        for f in 1..=60 {
            let c = r.render(f).unwrap();
            let _ = c.to_rgba8();
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / 60.0;
        println!("{name:<16} {ms:6.1} ms/frame  ({:.0} fps)", 1000.0 / ms);
    }
    let _ = Arc::new(0);
}
