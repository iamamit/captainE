//! The Effects page: the effects and transitions library on the left (search, categories, favourites, and a live
//! preview of each effect on the picture in the viewer), the viewer and timeline in the middle, and on the right
//! the selected clip's or title's effects, with a keyframe diamond on every setting.
//!
//! Apply an effect by double-clicking it (it goes on the selected clip or title) or by dragging it onto a clip
//! in the timeline; a transition goes on the cut before the selected clip, or on the cut it is dropped on.

use std::collections::HashMap;

use captaine_model::{Category, Effect, EffectKind, Item, TransitionKind};
use captaine_render::{apply_effects_at, Frame8, FxTime};
use egui::{pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke, Ui};

use crate::app::App;
use crate::theme::{self, ui_font};
use crate::ui::{chip, tab};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibTab {
    #[default]
    Effects,
    Transitions,
    Favourites,
}

#[derive(Default)]
pub struct State {
    pub tab: LibTab,
    pub search: String,
    /// Categories folded shut
    pub closed: Vec<Category>,
    /// Small pictures of effects on the frame in the viewer, by effect key
    previews: HashMap<&'static str, egui::TextureHandle>,
    /// Which viewer frame the previews were made from
    preview_of: u64,
}

/// An effect dragged from the library (dropped on a clip or title in the timeline).
#[derive(Clone, Copy, Debug)]
pub struct EffectDrag(pub EffectKind);

/// A transition dragged from the library (dropped on a cut).
#[derive(Clone, Copy, Debug)]
pub struct TransitionDrag(pub TransitionKind);

pub fn show(app: &mut App, root: &mut Ui) {
    crate::timeline::panel(app, root);
    library_panel(app, root);
    controls_panel(app, root);
    crate::viewer::central(app, root);
}

fn library_panel(app: &mut App, root: &mut Ui) {
    let avail = root.available_rect_before_wrap();
    egui::Panel::left("fx_library")
        .exact_size(app.prefs.pool_w)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(8.0);
                for (t, l) in [
                    (LibTab::Effects, "Effects"),
                    (LibTab::Transitions, "Transitions"),
                    (LibTab::Favourites, "Favourites"),
                ] {
                    if tab(ui, l, app.effects.tab == t).clicked() {
                        app.effects.tab = t;
                    }
                }
            });
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.cursor().top(),
                Stroke::new(1.0, theme::BORDER),
            );
            let t = app.effects.tab;
            library(app, ui, t);
        });
    let w = app.prefs.pool_w;
    crate::ui::drag_edge(
        app,
        root,
        Rect::from_min_size(avail.min, vec2(w, avail.height())),
        crate::keymap::PanelKind::Pool,
    );
}

/// One entry of the library.
#[derive(Clone, Copy)]
enum Entry {
    Effect(EffectKind),
    Transition(TransitionKind),
}

impl Entry {
    fn key(self) -> String {
        match self {
            Entry::Effect(k) => k.key().to_string(),
            Entry::Transition(t) => format!("t:{}", t.key()),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Entry::Effect(k) => k.label(),
            Entry::Transition(t) => t.label(),
        }
    }
    fn about(self) -> &'static str {
        match self {
            Entry::Effect(k) => k.about(),
            Entry::Transition(t) => t.about(),
        }
    }
    fn matches(self, query: &str) -> bool {
        let hay = match self {
            Entry::Effect(k) => format!(
                "{} {} {} {}",
                k.label(),
                k.key(),
                k.about(),
                k.category().label()
            ),
            Entry::Transition(t) => format!("{} {} {} transition", t.label(), t.key(), t.about()),
        }
        .to_lowercase();
        query.split_whitespace().all(|w| hay.contains(w))
    }
}

/// Effects you pick from a list. The subject matte needs the AI cut-out (Inspector > Remove background) and an
/// audio plugin needs choosing from the installed ones, so both are added from the inspector instead.
fn listed(k: EffectKind) -> bool {
    !matches!(k, EffectKind::Matte | EffectKind::Plugin)
}

