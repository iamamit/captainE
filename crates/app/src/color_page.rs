//! The Color page: grade the cut shot by shot.
//!
//! The picture stays in the middle. Scopes sit on the left, read live from the viewer's frame. Underneath are the
//! shots of the timeline as a strip, and the grading tools for the selected shot:
//! - primaries: exposure, contrast, white balance, saturation...
//! - colour wheels and curves (the same effects as in the Effects stack);
//! - HSL and a LUT;
//! - auto balance, match to the previous shot, copy/paste a grade, reset, and a before/after view.
//!
//! A shot's *grade* is its colour settings plus its colour effects (wheels, curves, HSL, LUT). Every change is an
//! ordinary undoable edit.

use captaine_model::{Clip, Effect, EffectKind, Id, Item};
use captaine_ops::Op;
use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke, TextureHandle, Ui};

use crate::app::App;
use crate::scopes::ScopeKind;
use crate::theme;
use crate::ui::{chip, slider_row};

/// What a shot looks like (colour settings and colour effects): shared with CoCaptain.
pub use captaine_model::{is_color_effect, Grade};

pub fn grade_of(c: &Clip) -> Grade {
    Grade::of(c)
}

/// The ops that give `clip` the grade `g`, keeping its other effects.
pub fn grade_ops(clip: &Clip, g: &Grade) -> Vec<Op> {
    captaine_ops::edit::set_grade(clip, g)
}

pub struct State {
    pub copied: Option<Grade>,
    /// Showing the selected shot without its grade
    pub before_after: bool,
    pub top: ScopeKind,
    pub bottom: ScopeKind,
    tex: [Option<TextureHandle>; 2],
    /// A new frame arrived for the scopes
    pub dirty: bool,
    /// The project version the before/after view was made from: any edit ends it
    before_rev: Option<(u64, u64)>,
}

impl Default for State {
    fn default() -> State {
        State {
            copied: None,
            before_after: false,
            top: ScopeKind::Parade,
            bottom: ScopeKind::Vectorscope,
            tex: [None, None],
            dirty: true,
            before_rev: None,
        }
    }
}

