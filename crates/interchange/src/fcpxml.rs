//! Final Cut Pro XML (version 1.9), export only: one spine from the first video track, further tracks as
//! connected clips on lanes. Enough to open the cut in Final Cut Pro, DaVinci Resolve or Premiere.

use captaine_model::{Id, Item, Project, TrackKind};
use captaine_time::RationalTime;

use crate::file_url;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A rational time as FCPXML writes it: `frames*den/num s`, e.g. `1001/24000s`.
fn t(frames: i64, rate: captaine_time::Rational) -> String {
    // frames / (num/den) seconds = frames*den / num
    let (n, d) = (frames * rate.den(), rate.num());
    format!("{n}/{d}s")
}

pub fn export(project: &Project, timeline: Id) -> Result<String, String> {
    let tl = project.timeline(timeline).ok_or("timeline not found")?;
    let rate = tl.rate;
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.9\">\n  <resources>\n");
    xml += &format!(
        "    <format id=\"r0\" name=\"captainE\" frameDuration=\"{}\" width=\"{}\" height=\"{}\"/>\n",
        t(1, rate),
        project.canvas(timeline).0,
        project.canvas(timeline).1
    );
    let mut ids: Vec<Id> = Vec::new();
    for track in &tl.tracks {
        for item in &track.items {
            if let Item::Clip(c) = item {
                if !ids.contains(&c.media) {
                    ids.push(c.media);
                }
            }
        }
    }
    for (i, id) in ids.iter().enumerate() {
        let Some(m) = project.media.get(id) else {
            continue;
        };
        let dur = m.duration.map_or(0, |d| d.value);
        let mrate = m.duration.map_or(rate, |d| d.rate);
        xml += &format!(
            "    <asset id=\"a{}\" name=\"{}\" start=\"0s\" duration=\"{}\" hasVideo=\"{}\" hasAudio=\"{}\" format=\"r0\">\n      <media-rep kind=\"original-media\" src=\"{}\"/>\n    </asset>\n",
            i + 1,
            esc(&m.name),
            t(dur, mrate),
            m.has_video as u8,
            m.has_audio as u8,
            esc(&file_url(&m.path)),
        );
    }
    xml += "  </resources>\n  <library>\n    <event name=\"captainE\">\n";
    xml += &format!(
        "      <project name=\"{}\">\n        <sequence format=\"r0\" duration=\"{}\" tcStart=\"0s\" tcFormat=\"NDF\">\n          <spine>\n",
        esc(&tl.name),
        t(tl.duration().value, rate)
    );
    let asset = |id: &Id| {
        ids.iter()
            .position(|x| x == id)
            .map(|i| format!("a{}", i + 1))
            .unwrap_or_default()
    };
    let clip_xml = |c: &captaine_model::Clip,
                    offset: i64,
                    lane: Option<usize>,
                    indent: &str|
     -> String {
        let src_rate = c.source_start.rate;
        let start = RationalTime::new(c.source_start.value, src_rate);
        let lane_attr = lane.map_or(String::new(), |l| format!(" lane=\"{l}\""));
        let mut s = format!(
            "{indent}<asset-clip ref=\"{}\" name=\"{}\" offset=\"{}\" start=\"{}\" duration=\"{}\"{lane_attr}{}",
            asset(&c.media),
            esc(&c.name),
            t(offset, rate),
            t(start.value, src_rate),
            t(c.duration.value, rate),
            if c.enabled { "" } else { " enabled=\"0\"" },
        );
        if c.retime.speed != 1.0 && !c.retime.freeze {
            s += &format!(">\n{indent}  <timeMap><timept time=\"0s\" value=\"0s\" interp=\"linear\"/><timept time=\"{}\" value=\"{}\" interp=\"linear\"/></timeMap>\n{indent}</asset-clip>\n",
                t(c.duration.value, rate), t(c.source_span(), src_rate));
        } else {
            s += "/>\n";
        }
        s
    };
    // spine from the first video track (or the first track with clips)
    let base = tl
        .tracks
        .iter()
        .position(|x| x.kind == TrackKind::Video)
        .unwrap_or(0);
    if let Some(track) = tl.tracks.get(base) {
        let starts = track.item_starts();
        for (item, start) in track.items.iter().zip(starts.iter()) {
            let (s, end) = (*start, *start + item.duration().value);
            let mut connected = String::new();
            let mut lane = 0usize;
            for (ti, other) in tl.tracks.iter().enumerate() {
                if ti == base {
                    continue;
                }
                lane += 1;
                let lane_no = if other.kind == TrackKind::Audio {
                    -(lane as i64)
                } else {
                    lane as i64
                };
                for (oi, ostart) in other.items.iter().zip(other.item_starts()) {
                    // attach to the spine element that holds its first frame
                    if ostart >= s && ostart < end {
                        if let Item::Clip(c) = oi {
                            let parent_time = match item {
                                Item::Clip(pc) => pc.source_start.value + (ostart - s),
                                _ => ostart - s,
                            };
                            let off = match item {
                                Item::Clip(pc) => {
                                    RationalTime::new(parent_time, pc.source_start.rate)
                                }
                                _ => RationalTime::new(parent_time, rate),
                            };
                            let mut x = clip_xml(
                                c,
                                0,
                                Some(lane_no.unsigned_abs() as usize),
                                "              ",
                            );
                            let rep = format!("offset=\"{}\"", t(0, rate));
                            x = x.replacen(
                                &rep,
                                &format!("offset=\"{}\"", t(off.value, off.rate)),
                                1,
                            );
                            if lane_no < 0 {
                                x = x.replacen("lane=\"", "lane=\"-", 1);
                            }
                            connected += &x;
                        }
                    }
                }
            }
            match item {
                Item::Clip(c) => {
                    let mut x = clip_xml(c, s, None, "            ");
                    // chapter markers that fall inside this clip, in the clip's own time
                    for m in tl.markers.iter().filter(|m| m.tick >= s && m.tick < end) {
                        let local = RationalTime::new(c.source_start.value, c.source_start.rate);
                        let off = RationalTime::new(m.tick - s, rate)
                            .rescale(local.rate, captaine_time::Rounding::Nearest)
                            .map_or(0, |x| x.value);
                        let at = local.value + off;
                        let tag = format!(
                            "              <chapter-marker start=\"{}\" duration=\"{}\" value=\"{}\"/>\n",
                            t(at, local.rate),
                            t(1, rate),
                            esc(&m.name)
                        );
                        connected += &tag;
                    }
                    if !connected.is_empty() {
                        if x.ends_with("/>\n") {
                            x.truncate(x.len() - 3);
                            x += &format!(">\n{connected}            </asset-clip>\n");
                        } else {
                            x = x.replacen(
                                "</asset-clip>",
                                &format!("{connected}            </asset-clip>"),
                                1,
                            );
                        }
                    }
                    xml += &x;
                }
                Item::Title(ti) => {
                    xml += &format!(
                        "            <gap name=\"{}\" offset=\"{}\" duration=\"{}\"/>\n",
                        esc(&ti.name),
                        t(s, rate),
                        t(ti.duration.value, rate)
                    );
                }
                Item::Gap(g) => {
                    xml += &format!(
                        "            <gap name=\"Gap\" offset=\"{}\" duration=\"{}\"/>\n",
                        t(s, rate),
                        t(g.duration.value, rate)
                    );
                }
            }
        }
    }
    xml += "          </spine>\n        </sequence>\n      </project>\n    </event>\n  </library>\n</fcpxml>\n";
    Ok(xml)
}