/// The library list (also the Edit page pool's Effects and Transitions tabs).
pub fn library(app: &mut App, ui: &mut Ui, which: LibTab) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.add_sized(
                [ui.available_width(), 26.0],
                egui::TextEdit::singleline(&mut app.effects.search)
                    .hint_text(match which {
                        LibTab::Transitions => "Search transitions",
                        _ => "Search effects",
                    })
                    .margin(egui::Margin::symmetric(8, 4)),
            );
            ui.add_space(4.0);
            let target = app.selected_clip.and_then(|id| app.item_with_start(id));
            let hint = match &target {
                Some((item, _, _)) => format!(
                    "Double-click to add to {}, or drag onto a clip",
                    item_name(item)
                ),
                None => "Select a clip, then double-click; or drag onto a clip".to_string(),
            };
            ui.label(RichText::new(hint).size(10.5).color(theme::FAINT));
            ui.add_space(4.0);
            let q = app.effects.search.to_lowercase();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match which {
                    LibTab::Effects => {
                        for cat in Category::ALL {
                            let items: Vec<Entry> = EffectKind::ALL
                                .into_iter()
                                .filter(|k| k.category() == cat && listed(*k))
                                .map(Entry::Effect)
                                .filter(|e| q.is_empty() || e.matches(&q))
                                .collect();
                            if items.is_empty() {
                                continue;
                            }
                            let open = !app.effects.closed.contains(&cat) || !q.is_empty();
                            if category_header(ui, cat.label(), items.len(), open) {
                                if open {
                                    app.effects.closed.push(cat);
                                } else {
                                    app.effects.closed.retain(|c| *c != cat);
                                }
                            }
                            if open {
                                for e in items {
                                    row(app, ui, e);
                                }
                            }
                        }
                    }
                    LibTab::Transitions => {
                        for t in TransitionKind::ALL
                            .into_iter()
                            .map(Entry::Transition)
                            .filter(|e| q.is_empty() || e.matches(&q))
                        {
                            row(app, ui, t);
                        }
                    }
                    LibTab::Favourites => {
                        let favs: Vec<Entry> = app
                            .prefs
                            .favourites
                            .iter()
                            .filter_map(|k| match k.strip_prefix("t:") {
                                Some(t) => TransitionKind::from_key(t).map(Entry::Transition),
                                None => EffectKind::from_key(k).map(Entry::Effect),
                            })
                            .filter(|e| q.is_empty() || e.matches(&q))
                            .collect();
                        if favs.is_empty() {
                            ui.add_space(20.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new("No favourites yet").color(theme::MUTED));
                                ui.label(
                                    RichText::new("Click the star next to an effect or transition")
                                        .size(11.0)
                                        .color(theme::FAINT),
                                );
                            });
                        }
                        for e in favs {
                            row(app, ui, e);
                        }
                    }
                });
        });
}

fn item_name(item: &Item) -> String {
    match item {
        Item::Clip(c) => c.name.clone(),
        Item::Title(t) => t.name.clone(),
        Item::Gap(_) => "a gap".into(),
    }
}

/// A category title that folds its list; true when clicked.
fn category_header(ui: &mut Ui, label: &str, n: usize, open: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Category {label}"))
    });
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 3.0, theme::FIELD);
    }
    // a small triangle: pointing down when open, right when folded
    let c = pos2(rect.left() + 8.0, rect.center().y);
    let pts = if open {
        vec![
            pos2(c.x - 4.0, c.y - 2.5),
            pos2(c.x + 4.0, c.y - 2.5),
            pos2(c.x, c.y + 3.0),
        ]
    } else {
        vec![
            pos2(c.x - 2.5, c.y - 4.0),
            pos2(c.x + 3.0, c.y),
            pos2(c.x - 2.5, c.y + 4.0),
        ]
    };
    p.add(egui::Shape::convex_polygon(pts, theme::MUTED, Stroke::NONE));
    p.text(
        pos2(rect.left() + 18.0, rect.center().y),
        Align2::LEFT_CENTER,
        format!("{label}  ({n})"),
        ui_font(12.0),
        theme::MUTED,
    );
    resp.clicked()
}

fn row(app: &mut App, ui: &mut Ui, e: Entry) {
    let key = e.key();
    let fav = app.prefs.favourites.contains(&key);
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, e.label()));
    let star = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(20.0, 20.0));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 4.0, theme::FIELD);
    }
    p.circle_filled(
        pos2(rect.left() + 14.0, rect.center().y),
        3.0,
        match e {
            Entry::Effect(k) if k.is_audio() => theme::AUDIO_WAVE,
            Entry::Effect(_) => theme::ACCENT,
            Entry::Transition(_) => theme::OVERLAY_CLIP,
        },
    );
    p.text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::LEFT_CENTER,
        e.label(),
        ui_font(12.5),
        theme::TEXT,
    );
    let over_star = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|q| star.contains(q));
    if fav || resp.hovered() {
        let col = if fav {
            Color32::from_rgb(0xf2, 0xc9, 0x4c)
        } else if over_star {
            theme::TEXT
        } else {
            theme::FAINT
        };
        draw_star(p, star.center(), 6.5, col, fav);
    }
    if resp.clicked() && over_star {
        toggle_favourite(app, key);
        return;
    }
    if resp.drag_started() {
        match e {
            Entry::Effect(k) => egui::DragAndDrop::set_payload(ui.ctx(), EffectDrag(k)),
            Entry::Transition(t) => egui::DragAndDrop::set_payload(ui.ctx(), TransitionDrag(t)),
        }
    }
    if resp.double_clicked() {
        apply(app, e);
    }
    let resp = resp.on_hover_ui(|ui| {
        ui.set_max_width(230.0);
        ui.label(RichText::new(e.label()).strong());
        ui.label(RichText::new(e.about()).size(11.5).color(theme::MUTED));
        if let Entry::Effect(k) = e {
            if let Some(tex) = preview(app, ui.ctx(), k) {
                ui.add_space(4.0);
                let w = 220.0;
                let h = w * tex.size()[1] as f32 / tex.size()[0].max(1) as f32;
                ui.image((tex.id(), vec2(w, h)));
            }
        }
    });
    resp.context_menu(|ui| {
        if ui.button("Apply to selected").clicked() {
            apply(app, e);
            ui.close();
        }
        if ui
            .button(if fav {
                "Remove from favourites"
            } else {
                "Add to favourites"
            })
            .clicked()
        {
            toggle_favourite(app, e.key());
            ui.close();
        }
    });
}

