//! The inspector: shows only what the selected node has, and an "Add"
//! button for the rest.

use crate::fields::{self, Kind};
use crate::theme;
use crate::widgets::{self, Icon};
use brandon_core::{Color, Rect};
use brandon_scene::{Align, Camera, Node, Overlay, RawBehaviour, Shape, Sprite, Text};
use egui::{DragValue, RichText, Sense, Ui};
use glam::Vec2;

/// The height of number fields, to sit centred in a 30 point row.
const FIELD_HEIGHT: f32 = 24.0;

/// What the inspector needs to know besides the node.
pub struct Context<'a> {
    /// Image files in the project, for the sprite's texture picker.
    pub images: &'a [String],
    /// Behaviour names used anywhere in the project's scenes.
    pub behaviour_names: &'a [String],
    /// Set when the node is inside a placed scene and cannot be changed here.
    pub read_only: bool,
}

/// What the user asked for that the inspector cannot do itself.
#[derive(Default)]
pub struct Requests {
    pub open_scene: Option<String>,
}

/// The icon for a node, from the parts it has.
pub fn node_icon(node: &Node) -> Icon {
    if node.instance_of().is_some() {
        Icon::Placed
    } else if node.camera.is_some() {
        Icon::Camera
    } else if node.text.is_some() {
        Icon::Text
    } else if node.sprite.is_some() {
        Icon::Image
    } else if node.overlay.is_some() {
        Icon::Overlay
    } else if node.shape.is_some() {
        Icon::Shape
    } else {
        Icon::Node
    }
}

/// Draws the inspector. Returns whether anything changed.
pub fn show(ui: &mut Ui, node: &mut Node, cx: &Context, requests: &mut Requests) -> bool {
    let mut changed = false;

    if cx.read_only {
        notice(ui, "Part of a placed scene. Open that scene to change it.");
        ui.disable();
    }

    // The node's kind, its name and whether it is shown.
    ui.horizontal(|ui| {
        let placed = node.instance_of().is_some();
        let (tile, _) = ui.allocate_exact_size(egui::Vec2::splat(34.0), Sense::hover());
        let fill = if placed { theme::PLACED } else { theme::ACCENT };
        ui.painter().rect_filled(tile, 10.0, fill);
        ui.painter().rect_stroke(
            tile.shrink(0.5),
            10.0,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(40)),
            egui::StrokeKind::Inside,
        );
        widgets::paint_icon(
            ui.painter(),
            node_icon(node),
            tile.shrink(8.0),
            egui::Color32::WHITE,
        );
        let width = ui.available_width() - 38.0;
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut node.name)
                    .desired_width(width)
                    .font(egui::FontId::new(16.0, theme::semibold()))
                    .margin(egui::Margin::symmetric(8, 6)),
            )
            .changed();
        let (icon, hint) = if node.visible {
            (Icon::Eye, "Shown. Click to hide.")
        } else {
            (Icon::EyeClosed, "Hidden. Click to show.")
        };
        if widgets::icon_button(ui, icon, 30.0, true, hint).clicked() {
            node.visible = !node.visible;
            changed = true;
        }
    });
    if let Some(path) = node.instance_of().map(str::to_string) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(42.0);
            ui.label(
                RichText::new("Placed from")
                    .color(theme::SECONDARY)
                    .size(12.0),
            );
            if ui
                .link(RichText::new(&path).color(theme::PLACED).size(12.0))
                .on_hover_text("Open this scene")
                .clicked()
            {
                requests.open_scene = Some(path.clone());
            }
        });
    }
    ui.add_space(10.0);

    section(ui, "Transform", None, |ui| {
        changed |= transform(ui, node);
    });

    let mut remove_part: Option<&'static str> = None;
    if let Some(sprite) = &mut node.sprite {
        if section(ui, "Sprite", Some("sprite"), |ui| {
            changed |= sprite_fields(ui, sprite, cx.images);
        }) {
            remove_part = Some("sprite");
        }
    }
    if let Some(text) = &mut node.text {
        if section(ui, "Text", Some("text"), |ui| {
            changed |= text_fields(ui, text);
        }) {
            remove_part = Some("text");
        }
    }
    if let Some(camera) = &mut node.camera {
        if section(ui, "Camera", Some("camera"), |ui| {
            changed |= camera_fields(ui, camera);
        }) {
            remove_part = Some("camera");
        }
    }
    if let Some(shape) = &mut node.shape {
        if section(ui, "Shape", Some("shape"), |ui| {
            changed |= shape_fields(ui, shape);
        }) {
            remove_part = Some("shape");
        }
    }
    if node.overlay.is_some()
        && section(ui, "Overlay", Some("overlay"), |ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Drawn fixed to the window, with its children.")
                    .color(theme::SECONDARY)
                    .size(12.0),
            );
            ui.add_space(4.0);
        })
    {
        remove_part = Some("overlay");
    }
    if let Some(part) = remove_part {
        match part {
            "sprite" => node.sprite = None,
            "text" => node.text = None,
            "camera" => node.camera = None,
            "shape" => node.shape = None,
            _ => node.overlay = None,
        }
        changed = true;
    }

    // Behaviours.
    let mut remove_behaviour = None;
    for (index, behaviour) in node.behaviours_of_mut::<RawBehaviour>().enumerate() {
        let title = behaviour.name.clone();
        let mut fields_changed = false;
        if section(ui, &title, Some("behaviour"), |ui| {
            fields_changed = behaviour_fields(ui, behaviour);
        }) {
            remove_behaviour = Some(index);
        }
        changed |= fields_changed;
    }
    if let Some(index) = remove_behaviour {
        node.remove_nth_behaviour::<RawBehaviour>(index);
        changed = true;
    }

    ui.add_space(4.0);
    changed |= add_menu(ui, node, cx);
    ui.add_space(8.0);
    changed
}

