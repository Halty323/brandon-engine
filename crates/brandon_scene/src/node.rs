use crate::behaviour::BehaviourSlot;
use crate::{Behaviour, Camera, NodeId, Overlay, Shape, Sprite, Text, Transform};
use std::any::Any;

/// The one building block of a scene.
///
/// A node has a name, a transform and a visibility flag. Parts (the public
/// `Option` fields) and behaviours give it abilities.
///
/// ```
/// use brandon_scene::{Node, Shape, Sprite};
///
/// let player = Node::new("Player")
///     .at(100.0, 200.0)
///     .with(Sprite::new("player.png"))
///     .with(Shape::circle(16.0))
///     .child(Node::new("Shadow").with(Sprite::new("shadow.png")));
/// ```
pub struct Node {
    pub name: String,
    pub transform: Transform,
    /// A hidden node hides its children too. Hidden nodes still update.
    pub visible: bool,
    pub sprite: Option<Sprite>,
    pub text: Option<Text>,
    pub camera: Option<Camera>,
    pub shape: Option<Shape>,
    pub overlay: Option<Overlay>,
    pub(crate) behaviours: Vec<BehaviourSlot>,
    pub(crate) id: NodeId,
    /// Children added before the node was put into a scene.
    pub(crate) pending_children: Vec<Node>,
    /// The scene file this node was created from, if any.
    pub(crate) instance: Option<String>,
}

impl Node {
    /// A new, empty node.
    pub fn new(name: impl Into<String>) -> Self {
        Node {
            name: name.into(),
            transform: Transform::IDENTITY,
            visible: true,
            sprite: None,
            text: None,
            camera: None,
            shape: None,
            overlay: None,
            behaviours: Vec::new(),
            id: NodeId::NONE,
            pending_children: Vec::new(),
            instance: None,
        }
    }

    /// Attaches a part or a behaviour. Attaching a part the node already has
    /// replaces it.
    pub fn with(mut self, part: impl Attach) -> Self {
        part.attach(&mut self);
        self
    }

    /// Sets the position.
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.transform.position = glam::Vec2::new(x, y);
        self
    }

    /// Sets the rotation, in radians.
    pub fn rotated(mut self, radians: f32) -> Self {
        self.transform.rotation = radians;
        self
    }

    /// Sets the scale on both axes.
    pub fn scaled(mut self, scale: f32) -> Self {
        self.transform.scale = glam::Vec2::splat(scale);
        self
    }

    /// Sets the draw order. Higher is on top.
    pub fn z(mut self, z: i32) -> Self {
        self.transform.z = z;
        self
    }

    /// Starts hidden.
    pub fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }

    /// Adds a child. Only for nodes that are not in a scene yet. For nodes
    /// already in a scene, use [`Scene::spawn_child`](crate::Scene::spawn_child).
    pub fn child(mut self, child: Node) -> Self {
        if self.id.is_none() {
            self.pending_children.push(child);
        } else {
            log::warn!(
                "Node::child was called on `{}`, which is already in a scene. Use Scene::spawn_child instead.",
                self.name
            );
        }
        self
    }

    /// Attaches a part or a behaviour.
    pub fn attach(&mut self, part: impl Attach) {
        part.attach(self);
    }

    /// This node's id, or [`NodeId::NONE`] if it is not in a scene yet.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The scene file this node was created from, if any.
    pub fn instance_of(&self) -> Option<&str> {
        self.instance.as_deref()
    }

    /// The attached behaviour of type `T`, if there is one.
    pub fn behaviour<T: Behaviour>(&self) -> Option<&T> {
        self.behaviours
            .iter()
            .filter_map(BehaviourSlot::get)
            .find_map(|b| (b as &dyn Any).downcast_ref::<T>())
    }

    /// The attached behaviour of type `T`, if there is one.
    pub fn behaviour_mut<T: Behaviour>(&mut self) -> Option<&mut T> {
        self.behaviours
            .iter_mut()
            .filter_map(BehaviourSlot::get_mut)
            .find_map(|b| (b as &mut dyn Any).downcast_mut::<T>())
    }

    /// Is a behaviour of type `T` attached?
    ///
    /// Note that a behaviour cannot see itself this way while it is running.
    pub fn has<T: Behaviour>(&self) -> bool {
        self.behaviour::<T>().is_some()
    }

    /// Detaches the behaviour of type `T`. Returns whether there was one.
    pub fn remove_behaviour<T: Behaviour>(&mut self) -> bool {
        for slot in &mut self.behaviours {
            let matches = slot.get().is_some_and(|b| (b as &dyn Any).is::<T>());
            if matches {
                slot.removed = true;
                slot.behaviour = None;
                return true;
            }
        }
        false
    }

    /// How many behaviours are attached.
    pub fn behaviour_count(&self) -> usize {
        self.behaviours.iter().filter(|s| !s.removed).count()
    }

    pub(crate) fn push_behaviour(&mut self, behaviour: Box<dyn Behaviour>) {
        self.behaviours.push(BehaviourSlot::new(behaviour));
    }
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("name", &self.name)
            .field("id", &self.id)
            .field("transform", &self.transform)
            .field("visible", &self.visible)
            .field("sprite", &self.sprite)
            .field("text", &self.text)
            .field("camera", &self.camera)
            .field("shape", &self.shape)
            .field("overlay", &self.overlay)
            .field("behaviours", &self.behaviour_count())
            .finish()
    }
}

/// Something that can be attached to a node with [`Node::with`]: a part or a
/// behaviour.
pub trait Attach {
    fn attach(self, node: &mut Node);
}

impl Attach for Sprite {
    fn attach(self, node: &mut Node) {
        node.sprite = Some(self);
    }
}

impl Attach for Text {
    fn attach(self, node: &mut Node) {
        node.text = Some(self);
    }
}

impl Attach for Camera {
    fn attach(self, node: &mut Node) {
        node.camera = Some(self);
    }
}

impl Attach for Shape {
    fn attach(self, node: &mut Node) {
        node.shape = Some(self);
    }
}

impl Attach for Overlay {
    fn attach(self, node: &mut Node) {
        node.overlay = Some(self);
    }
}

impl<B: Behaviour> Attach for B {
    fn attach(self, node: &mut Node) {
        node.push_behaviour(Box::new(self));
    }
}
