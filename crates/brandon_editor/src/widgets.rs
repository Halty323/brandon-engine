//! The editor's own controls and icons: switches, segmented controls,
//! capsule buttons, grouped cards and small line icons drawn with egui's
//! painter, so they stay sharp at any size and need no icon font.

use crate::theme;
use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{
    Color32, CornerRadius, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
    Widget,
};

// --- Icons ---------------------------------------------------------------------

/// A small line icon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    Plus,
    Search,
    Undo,
    Redo,
    Play,
    Stop,
    Eye,
    EyeClosed,
    /// Points right when closed and down when open; `0.0` to `1.0`.
    Chevron(f32),
    ChevronUpDown,
    Close,
    /// Corner brackets, for framing things in the viewport.
    Frame,
    /// A tick, for the chosen item of a list.
    Check,
    /// An arrow down into a tray.
    Save,
    /// The kinds of node in the scene tree.
    Scene,
    Placed,
    Image,
    Text,
    Camera,
    Shape,
    Overlay,
    Node,
}

/// Draws `icon` to fit inside `rect`.
pub fn paint_icon(painter: &egui::Painter, icon: Icon, rect: Rect, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let at = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    let w = (s * 0.09).clamp(1.2, 2.0);
    let stroke = Stroke::new(w, color);
    let line = |points: Vec<Pos2>| {
        painter.add(PathShape::line(points, PathStroke::new(w, color)));
    };
    match icon {
        Icon::Plus => {
            line(vec![at(-0.32, 0.0), at(0.32, 0.0)]);
            line(vec![at(0.0, -0.32), at(0.0, 0.32)]);
        }
        Icon::Close => {
            line(vec![at(-0.24, -0.24), at(0.24, 0.24)]);
            line(vec![at(0.24, -0.24), at(-0.24, 0.24)]);
        }
        Icon::Check => {
            line(vec![at(-0.28, 0.02), at(-0.08, 0.22), at(0.3, -0.2)]);
        }
        Icon::Frame => {
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                line(vec![
                    at(0.34 * x, 0.12 * y),
                    at(0.34 * x, 0.34 * y),
                    at(0.12 * x, 0.34 * y),
                ]);
            }
        }
        Icon::Save => {
            line(vec![at(0.0, -0.34), at(0.0, 0.1)]);
            line(vec![at(-0.15, -0.04), at(0.0, 0.11), at(0.15, -0.04)]);
            line(vec![
                at(-0.32, 0.06),
                at(-0.32, 0.32),
                at(0.32, 0.32),
                at(0.32, 0.06),
            ]);
        }
        Icon::Search => {
            painter.circle_stroke(at(-0.07, -0.07), s * 0.25, stroke);
            line(vec![at(0.12, 0.12), at(0.34, 0.34)]);
        }
        Icon::Undo | Icon::Redo => {
            let flip = if icon == Icon::Redo { -1.0 } else { 1.0 };
            let p = |x: f32, y: f32| at(x * flip, y);
            // An arrow that heads back, then turns down and round.
            let mut points = vec![p(-0.3, -0.14)];
            for step in 0..=12 {
                let a = (-90.0 + 180.0 * step as f32 / 12.0f32).to_radians();
                points.push(p(0.08 + a.cos() * 0.24, 0.1 + a.sin() * 0.24));
            }
            points.push(p(-0.12, 0.34));
            line(points);
            line(vec![p(-0.14, -0.3), p(-0.3, -0.14), p(-0.14, 0.02)]);
        }
        Icon::Play => {
            let points = vec![at(-0.2, -0.3), at(0.32, 0.0), at(-0.2, 0.3)];
            painter.add(PathShape::convex_polygon(
                points,
                color,
                Stroke::new(w, color),
            ));
        }
        Icon::Stop => {
            painter.rect_filled(
                Rect::from_center_size(c, Vec2::splat(s * 0.5)),
                s * 0.08,
                color,
            );
        }
        Icon::Eye | Icon::EyeClosed => {
            let left = at(-0.4, 0.0);
            let right = at(0.4, 0.0);
            for dir in [-1.0, 1.0] {
                painter.add(CubicBezierShape::from_points_stroke(
                    [left, at(-0.18, 0.3 * dir), at(0.18, 0.3 * dir), right],
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                ));
            }
            painter.circle_filled(c, s * 0.1, color);
            if icon == Icon::EyeClosed {
                line(vec![at(-0.34, -0.34), at(0.34, 0.34)]);
            }
        }
        Icon::Chevron(open) => {
            let angle = open.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
            let rot = |x: f32, y: f32| {
                let (sin, cos) = angle.sin_cos();
                at(x * cos - y * sin, x * sin + y * cos)
            };
            line(vec![rot(-0.1, -0.22), rot(0.12, 0.0), rot(-0.1, 0.22)]);
        }
        Icon::ChevronUpDown => {
            line(vec![at(-0.16, -0.1), at(0.0, -0.26), at(0.16, -0.1)]);
            line(vec![at(-0.16, 0.1), at(0.0, 0.26), at(0.16, 0.1)]);
        }
        Icon::Scene => {
            let back = Rect::from_min_max(at(-0.22, -0.34), at(0.34, 0.2));
            let front = Rect::from_min_max(at(-0.34, -0.2), at(0.22, 0.34));
            painter.rect_stroke(back, s * 0.08, stroke, StrokeKind::Middle);
            painter.rect_filled(front, s * 0.08, color.gamma_multiply(0.25));
            painter.rect_stroke(front, s * 0.08, stroke, StrokeKind::Middle);
        }
        Icon::Placed => {
            let points = vec![at(0.0, -0.36), at(0.34, 0.0), at(0.0, 0.36), at(-0.34, 0.0)];
            painter.add(PathShape::convex_polygon(
                points,
                color.gamma_multiply(0.35),
                Stroke::new(w, color),
            ));
        }
        Icon::Image => {
            let frame = Rect::from_min_max(at(-0.36, -0.28), at(0.36, 0.28));
            painter.rect_stroke(frame, s * 0.08, stroke, StrokeKind::Middle);
            line(vec![
                at(-0.3, 0.2),
                at(-0.08, -0.02),
                at(0.06, 0.1),
                at(0.14, 0.04),
                at(0.3, 0.2),
            ]);
            painter.circle_filled(at(0.16, -0.12), s * 0.06, color);
        }
        Icon::Text => {
            line(vec![at(-0.28, -0.24), at(0.28, -0.24)]);
            line(vec![at(0.0, -0.24), at(0.0, 0.3)]);
        }
        Icon::Camera => {
            let body = Rect::from_min_max(at(-0.38, -0.22), at(0.14, 0.22));
            painter.rect_stroke(body, s * 0.07, stroke, StrokeKind::Middle);
            painter.add(PathShape::convex_polygon(
                vec![
                    at(0.18, -0.02),
                    at(0.38, -0.16),
                    at(0.38, 0.16),
                    at(0.18, 0.02),
                ],
                color,
                Stroke::new(w * 0.6, color),
            ));
        }
        Icon::Shape => {
            painter.circle_stroke(c, s * 0.3, stroke);
            painter.circle_filled(c, s * 0.08, color);
        }
        Icon::Overlay => {
            let back = Rect::from_min_max(at(-0.36, -0.3), at(0.2, 0.14));
            let front = Rect::from_min_max(at(-0.2, -0.14), at(0.36, 0.3));
            painter.rect_stroke(back, s * 0.06, stroke, StrokeKind::Middle);
            painter.rect_filled(front, s * 0.06, color.gamma_multiply(0.3));
            painter.rect_stroke(front, s * 0.06, stroke, StrokeKind::Middle);
        }
        Icon::Node => {
            painter.circle_stroke(c, s * 0.2, stroke);
        }
    }
}

