use crate::{Ctx, Node};
use std::any::Any;

/// Your own game logic, attached to a node.
///
/// ```
/// use brandon_scene::{Behaviour, Ctx, Node};
///
/// struct Spin {
///     speed: f32,
/// }
///
/// impl Behaviour for Spin {
///     fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
///         node.transform.rotation += self.speed * ctx.dt();
///     }
/// }
/// ```
///
/// While a behaviour runs, `node` is the node it is attached to. Reach other
/// nodes through `ctx.scene`.
pub trait Behaviour: Any {
    /// Called once, just before the first `update`.
    fn ready(&mut self, _node: &mut Node, _ctx: &mut Ctx) {}

    /// Called every frame.
    fn update(&mut self, _node: &mut Node, _ctx: &mut Ctx) {}
}

pub(crate) struct BehaviourSlot {
    /// `None` while the behaviour is running.
    pub(crate) behaviour: Option<Box<dyn Behaviour>>,
    pub(crate) ready: bool,
    pub(crate) removed: bool,
}

impl BehaviourSlot {
    pub(crate) fn new(behaviour: Box<dyn Behaviour>) -> Self {
        BehaviourSlot { behaviour: Some(behaviour), ready: false, removed: false }
    }

    pub(crate) fn get(&self) -> Option<&dyn Behaviour> {
        if self.removed {
            return None;
        }
        self.behaviour.as_deref()
    }

    pub(crate) fn get_mut(&mut self) -> Option<&mut dyn Behaviour> {
        if self.removed {
            return None;
        }
        self.behaviour.as_deref_mut()
    }
}