/// A five-pointed star, filled or outlined.
fn draw_star(p: &egui::Painter, c: egui::Pos2, r: f32, col: Color32, filled: bool) {
    let pts: Vec<egui::Pos2> = (0..10)
        .map(|i| {
            let a = std::f32::consts::PI * i as f32 / 5.0 - std::f32::consts::FRAC_PI_2;
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            pos2(c.x + rr * a.cos(), c.y + rr * a.sin())
        })
        .collect();
    if filled {
        // a star is not convex: fill it as five triangles around the middle
        for i in 0..10 {
            let (a, b) = (pts[i], pts[(i + 1) % 10]);
            p.add(egui::Shape::convex_polygon(
                vec![c, a, b],
                col,
                Stroke::NONE,
            ));
        }
    } else {
        let mut ring = pts.clone();
        ring.push(pts[0]);
        p.add(egui::Shape::line(ring, Stroke::new(1.2, col)));
    }
}

fn toggle_favourite(app: &mut App, key: String) {
    if let Some(i) = app.prefs.favourites.iter().position(|k| *k == key) {
        app.prefs.favourites.remove(i);
    } else {
        app.prefs.favourites.push(key);
    }
    let _ = app.prefs.save();
}

/// Add the effect to the selected item, or put the transition on the cut before the selected clip.
fn apply(app: &mut App, e: Entry) {
    let Some(id) = app.selected_clip else {
        app.toast(
            "Select a clip or title in the timeline first (or drag it onto one)",
            true,
        );
        return;
    };
    let out = match e {
        Entry::Effect(k) => app.add_effect(id, k),
        Entry::Transition(t) => app.set_transition_kind(id, t),
    };
    match out {
        Ok(m) => app.toast(m, false),
        Err(m) => app.toast(m, true),
    }
}

/// Settings that make an effect visible in its preview: effects that start neutral get a typical use.
pub fn showcase(kind: EffectKind) -> Effect {
    let mut e = Effect::new(kind);
    let set = |e: &mut Effect, i: usize, v: f64| e.params[i] = v;
    match kind {
        EffectKind::BrightnessContrast => {
            set(&mut e, 0, 25.0);
            set(&mut e, 1, 30.0);
        }
        EffectKind::Levels => set(&mut e, 0, 40.0),
        EffectKind::ChannelMixer => {
            set(&mut e, 1, 40.0);
            set(&mut e, 8, 70.0);
        }
        EffectKind::CornerPin => {
            set(&mut e, 0, 12.0);
            set(&mut e, 1, 8.0);
            set(&mut e, 7, 92.0);
        }
        EffectKind::Offset => set(&mut e, 0, 30.0),
        EffectKind::Crop => {
            for i in 0..4 {
                set(&mut e, i, 10.0);
            }
            set(&mut e, 4, 20.0);
        }
        EffectKind::ColorWheels => {
            set(&mut e, 3, 0.1);
            set(&mut e, 9, 0.08);
        }
        EffectKind::Hsl => set(&mut e, 7, 30.0),
        EffectKind::Curves => {
            set(&mut e, 1, 0.18);
            set(&mut e, 3, 0.82);
        }
        _ => {}
    }
    e
}

