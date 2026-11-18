//! Play a real project the way the editor viewer does (or export it), and time every frame.
//!
//!     cargo run --release -p captaine-app --example project_bench -- <project.json> [--full] [--from S] [--to S]
//!
//! Default: preview settings (960 px wide, decode at preview size, hardware decode, proxies when present),
//! frames in order like playback. `--full`: export settings (canvas size, software decode).
//! Prints the slowest frames with what was on screen, so stalls at cuts, transitions and effects stand out.
use std::time::Instant;

use captaine_project::Session;
use captaine_render::{preview_project, RenderOptions, Renderer};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .ok_or("usage: project_bench <project.json> [--full]")?;
    let full = args.iter().any(|a| a == "--full");
    // pace frames like the viewer does (one per frame period) instead of as fast as possible
    let realtime = args.iter().any(|a| a == "--realtime");
    let arg = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<f64>().ok())
    };
    // jump to N pseudo-random frames, like dragging the playhead around
    let scrub = arg("--scrub").map(|n| n as usize);
    let s = Session::load(std::path::Path::new(path))?;
    let p = s.editor.project();
    let (tl_id, tl) = p.timelines.iter().next().ok_or("no timeline")?;
    let rate = tl.rate;
    let fps = rate.num() as f64 / rate.den() as f64;
    let total = tl.duration().value;
    let from = arg("--from").map_or(0, |s| (s * fps).round() as i64);
    let to = arg("--to")
        .map_or(total, |s| (s * fps).round() as i64)
        .min(total);

    let (project, opts) = if full {
        (p.clone(), RenderOptions::default())
    } else {
        // --width W renders the preview W pixels wide (default 960); --originals ignores proxies
        let width = arg("--width").unwrap_or(960.0);
        let scale = (width / p.canvas(*tl_id).0 as f64).min(1.0);
        (
            preview_project(p, scale),
            RenderOptions {
                use_proxy: !args.iter().any(|a| a == "--originals"),
                decode_scale: scale,
                hw_decode: true,
                ..RenderOptions::default()
            },
        )
    };
    let mut r = Renderer::new(&project, *tl_id, opts).map_err(|e| e.to_string())?;
    let (w, h) = r.size();
    println!(
        "{} {}x{} frames {from}..{to} ({:.2} s of timeline)",
        if full { "EXPORT" } else { "PREVIEW" },
        w,
        h,
        (to - from) as f64 / fps
    );
    let mut times: Vec<(f64, i64)> = Vec::new();
    let t_all = Instant::now();
    let mut dropped = 0usize;
    let order: Vec<i64> = match scrub {
        Some(n) => {
            let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
            (0..n)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    from + (x % (to - from).max(1) as u64) as i64
                })
                .collect()
        }
        None => (from..to).collect(),
    };
    for tick in order {
        if realtime {
            let due = t_all.elapsed().as_secs_f64();
            let slot = (tick - from) as f64 / fps;
            if slot > due {
                std::thread::sleep(std::time::Duration::from_secs_f64(slot - due));
            } else if due - slot > 1.0 / fps {
                // the viewer would skip a frame here to stay in time
                dropped += 1;
            }
        }
        let t = Instant::now();
        let c = r.render(tick).map_err(|e| e.to_string())?;
        if full {
            // what export does with each picture
            std::hint::black_box(c.to_rgb8());
        } else {
            // what the worker does before handing the picture to the UI
            std::hint::black_box(c.to_rgba8());
        }
        r.recycle(c);
        times.push((t.elapsed().as_secs_f64() * 1000.0, tick));
    }
    let wall = t_all.elapsed().as_secs_f64();
    let mut sorted: Vec<f64> = times.iter().map(|t| t.0).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
    let budget = 1000.0 / fps;
    let late = sorted.iter().filter(|&&t| t > budget).count();
    println!(
        "frames {}  wall {:.2} s  avg {:.1} ms  p50 {:.1}  p95 {:.1}  max {:.1}  -> {:.1} fps; {} of {} frames over the {:.1} ms budget",
        sorted.len(),
        wall,
        wall * 1000.0 / sorted.len() as f64,
        pct(0.5),
        pct(0.95),
        pct(1.0),
        sorted.len() as f64 / wall,
        late,
        sorted.len(),
        budget
    );
    let mut worst = times.clone();
    worst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    if realtime {
        println!("real-time playback: {dropped} frames would be skipped to keep up");
    }
    let pr = r.profile();
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0 / pr.frames.max(1) as f64;
    println!(
        "per frame: canvas {:.1} ms, decode {:.1}, clip effects {:.1}, draw {:.1}, adjustment layers {:.1}, titles {:.1}",
        ms(pr.canvas), ms(pr.decode), ms(pr.clip_effects), ms(pr.draw), ms(pr.adjustment), ms(pr.titles)
    );
    println!("slowest frames:");
    for (ms, tick) in worst.iter().take(12) {
        println!(
            "  {:6.1} ms  at {:6.3} s (frame {tick})",
            ms,
            *tick as f64 / fps
        );
    }
    Ok(())
}