// --- Buttons -------------------------------------------------------------------

/// How a capsule button looks.
#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    /// The main action, filled with the accent colour.
    Primary,
    /// Like `Primary`, in red, for stopping something.
    Danger,
    /// No fill until hovered.
    Bare,
}

/// A round button with just an icon.
pub fn icon_button(ui: &mut Ui, icon: Icon, size: f32, enabled: bool, tooltip: &str) -> Response {
    // A disabled button only senses hovering, so it never reports clicks.
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), sense);
    if ui.is_rect_visible(rect) {
        let fill = if !enabled {
            Color32::TRANSPARENT
        } else if response.is_pointer_button_down_on() {
            theme::FILL_STRONG
        } else if response.hovered() {
            theme::FILL
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().circle_filled(rect.center(), size / 2.0, fill);
        let color = if enabled {
            theme::TEXT
        } else {
            theme::TERTIARY
        };
        paint_icon(
            ui.painter(),
            icon,
            Rect::from_center_size(rect.center(), Vec2::splat(size * 0.62)),
            color,
        );
    }
    response.on_hover_text(tooltip)
}

/// A capsule button with an optional icon and a label.
pub fn capsule(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    tone: Tone,
    enabled: bool,
) -> Response {
    let font = egui::FontId::new(13.0, theme::semibold());
    let text_color = match tone {
        Tone::Primary | Tone::Danger => Color32::WHITE,
        _ if enabled => theme::TEXT,
        _ => theme::TERTIARY,
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, text_color);
    let icon_room = if icon.is_some() { 20.0 } else { 0.0 };
    let gap = if icon.is_some() && !label.is_empty() {
        6.0
    } else {
        0.0
    };
    let height = 30.0;
    let padding = if label.is_empty() { 7.0 } else { 14.0 };
    let width = (padding * 2.0 + icon_room + gap + galley.size().x).max(height);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), sense);
    if ui.is_rect_visible(rect) {
        let down = response.is_pointer_button_down_on() && enabled;
        let hovered = response.hovered() && enabled;
        let fill = match tone {
            Tone::Primary => theme::ACCENT,
            Tone::Danger => theme::DANGER,
            Tone::Bare => Color32::TRANSPARENT,
        };
        let fill = match (tone, down, hovered) {
            (Tone::Primary | Tone::Danger, true, _) => fill.gamma_multiply(0.8),
            (Tone::Primary | Tone::Danger, _, true) => brighten(fill, 0.12),
            (_, true, _) => theme::FILL_STRONG,
            (_, _, true) => theme::FILL,
            _ => fill,
        };
        let fill = if enabled {
            fill
        } else {
            fill.gamma_multiply(0.4)
        };
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same((height / 2.0) as u8), fill);
        if matches!(tone, Tone::Primary | Tone::Danger) {
            // A soft inner highlight along the top, like the glass around it.
            painter.rect_stroke(
                rect.shrink(0.5),
                CornerRadius::same((height / 2.0) as u8),
                Stroke::new(1.0, Color32::from_white_alpha(40)),
                StrokeKind::Inside,
            );
        }
        let mut x = rect.left() + padding;
        if let Some(icon) = icon {
            let icon_rect = Rect::from_min_size(
                Pos2::new(x, rect.center().y - icon_room / 2.0),
                Vec2::splat(icon_room),
            );
            paint_icon(painter, icon, icon_rect.shrink(3.0), text_color);
            x += icon_room + gap;
        }
        if !label.is_empty() {
            painter.galley(
                Pos2::new(x, rect.center().y - galley.size().y / 2.0),
                galley,
                text_color,
            );
        }
    }
    response
}

