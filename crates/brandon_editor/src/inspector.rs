//! The inspector: shows only what the selected node has, and an "Add"
//! button for the rest.

use crate::fields::{self, Kind};
use crate::theme;
use brandon_core::{Color, Rect};
use brandon_scene::{Align, Camera, Node, Overlay, RawBehaviour, Shape, Sprite, Text};
use egui::{DragValue, RichText, Ui};
use glam::Vec2;

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

/// Draws the inspector. Returns whether anything changed.
pub fn show(ui: &mut Ui, node: &mut Node, cx: &Context, requests: &mut Requests) -> bool {
    let mut changed = false;

    if cx.read_only {
        notice(ui, "Part of a placed scene. Open that scene to change it.");
        ui.disable();
    }

    // Name and visibility.
    ui.horizontal(|ui| {
        let width = ui.available_width() - 34.0;
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut node.name)
                    .desired_width(width)
                    .font(egui::TextStyle::Heading),
            )
            .changed();
        let (icon, color) = if node.visible {
            ("●", theme::ACCENT)
        } else {
            ("○", theme::WEAK)
        };
        let response = ui
            .add(
                egui::Button::new(RichText::new(icon).size(13.0).color(color))
                    .frame(false)
                    .min_size(egui::vec2(26.0, 26.0)),
            )
            .on_hover_text(if node.visible {
                "Visible. Click to hide."
            } else {
                "Hidden. Click to show."
            });
        if response.clicked() {
            node.visible = !node.visible;
            changed = true;
        }
    });
    if let Some(path) = node.instance_of().map(str::to_string) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("◆ Placed from").color(theme::PLACED).small());
            if ui
                .link(RichText::new(&path).small())
                .on_hover_text("Open this scene")
                .clicked()
            {
                requests.open_scene = Some(path.clone());
            }
        });
    }
    ui.add_space(6.0);

    section(ui, "Transform", None, true, |ui| {
        changed |= transform(ui, node);
        false
    });

    let mut remove_part: Option<&'static str> = None;
    if let Some(sprite) = &mut node.sprite {
        if section(ui, "Sprite", Some("sprite"), true, |ui| {
            changed |= sprite_fields(ui, sprite, cx.images);
            false
        }) {
            remove_part = Some("sprite");
        }
    }
    if let Some(text) = &mut node.text {
        if section(ui, "Text", Some("text"), true, |ui| {
            changed |= text_fields(ui, text);
            false
        }) {
            remove_part = Some("text");
        }
    }
    if let Some(camera) = &mut node.camera {
        if section(ui, "Camera", Some("camera"), true, |ui| {
            changed |= camera_fields(ui, camera);
            false
        }) {
            remove_part = Some("camera");
        }
    }
    if let Some(shape) = &mut node.shape {
        if section(ui, "Shape", Some("shape"), true, |ui| {
            changed |= shape_fields(ui, shape);
            false
        }) {
            remove_part = Some("shape");
        }
    }
    if node.overlay.is_some()
        && section(ui, "Overlay", Some("overlay"), true, |ui| {
            ui.label(
                RichText::new("Drawn fixed to the window, with its children.")
                    .weak()
                    .small(),
            );
            false
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
        if section(ui, &title, Some("behaviour"), true, |ui| {
            fields_changed = behaviour_fields(ui, behaviour);
            false
        }) {
            remove_behaviour = Some(index);
        }
        changed |= fields_changed;
    }
    if let Some(index) = remove_behaviour {
        node.remove_nth_behaviour::<RawBehaviour>(index);
        changed = true;
    }

    ui.add_space(8.0);
    changed |= add_menu(ui, node, cx);
    changed
}

fn notice(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .fill(theme::RAISED)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(theme::PLACED).small());
        });
    ui.add_space(6.0);
}

/// A collapsible section with an optional remove button. Returns `true` if
/// the remove button was clicked.
fn section(
    ui: &mut Ui,
    title: &str,
    kind: Option<&str>,
    open: bool,
    body: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    let id = ui.make_persistent_id(("section", title, kind));
    let mut remove = false;
    ui.separator();
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open)
        .show_header(ui, |ui| {
            ui.label(RichText::new(title).strong());
            if kind.is_some() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response = ui
                        .add(
                            egui::Button::new(RichText::new("×").color(theme::WEAK).size(11.0))
                                .frame(false),
                        )
                        .on_hover_text("Remove");
                    remove = response.clicked();
                });
            }
        })
        .body(|ui| {
            ui.add_space(2.0);
            body(ui);
            ui.add_space(4.0);
        });
    remove
}

