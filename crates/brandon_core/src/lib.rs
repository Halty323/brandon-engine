//! Core types shared by every Brandon crate: maths, colour, time, input and assets.

mod assets;
mod color;
mod input;
mod random;
mod rect;
mod time;

pub use assets::{AssetError, Assets, ImageData};
pub use color::Color;
pub use input::{Input, Key, MouseButton};
pub use random::Random;
pub use rect::Rect;
pub use time::Time;

/// Maths types, re-exported from [`glam`].
pub mod math {
    pub use glam::{vec2, Affine2, Mat2, Mat4, Vec2};
}
