use crate::registry;
use crate::{Behaviour, Camera, Node, NodeId, Overlay, Scene, Shape, Sprite, Text, Transform};
use brandon_core::{AssetError, Assets};
use serde::de::{DeserializeSeed, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

/// Something went wrong loading or saving a scene.
#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    #[error(transparent)]
    Asset(#[from] AssetError),
    #[error("scene `{path}` has a mistake at {message}")]
    Parse { path: String, message: String },
    #[error("scene `{path}` places itself inside itself (through {})", chain.join(" -> "))]
    Cycle { path: String, chain: Vec<String> },
    #[error("could not write the scene: {0}")]
    Write(String),
    #[error("that node no longer exists")]
    MissingNode,
}

fn ron_options() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

fn is_true(v: &bool) -> bool {
    *v
}

/// A behaviour whose Rust type is not known to this program.
///
/// It does nothing when the scene runs. It exists so tools such as the
/// editor can open, change and save scenes without the game's code. See
/// [`keep_unknown_behaviours`](crate::keep_unknown_behaviours).
#[derive(Clone, Debug, PartialEq)]
pub struct RawBehaviour {
    /// The name the behaviour has in scene files.
    pub name: String,
    /// The behaviour's data as scene-file text, for example `(speed: 200.0)`.
    pub ron: String,
}

impl RawBehaviour {
    pub fn new(name: impl Into<String>, ron: impl Into<String>) -> Self {
        RawBehaviour {
            name: name.into(),
            ron: ron.into(),
        }
    }
}

impl Behaviour for RawBehaviour {}

fn as_raw(behaviour: &dyn Behaviour) -> Option<&RawBehaviour> {
    (behaviour as &dyn std::any::Any).downcast_ref::<RawBehaviour>()
}

/// Copies a behaviour, if it is registered or raw.
fn clone_behaviour(behaviour: &dyn Behaviour) -> Option<Box<dyn Behaviour>> {
    match as_raw(behaviour) {
        Some(raw) => Some(Box::new(raw.clone())),
        None => registry::entry_for(behaviour).map(|entry| (entry.clone)(behaviour)),
    }
}

/// A node as it is written in a scene file.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct NodeData {
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    /// Another scene file to place here.
    #[serde(skip_serializing_if = "Option::is_none")]
    instance: Option<String>,
    #[serde(skip_serializing_if = "Transform::is_identity")]
    transform: Transform,
    #[serde(skip_serializing_if = "is_true")]
    visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    sprite: Option<Sprite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<Text>,
    #[serde(skip_serializing_if = "Option::is_none")]
    camera: Option<Camera>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shape: Option<Shape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    overlay: Option<Overlay>,
    #[serde(skip_serializing_if = "BehaviourList::is_empty")]
    behaviours: BehaviourList,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<NodeData>,
}

impl Default for NodeData {
    fn default() -> Self {
        NodeData {
            name: String::new(),
            instance: None,
            transform: Transform::IDENTITY,
            visible: true,
            sprite: None,
            text: None,
            camera: None,
            shape: None,
            overlay: None,
            behaviours: BehaviourList::default(),
            children: Vec::new(),
        }
    }
}

/// Behaviours in a scene file: a map from registered name to the
/// behaviour's own data, such as `{ "Player": (speed: 200.0) }`.
#[derive(Default)]
struct BehaviourList(Vec<Box<dyn Behaviour>>);

impl BehaviourList {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Clone for BehaviourList {
    fn clone(&self) -> Self {
        BehaviourList(
            self.0
                .iter()
                .filter_map(|b| clone_behaviour(b.as_ref()))
                .collect(),
        )
    }
}

impl Serialize for BehaviourList {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for behaviour in &self.0 {
            // Only registered and raw behaviours get into a `BehaviourList`.
            if let Some(raw) = as_raw(behaviour.as_ref()) {
                let value = ron::value::RawValue::from_ron(&raw.ron).map_err(|e| {
                    serde::ser::Error::custom(format!(
                        "the data of behaviour `{}` is not valid: {}",
                        raw.name, e.code
                    ))
                })?;
                map.serialize_entry(&raw.name, value)?;
            } else if let Some(entry) = registry::entry_for(behaviour.as_ref()) {
                map.serialize_entry(&entry.name, (entry.serialize)(behaviour.as_ref()))?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for BehaviourList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Seed(Arc<registry::Entry>);

        impl<'de> DeserializeSeed<'de> for Seed {
            type Value = Box<dyn Behaviour>;

            fn deserialize<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                let mut erased = <dyn erased_serde::Deserializer>::erase(deserializer);
                (self.0.deserialize)(&mut erased).map_err(|e| {
                    serde::de::Error::custom(format!("in behaviour `{}`: {e}", self.0.name))
                })
            }
        }