/// A label column and a value column.
fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [72.0, 22.0],
            egui::Label::new(RichText::new(label).color(theme::WEAK)).truncate(),
        );
        add(ui);
    });
}

fn vec2_fields(ui: &mut Ui, value: &mut Vec2, speed: f64) -> bool {
    let width = ((ui.available_width() - 8.0) / 2.0).max(40.0);
    let x = ui.add_sized(
        [width, 22.0],
        DragValue::new(&mut value.x)
            .speed(speed)
            .max_decimals(3)
            .prefix("x "),
    );
    let y = ui.add_sized(
        [width, 22.0],
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
                [ui.available_width(), 22.0],
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
                [ui.available_width(), 22.0],
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
    ui.label(RichText::new(color.to_hex_string()).weak().small());
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
            .unwrap_or_else(|| "None (plain colour)".to_string());
        egui::ComboBox::from_id_salt("sprite-image")
            .selected_text(current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(sprite.texture.is_none(), "None (plain colour)")
                    .clicked()
                {
                    sprite.texture = None;
                    changed = true;
                }
                for image in images {
                    if ui
                        .selectable_label(sprite.texture.as_deref() == Some(image), image)
                        .clicked()
                    {
                        sprite.texture = Some(image.clone());
                        changed = true;
                    }
                }
            });
    });
    row(ui, "Tint", |ui| {
        changed |= color_field(ui, &mut sprite.color)
    });
    row(ui, "Size", |ui| {
        let mut custom = sprite.size.is_some();
        if ui
            .checkbox(&mut custom, "")
            .on_hover_text("Off: use the image's own size")
            .changed()
        {
            sprite.size = custom.then_some(Sprite::DEFAULT_SIZE);
            changed = true;
        }
        match &mut sprite.size {
            Some(size) => changed |= vec2_fields(ui, size, 1.0),
            None => {
                ui.label(RichText::new("From the image").weak());
            }
        }
    });
    row(ui, "Anchor", |ui| {
        changed |= vec2_fields(ui, &mut sprite.anchor, 0.01)
    });
    row(ui, "Region", |ui| {
        let mut custom = sprite.region.is_some();
        if ui
            .checkbox(&mut custom, "")
            .on_hover_text("Draw only part of the image, in pixels")
            .changed()
        {
            sprite.region = custom.then_some(Rect::new(0.0, 0.0, 32.0, 32.0));
            changed = true;
        }
        match &mut sprite.region {
            Some(region) => {
                let mut corner = Vec2::new(region.x, region.y);
                let mut size = Vec2::new(region.w, region.h);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| changed |= vec2_fields(ui, &mut corner, 1.0));
                    ui.horizontal(|ui| changed |= vec2_fields(ui, &mut size, 1.0));
                });
                *region = Rect::new(corner.x, corner.y, size.x, size.y);
            }
            None => {
                ui.label(RichText::new("Whole image").weak());
            }
        }
    });
    // Kept to short rows, so the inspector never pushes its panel wider.
    row(ui, "Flip", |ui| {
        changed |= ui.checkbox(&mut sprite.flip_x, "X").changed();
        changed |= ui.checkbox(&mut sprite.flip_y, "Y").changed();
    });
    row(ui, "Filter", |ui| {
        changed |= ui
            .checkbox(&mut sprite.smooth, "Smooth")
            .on_hover_text("Turn off for pixel art")
            .changed();
    });
    changed
}

fn text_fields(ui: &mut Ui, text: &mut Text) -> bool {
    let mut changed = false;
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut text.text)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        )
        .changed();
    row(ui, "Size", |ui| {
        changed |= ui
            .add_sized(
                [ui.available_width(), 22.0],
                DragValue::new(&mut text.size).speed(0.2).range(1.0..=512.0),
            )
            .changed();
    });
    row(ui, "Colour", |ui| {
        changed |= color_field(ui, &mut text.color)
    });
    row(ui, "Align", |ui| {
        for (align, label) in [
            (Align::Left, "Left"),
            (Align::Center, "Centre"),
            (Align::Right, "Right"),
        ] {
            if ui.selectable_label(text.align == align, label).clicked() {
                text.align = align;
                changed = true;
            }
        }
    });
    changed
}