fn notice(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .fill(theme::PLACED.gamma_multiply(0.16))
        .corner_radius(14)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(egui::Vec2::splat(16.0), Sense::hover());
                widgets::paint_icon(ui.painter(), Icon::Placed, icon, theme::PLACED);
                ui.label(RichText::new(text).color(theme::TEXT).size(12.0));
            });
        });
    ui.add_space(10.0);
}

/// A titled group of rows on a card, which can be folded, and removed when
/// `kind` is given. Returns `true` if the remove button was clicked.
fn section(ui: &mut Ui, title: &str, kind: Option<&str>, body: impl FnOnce(&mut Ui)) -> bool {
    let id = ui.make_persistent_id(("section", title, kind));
    let (open, remove) = widgets::section_header(ui, id, title, kind.is_some());
    if open {
        widgets::card(ui, body);
    }
    ui.add_space(10.0);
    remove
}

/// A label and its value, with a hairline above every row but the first
/// of a card.
fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    separate(ui);
    widgets::row(ui, label, add);
}

/// A label with an on/off switch at the far right.
fn switch_row(ui: &mut Ui, label: &str, on: &mut bool, hint: &str) -> bool {
    separate(ui);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        ui.label(RichText::new(label).color(theme::SECONDARY));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let response = ui.add(widgets::Switch::new(on));
            let response = if hint.is_empty() {
                response
            } else {
                response.on_hover_text(hint)
            };
            changed = response.changed();
        });
    });
    changed
}

/// Draws a hairline unless this is the first thing on the card.
fn separate(ui: &mut Ui) {
    if ui.min_rect().height() > 1.0 {
        widgets::hairline(ui);
    }
}

fn vec2_fields(ui: &mut Ui, value: &mut Vec2, speed: f64) -> bool {
    let width = ((ui.available_width() - 8.0) / 2.0).max(40.0);
    let x = ui.add_sized(
        [width, FIELD_HEIGHT],
        DragValue::new(&mut value.x)
            .speed(speed)
            .max_decimals(3)
            .prefix("x "),
    );
    let y = ui.add_sized(
        [width, FIELD_HEIGHT],
        DragValue::new(&mut value.y)
            .speed(speed)
            .max_decimals(3)
            .prefix("y "),
    );
    x.changed() || y.changed()
}