        struct ListVisitor;

        impl<'de> Visitor<'de> for ListVisitor {
            type Value = BehaviourList;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("behaviours like { \"Player\": (speed: 200.0) }")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<BehaviourList, A::Error> {
                let mut list: Vec<Box<dyn Behaviour>> = Vec::new();
                while let Some(name) = map.next_key::<String>()? {
                    let Some(entry) = registry::entry_by_name(&name) else {
                        if registry::keeps_unknown() {
                            let value: Box<ron::value::RawValue> = map.next_value()?;
                            list.push(Box::new(RawBehaviour {
                                name,
                                ron: value.get_ron().trim().to_string(),
                            }));
                            continue;
                        }
                        let known = registry::known_names();
                        let hint = if known.is_empty() {
                            "no behaviours are registered".to_string()
                        } else {
                            format!("registered behaviours: {}", known.join(", "))
                        };
                        return Err(serde::de::Error::custom(format!(
                            "unknown behaviour `{name}` ({hint}). Register it with `brandon::register::<{name}>()` before loading scenes"
                        )));
                    };
                    let behaviour: Box<dyn Behaviour> = map.next_value_seed(Seed(entry))?;
                    list.push(behaviour);
                }
                Ok(BehaviourList(list))
            }
        }

        deserializer.deserialize_map(ListVisitor)
    }
}

impl NodeData {
    fn parse(path: &str, text: &str) -> Result<NodeData, SceneError> {
        ron_options()
            .from_str::<NodeData>(text)
            .map_err(|e| SceneError::Parse {
                path: path.to_string(),
                message: format!("line {}: {}", e.position, e.code),
            })
    }

    /// Builds a node without loading the scene files it refers to.
    fn into_node(self) -> Node {
        let mut node = Node::new(self.name);
        node.instance = self.instance;
        node.transform = self.transform;
        node.visible = self.visible;
        node.sprite = self.sprite;
        node.text = self.text;
        node.camera = self.camera;
        node.shape = self.shape;
        node.overlay = self.overlay;
        for behaviour in self.behaviours.0 {
            node.push_behaviour(behaviour);
        }
        node.pending_children = self.children.into_iter().map(NodeData::into_node).collect();
        node
    }
}

impl Node {
    /// Reads a node tree from scene-file text.
    ///
    /// Places where the text refers to another scene file are left empty. To
    /// load scene files with everything they refer to, use a [`SceneLibrary`].
    pub fn from_ron(text: &str) -> Result<Node, SceneError> {
        Ok(NodeData::parse("<text>", text)?.into_node())
    }
}

/// Loads scene files and remembers them, so spawning the same scene many
/// times reads the file only once.
#[derive(Default)]
pub struct SceneLibrary {
    cache: RefCell<HashMap<String, Rc<NodeData>>>,
    reported: RefCell<HashSet<String>>,
}

impl SceneLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets every loaded file, so they are read again when next used.
    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
        self.reported.borrow_mut().clear();
    }

    /// Loads a scene file as the scene being run or edited.
    pub fn load(&self, assets: &Assets, path: &str) -> Result<Node, SceneError> {
        let mut chain = vec![path.to_string()];
        let data = self.data(assets, path)?;
        self.build(assets, &data, &mut chain)
    }

    /// Reads scene-file text as the scene being run or edited, loading the
    /// scene files it places.
    pub fn load_text(&self, assets: &Assets, text: &str) -> Result<Node, SceneError> {
        let data = NodeData::parse("<text>", text)?;
        self.build(assets, &data, &mut Vec::new())
    }

    /// Creates a copy of a scene file to place inside another scene. The
    /// returned node remembers which file it came from.
    pub fn instantiate(&self, assets: &Assets, path: &str) -> Result<Node, SceneError> {
        let mut node = self.load(assets, path)?;
        node.instance = Some(path.to_string());
        Ok(node)
    }

    /// Like [`instantiate`](Self::instantiate), but on failure logs the
    /// problem (once per file) and returns an empty stand-in node.
    pub fn instantiate_or_placeholder(&self, assets: &Assets, path: &str) -> Node {
        self.instantiate(assets, path).unwrap_or_else(|error| {
            if self.reported.borrow_mut().insert(path.to_string()) {
                log::error!("{error}");
            }
            Node::new(format!("<missing {path}>"))
        })
    }

    fn data(&self, assets: &Assets, path: &str) -> Result<Rc<NodeData>, SceneError> {
        if let Some(data) = self.cache.borrow().get(path) {
            return Ok(data.clone());
        }
        let text = assets.read_text(path)?;
        let data = Rc::new(NodeData::parse(path, &text)?);
        self.cache
            .borrow_mut()
            .insert(path.to_string(), data.clone());
        Ok(data)
    }

    fn build(
        &self,
        assets: &Assets,
        data: &NodeData,
        chain: &mut Vec<String>,
    ) -> Result<Node, SceneError> {
        let mut node = match &data.instance {
            None => {
                let mut node = Node::new(data.name.clone());
                node.visible = data.visible;
                node
            }
            Some(path) => {
                if chain.contains(path) {
                    let mut full = chain.clone();
                    full.push(path.clone());
                    return Err(SceneError::Cycle {
                        path: path.clone(),
                        chain: full,
                    });
                }
                chain.push(path.clone());
                let placed = self.data(assets, path)?;
                let mut base = self.build(assets, &placed, chain)?;
                chain.pop();
                base.instance = Some(path.clone());
                base.placed_as = Some(Box::new(data.clone()));
                if !data.name.is_empty() {
                    base.name = data.name.clone();
                }
                base.visible &= data.visible;
                base
            }
        };
        // What the file says on this node wins over what the placed scene had.
        node.transform = data.transform;
        if data.sprite.is_some() {
            node.sprite = data.sprite.clone();
        }
        if data.text.is_some() {
            node.text = data.text.clone();
        }
        if data.camera.is_some() {
            node.camera = data.camera;
        }
        if data.shape.is_some() {
            node.shape = data.shape;
        }
        if data.overlay.is_some() {
            node.overlay = data.overlay;
        }
        for behaviour in data.behaviours.clone().0 {
            node.push_behaviour(behaviour);
        }
        for child in &data.children {
            node.pending_children
                .push(self.build(assets, child, chain)?);
        }
        Ok(node)
    }
}

