use crate::{Behaviour, Node};
use glam::{Affine2, Vec2};

/// Identifies a node in a [`Scene`].
///
/// Ids stay valid until the node is removed. An id of a removed node is never
/// reused for another node.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

impl NodeId {
    /// The id of no node.
    pub const NONE: NodeId = NodeId {
        index: u32::MAX,
        generation: 0,
    };

    pub fn is_none(self) -> bool {
        self == Self::NONE
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::NONE
    }
}

impl std::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_none() {
            f.write_str("NodeId(none)")
        } else {
            write!(f, "NodeId({}v{})", self.index, self.generation)
        }
    }
}

// Nodes are stored inline on purpose: one allocation for the whole tree.
#[allow(clippy::large_enum_variant)]
enum Content {
    Free,
    Here(Node),
    /// The node is lent out to its own behaviours while they run.
    Taken,
}

struct Slot {
    generation: u32,
    content: Content,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    world: Affine2,
    z: i32,
    overlay: bool,
    shown: bool,
}

/// Where a visible node ends up on screen. See [`Scene::for_each_visible`].
#[derive(Clone, Copy, Debug)]
pub struct Visible {
    /// The node's transform in world space (or window space for overlays).
    pub world: Affine2,
    /// The node's draw order, including its ancestors'.
    pub z: i32,
    /// Is the node drawn fixed to the window?
    pub overlay: bool,
}

/// A tree of nodes.
#[derive(Default)]
pub struct Scene {
    slots: Vec<Slot>,
    free: Vec<u32>,
    roots: Vec<NodeId>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    // --- Building the tree ----------------------------------------------

    /// Adds a node (and the children it was built with) at the top level.
    pub fn spawn(&mut self, node: Node) -> NodeId {
        self.insert(None, node)
    }

    /// Adds a node (and the children it was built with) under `parent`.
    ///
    /// If `parent` no longer exists the node is dropped and
    /// [`NodeId::NONE`] is returned.
    pub fn spawn_child(&mut self, parent: NodeId, node: Node) -> NodeId {
        if !self.contains(parent) {
            log::warn!(
                "spawn_child: the parent of `{}` no longer exists, so it was not added",
                node.name
            );
            return NodeId::NONE;
        }
        self.insert(Some(parent), node)
    }