fn brighten(color: Color32, amount: f32) -> Color32 {
    let [r, g, b, a] = color.to_array();
    let up = |c: u8| (c as f32 + (255.0 - c as f32) * amount) as u8;
    Color32::from_rgba_premultiplied(up(r), up(g), up(b), a)
}

/// A full-width rounded button with an icon and a label in the accent
/// colour, such as "Add" at the end of a list.
pub fn wide_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if response.is_pointer_button_down_on() {
            theme::FILL_STRONG
        } else if response.hovered() {
            theme::FILL
        } else {
            theme::FILL_SOFT
        };
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(16), fill);
        let galley = painter.layout_no_wrap(
            label.to_string(),
            egui::FontId::new(13.0, theme::semibold()),
            theme::ACCENT,
        );
        let width = 16.0 + 6.0 + galley.size().x;
        let left = rect.center().x - width / 2.0;
        paint_icon(
            painter,
            icon,
            Rect::from_center_size(Pos2::new(left + 8.0, rect.center().y), Vec2::splat(15.0)),
            theme::ACCENT,
        );
        painter.galley(
            Pos2::new(left + 22.0, rect.center().y - galley.size().y / 2.0),
            galley,
            theme::ACCENT,
        );
    }
    response
}

// --- Switch and segmented control ---------------------------------------------