impl Scene {
    /// Writes a node and everything under it as scene-file text.
    ///
    /// Nodes that were placed from another scene file are written as a
    /// reference to that file, with their current name, transform and
    /// visibility, plus whatever else the file that placed them said.
    /// Behaviours that were not [registered](crate::register) are left out,
    /// with a warning.
    pub fn to_ron(&self, id: NodeId) -> Result<String, SceneError> {
        self.write(id, true)
    }

    /// Like [`to_ron`](Self::to_ron), but if the node itself was placed from
    /// another scene file it is written as a reference too. This is the text
    /// to use for copying a node within a scene.
    pub fn copy_to_ron(&self, id: NodeId) -> Result<String, SceneError> {
        self.write(id, false)
    }

    fn write(&self, id: NodeId, expand_root: bool) -> Result<String, SceneError> {
        let data = self
            .to_data(id, expand_root)
            .ok_or(SceneError::MissingNode)?;
        let pretty = ron::ser::PrettyConfig::new()
            .struct_names(false)
            .indentor("    ".to_string());
        ron_options()
            .to_string_pretty(&data, pretty)
            .map(|text| crate::layout::tidy(&text))
            .map_err(|e| SceneError::Write(e.to_string()))
    }

    fn to_data(&self, id: NodeId, is_root: bool) -> Option<NodeData> {
        let node = self.get(id)?;
        if !is_root {
            if let Some(path) = &node.instance {
                return Some(NodeData {
                    name: node.name.clone(),
                    instance: Some(path.clone()),
                    transform: node.transform,
                    visible: node.visible,
                    ..node.placed_as.as_deref().cloned().unwrap_or_default()
                });
            }
        }
        let mut behaviours = Vec::new();
        for behaviour in node.behaviours.iter().filter_map(|slot| slot.get()) {
            match clone_behaviour(behaviour) {
                Some(copy) => behaviours.push(copy),
                None => log::warn!(
                    "a behaviour on `{}` is not registered, so it was not saved. Register it with `brandon::register`.",
                    node.name
                ),
            }
        }
        Some(NodeData {
            name: node.name.clone(),
            instance: None,
            transform: node.transform,
            visible: node.visible,
            sprite: node.sprite.clone(),
            text: node.text.clone(),
            camera: node.camera,
            shape: node.shape,
            overlay: node.overlay,
            behaviours: BehaviourList(behaviours),
            children: self
                .children(id)
                .iter()
                .filter_map(|c| self.to_data(*c, false))
                .collect(),
        })
    }
}
