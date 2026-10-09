//! Look and feel, taken from the design mockup: near-black panels, one violet accent,
//! blue video clips, green audio clips, red playhead, mono numerals.

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke,
    TextStyle,
};

pub const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const BG: Color32 = rgb(0x18, 0x18, 0x1c);
pub const PANEL: Color32 = rgb(0x23, 0x23, 0x28);
pub const PANEL_DARK: Color32 = rgb(0x1c, 0x1c, 0x20);
pub const VIEWER_BG: Color32 = rgb(0x15, 0x15, 0x18);
pub const FIELD: Color32 = rgb(0x2b, 0x2b, 0x31);
pub const BORDER: Color32 = rgb(0x34, 0x34, 0x3b);
pub const TEXT: Color32 = rgb(0xe2, 0xe2, 0xe6);
pub const MUTED: Color32 = rgb(0x8d, 0x8d, 0x98);
pub const FAINT: Color32 = rgb(0x5e, 0x5e, 0x68);
pub const ACCENT: Color32 = rgb(0x5b, 0x4f, 0xd6);
pub const ACCENT_HOVER: Color32 = rgb(0x6b, 0x60, 0xe6);
pub const ACCENT_SOFT: Color32 = rgb(0x3a, 0x33, 0x86);
pub const VIDEO_CLIP: Color32 = rgb(0x3b, 0x66, 0xa4);
pub const AUDIO_CLIP: Color32 = rgb(0x3b, 0x72, 0x53);
pub const AUDIO_WAVE: Color32 = rgb(0x9a, 0xd1, 0xae);
pub const OVERLAY_CLIP: Color32 = rgb(0x6a, 0x4d, 0xb0);
pub const PLAYHEAD: Color32 = rgb(0xe5, 0x53, 0x4b);
pub const OK: Color32 = rgb(0x5f, 0xc0, 0x8a);

pub fn ui_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Use the system's own UI and mono fonts when present (macOS), keep egui's built-ins as fallback.
fn load_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    for (name, path, family) in [
        (
            "system-ui",
            "/System/Library/Fonts/SFNS.ttf",
            FontFamily::Proportional,
        ),
        (
            "system-mono",
            "/System/Library/Fonts/SFNSMono.ttf",
            FontFamily::Monospace,
        ),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                name.to_string(),
                std::sync::Arc::new(FontData::from_owned(bytes)),
            );
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, name.to_string());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &Context) {
    load_fonts(ctx);
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = PANEL_DARK;
    v.override_text_color = Some(TEXT);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = 8.0.into();
    v.menu_corner_radius = 6.0.into();
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT_HOVER;
    let r = CornerRadius::same(4);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.bg_fill = FIELD;
    v.widgets.inactive.weak_bg_fill = FIELD;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_fill = rgb(0x36, 0x36, 0x3e);
    v.widgets.hovered.weak_bg_fill = rgb(0x36, 0x36, 0x3e);
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = ACCENT;
    ctx.set_visuals(v);

    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Small, ui_font(10.5)),
        (TextStyle::Body, ui_font(12.5)),
        (TextStyle::Button, ui_font(12.5)),
        (TextStyle::Heading, ui_font(15.0)),
        (TextStyle::Monospace, mono(11.5)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(6.0, 5.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.menu_margin = 6.0.into();
    ctx.set_global_style(style);
}