/// An on/off switch.
pub struct Switch<'a> {
    on: &'a mut bool,
}

impl<'a> Switch<'a> {
    pub fn new(on: &'a mut bool) -> Self {
        Switch { on }
    }
}

impl Widget for Switch<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let size = Vec2::new(36.0, 21.0);
        let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
        if response.clicked() {
            *self.on = !*self.on;
            response.mark_changed();
        }
        let t = ui.ctx().animate_bool_with_time(response.id, *self.on, 0.16);
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let off = theme::FILL_STRONG;
            let track = lerp_color(off, theme::ACCENT, t);
            painter.rect_filled(rect, CornerRadius::same(11), track);
            let radius = rect.height() / 2.0 - 2.0;
            let x = egui::lerp(
                (rect.left() + radius + 2.0)..=(rect.right() - radius - 2.0),
                t,
            );
            let center = Pos2::new(x, rect.center().y);
            painter.circle_filled(
                center + Vec2::new(0.0, 1.0),
                radius + 0.5,
                Color32::from_black_alpha(50),
            );
            painter.circle_filled(center, radius, Color32::WHITE);
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *self.on, "")
        });
        response
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let a = a.to_array();
    let b = b.to_array();
    let mix = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(mix(0), mix(1), mix(2), mix(3))
}

/// Picks one of a few options. Returns whether the choice changed.
pub fn segmented(ui: &mut Ui, selected: &mut usize, options: &[&str]) -> bool {
    let height = 26.0;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let count = options.len().max(1);
    let segment = width / count as f32;
    let mut changed = false;
    let id = ui
        .scope_id()
        .with(("segmented", rect.min.x as i32, rect.min.y as i32));
    for (index, _) in options.iter().enumerate() {
        let part = Rect::from_min_size(
            Pos2::new(rect.left() + segment * index as f32, rect.top()),
            Vec2::new(segment, height),
        );
        let response = ui.interact(part, id.with(index), Sense::click());
        if response.clicked() && *selected != index {
            *selected = index;
            changed = true;
        }
    }
    let target = *selected as f32;
    let position = ui.ctx().animate_value_with_time(id, target, 0.16);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(9), theme::FILL_SOFT);
        let thumb = Rect::from_min_size(
            Pos2::new(rect.left() + segment * position, rect.top()),
            Vec2::new(segment, height),
        )
        .shrink(2.0);
        painter.rect_filled(
            thumb.translate(Vec2::new(0.0, 1.0)),
            CornerRadius::same(7),
            Color32::from_black_alpha(40),
        );
        painter.rect_filled(thumb, CornerRadius::same(7), theme::THUMB);
        for (index, label) in options.iter().enumerate() {
            let center = Pos2::new(
                rect.left() + segment * (index as f32 + 0.5),
                rect.center().y,
            );
            let color = if index == *selected {
                theme::TEXT
            } else {
                theme::SECONDARY
            };
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                *label,
                egui::FontId::new(12.5, theme::semibold()),
                color,
            );
        }
    }
    changed
}

// --- Grouped cards ---------------------------------------------------------------

/// The background of a group of rows, as in system settings: a rounded,
/// faintly lighter card. Rows inside are separated by hairlines.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(14)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A thin line between rows of a card.
pub fn hairline(ui: &mut Ui) {
    let rect = ui.available_rect_before_wrap();
    let y = ui.cursor().top() + 1.0;
    ui.painter().hline(
        rect.left()..=rect.right() + 12.0,
        y,
        Stroke::new(1.0, theme::SEPARATOR),
    );
    ui.add_space(3.0);
}