/// The effect on a small copy of the picture in the viewer (made when first hovered, kept until the picture
/// changes).
fn preview(app: &mut App, ctx: &egui::Context, kind: EffectKind) -> Option<egui::TextureHandle> {
    if kind.is_audio() {
        return None;
    }
    if app.effects.preview_of != app.scope_serial {
        app.effects.previews.clear();
        app.effects.preview_of = app.scope_serial;
    }
    if let Some(t) = app.effects.previews.get(kind.key()) {
        return Some(t.clone());
    }
    let src = app.scope_src.as_ref()?;
    let small = shrink(src, 220);
    let out =
        apply_effects_at(&small, &[showcase(kind)], &[], FxTime::new(12, 0.4)).unwrap_or(small);
    let img = egui::ColorImage::from_rgba_unmultiplied(
        [out.width as usize, out.height as usize],
        &out.rgba,
    );
    let tex = ctx.load_texture(
        format!("fx-preview-{}", kind.key()),
        img,
        egui::TextureOptions::LINEAR,
    );
    app.effects.previews.insert(kind.key(), tex.clone());
    Some(tex)
}

/// A box-averaged copy `w` pixels wide.
fn shrink(src: &egui::ColorImage, w: usize) -> Frame8 {
    let (sw, sh) = (src.size[0].max(1), src.size[1].max(1));
    let w = w.min(sw);
    let h = (sh * w / sw).max(1);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let (x0, x1) = (x * sw / w, ((x + 1) * sw / w).max(x * sw / w + 1));
            let (y0, y1) = (y * sh / h, ((y + 1) * sh / h).max(y * sh / h + 1));
            let mut acc = [0u32; 4];
            for yy in y0..y1.min(sh) {
                for xx in x0..x1.min(sw) {
                    let c = src.pixels[yy * sw + xx].to_srgba_unmultiplied();
                    for k in 0..4 {
                        acc[k] += c[k] as u32;
                    }
                }
            }
            let n = ((x1.min(sw) - x0) * (y1.min(sh) - y0)).max(1) as u32;
            rgba.extend(acc.map(|v| (v / n) as u8));
        }
    }
    Frame8 {
        width: w as u32,
        height: h as u32,
        rgba,
    }
}

fn controls_panel(app: &mut App, root: &mut Ui) {
    let avail = root.available_rect_before_wrap();
    egui::Panel::right("fx_controls")
        .exact_size(app.prefs.inspector_w)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER)),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(8.0);
                tab(ui, "Effect Controls", true);
            });
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.cursor().top(),
                Stroke::new(1.0, theme::BORDER),
            );
            egui::Frame::new()
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| controls(app, ui));
                });
        });
    let w = app.prefs.inspector_w;
    crate::ui::drag_edge(
        app,
        root,
        Rect::from_min_size(
            pos2(avail.right() - w, avail.top()),
            vec2(w, avail.height()),
        ),
        crate::keymap::PanelKind::Inspector,
    );
}

fn controls(app: &mut App, ui: &mut Ui) {
    let Some((item, track, _)) = app.selected_clip.and_then(|id| app.item_with_start(id)) else {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Nothing selected").color(theme::MUTED));
            ui.label(
                RichText::new(
                    "Click a clip or title in the timeline to see and animate its effects.",
                )
                .size(11.0)
                .color(theme::FAINT),
            );
        });
        return;
    };
    ui.label(RichText::new(item_name(&item)).strong().size(14.0));
    let n = match &item {
        Item::Clip(c) => c.effects.len(),
        Item::Title(t) => t.effects.len(),
        Item::Gap(_) => 0,
    };
    ui.label(
        RichText::new(format!(
            "{n} effect{} · the diamond next to a setting keys it at the playhead",
            if n == 1 { "" } else { "s" }
        ))
        .size(11.0)
        .color(theme::FAINT),
    );
    ui.add_space(6.0);
    match &item {
        Item::Clip(c) => {
            let media = app.project().media.get(&c.media).cloned();
            let (v, a) = media.map_or((true, false), |m| (m.has_video, m.has_audio));
            if v {
                crate::ui::transition_controls(app, ui, c, track);
            }
            crate::ui::effects_stack(app, ui, c.id, &c.effects, v, a);
        }
        Item::Title(t) => crate::ui::effects_stack(app, ui, t.id, &t.effects, true, false),
        Item::Gap(_) => {}
    }
    ui.add_space(8.0);
    if chip(ui, "Remove all effects", false, n > 0).clicked() {
        app.end_merge();
        app.do_op(captaine_ops::Op::SetEffects {
            item: item.id(),
            effects: Vec::new(),
        });
        let anim = match &item {
            Item::Clip(c) => c.anim.clone(),
            Item::Title(t) => t.anim.clone(),
            Item::Gap(_) => Default::default(),
        };
        for prop in anim.0.keys().filter(|k| k.starts_with("fx.")) {
            app.do_op(captaine_ops::Op::SetAnimTrack {
                item: item.id(),
                prop: prop.clone(),
                keys: Vec::new(),
            });
        }
    }
}
