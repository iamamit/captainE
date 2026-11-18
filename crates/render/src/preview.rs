use captaine_model::{Item, Project, Timeline};
use std::sync::Arc;

/// A copy of `project` whose canvas is `scale` times the size, with every clip position scaled to match,
/// so a preview can be rendered cheaply and look the same as the full-size render.
pub fn preview_project(project: &Project, scale: f64) -> Project {
    let even = |v: u32| ((v as f64 * scale).round() as u32).max(2) & !1;
    let mut p = project.clone();
    p.settings.width = even(project.settings.width);
    p.settings.height = even(project.settings.height);
    let settings_width = p.settings.width;
    for tl in p.timelines.values_mut() {
        let tl: &mut Timeline = Arc::make_mut(tl);
        // use the exact ratio actually achieved after rounding to even sizes
        let (ow, _) = tl
            .canvas
            .unwrap_or((project.settings.width, project.settings.height));
        if let Some((w, h)) = tl.canvas {
            tl.canvas = Some((even(w), even(h)));
        }
        let nw = tl.canvas.map_or(settings_width, |c| c.0);
        let sx = nw as f64 / ow as f64;
        for track in tl.tracks.iter_mut() {
            let track = Arc::make_mut(track);
            for item in track.items.iter_mut() {
                // positions are canvas pixels: static ones and keyframed ones scale with the canvas
                let (video, anim) = match item {
                    Item::Clip(c) => (&mut c.video, &mut c.anim),
                    Item::Title(t) => (&mut t.video, &mut t.anim),
                    Item::Gap(_) => continue,
                };
                video.transform.position.0 *= sx;
                video.transform.position.1 *= sx;
                for prop in ["position.x", "position.y"] {
                    let keys: Vec<_> = anim
                        .keys(prop)
                        .iter()
                        .map(|k| captaine_model::Keyframe { v: k.v * sx, ..*k })
                        .collect();
                    if !keys.is_empty() {
                        anim.set_track(prop, keys);
                    }
                }
            }
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use captaine_model::*;
    use captaine_time::{Rational, RationalTime};

    #[test]
    fn scales_canvas_and_positions() {
        let mut p = Project::new(ProjectSettings {
            frame_rate: Rational::FPS_30,
            width: 1920,
            height: 1080,
            sample_rate: 48000,
        });
        let m = Id::from_u128(1);
        let t = |v| RationalTime::new(v, Rational::FPS_30);
        let mut c = Clip::new(Id::from_u128(5), "c", m, t(0), t(10));
        c.video.transform.position = (200.0, -100.0);
        let mut tr = Track::new(Id::from_u128(3), "V1", TrackKind::Video);
        tr.items.push(Item::Clip(c));
        let mut tl = Timeline::new(Id::from_u128(2), "t", Rational::FPS_30);
        tl.tracks.push(Arc::new(tr));
        p.timelines.insert(tl.id, Arc::new(tl));
        let small = preview_project(&p, 0.5);
        assert_eq!((small.settings.width, small.settings.height), (960, 540));
        let Item::Clip(c) = &small.timeline(Id::from_u128(2)).unwrap().tracks[0].items[0] else {
            panic!()
        };
        assert_eq!(c.video.transform.position, (100.0, -50.0));
        // the original is untouched
        let Item::Clip(o) = &p.timeline(Id::from_u128(2)).unwrap().tracks[0].items[0] else {
            panic!()
        };
        assert_eq!(o.video.transform.position, (200.0, -100.0));
    }
}
