//! The editor's look: dark glass over the scene, one accent colour, clear
//! type. The colours follow the conventions of Apple's dark appearance:
//! labels are white at decreasing strengths, and fills are see-through
//! greys, so everything sits well on glass whatever is behind it.

use crate::glass;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Margin, Shadow, Stroke, Vec2,
};
use std::sync::Arc;

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    // Premultiplied, as egui stores colours.
    let alpha = (a * 255.0) as u32;
    Color32::from_rgba_premultiplied(
        (r as u32 * alpha / 255) as u8,
        (g as u32 * alpha / 255) as u8,
        (b as u32 * alpha / 255) as u8,
        alpha as u8,
    )
}

// Labels.
pub const TEXT: Color32 = Color32::from_rgb(0xf5, 0xf5, 0xf7);
pub const SECONDARY: Color32 = rgba(235, 235, 245, 0.62);
pub const TERTIARY: Color32 = rgba(235, 235, 245, 0.32);

// See-through fills for controls, from faint to strong.
pub const CARD: Color32 = rgba(255, 255, 255, 0.055);
pub const FILL_SOFT: Color32 = rgba(118, 118, 128, 0.22);
pub const FILL: Color32 = rgba(118, 118, 128, 0.32);
pub const FILL_STRONG: Color32 = rgba(118, 118, 128, 0.44);
/// The raised part of a segmented control.
pub const THUMB: Color32 = rgba(150, 150, 160, 0.55);
pub const SEPARATOR: Color32 = rgba(255, 255, 255, 0.07);

// Colours with meaning.
pub const ACCENT: Color32 = Color32::from_rgb(0x0a, 0x84, 0xff);
pub const GREEN: Color32 = Color32::from_rgb(0x30, 0xd1, 0x58);
pub const ORANGE: Color32 = Color32::from_rgb(0xff, 0x9f, 0x0a);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x45, 0x3a);
/// Things placed from another scene file.
pub const PLACED: Color32 = Color32::from_rgb(0xbf, 0x5a, 0xf2);
pub const WARN: Color32 = ORANGE;
/// Gizmo colours in the viewport.
pub const ROTATE: Color32 = GREEN;
pub const SCALE: Color32 = ORANGE;

/// The corner radius of floating panels. Things inside use smaller radii
/// so the corners stay concentric.
pub const PANEL_RADIUS: u8 = 26;
/// The space around and between floating panels.
pub const GAP: f32 = 10.0;

pub fn panel_shadow() -> Shadow {
    Shadow {
        offset: [0, 12],
        blur: 40,
        spread: 0,
        color: Color32::from_black_alpha(96),
    }
}

/// The semibold font, for titles and buttons.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn apply(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(brandon_render::DEFAULT_FONT)),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/Inter-SemiBold.otf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".into());
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut semibold_fonts = vec!["inter-semibold".to_string()];
    semibold_fonts.extend(fallbacks);
    fonts.families.insert(semibold(), semibold_fonts);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.override_text_color = Some(TEXT);
        v.weak_text_color = Some(SECONDARY);
        v.panel_fill = Color32::TRANSPARENT;
        // Menus, popups and tooltips are glass.
        v.window_fill = glass::GLASS;
        v.window_stroke = Stroke::NONE;
        v.window_corner_radius = CornerRadius::same(PANEL_RADIUS);
        v.menu_corner_radius = CornerRadius::same(16);
        v.window_shadow = panel_shadow();
        v.popup_shadow = Shadow {
            offset: [0, 10],
            blur: 30,
            spread: 0,
            color: Color32::from_black_alpha(90),
        };
        v.extreme_bg_color = FILL_SOFT;
        v.text_edit_bg_color = Some(FILL_SOFT);
        v.faint_bg_color = CARD;
        v.code_bg_color = FILL_SOFT;
        v.selection.bg_fill = ACCENT.gamma_multiply(0.55);
        v.selection.stroke = Stroke::new(1.5, ACCENT);
        v.hyperlink_color = ACCENT;
        v.warn_fg_color = WARN;
        v.error_fg_color = DANGER;
        v.indent_has_left_vline = false;
        v.collapsing_header_frame = false;
        v.striped = false;
        v.button_frame = true;
        v.handle_shape = egui::style::HandleShape::Circle;
        v.slider_trailing_fill = true;

        let radius = CornerRadius::same(8);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = Color32::TRANSPARENT;
        w.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
        w.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = radius;
        for (state, fill) in [
            (&mut w.inactive, FILL_SOFT),
            (&mut w.hovered, FILL),
            (&mut w.active, FILL_STRONG),
            (&mut w.open, FILL),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::NONE;
            state.corner_radius = radius;
            state.fg_stroke = Stroke::new(1.5, TEXT);
            state.expansion = 0.0;
        }
        // Focused text fields get an accent ring.
        w.active.bg_stroke = Stroke::new(1.5, ACCENT);

        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 6.0);
        s.button_padding = Vec2::new(12.0, 5.0);
        s.interact_size = Vec2::new(40.0, 26.0);
        s.window_margin = Margin::same(14);
        s.menu_margin = Margin::same(6);
        s.indent = 16.0;
        s.combo_width = 120.0;
        s.icon_width = 16.0;
        s.icon_spacing = 6.0;
        s.scroll = egui::style::ScrollStyle::floating();
        s.scroll.bar_width = 6.0;

        style.interaction.selectable_labels = false;

        use egui::{FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Heading, FontId::new(20.0, semibold())),
            (TextStyle::Monospace, FontId::monospace(12.5)),
        ]
        .into();
    });
}