    fn insert(&mut self, parent: Option<NodeId>, mut node: Node) -> NodeId {
        let pending = std::mem::take(&mut node.pending_children);
        let (parent_world, parent_z, parent_overlay, parent_shown) = match parent {
            Some(p) => {
                let s = &self.slots[p.index as usize];
                (s.world, s.z, s.overlay, s.shown)
            }
            None => (Affine2::IDENTITY, 0, false, true),
        };
        let world = parent_world * node.transform.to_affine();
        let z = parent_z + node.transform.z;
        let overlay = parent_overlay || node.overlay.is_some();
        let shown = parent_shown && node.visible;

        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                self.slots.push(Slot {
                    generation: 0,
                    content: Content::Free,
                    parent: None,
                    children: Vec::new(),
                    world: Affine2::IDENTITY,
                    z: 0,
                    overlay: false,
                    shown: true,
                });
                (self.slots.len() - 1) as u32
            }
        };
        let slot = &mut self.slots[index as usize];
        let id = NodeId {
            index,
            generation: slot.generation,
        };
        node.id = id;
        slot.content = Content::Here(node);
        slot.parent = parent;
        slot.world = world;
        slot.z = z;
        slot.overlay = overlay;
        slot.shown = shown;

        match parent {
            Some(p) => self.slots[p.index as usize].children.push(id),
            None => self.roots.push(id),
        }
        for child in pending {
            self.insert(Some(id), child);
        }
        id
    }

    /// Removes a node and everything under it. Returns whether it existed.
    pub fn remove(&mut self, id: NodeId) -> bool {
        if !self.contains(id) {
            return false;
        }
        match self.slots[id.index as usize].parent {
            Some(p) => self.slots[p.index as usize].children.retain(|c| *c != id),
            None => self.roots.retain(|c| *c != id),
        }
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let slot = &mut self.slots[id.index as usize];
            stack.append(&mut slot.children);
            slot.content = Content::Free;
            slot.parent = None;
            slot.generation = slot.generation.wrapping_add(1);
            self.free.push(id.index);
        }
        true
    }

    /// Removes every node.
    pub fn clear(&mut self) {
        for root in std::mem::take(&mut self.roots) {
            // `remove` looks the root up in `self.roots`, which is now empty,
            // and that is fine.
            self.remove(root);
        }
    }

    /// Moves a node under a new parent, or to the top level with `None`.
    /// The node keeps its local transform. Returns `false` if that is not
    /// possible, for example because it would make a node its own ancestor.
    pub fn set_parent(&mut self, id: NodeId, new_parent: Option<NodeId>) -> bool {
        if !self.contains(id) {
            return false;
        }
        if let Some(p) = new_parent {
            if !self.contains(p) {
                return false;
            }
            let mut cursor = Some(p);
            while let Some(c) = cursor {
                if c == id {
                    return false;
                }
                cursor = self.parent(c);
            }
        }
        match self.slots[id.index as usize].parent {
            Some(p) => self.slots[p.index as usize].children.retain(|c| *c != id),
            None => self.roots.retain(|c| *c != id),
        }
        self.slots[id.index as usize].parent = new_parent;
        match new_parent {
            Some(p) => self.slots[p.index as usize].children.push(id),
            None => self.roots.push(id),
        }
        true
    }

    /// Moves a node to position `index` among the children of `new_parent`
    /// (or among the top-level nodes with `None`). An index past the end
    /// puts it last. The node keeps its local transform. Returns `false` if
    /// that is not possible, for example because it would make a node its
    /// own ancestor.
    pub fn move_to(&mut self, id: NodeId, new_parent: Option<NodeId>, index: usize) -> bool {
        if !self.set_parent(id, new_parent) {
            return false;
        }
        let siblings = match new_parent {
            Some(p) => &mut self.slots[p.index as usize].children,
            None => &mut self.roots,
        };
        siblings.retain(|c| *c != id);
        siblings.insert(index.min(siblings.len()), id);
        true
    }

    // --- Looking things up ----------------------------------------------

    fn slot(&self, id: NodeId) -> Option<&Slot> {
        self.slots
            .get(id.index as usize)
            .filter(|s| s.generation == id.generation && !matches!(s.content, Content::Free))
    }

    /// Does this node still exist?
    pub fn contains(&self, id: NodeId) -> bool {
        self.slot(id).is_some()
    }

    /// Number of nodes in the scene.
    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The node with this id.
    ///
    /// Returns `None` if the node was removed. It also returns `None` for the
    /// node whose behaviour is running right now: use the `node` argument of
    /// the behaviour instead.
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        match &self.slot(id)?.content {
            Content::Here(node) => Some(node),
            _ => None,
        }
    }

    /// The node with this id, mutably. See [`get`](Self::get).
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        let slot = self
            .slots
            .get_mut(id.index as usize)
            .filter(|s| s.generation == id.generation)?;
        match &mut slot.content {
            Content::Here(node) => Some(node),
            _ => None,
        }
    }

    /// The parent of a node, or `None` for top-level nodes.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.slot(id)?.parent
    }

    /// The children of a node, in order.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.slot(id).map(|s| s.children.as_slice()).unwrap_or(&[])
    }

    /// The top-level nodes, in order.
    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    /// Every node id, parents before their children.
    pub fn ids(&self) -> Vec<NodeId> {
        let mut out = Vec::with_capacity(self.len());
        let mut stack: Vec<NodeId> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            out.push(id);
            stack.extend(self.slots[id.index as usize].children.iter().rev());
        }
        out
    }

    /// The first node with this name, searching the whole tree.
    pub fn find(&self, name: &str) -> Option<NodeId> {
        self.ids()
            .into_iter()
            .find(|id| self.get(*id).is_some_and(|n| n.name == name))
    }

    /// The first node with this name among the descendants of `ancestor`.
    pub fn find_under(&self, ancestor: NodeId, name: &str) -> Option<NodeId> {
        let mut stack: Vec<NodeId> = self.children(ancestor).iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            if self.get(id).is_some_and(|n| n.name == name) {
                return Some(id);
            }
            stack.extend(self.children(id).iter().rev());
        }
        None
    }

    /// The first node that has a behaviour of type `T`.
    pub fn first_with<T: Behaviour>(&self) -> Option<NodeId> {
        self.ids()
            .into_iter()
            .find(|id| self.get(*id).is_some_and(|n| n.has::<T>()))
    }

    /// The behaviour of type `T` on a node.
    pub fn behaviour<T: Behaviour>(&self, id: NodeId) -> Option<&T> {
        self.get(id)?.behaviour::<T>()
    }

    /// The behaviour of type `T` on a node, mutably.
    pub fn behaviour_mut<T: Behaviour>(&mut self, id: NodeId) -> Option<&mut T> {
        self.get_mut(id)?.behaviour_mut::<T>()
    }

    // --- Transforms -----------------------------------------------------

    /// A node's transform in world space, as of the start of the frame (or
    /// the last [`refresh_transforms`](Self::refresh_transforms)).
    pub fn world_transform(&self, id: NodeId) -> Affine2 {
        self.slot(id).map(|s| s.world).unwrap_or(Affine2::IDENTITY)
    }

    /// A node's position in world space. See [`world_transform`](Self::world_transform).
    pub fn world_position(&self, id: NodeId) -> Vec2 {
        self.world_transform(id).translation
    }

    /// The world transform of a node you hold, using its current local
    /// transform. Works for the node passed to a running behaviour.
    pub fn world_transform_of(&self, node: &Node) -> Affine2 {
        let parent_world = self
            .parent(node.id)
            .map(|p| self.world_transform(p))
            .unwrap_or(Affine2::IDENTITY);
        parent_world * node.transform.to_affine()
    }

    /// Recomputes every node's world transform. The engine does this before
    /// behaviours update and before drawing.
    pub fn refresh_transforms(&mut self) {
        let mut stack: Vec<(NodeId, Affine2, i32, bool, bool)> = self
            .roots
            .iter()
            .rev()
            .map(|id| (*id, Affine2::IDENTITY, 0, false, true))
            .collect();
        while let Some((id, parent_world, parent_z, parent_overlay, parent_shown)) = stack.pop() {
            let slot = &mut self.slots[id.index as usize];
            if let Content::Here(node) = &slot.content {
                slot.world = parent_world * node.transform.to_affine();
                slot.z = parent_z + node.transform.z;
                slot.overlay = parent_overlay || node.overlay.is_some();
                slot.shown = parent_shown && node.visible;
            }
            let (world, z, overlay, shown) = (slot.world, slot.z, slot.overlay, slot.shown);
            stack.extend(
                slot.children
                    .iter()
                    .rev()
                    .map(|c| (*c, world, z, overlay, shown)),
            );
        }
    }

    /// Calls `f` for every visible node, parents before their children, with
    /// where it ends up on screen. Uses the transforms from the last
    /// [`refresh_transforms`](Self::refresh_transforms).
    pub fn for_each_visible(&self, mut f: impl FnMut(&Node, Visible)) {
        let mut stack: Vec<NodeId> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let slot = &self.slots[id.index as usize];
            if !slot.shown {
                continue;
            }
            if let Content::Here(node) = &slot.content {
                f(
                    node,
                    Visible {
                        world: slot.world,
                        z: slot.z,
                        overlay: slot.overlay,
                    },
                );
            }
            stack.extend(slot.children.iter().rev());
        }
    }

    /// The transform from world space to window pixels for a window of the
    /// given size, from the first active [`Camera`](crate::Camera).
    ///
    /// Without a camera this is the identity: world `(0, 0)` is the top-left
    /// corner of the window.
    pub fn view_transform(&self, window_size: Vec2) -> Affine2 {
        let mut found = None;
        self.for_each_node(|node, world| {
            if found.is_none() {
                if let Some(camera) = node.camera.filter(|c| c.active) {
                    found = Some((world, camera.zoom));
                }
            }
        });
        let Some((world, zoom)) = found else {
            return Affine2::IDENTITY;
        };
        let (_, rotation, translation) = world.to_scale_angle_translation();
        let camera = Affine2::from_angle_translation(rotation, translation);
        Affine2::from_translation(window_size * 0.5)
            * Affine2::from_scale(Vec2::splat(zoom.max(1e-6)))
            * camera.inverse()
    }

    fn for_each_node(&self, mut f: impl FnMut(&Node, Affine2)) {
        for id in self.ids() {
            let slot = &self.slots[id.index as usize];
            if let Content::Here(node) = &slot.content {
                f(node, slot.world);
            }
        }
    }

    // --- Lending nodes to their behaviours ---------------------------------

    pub(crate) fn has_behaviours(&self, id: NodeId) -> bool {
        self.get(id).is_some_and(|n| !n.behaviours.is_empty())
    }

    pub(crate) fn take(&mut self, id: NodeId) -> Option<Node> {
        let slot = self
            .slots
            .get_mut(id.index as usize)
            .filter(|s| s.generation == id.generation)?;
        match std::mem::replace(&mut slot.content, Content::Taken) {
            Content::Here(node) => Some(node),
            other => {
                slot.content = other;
                None
            }
        }
    }

    /// Returns a node taken with [`take`](Self::take). If the node was removed
    /// in the meantime, it is dropped.
    pub(crate) fn put_back(&mut self, node: Node) {
        let id = node.id;
        if let Some(slot) = self
            .slots
            .get_mut(id.index as usize)
            .filter(|s| s.generation == id.generation)
        {
            if matches!(slot.content, Content::Taken) {
                slot.content = Content::Here(node);
            }
        }
    }
}