fn camera_fields(ui: &mut Ui, camera: &mut Camera) -> bool {
    let mut changed = false;
    row(ui, "Zoom", |ui| {
        changed |= ui
            .add_sized(
                [ui.available_width(), 22.0],
                DragValue::new(&mut camera.zoom)
                    .speed(0.01)
                    .range(0.01..=64.0),
            )
            .changed();
    });
    row(ui, "", |ui| {
        changed |= ui.checkbox(&mut camera.active, "Active").changed()
    });
    changed
}

fn shape_fields(ui: &mut Ui, shape: &mut Shape) -> bool {
    let mut changed = false;
    row(ui, "Kind", |ui| {
        let is_circle = matches!(shape, Shape::Circle { .. });
        if ui.selectable_label(is_circle, "Circle").clicked() && !is_circle {
            *shape = Shape::circle(16.0);
            changed = true;
        }
        if ui.selectable_label(!is_circle, "Rectangle").clicked() && is_circle {
            *shape = Shape::rect(32.0, 32.0);
            changed = true;
        }
    });
    match shape {
        Shape::Circle { radius } => row(ui, "Radius", |ui| {
            changed |= ui
                .add_sized(
                    [ui.available_width(), 22.0],
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
        let response = ui.add(
            egui::TextEdit::multiline(&mut text)
                .code_editor()
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        let valid = fields::is_valid(&text);
        if !valid {
            ui.label(
                RichText::new("Not valid scene data yet. It will be kept as it was.")
                    .color(theme::WARN)
                    .small(),
            );
        }
        if response.changed() && valid {
            behaviour.ron = text;
            return true;
        }
        return false;
    };
    if list.is_empty() {
        ui.label(RichText::new("No settings").weak().small());
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
                if ui.add_sized([ui.available_width(), 22.0], drag).changed() {
                    field.value = fields::number_text(number, whole);
                    changed = true;
                }
            }
            Kind::Bool(mut on) => {
                if ui.checkbox(&mut on, "").changed() {
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
    let width = ui.available_width();
    let button = egui::Button::new(RichText::new("+  Add").color(theme::ACCENT))
        .min_size(egui::vec2(width, 28.0));
    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        ui.set_min_width(220.0);
        ui.label(RichText::new("Parts").weak().small());
        let part = |ui: &mut Ui, present: bool, label: &str, hint: &str| -> bool {
            !present && ui.button(label).on_hover_text(hint).clicked()
        };
        if part(
            ui,
            node.sprite.is_some(),
            "Sprite",
            "Draws an image or a coloured rectangle",
        ) {
            node.sprite = Some(Sprite::rect(32.0, 32.0, Color::WHITE));
            changed = true;
        }
        if part(ui, node.text.is_some(), "Text", "Draws text") {
            node.text = Some(Text::new("Text"));
            changed = true;
        }
        if part(
            ui,
            node.shape.is_some(),
            "Shape",
            "An area for detecting overlaps",
        ) {
            node.shape = Some(Shape::circle(16.0));
            changed = true;
        }
        if part(
            ui,
            node.camera.is_some(),
            "Camera",
            "Makes the view follow this node",
        ) {
            node.camera = Some(Camera::new());
            changed = true;
        }
        if part(
            ui,
            node.overlay.is_some(),
            "Overlay",
            "Draws this node fixed to the window",
        ) {
            node.overlay = Some(Overlay);
            changed = true;
        }
        ui.separator();
        ui.label(RichText::new("Behaviours").weak().small());
        for name in cx.behaviour_names {
            if ui.button(name).clicked() {
                node.attach(RawBehaviour::new(name.clone(), "()"));
                changed = true;
            }
        }
        let id = ui.make_persistent_id("new-behaviour-name");
        let mut name: String = ui.data(|d| d.get_temp(id).unwrap_or_default());
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .hint_text("Behaviour name")
                    .desired_width(140.0),
            );
            let valid = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_');
            let add = ui.add_enabled(valid, egui::Button::new("Add")).clicked()
                || (valid
                    && response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if add {
                node.attach(RawBehaviour::new(name.clone(), "()"));
                name.clear();
                changed = true;
            }
        });
        ui.data_mut(|d| d.insert_temp(id, name));
        if changed {
            ui.close();
        }
    });
    changed
}