fn transform(ui: &mut Ui, node: &mut Node) -> bool {
    let mut changed = false;
    let t = &mut node.transform;
    row(ui, "Position", |ui| {
        changed |= vec2_fields(ui, &mut t.position, 1.0)
    });
    row(ui, "Rotation", |ui| {
        let mut degrees = t.rotation.to_degrees();
        if ui
            .add_sized(
                [ui.available_width(), FIELD_HEIGHT],
                DragValue::new(&mut degrees)
                    .speed(1.0)
                    .max_decimals(1)
                    .suffix("°"),
            )
            .changed()
        {
            t.rotation = degrees.to_radians();
            changed = true;
        }
    });
    row(ui, "Scale", |ui| {
        changed |= vec2_fields(ui, &mut t.scale, 0.01)
    });
    row(ui, "Layer", |ui| {
        changed |= ui
            .add_sized(
                [ui.available_width(), FIELD_HEIGHT],
                DragValue::new(&mut t.z).speed(0.1),
            )
            .on_hover_text("Draw order. Higher is drawn on top.")
            .changed();
    });
    changed
}

fn color_field(ui: &mut Ui, color: &mut Color) -> bool {
    let mut rgba = [color.r, color.g, color.b, color.a];
    let response = ui.color_edit_button_rgba_unmultiplied(&mut rgba);
    ui.label(
        RichText::new(color.to_hex_string())
            .color(theme::SECONDARY)
            .monospace()
            .size(12.0),
    );
    if response.changed() {
        *color = Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        true
    } else {
        false
    }
}

fn sprite_fields(ui: &mut Ui, sprite: &mut Sprite, images: &[String]) -> bool {
    let mut changed = false;
    row(ui, "Image", |ui| {
        let current = sprite
            .texture
            .clone()
            .unwrap_or_else(|| "Plain colour".to_string());
        egui::ComboBox::from_id_salt("sprite-image")
            .selected_text(current)
            .width(ui.available_width())
            .icon(|ui, rect, visuals, _open| {
                widgets::paint_icon(
                    ui.painter(),
                    Icon::ChevronUpDown,
                    rect.expand(2.0),
                    visuals.fg_stroke.color,
                );
            })
            .show_ui(ui, |ui| {
                widgets::menu_style(ui);
                let none = sprite.texture.is_none();
                if widgets::menu_item(ui, none.then_some(Icon::Check), "Plain colour", "").clicked()
                {
                    sprite.texture = None;
                    changed = true;
                }
                for image in images {
                    let chosen = sprite.texture.as_deref() == Some(image);
                    if widgets::menu_item(ui, chosen.then_some(Icon::Check), image, "").clicked() {
                        sprite.texture = Some(image.clone());
                        changed = true;
                    }
                }
            });
    });
    row(ui, "Tint", |ui| {
        changed |= color_field(ui, &mut sprite.color)
    });
    let mut custom = sprite.size.is_some();
    if switch_row(ui, "Own size", &mut custom, "Off: use the image's own size") {
        sprite.size = custom.then_some(Sprite::DEFAULT_SIZE);
        changed = true;
    }
    if let Some(size) = &mut sprite.size {
        row(ui, "Size", |ui| changed |= vec2_fields(ui, size, 1.0));
    }
    row(ui, "Anchor", |ui| {
        changed |= vec2_fields(ui, &mut sprite.anchor, 0.01)
    });
    let mut custom = sprite.region.is_some();
    if switch_row(
        ui,
        "Part of the image",
        &mut custom,
        "Draw only part of the image, in pixels",
    ) {
        sprite.region = custom.then_some(Rect::new(0.0, 0.0, 32.0, 32.0));
        changed = true;
    }
    if let Some(region) = &mut sprite.region {
        let mut corner = Vec2::new(region.x, region.y);
        let mut size = Vec2::new(region.w, region.h);
        row(ui, "From", |ui| {
            changed |= vec2_fields(ui, &mut corner, 1.0)
        });
        row(ui, "Size", |ui| changed |= vec2_fields(ui, &mut size, 1.0));
        *region = Rect::new(corner.x, corner.y, size.x, size.y);
    }
    changed |= switch_row(
        ui,
        "Flip across",
        &mut sprite.flip_x,
        "Mirror left to right",
    );
    changed |= switch_row(ui, "Flip upside down", &mut sprite.flip_y, "");
    changed |= switch_row(ui, "Smooth", &mut sprite.smooth, "Turn off for pixel art");
    changed
}

