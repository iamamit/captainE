//! Loudness commands of the mixer tab.

use captaine_ops::Op;

use crate::app::App;

impl App {
    /// What the noise in the marked range of the selected clip sounds like, as an effect's profile text.
    pub fn noise_profile_from_marks(&self) -> Result<String, String> {
        let (clip, _, start) = self
            .selected()
            .ok_or("select the clip with the noise first")?;
        let (a, b) = match (self.mark_in, self.mark_out) {
            (Some(a), Some(b)) if b > a => (a, b),
            _ => return Err("mark a stretch of just noise with I and O first".into()),
        };
        let media = self
            .project()
            .media
            .get(&clip.media)
            .ok_or("the clip's media is missing")?;
        if !media.has_audio {
            return Err("that clip has no audio".into());
        }
        let fps = clip.duration.rate.as_f64();
        let from = ((a - start) as f64 / fps).max(0.0);
        let to = ((b - start) as f64 / fps).min(clip.duration.value as f64 / fps);
        if to <= from {
            return Err("the marked range is outside the selected clip".into());
        }
        let sr = self.project().settings.sample_rate;
        let mono = captaine_audio::decode_mono_window(
            &media.path,
            clip.source_start.to_seconds_f64() + from,
            ((to - from) * sr as f64) as usize,
            sr,
        )
        .map_err(|e| e.to_string())?;
        captaine_audio::learn_noise_profile(&mono)
    }

    pub fn measure_loudness(&mut self) {
        let Some(tl) = self.timeline_id() else { return };
        match captaine_agent::sequence_loudness(self.project(), tl) {
            Ok(l) => self.loudness = Some(l),
            Err(e) => self.toast(e, true),
        }
    }

    pub fn normalize_mix(&mut self) {
        let Some(tl) = self.timeline_id() else { return };
        match captaine_agent::normalize_sequence(self.project(), tl, self.loudness_target) {
            Ok((ops, g)) => {
                if self.do_op(Op::Transaction {
                    description: format!("Normalize to {} LUFS", self.loudness_target),
                    ops,
                }) {
                    self.toast(format!("Changed the level by {g:+.1} dB"), false);
                    self.measure_loudness();
                }
            }
            Err(e) => self.toast(e, true),
        }
    }

    pub fn normalize_selected_clip(&mut self) {
        let Some((clip, ..)) = self.selected() else {
            return;
        };
        match captaine_agent::normalize_clip(self.project(), &clip, self.loudness_target) {
            Ok(ops) => {
                self.do_op(Op::Transaction {
                    description: format!("Normalize clip to {} LUFS", self.loudness_target),
                    ops,
                });
            }
            Err(e) => self.toast(e, true),
        }
    }
}
