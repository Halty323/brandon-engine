//! The Brandon scene model.
//!
//! A game is a tree of [`Node`]s. Every node has a name, a [`Transform`] and
//! children, and nothing else. What a node *does* comes from what is attached
//! to it:
//!
//! - **Parts** are built-in abilities from a short, flat list: [`Sprite`],
//!   [`Text`], [`Camera`], [`Shape`] and [`Overlay`]. A node has at most one
//!   of each.
//! - **Behaviours** are your own Rust types that implement [`Behaviour`].
//!
//! Scenes are saved as readable RON files, and one scene can be placed inside
//! another.

mod behaviour;
mod collide;
mod ctx;
mod file;
mod node;
mod parts;
mod registry;
mod scene;
mod transform;

pub use behaviour::Behaviour;
pub use ctx::{Ctx, Requests, View};
pub use file::{RawBehaviour, SceneError, SceneLibrary};
pub use node::{Attach, Node};
pub use parts::{Align, Camera, Overlay, Shape, Sprite, Text};
pub use registry::{keep_unknown_behaviours, register, register_as};
pub use scene::{NodeId, Scene, Visible};
pub use transform::Transform;