fn text_fields(ui: &mut Ui, text: &mut Text) -> bool {
    let mut changed = false;
    ui.add_space(6.0);
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut text.text)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(8, 6)),
        )
        .changed();
    ui.add_space(4.0);
    row(ui, "Size", |ui| {
        changed |= ui
            .add_sized(
                [ui.available_width(), FIELD_HEIGHT],
                DragValue::new(&mut text.size).speed(0.2).range(1.0..=512.0),
            )
            .changed();
    });
    row(ui, "Colour", |ui| {
        changed |= color_field(ui, &mut text.color)
    });
    row(ui, "Align", |ui| {
        let aligns = [Align::Left, Align::Center, Align::Right];
        let mut index = aligns.iter().position(|a| *a == text.align).unwrap_or(0);
        if widgets::segmented(ui, &mut index, &["Left", "Centre", "Right"]) {
            text.align = aligns[index];
            changed = true;
        }
    });
    ui.add_space(2.0);
    changed
}

fn camera_fields(ui: &mut Ui, camera: &mut Camera) -> bool {
    let mut changed = false;
    row(ui, "Zoom", |ui| {
        changed |= ui
            .add_sized(
                [ui.available_width(), FIELD_HEIGHT],
                DragValue::new(&mut camera.zoom)
                    .speed(0.01)
                    .range(0.01..=64.0),
            )
            .changed();
    });
    changed |= switch_row(
        ui,
        "Active",
        &mut camera.active,
        "The view follows the active camera",
    );
    changed
}

fn shape_fields(ui: &mut Ui, shape: &mut Shape) -> bool {
    let mut changed = false;
    row(ui, "Kind", |ui| {
        let mut index = if matches!(shape, Shape::Circle { .. }) {
            0
        } else {
            1
        };
        if widgets::segmented(ui, &mut index, &["Circle", "Rectangle"]) {
            *shape = if index == 0 {
                Shape::circle(16.0)
            } else {
                Shape::rect(32.0, 32.0)
            };
            changed = true;
        }
    });
    match shape {
        Shape::Circle { radius } => row(ui, "Radius", |ui| {
            changed |= ui
                .add_sized(
                    [ui.available_width(), FIELD_HEIGHT],
                    DragValue::new(radius).speed(0.5).range(0.0..=f32::MAX),
                )
                .changed();
        }),
        Shape::Rect { size } => row(ui, "Size", |ui| changed |= vec2_fields(ui, size, 0.5)),
    }
    changed
}