/// A section's title above its card, with a chevron to fold it and an
/// optional remove button. Returns `(open, remove clicked)`.
pub fn section_header(ui: &mut Ui, id: egui::Id, title: &str, removable: bool) -> (bool, bool) {
    let mut open: bool = ui.data(|d| d.get_temp(id).unwrap_or(true));
    let height = 26.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if response.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    let t = ui.ctx().animate_bool_with_time(id.with("open"), open, 0.14);
    let hovered = ui.rect_contains_pointer(rect);
    let painter = ui.painter();
    let color = if response.hovered() {
        theme::TEXT
    } else {
        theme::SECONDARY
    };
    painter.text(
        Pos2::new(rect.left() + 4.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::new(12.5, theme::semibold()),
        color,
    );
    let chevron = Rect::from_center_size(
        Pos2::new(
            rect.right() - (if removable { 40.0 } else { 12.0 }),
            rect.center().y,
        ),
        Vec2::splat(14.0),
    );
    if hovered || !open {
        paint_icon(painter, Icon::Chevron(t), chevron, theme::TERTIARY);
    }
    let mut remove = false;
    if removable {
        let button = Rect::from_center_size(
            Pos2::new(rect.right() - 12.0, rect.center().y),
            Vec2::splat(22.0),
        );
        let response = ui
            .interact(button, id.with("remove"), Sense::click())
            .on_hover_text("Remove");
        if hovered {
            let fill = if response.hovered() {
                theme::FILL
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().circle_filled(button.center(), 11.0, fill);
            paint_icon(
                ui.painter(),
                Icon::Close,
                button.shrink(5.0),
                if response.hovered() {
                    theme::DANGER
                } else {
                    theme::SECONDARY
                },
            );
        }
        remove = response.clicked();
    }
    (open, remove)
}

/// A row of a card: a label on the left, the value on the right.
pub fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        ui.allocate_ui_with_layout(
            Vec2::new(76.0, 22.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(76.0);
                ui.add(egui::Label::new(RichText::new(label).color(theme::SECONDARY)).truncate());
            },
        );
        add(ui);
    });
}

// --- Menus ---------------------------------------------------------------------

/// Spacing for a menu of [`menu_item`]s, and the same highlight for egui's
/// own submenu buttons.
pub fn menu_style(ui: &mut Ui) {
    let style = ui.style_mut();
    style.spacing.item_spacing.y = 0.0;
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    let radius = CornerRadius::same(MENU_ITEM_RADIUS);
    let w = &mut style.visuals.widgets;
    for state in [&mut w.hovered, &mut w.open, &mut w.active] {
        state.weak_bg_fill = theme::ACCENT;
        state.bg_fill = theme::ACCENT;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        state.corner_radius = radius;
    }
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.corner_radius = radius;
}

/// Menu rows sit inside the menu's 6 point margin, so their corners are
/// that much tighter than the menu's.
const MENU_ITEM_RADIUS: u8 = 10;
const MENU_ITEM_HEIGHT: f32 = 28.0;

/// A row of a menu: an optional icon, a label and a shortcut on the right.
/// The row lights up in the accent colour under the pointer.
pub fn menu_item(ui: &mut Ui, icon: Option<Icon>, label: &str, shortcut: &str) -> Response {
    menu_row(
        ui,
        MenuRow::new(icon, label, shortcut, theme::TEXT, theme::ACCENT),
    )
}

/// A menu row for something destructive, in red.
pub fn menu_item_danger(ui: &mut Ui, label: &str, shortcut: &str) -> Response {
    menu_row(
        ui,
        MenuRow::new(None, label, shortcut, theme::DANGER, theme::DANGER),
    )
}

/// A menu row that opens `content` as a menu beside it.
pub fn submenu_item(ui: &mut Ui, icon: Option<Icon>, label: &str, content: impl FnOnce(&mut Ui)) {
    use egui::containers::menu::{MenuState, SubMenu};
    let id = ui.next_auto_id();
    let open = MenuState::from_ui(ui, |state, _| {
        state.open_item == Some(SubMenu::id_from_widget_id(id))
    });
    let mut row = MenuRow::new(icon, label, "", theme::TEXT, theme::ACCENT);
    row.arrow = true;
    row.lit = open;
    let response = menu_row(ui, row);
    SubMenu::new().show(ui, &response, |ui| {
        menu_style(ui);
        content(ui)
    });
}

