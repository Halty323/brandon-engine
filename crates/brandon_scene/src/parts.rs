use brandon_core::{Color, Rect};
use glam::Vec2;
use serde::{Deserialize, Serialize};

fn is_false(v: &bool) -> bool {
    !*v
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_white(c: &Color) -> bool {
    *c == Color::WHITE
}
fn is_center(v: &Vec2) -> bool {
    *v == Vec2::splat(0.5)
}

/// Draws an image, or a plain rectangle when there is no texture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sprite {
    /// Asset path of the image, for example `"player.png"`. `None` draws a
    /// plain rectangle in [`color`](Self::color).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    /// Tint. White leaves the image unchanged.
    #[serde(skip_serializing_if = "is_white")]
    pub color: Color,
    /// Size in world units. `None` uses the size of the image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Vec2>,
    /// The part of the image to draw, in pixels. `None` draws the whole image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<Rect>,
    /// Which point of the sprite sits on the node's position. `(0.5, 0.5)` is
    /// the centre, `(0, 0)` the top-left corner.
    #[serde(skip_serializing_if = "is_center")]
    pub anchor: Vec2,
    #[serde(skip_serializing_if = "is_false")]
    pub flip_x: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub flip_y: bool,
    /// Smooth scaling. Turn off for crisp pixel art.
    #[serde(skip_serializing_if = "is_true")]
    pub smooth: bool,
}

impl Sprite {
    /// The size used for a sprite that has neither a texture nor a size.
    pub const DEFAULT_SIZE: Vec2 = Vec2::splat(32.0);

    /// A sprite showing an image.
    pub fn new(texture: impl Into<String>) -> Self {
        Sprite {
            texture: Some(texture.into()),
            ..Default::default()
        }
    }

    /// A plain coloured rectangle.
    pub fn rect(width: f32, height: f32, color: Color) -> Self {
        Sprite {
            size: Some(Vec2::new(width, height)),
            color,
            ..Default::default()
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = Some(Vec2::new(width, height));
        self
    }

    pub fn with_region(mut self, region: Rect) -> Self {
        self.region = Some(region);
        self
    }

    pub fn with_anchor(mut self, x: f32, y: f32) -> Self {
        self.anchor = Vec2::new(x, y);
        self
    }

    /// Crisp, unsmoothed scaling for pixel art.
    pub fn pixelated(mut self) -> Self {
        self.smooth = false;
        self
    }
}

impl Default for Sprite {
    fn default() -> Self {
        Sprite {
            texture: None,
            color: Color::WHITE,
            size: None,
            region: None,
            anchor: Vec2::splat(0.5),
            flip_x: false,
            flip_y: false,
            smooth: true,
        }
    }
}

/// Horizontal alignment of text around the node's position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Align {
    /// The text starts at the node's position.
    #[default]
    Left,
    /// The text is centred on the node's position.
    Center,
    /// The text ends at the node's position.
    Right,
}

fn is_left(a: &Align) -> bool {
    *a == Align::Left
}

/// Draws text. The node's position is the top of the first line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Text {
    pub text: String,
    /// Font size in world units.
    pub size: f32,
    #[serde(skip_serializing_if = "is_white")]
    pub color: Color,
    #[serde(skip_serializing_if = "is_left")]
    pub align: Align,
}

impl Text {
    pub fn new(text: impl Into<String>) -> Self {
        Text {
            text: text.into(),
            ..Default::default()
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }
}

impl Default for Text {
    fn default() -> Self {
        Text {
            text: String::new(),
            size: 24.0,
            color: Color::WHITE,
            align: Align::Left,
        }
    }
}

/// Makes the view follow this node: the node's position is shown at the
/// centre of the window.
///
/// Without any camera, world position `(0, 0)` is the top-left corner of the
/// window. If several cameras are active, the first one in the tree is used.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Camera {
    /// `2.0` makes everything twice as big.
    pub zoom: f32,
    #[serde(skip_serializing_if = "is_true")]
    pub active: bool,
}

impl Camera {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_zoom(mut self, zoom: f32) -> Self {
        self.zoom = zoom;
        self
    }
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            zoom: 1.0,
            active: true,
        }
    }
}

/// An area used to detect overlaps with other nodes, centred on the node.
///
/// See [`Scene::overlapping`](crate::Scene::overlapping).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Rect { size: Vec2 },
    Circle { radius: f32 },
}

impl Shape {
    pub fn rect(width: f32, height: f32) -> Self {
        Shape::Rect {
            size: Vec2::new(width, height),
        }
    }

    pub fn circle(radius: f32) -> Self {
        Shape::Circle { radius }
    }
}

/// Draws this node and its children fixed to the window, on top of the world
/// and unaffected by the camera. Positions are pixels from the top-left
/// corner. Use it for scores, menus and other on-screen display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Overlay;
