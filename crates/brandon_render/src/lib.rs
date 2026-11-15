//! The Brandon 2D renderer.
//!
//! It draws textured quads, batched by texture, straight on Vulkan. Everything
//! the engine shows (sprites, text, plain rectangles) ends up as a [`Quad`].
//!
//! The renderer does not know about scenes or windows. Give it anything that
//! has window handles, then hand it a [`Frame`] to draw.

mod renderer;
mod shader;
mod text;

pub use renderer::{Renderer, Texture};
pub use text::{PlacedGlyph, TextLayout, TextRenderer};

use brandon_core::Color;
use glam::{Affine2, Vec2};

/// Something went wrong in the renderer.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("Vulkan could not be loaded. Is a Vulkan driver installed? ({0})")]
    NoVulkan(String),
    #[error("no suitable graphics device was found: {0}")]
    NoDevice(String),
    #[error("Vulkan error while {doing}: {result}")]
    Vulkan {
        doing: &'static str,
        result: ash::vk::Result,
    },
    #[error("graphics memory error: {0}")]
    Memory(String),
    #[error("shader error:\n{0}")]
    Shader(String),
    #[error("window error: {0}")]
    Window(String),
}

/// How a texture is scaled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Filter {
    /// Smooth scaling, for most art.
    #[default]
    Smooth,
    /// Crisp scaling, for pixel art.
    Pixelated,
}

/// One textured, tinted quad.
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    /// Where the quad's top-left corner is.
    pub origin: Vec2,
    /// From the top-left corner to the top-right corner.
    pub x_axis: Vec2,
    /// From the top-left corner to the bottom-left corner.
    pub y_axis: Vec2,
    /// Texture coordinates `[u0, v0, u1, v1]`, from `0.0` to `1.0`.
    pub uv: [f32; 4],
    pub color: Color,
    pub texture: Texture,
    pub filter: Filter,
    /// Draw order within the layer. Higher is on top. Quads with the same
    /// `z` are drawn in the order they were added.
    pub z: i32,
}

/// A group of quads drawn with the same view.
#[derive(Clone, Debug)]
pub struct Layer {
    /// Transform from the quads' space to frame units (see [`Frame::size`]).
    pub view: Affine2,
    pub quads: Vec<Quad>,
}

/// Everything to draw for one frame. Layers are drawn in order.
#[derive(Clone, Debug)]
pub struct Frame {
    pub clear: Color,
    /// The size of the frame in the units layers use, with `(0, 0)` at the
    /// top-left corner. Usually the window size in logical pixels.
    pub size: Vec2,
    pub layers: Vec<Layer>,
}