/// Every video clip of the timeline in playing order: (clip, track, start tick).
fn shots(app: &App) -> Vec<(Clip, Id, i64)> {
    let Some(tl) = app.timeline() else {
        return Vec::new();
    };
    let mut v: Vec<(Clip, Id, i64)> = tl
        .tracks
        .iter()
        .filter(|t| t.kind == captaine_model::TrackKind::Video)
        .flat_map(|t| {
            t.items
                .iter()
                .zip(t.item_starts())
                .filter_map(|(it, st)| match it {
                    Item::Clip(c) => Some((c.clone(), t.id, st)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .filter(|(c, ..)| {
            app.project()
                .media
                .get(&c.media)
                .is_some_and(|m| m.has_video)
        })
        .collect();
    v.sort_by_key(|(_, _, st)| *st);
    v
}

pub fn show(app: &mut App, root: &mut Ui) {
    let rev = (app.session.epoch(), app.session.editor.revision());
    if app.color.before_after && app.color.before_rev != Some(rev) {
        // something was edited: show the real, graded picture again
        app.color.before_after = false;
        app.set_preview(None);
    }
    // a shot is always selected here: the one under the playhead unless the user picked another
    if app.selected().is_none() {
        let at = app.playhead;
        if let Some((c, ..)) = shots(app)
            .into_iter()
            .find(|(c, _, st)| at >= *st && at < st + c.duration.value)
        {
            app.selected_clip = Some(c.id);
        }
    }
    grading_panel(app, root);
    scopes_panel(app, root);
    crate::viewer::central(app, root);
}

// ---- scopes ------------------------------------------------------------------------------------------------

fn scopes_panel(app: &mut App, root: &mut Ui) {
    egui::Panel::left("color_scopes")
        .exact_size(330.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .inner_margin(10),
        )
        .show(root, |ui| {
            if app.color.dirty && app.scope_src.is_some() {
                let src = app.scope_src.clone().expect("checked");
                for (k, kind) in [app.color.top, app.color.bottom].into_iter().enumerate() {
                    let img = crate::scopes::render(kind, &src);
                    match &mut app.color.tex[k] {
                        Some(t) => t.set(img, egui::TextureOptions::NEAREST),
                        None => {
                            app.color.tex[k] = Some(ui.ctx().load_texture(
                                format!("color-scope-{k}"),
                                img,
                                egui::TextureOptions::NEAREST,
                            ))
                        }
                    }
                }
                app.color.dirty = false;
            }
            for k in 0..2 {
                let current = if k == 0 {
                    app.color.top
                } else {
                    app.color.bottom
                };
                ui.horizontal(|ui| {
                    for kind in ScopeKind::ALL {
                        if chip(ui, kind.label(), current == kind, true).clicked()
                            && kind != current
                        {
                            if k == 0 {
                                app.color.top = kind;
                            } else {
                                app.color.bottom = kind;
                            }
                            app.color.dirty = true;
                        }
                    }
                });
                let side = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(vec2(side, side * 0.78), Sense::hover());
                ui.painter()
                    .rect_filled(rect, 4.0, Color32::from_rgb(10, 10, 12));
                match &app.color.tex[k] {
                    Some(t) => {
                        ui.painter().image(
                            t.id(),
                            rect.shrink(4.0),
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    None => {
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            "Waiting for a frame",
                            crate::theme::ui_font(11.0),
                            theme::MUTED,
                        );
                    }
                }
                ui.add_space(10.0);
            }
        });
}

// ---- shots and grading tools -------------------------------------------------------------------------------

fn grading_panel(app: &mut App, root: &mut Ui) {
    egui::Panel::bottom("color_grading")
        .exact_size(390.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL_DARK)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .inner_margin(10),
        )
        .show(root, |ui| {
            shot_strip(app, ui);
            ui.add_space(8.0);
            let Some((clip, _track, _start)) = app.selected() else {
                ui.label(
                    RichText::new("No shot here: put the playhead over a video clip")
                        .color(theme::MUTED),
                );
                return;
            };
            // three equal columns, each laid out top to bottom
            ui.columns(3, |cols| {
                egui::ScrollArea::vertical()
                    .id_salt("color_primaries")
                    .show(&mut cols[0], |ui| primaries(app, ui, &clip));
                egui::ScrollArea::vertical()
                    .id_salt("color_wheels")
                    .show(&mut cols[1], |ui| wheels_and_curves(app, ui, &clip));
                egui::ScrollArea::vertical()
                    .id_salt("color_tools")
                    .show(&mut cols[2], |ui| tools(app, ui, &clip));
            });
        });
}

/// The shots as numbered thumbnails; a click selects one and moves the playhead into it.
fn shot_strip(app: &mut App, ui: &mut Ui) {
    let shots = shots(app);
    let selected = app.selected_clip;
    egui::ScrollArea::horizontal()
        .id_salt("color_shots")
        .max_height(78.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (n, (c, _, start)) in shots.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(112.0, 72.0), Sense::click());
                    let pic = Rect::from_min_size(rect.min, vec2(112.0, 56.0));
                    let p = ui.painter();
                    p.rect_filled(pic, 3.0, Color32::from_rgb(24, 24, 28));
                    if let Some(t) = app.thumbs.get(&c.media) {
                        p.image(
                            t.id(),
                            pic,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    let is_sel = selected == Some(c.id);
                    p.rect_stroke(
                        pic,
                        3.0,
                        Stroke::new(
                            if is_sel { 2.0 } else { 1.0 },
                            if is_sel { theme::ACCENT } else { theme::BORDER },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    // graded shots carry a dot
                    if !Grade::of(c).is_neutral() {
                        p.circle_filled(pic.right_top() + vec2(-8.0, 8.0), 4.0, theme::ACCENT);
                    }
                    p.text(
                        pos2(rect.left() + 2.0, rect.bottom() - 7.0),
                        Align2::LEFT_CENTER,
                        format!("{:02}  {}", n + 1, app.tc(*start)),
                        crate::theme::mono(10.0),
                        if is_sel { Color32::WHITE } else { theme::MUTED },
                    );
                    if resp.clicked() {
                        app.selected_clip = Some(c.id);
                        // show the middle of the shot
                        app.set_playhead(start + c.duration.value / 2);
                        app.color.before_after = false;
                        app.set_preview(None);
                    }
                }
            });
        });
}

/// A primaries slider: label, value, range, default, decimals, suffix, and the property name it animates as.
type SliderSpec<'a> = (
    &'a str,
    &'a mut f64,
    std::ops::RangeInclusive<f64>,
    f64,
    usize,
    &'a str,
    &'a str,
);

fn primaries(app: &mut App, ui: &mut Ui, clip: &Clip) {
    ui.label(RichText::new("Primaries").strong().size(12.5));
    let mut c = clip.video.color;
    let mut changed = false;
    let mut done = false;
    // (label, value, range, default, decimals, suffix, animated property name)
    let animated = |p: &str| clip.anim.has(p);
    let rows: [SliderSpec; 8] = [
        (
            "Exposure",
            &mut c.exposure,
            -4.0..=4.0,
            0.0,
            2,
            " st",
            "exposure",
        ),
        (
            "Contrast",
            &mut c.contrast,
            0.0..=3.0,
            1.0,
            2,
            "",
            "contrast",
        ),
        (
            "Saturation",
            &mut c.saturation,
            0.0..=200.0,
            100.0,
            0,
            "",
            "saturation",
        ),
        (
            "Temperature",
            &mut c.temperature,
            2000.0..=11000.0,
            6500.0,
            0,
            " K",
            "temperature",
        ),
        ("Tint", &mut c.tint, -100.0..=100.0, 0.0, 0, "", "tint"),
        (
            "Highlights",
            &mut c.highlights,
            -100.0..=100.0,
            0.0,
            0,
            "",
            "highlights",
        ),
        (
            "Shadows",
            &mut c.shadows,
            -100.0..=100.0,
            0.0,
            0,
            "",
            "shadows",
        ),
        (
            "Vibrance",
            &mut c.vibrance,
            -100.0..=100.0,
            0.0,
            0,
            "",
            "vibrance",
        ),
    ];
    let mut any_animated = false;
    for (label, v, range, def, dec, suf, prop) in rows {
        let live = !animated(prop);
        any_animated |= !live;
        let r = slider_row(ui, label, v, range, def, dec, suf, live);
        changed |= r.changed;
        done |= r.done;
    }
    if any_animated {
        ui.label(
            RichText::new(
                "Greyed-out settings are animated: change them with keyframes on the Edit page",
            )
            .size(10.5)
            .color(theme::MUTED),
        );
    }
    if changed {
        let mut video = clip.video;
        video.color = c;
        app.do_merging(
            Op::SetVideo {
                item: clip.id,
                video,
            },
            &format!("color:{}", clip.id),
        );
    }
    if done {
        app.end_merge();
    }
}

/// The clip's colour effect of `kind`, or a fresh one (not yet on the clip).
fn effect_of(clip: &Clip, kind: EffectKind) -> (Effect, bool) {
    match clip.effects.iter().find(|e| e.kind == kind) {
        Some(e) => (e.clone(), true),
        None => (Effect::new(kind), false),
    }
}

/// Put `e` on the clip: replace its effect of that kind, or add it in front of the shot's other effects.
fn set_effect(app: &mut App, clip: &Clip, e: Effect, merge: bool) {
    let mut effects = clip.effects.clone();
    match effects.iter().position(|x| x.kind == e.kind) {
        Some(i) => effects[i] = e,
        None => {
            let at = effects
                .iter()
                .take_while(|x| is_color_effect(x.kind))
                .count();
            effects.insert(at, e);
        }
    }
    let op = Op::SetEffects {
        item: clip.id,
        effects,
    };
    if merge {
        app.do_merging(op, &format!("fx:{}", clip.id));
    } else {
        app.do_op(op);
    }
}

fn wheels_and_curves(app: &mut App, ui: &mut Ui, clip: &Clip) {
    ui.label(RichText::new("Wheels").strong().size(12.5));
    let (mut wheels, _) = effect_of(clip, EffectKind::ColorWheels);
    let (c, d) = crate::effect_widgets::wheels_row(ui, &mut wheels);
    if c {
        set_effect(app, clip, wheels, true);
    }
    if d {
        app.end_merge();
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Curves").strong().size(12.5));
        for (k, name) in ["Master", "Red", "Green", "Blue"].into_iter().enumerate() {
            if chip(ui, name, app.curve_tab == k, true).clicked() {
                app.curve_tab = k;
            }
        }
    });
    let (mut curves, _) = effect_of(clip, EffectKind::Curves);
    let (c, d) = crate::effect_widgets::curve_editor(ui, &mut curves, app.curve_tab);
    if c {
        set_effect(app, clip, curves, true);
    }
    if d {
        app.end_merge();
    }
}

fn tools(app: &mut App, ui: &mut Ui, clip: &Clip) {
    ui.label(RichText::new("Shot").strong().size(12.5));
    let shots = shots(app);
    let idx = shots.iter().position(|(c, ..)| c.id == clip.id);
    ui.horizontal_wrapped(|ui| {
        if chip(ui, "Auto balance", false, true)
            .on_hover_text("Fix exposure, colour cast and flat colour from the footage")
            .clicked()
        {
            app.auto_color(clip.id);
        }
        let prev = idx
            .and_then(|i| i.checked_sub(1))
            .map(|i| shots[i].0.clone());
        if chip(ui, "Match previous shot", false, prev.is_some())
            .on_hover_text("Make this shot's brightness and colour match the shot before it")
            .clicked()
        {
            if let Some(p) = prev {
                match_to(app, clip, &p);
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        if chip(ui, "Copy grade", false, true).clicked() {
            app.color.copied = Some(grade_of(clip));
            app.toast("Grade copied", false);
        }
        let has = app.color.copied.is_some();
        if chip(ui, "Paste", false, has)
            .on_hover_text("Give this shot the copied grade")
            .clicked()
        {
            if let Some(g) = app.color.copied.clone() {
                let ops = grade_ops(clip, &g);
                if !ops.is_empty() {
                    app.do_op(Op::Transaction {
                        description: "Paste grade".into(),
                        ops,
                    });
                }
            }
        }
        if chip(ui, "Paste to all shots", false, has).clicked() {
            if let Some(g) = app.color.copied.clone() {
                let ops: Vec<Op> = shots.iter().flat_map(|(c, ..)| grade_ops(c, &g)).collect();
                if ops.is_empty() {
                    return;
                }
                app.do_op(Op::Transaction {
                    description: format!("Paste grade to {} shots", shots.len()),
                    ops,
                });
            }
        }
        if chip(ui, "Reset", false, true)
            .on_hover_text("Remove this shot's grade")
            .clicked()
        {
            let ops = grade_ops(clip, &Grade::neutral());
            if !ops.is_empty() {
                app.do_op(Op::Transaction {
                    description: "Reset grade".into(),
                    ops,
                });
            }
        }
    });
    // before / after: the viewer shows the shot without its grade while this is on
    let ba = app.color.before_after;
    if chip(
        ui,
        if ba {
            "Showing: before"
        } else {
            "Before / after"
        },
        ba,
        true,
    )
    .on_hover_text("See this shot without its grade")
    .clicked()
    {
        app.color.before_after = !ba;
        if app.color.before_after {
            let ungraded = grade_ops(clip, &Grade::neutral());
            let mut p = app.project().clone();
            for op in &ungraded {
                if let Ok((n, _)) = captaine_ops::apply(&p, op) {
                    p = n;
                }
            }
            app.set_preview(Some(p));
            app.color.before_rev = Some((app.session.epoch(), app.session.editor.revision()));
        } else {
            app.set_preview(None);
        }
    }

    ui.add_space(8.0);
    ui.label(RichText::new("HSL").strong().size(12.5));
    let (mut hsl, _) = effect_of(clip, EffectKind::Hsl);
    // one row per colour band: hue shift, saturation and lightness
    let mut changed = false;
    let mut done = false;
    egui::ScrollArea::vertical()
        .id_salt("color_hsl")
        .max_height(120.0)
        .show(ui, |ui| {
            for (b, band) in ["Red", "Yellow", "Green", "Cyan", "Blue", "Magenta"]
                .iter()
                .enumerate()
            {
                for (k, what) in ["hue", "sat", "lum"].iter().enumerate() {
                    let i = b * 3 + k;
                    if i >= hsl.params.len() {
                        continue;
                    }
                    let d = hsl.kind.params()[i].default;
                    let (lo, hi) = (hsl.kind.params()[i].min, hsl.kind.params()[i].max);
                    let r = slider_row(
                        ui,
                        &format!("{band} {what}"),
                        &mut hsl.params[i],
                        lo..=hi,
                        d,
                        0,
                        "",
                        true,
                    );
                    changed |= r.changed;
                    done |= r.done;
                }
            }
        });
    if changed {
        set_effect(app, clip, hsl, true);
    }
    if done {
        app.end_merge();
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("LUT").strong().size(12.5));
        let (lut, has_lut) = effect_of(clip, EffectKind::Lut);
        let name = std::path::Path::new(&lut.text)
            .file_name()
            .map_or("none".to_string(), |n| n.to_string_lossy().into());
        ui.label(
            RichText::new(if has_lut { name } else { "none".into() })
                .size(11.0)
                .color(theme::MUTED),
        );
        if chip(ui, "Choose .cube…", false, true).clicked() {
            if let Some(f) = rfd::FileDialog::new()
                .add_filter("LUT", &["cube"])
                .pick_file()
            {
                let mut e = lut;
                e.text = f.to_string_lossy().into();
                set_effect(app, clip, e, false);
            }
        }
        if has_lut && chip(ui, "Remove", false, true).clicked() {
            let effects: Vec<Effect> = clip
                .effects
                .iter()
                .filter(|e| e.kind != EffectKind::Lut)
                .cloned()
                .collect();
            app.do_op(Op::SetEffects {
                item: clip.id,
                effects,
            });
        }
    });
}

/// Make `clip` look like `reference` (brightness, balance and colour measured from both).
fn match_to(app: &mut App, clip: &Clip, reference: &Clip) {
    let media = |id| app.project().media.get(&id).cloned();
    let (Some(m), Some(rm)) = (media(clip.media), media(reference.media)) else {
        return;
    };
    match captaine_agent::match_color(clip, &m, reference, &rm) {
        Ok(color) => {
            let mut video = clip.video;
            video.color = color;
            app.do_op(Op::SetVideo {
                item: clip.id,
                video,
            });
        }
        Err(e) => app.toast(e, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use captaine_time::{Rational, RationalTime};

    fn clip() -> Clip {
        let r = Rational::FPS_30;
        Clip::new(
            Id::new(),
            "a",
            Id::new(),
            RationalTime::new(0, r),
            RationalTime::new(30, r),
        )
    }

    #[test]
    fn a_grade_is_colour_and_colour_effects_only() {
        let mut c = clip();
        c.video.color.exposure = 0.5;
        c.effects = vec![
            Effect::new(EffectKind::Blur),
            Effect::new(EffectKind::Curves),
        ];
        let g = grade_of(&c);
        assert_eq!(g.color.exposure, 0.5);
        assert_eq!(g.effects.len(), 1);
        assert_eq!(g.effects[0].kind, EffectKind::Curves);
    }

    #[test]
    fn pasting_a_grade_keeps_the_shots_other_effects() {
        let mut from = clip();
        from.video.color.saturation = 60.0;
        from.effects = vec![Effect::new(EffectKind::Lut)];
        let mut to = clip();
        to.effects = vec![Effect::new(EffectKind::Blur), Effect::new(EffectKind::Hsl)];
        let ops = grade_ops(&to, &grade_of(&from));
        let Op::SetVideo { video, .. } = &ops[0] else {
            panic!("video first")
        };
        assert_eq!(video.color.saturation, 60.0);
        let Op::SetEffects { effects, .. } = &ops[1] else {
            panic!("then effects")
        };
        let kinds: Vec<EffectKind> = effects.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [EffectKind::Lut, EffectKind::Blur],
            "the old HSL goes, blur stays"
        );
    }

    #[test]
    fn resetting_an_ungraded_shot_changes_nothing() {
        let c = clip();
        let ops = grade_ops(&c, &Grade::neutral());
        assert!(
            ops.is_empty(),
            "nothing to undo on a shot that was never graded"
        );
    }
}
