//! The editor's look: calm, dark, few colours, clear type.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Margin, Shadow, Stroke, Vec2,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0x15, 0x17, 0x1c);
pub const PANEL: Color32 = Color32::from_rgb(0x1b, 0x1e, 0x24);
pub const RAISED: Color32 = Color32::from_rgb(0x24, 0x28, 0x30);
pub const HOVER: Color32 = Color32::from_rgb(0x2d, 0x32, 0x3c);
pub const LINE: Color32 = Color32::from_rgb(0x2a, 0x2e, 0x37);
pub const TEXT: Color32 = Color32::from_rgb(0xd8, 0xdb, 0xe2);
pub const WEAK: Color32 = Color32::from_rgb(0x8a, 0x90, 0x9c);
pub const ACCENT: Color32 = Color32::from_rgb(0x5b, 0x8c, 0xff);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x2c, 0x3f, 0x6b);
/// Colours for things placed from another scene file.
pub const PLACED: Color32 = Color32::from_rgb(0xb5, 0x9c, 0xff);
pub const WARN: Color32 = Color32::from_rgb(0xff, 0xb8, 0x5c);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x6b, 0x6b);
/// Gizmo colours in the viewport.
pub const ROTATE: Color32 = Color32::from_rgb(0x5c, 0xd6, 0x8a);
pub const SCALE: Color32 = Color32::from_rgb(0xff, 0xb8, 0x5c);

pub fn apply(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(brandon_render::DEFAULT_FONT)),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.override_text_color = Some(TEXT);
        v.weak_text_color = Some(WEAK);
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = BG;
        v.faint_bg_color = RAISED;
        v.code_bg_color = BG;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_shadow = Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(110),
        };
        v.popup_shadow = Shadow {
            offset: [0, 6],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(90),
        };
        v.selection.bg_fill = ACCENT_DIM;
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        v.warn_fg_color = WARN;
        v.error_fg_color = DANGER;
        v.indent_has_left_vline = false;
        v.collapsing_header_frame = false;
        v.striped = false;

        let radius = CornerRadius::same(6);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = PANEL;
        w.noninteractive.weak_bg_fill = PANEL;
        w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = radius;
        for (state, fill) in [
            (&mut w.inactive, RAISED),
            (&mut w.hovered, HOVER),
            (&mut w.active, HOVER),
            (&mut w.open, HOVER),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::NONE;
            state.corner_radius = radius;
            state.fg_stroke = Stroke::new(1.0, TEXT);
            state.expansion = 0.0;
        }
        w.hovered.bg_stroke = Stroke::new(1.0, LINE);
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);

        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 6.0);
        s.button_padding = Vec2::new(10.0, 4.0);
        s.interact_size = Vec2::new(40.0, 24.0);
        s.window_margin = Margin::same(14);
        s.menu_margin = Margin::same(6);
        s.indent = 14.0;
        s.combo_width = 120.0;

        use egui::{FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(13.5)),
            (TextStyle::Button, FontId::proportional(13.5)),
            (TextStyle::Heading, FontId::proportional(17.0)),
            (TextStyle::Monospace, FontId::monospace(12.5)),
        ]
        .into();
    });
}
