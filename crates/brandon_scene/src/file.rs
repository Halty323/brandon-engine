use crate::registry;
use crate::{Behaviour, Camera, Node, NodeId, Overlay, Scene, Shape, Sprite, Text, Transform};
use brandon_core::{AssetError, Assets};
use serde::de::{DeserializeSeed, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

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

/// A node as it is written in a scene file.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NodeData {
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
                .filter_map(|b| registry::entry_for(b.as_ref()).map(|entry| (entry.clone)(b.as_ref())))
                .collect(),
        )
    }
}

impl Serialize for BehaviourList {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for behaviour in &self.0 {
            // Only registered behaviours get into a `BehaviourList`.
            if let Some(entry) = registry::entry_for(behaviour.as_ref()) {
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

            fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
                let mut erased = <dyn erased_serde::Deserializer>::erase(deserializer);
                (self.0.deserialize)(&mut erased)
                    .map_err(|e| serde::de::Error::custom(format!("in behaviour `{}`: {e}", self.0.name)))
            }
        }

        struct ListVisitor;

        impl<'de> Visitor<'de> for ListVisitor {
            type Value = BehaviourList;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("behaviours like { \"Player\": (speed: 200.0) }")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<BehaviourList, A::Error> {
                let mut list = Vec::new();
                while let Some(name) = map.next_key::<String>()? {
                    let Some(entry) = registry::entry_by_name(&name) else {
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
                    list.push(map.next_value_seed(Seed(entry))?);
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
            .map_err(|e| SceneError::Parse { path: path.to_string(), message: format!("line {}: {}", e.position, e.code) })
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
    cache: Mutex<HashMap<String, Arc<NodeData>>>,
    reported: Mutex<HashSet<String>>,
}

impl SceneLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets every loaded file, so they are read again when next used.
    pub fn clear(&self) {
        self.cache.lock().unwrap().clear();
        self.reported.lock().unwrap().clear();
    }

    /// Loads a scene file as the scene being run or edited.
    pub fn load(&self, assets: &Assets, path: &str) -> Result<Node, SceneError> {
        let mut chain = vec![path.to_string()];
        let data = self.data(assets, path)?;
        self.build(assets, &data, &mut chain)
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
            if self.reported.lock().unwrap().insert(path.to_string()) {
                log::error!("{error}");
            }
            Node::new(format!("<missing {path}>"))
        })
    }

    fn data(&self, assets: &Assets, path: &str) -> Result<Arc<NodeData>, SceneError> {
        if let Some(data) = self.cache.lock().unwrap().get(path) {
            return Ok(data.clone());
        }
        let text = assets.read_text(path)?;
        let data = Arc::new(NodeData::parse(path, &text)?);
        self.cache.lock().unwrap().insert(path.to_string(), data.clone());
        Ok(data)
    }

    fn build(&self, assets: &Assets, data: &NodeData, chain: &mut Vec<String>) -> Result<Node, SceneError> {
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
                    return Err(SceneError::Cycle { path: path.clone(), chain: full });
                }
                chain.push(path.clone());
                let placed = self.data(assets, path)?;
                let mut base = self.build(assets, &placed, chain)?;
                chain.pop();
                base.instance = Some(path.clone());
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
            node.pending_children.push(self.build(assets, child, chain)?);
        }
        Ok(node)
    }
}

impl Scene {
    /// Writes a node and everything under it as scene-file text.
    ///
    /// Nodes that were placed from another scene file are written as a
    /// reference to that file, with their name, transform and visibility.
    /// Behaviours that were not [registered](crate::register) are left out,
    /// with a warning.
    pub fn to_ron(&self, id: NodeId) -> Result<String, SceneError> {
        let data = self.to_data(id, true).ok_or(SceneError::MissingNode)?;
        let pretty = ron::ser::PrettyConfig::new().struct_names(false).indentor("    ".to_string());
        ron_options().to_string_pretty(&data, pretty).map_err(|e| SceneError::Write(e.to_string()))
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
                    ..Default::default()
                });
            }
        }
        let mut behaviours = Vec::new();
        for behaviour in node.behaviours.iter().filter_map(|slot| slot.get()) {
            match registry::entry_for(behaviour) {
                Some(entry) => behaviours.push((entry.clone)(behaviour)),
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
            children: self.children(id).iter().filter_map(|c| self.to_data(*c, false)).collect(),
        })
    }
}