struct MenuRow<'a> {
    icon: Option<Icon>,
    label: &'a str,
    shortcut: &'a str,
    color: Color32,
    highlight: Color32,
    /// Shows an arrow on the right, for a submenu.
    arrow: bool,
    /// Lit as if hovered, while its submenu is open.
    lit: bool,
}

impl<'a> MenuRow<'a> {
    fn new(
        icon: Option<Icon>,
        label: &'a str,
        shortcut: &'a str,
        color: Color32,
        highlight: Color32,
    ) -> Self {
        MenuRow {
            icon,
            label,
            shortcut,
            color,
            highlight,
            arrow: false,
            lit: false,
        }
    }
}

fn menu_row(ui: &mut Ui, row: MenuRow) -> Response {
    let font = egui::FontId::proportional(13.0);
    let label_galley = ui
        .painter()
        .layout_no_wrap(row.label.to_string(), font, row.color);
    let shortcut_galley = ui.painter().layout_no_wrap(
        row.shortcut.to_string(),
        egui::FontId::proportional(12.0),
        theme::SECONDARY,
    );
    let icon_room = 26.0;
    let shortcut_room = if row.arrow {
        28.0
    } else if row.shortcut.is_empty() {
        0.0
    } else {
        shortcut_galley.size().x + 24.0
    };
    let desired = Vec2::new(
        10.0 + icon_room + label_galley.size().x + shortcut_room + 10.0,
        MENU_ITEM_HEIGHT,
    );
    // In a menu's justified layout the row stretches to the menu's width.
    let response = ui.allocate_response(desired, Sense::click());
    let rect = response.rect;
    if ui.is_rect_visible(rect) {
        let lit = response.hovered() || row.lit;
        let painter = ui.painter();
        let text_color = if lit { Color32::WHITE } else { row.color };
        if lit {
            painter.rect_filled(rect, CornerRadius::same(MENU_ITEM_RADIUS), row.highlight);
        }
        if let Some(icon) = row.icon {
            let icon_rect = Rect::from_center_size(
                Pos2::new(rect.left() + 10.0 + 8.0, rect.center().y),
                Vec2::splat(16.0),
            );
            paint_icon(painter, icon, icon_rect, text_color);
        }
        painter.galley(
            Pos2::new(
                rect.left() + 10.0 + icon_room,
                rect.center().y - label_galley.size().y / 2.0,
            ),
            label_galley,
            text_color,
        );
        let quiet = if lit {
            Color32::from_white_alpha(200)
        } else {
            theme::SECONDARY
        };
        if row.arrow {
            paint_icon(
                painter,
                Icon::Chevron(0.0),
                Rect::from_center_size(
                    Pos2::new(rect.right() - 16.0, rect.center().y),
                    Vec2::splat(14.0),
                ),
                quiet,
            );
        } else if !row.shortcut.is_empty() {
            painter.galley(
                Pos2::new(
                    rect.right() - 10.0 - shortcut_galley.size().x,
                    rect.center().y - shortcut_galley.size().y / 2.0,
                ),
                shortcut_galley,
                quiet,
            );
        }
    }
    response
}

/// A thin line between groups of menu rows.
pub fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.min_rect().width(), 9.0), Sense::hover());
    let left = ui.min_rect().left() + 10.0;
    let right = ui.min_rect().right().max(rect.right()) - 10.0;
    ui.painter().hline(
        left..=right,
        rect.center().y,
        Stroke::new(1.0, theme::SEPARATOR),
    );
}

/// A small title over a group of menu rows.
pub fn menu_heading(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(
            RichText::new(text)
                .family(theme::semibold())
                .size(11.5)
                .color(theme::TERTIARY),
        );
    });
    ui.add_space(2.0);
}