fn behaviour_fields(ui: &mut Ui, behaviour: &mut RawBehaviour) -> bool {
    let Some(mut list) = fields::split(&behaviour.ron) else {
        // Not a plain list of fields: edit the data as written.
        let mut text = behaviour.ron.clone();
        ui.add_space(6.0);
        let response = ui.add(
            egui::TextEdit::multiline(&mut text)
                .code_editor()
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        let valid = fields::is_valid(&text);
        if !valid {
            ui.label(
                RichText::new("Not valid scene data yet. It will be kept as it was.")
                    .color(theme::WARN)
                    .size(12.0),
            );
        }
        if response.changed() && valid {
            behaviour.ron = text;
            return true;
        }
        return false;
    };
    if list.is_empty() {
        ui.add_space(4.0);
        ui.label(
            RichText::new("No settings")
                .color(theme::SECONDARY)
                .size(12.0),
        );
        ui.add_space(4.0);
        return false;
    }
    let mut changed = false;
    for field in &mut list {
        let name = field.name.clone();
        row(ui, &name, |ui| match field.kind() {
            Kind::Number(mut number) => {
                let whole = fields::is_whole(&field.value);
                let drag = if whole {
                    DragValue::new(&mut number).speed(0.25).max_decimals(0)
                } else {
                    DragValue::new(&mut number).speed(0.01)
                };
                if ui
                    .add_sized([ui.available_width(), FIELD_HEIGHT], drag)
                    .changed()
                {
                    field.value = fields::number_text(number, whole);
                    changed = true;
                }
            }
            Kind::Bool(mut on) => {
                if ui.add(widgets::Switch::new(&mut on)).changed() {
                    field.value = on.to_string();
                    changed = true;
                }
            }
            Kind::Text(mut text) => {
                if ui
                    .add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY))
                    .changed()
                {
                    field.value = fields::text_value(&text);
                    changed = true;
                }
            }
            Kind::Other => {
                let mut text = field.value.clone();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .code_editor()
                        .desired_width(f32::INFINITY),
                );
                if response.changed() && fields::is_valid(&text) {
                    field.value = text;
                    changed = true;
                }
            }
        });
    }
    if changed {
        behaviour.ron = fields::join(&list);
    }
    changed
}

fn add_menu(ui: &mut Ui, node: &mut Node, cx: &Context) -> bool {
    let mut changed = false;
    let button = widgets::wide_button(ui, Icon::Plus, "Add");
    egui::Popup::menu(&button)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            widgets::menu_style(ui);
            ui.set_min_width(240.0);
            widgets::menu_heading(ui, "Parts");
            let part = |ui: &mut Ui, present: bool, icon: Icon, label: &str, hint: &str| {
                !present
                    && widgets::menu_item(ui, Some(icon), label, "")
                        .on_hover_text(hint)
                        .clicked()
            };
            if part(
                ui,
                node.sprite.is_some(),
                Icon::Image,
                "Sprite",
                "Draws an image or a coloured rectangle",
            ) {
                node.sprite = Some(Sprite::rect(32.0, 32.0, Color::WHITE));
                changed = true;
            }
            if part(ui, node.text.is_some(), Icon::Text, "Text", "Draws text") {
                node.text = Some(Text::new("Text"));
                changed = true;
            }
            if part(
                ui,
                node.shape.is_some(),
                Icon::Shape,
                "Shape",
                "An area for detecting overlaps",
            ) {
                node.shape = Some(Shape::circle(16.0));
                changed = true;
            }
            if part(
                ui,
                node.camera.is_some(),
                Icon::Camera,
                "Camera",
                "Makes the view follow this node",
            ) {
                node.camera = Some(Camera::new());
                changed = true;
            }
            if part(
                ui,
                node.overlay.is_some(),
                Icon::Overlay,
                "Overlay",
                "Draws this node fixed to the window",
            ) {
                node.overlay = Some(Overlay);
                changed = true;
            }
            widgets::menu_separator(ui);
            widgets::menu_heading(ui, "Behaviours");
            for name in cx.behaviour_names {
                if widgets::menu_item(ui, Some(Icon::Node), name, "").clicked() {
                    node.attach(RawBehaviour::new(name.clone(), "()"));
                    changed = true;
                }
            }
            let id = ui.make_persistent_id("new-behaviour-name");
            let mut name: String = ui.data(|d| d.get_temp(id).unwrap_or_default());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text("New behaviour")
                        .desired_width(ui.available_width() - 70.0)
                        .margin(egui::Margin::symmetric(8, 5)),
                );
                let valid =
                    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_');
                let add = widgets::capsule(ui, None, "Add", widgets::Tone::Primary, valid)
                    .clicked()
                    || (valid
                        && response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                if add {
                    node.attach(RawBehaviour::new(name.clone(), "()"));
                    name.clear();
                    changed = true;
                }
            });
            ui.add_space(4.0);
            ui.data_mut(|d| d.insert_temp(id, name));
            if changed {
                ui.close();
            }
        });
    changed
}
