//! Brandon is a 2D game engine that is easy to learn and capable of
//! beautiful graphics.
//!
//! A game is a tree of [`Node`]s. Parts such as [`Sprite`] and [`Text`] make
//! nodes visible, and your own [`Behaviour`]s make them do things.
//!
//! ```no_run
//! use brandon::prelude::*;
//!
//! #[behaviour]
//! #[derive(Default)]
//! struct Player {
//!     speed: f32,
//! }
//!
//! impl Behaviour for Player {
//!     fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
//!         let direction = ctx.input.axis2d(Key::A, Key::D, Key::W, Key::S);
//!         node.transform.position += direction * self.speed * ctx.dt();
//!     }
//! }
//!
//! fn main() {
//!     Game::new()
//!         .title("My game")
//!         .setup(|scene| {
//!             scene.spawn(
//!                 Node::new("Player")
//!                     .at(640.0, 360.0)
//!                     .with(Sprite::rect(48.0, 48.0, Color::hex(0xffb000)))
//!                     .with(Player { speed: 300.0 }),
//!             );
//!         })
//!         .run();
//! }
//! ```
//!
//! # Coordinates
//!
//! `x` grows to the right and `y` grows downwards. One unit is one pixel at
//! normal zoom. Without a [`Camera`], `(0, 0)` is the top-left corner of the
//! window.

mod app;
pub mod draw;
mod game;
mod keys;

pub use brandon_core::math::*;
pub use brandon_core::{AssetError, Assets, Color, Input, Key, MouseButton, Random, Rect, Time};
pub use brandon_macros::{behaviour, data};
pub use brandon_render::RenderError;
pub use brandon_scene::{
    register, register_as, Align, Attach, Behaviour, Camera, Ctx, Node, NodeId, Overlay, Scene,
    SceneError, SceneLibrary, Shape, Sprite, Text, Transform,
};
pub use game::{Error, Game};

// Used by the code `#[behaviour]` and `#[data]` generate.
#[doc(hidden)]
pub use serde;

/// Runs a game until its window is closed. If the game cannot start, the
/// problem is printed and the program exits.
pub fn run(game: Game) {
    game.run()
}

/// Everything most games need, in one import: `use brandon::prelude::*;`
pub mod prelude {
    pub use crate::{
        behaviour, data, register, vec2, Align, Behaviour, Camera, Color, Ctx, Game, Input, Key,
        MouseButton, Node, NodeId, Overlay, Random, Rect, Scene, Shape, Sprite, Text, Transform,
        Vec2,
    };
}
