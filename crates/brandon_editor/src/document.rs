//! The scene being edited: opening, saving, undo and changes to the tree.
//! Nothing here knows about windows or widgets.

use brandon_core::Assets;
use brandon_scene::{Node, NodeId, Scene, SceneError, SceneLibrary};
use std::path::{Path, PathBuf};

/// How many undo steps are kept.
const UNDO_LIMIT: usize = 200;

pub struct Document {
    pub assets: Assets,
    pub library: SceneLibrary,
    pub scene: Scene,
    pub root: NodeId,
    /// Asset path of the open scene file, if it has one yet.
    pub path: Option<String>,
    pub selection: Option<NodeId>,
    /// The scene as text, as of the last [`commit`](Self::commit).
    current: String,
    saved: String,
    undo: Vec<String>,
    redo: Vec<String>,
}

impl Document {
    /// A new, unsaved scene with just a root node.
    pub fn new(assets: Assets) -> Self {
        let mut document = Document {
            assets,
            library: SceneLibrary::new(),
            scene: Scene::new(),
            root: NodeId::NONE,
            path: None,
            selection: None,
            current: String::new(),
            saved: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
        };
        document.root = document.scene.spawn(Node::new("Root"));
        document.current = document.text();
        document.saved = document.current.clone();
        document
    }

    fn text(&self) -> String {
        self.scene.to_ron(self.root).unwrap_or_default()
    }

    /// Opens a scene file, replacing what was being edited.
    pub fn open(&mut self, path: &str) -> Result<(), SceneError> {
        // Read files fresh: they may have changed since they were last used.
        self.library.clear();
        let root = self.library.load(&self.assets, path)?;
        self.scene.clear();
        self.root = self.scene.spawn(root);
        self.path = Some(path.to_string());
        self.selection = None;
        self.current = self.text();
        self.saved = self.current.clone();
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }

    /// The folder scene and image files live in.
    pub fn assets_folder(&self) -> Option<&Path> {
        self.assets.roots().iter().find(|root| root.is_dir()).or(self.assets.roots().first()).map(PathBuf::as_path)
    }

    /// Writes the scene to its file. Returns where it was written.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        let path = self.path.clone().ok_or("this scene has no file name yet")?;
        let text = self.scene.to_ron(self.root).map_err(|e| e.to_string())?;
        let folder = self.assets_folder().ok_or("there is no assets folder to save into")?;
        let file = folder.join(&path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        std::fs::write(&file, format!("{text}\n")).map_err(|e| format!("could not write {}: {e}", file.display()))?;
        self.current = text;
        self.saved = self.current.clone();
        Ok(file)
    }

    /// Does the scene differ from what is in its file?
    pub fn is_dirty(&self) -> bool {
        self.current != self.saved
    }

    /// Records the scene's current state as an undo step, if it changed
    /// since the last call. Returns whether it changed.
    pub fn commit(&mut self) -> bool {
        let Ok(text) = self.scene.to_ron(self.root) else { return false };
        if text == self.current {
            return false;
        }
        self.undo.push(std::mem::replace(&mut self.current, text));
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        self.commit();
        let Some(text) = self.undo.pop() else { return false };
        let now = std::mem::replace(&mut self.current, text);
        self.redo.push(now);
        self.restore();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(text) = self.redo.pop() else { return false };
        let now = std::mem::replace(&mut self.current, text);
        self.undo.push(now);
        self.restore();
        true
    }

    /// Rebuilds the scene from `self.current`, keeping the selection where
    /// it was in the tree.
    fn restore(&mut self) {
        let selected = self.selection.map(|id| self.tree_path(id));
        match self.library.load_text(&self.assets, &self.current) {
            Ok(root) => {
                self.scene.clear();
                self.root = self.scene.spawn(root);
                self.selection = selected.and_then(|path| self.node_at(&path));
            }
            Err(error) => log::error!("could not restore the scene: {error}"),
        }
    }

    /// Where a node is in the tree: the child index to follow at each level,
    /// starting from the root.
    pub fn tree_path(&self, id: NodeId) -> Vec<usize> {
        let mut path = Vec::new();
        let mut cursor = id;
        while let Some(parent) = self.scene.parent(cursor) {
            path.push(self.scene.children(parent).iter().position(|c| *c == cursor).unwrap_or(0));
            cursor = parent;
        }
        path.reverse();
        path
    }

    /// The node at a [`tree_path`](Self::tree_path), if there still is one.
    pub fn node_at(&self, path: &[usize]) -> Option<NodeId> {
        let mut cursor = self.root;
        for index in path {
            cursor = *self.scene.children(cursor).get(*index)?;
        }
        Some(cursor)
    }

    /// Is this node, or one of its ancestors below the root, placed from
    /// another scene file? Such nodes' insides belong to that other file.
    pub fn inside_placed_scene(&self, id: NodeId) -> bool {
        let mut cursor = self.scene.parent(id);
        while let Some(ancestor) = cursor {
            if ancestor != self.root && self.scene.get(ancestor).is_some_and(|n| n.instance_of().is_some()) {
                return true;
            }
            cursor = self.scene.parent(ancestor);
        }
        false
    }

    /// Adds a node under `parent` and selects it.
    pub fn add_child(&mut self, parent: NodeId, node: Node) -> NodeId {
        let id = self.scene.spawn_child(parent, node);
        if !id.is_none() {
            self.scene.refresh_transforms();
            self.selection = Some(id);
        }
        id
    }

    /// Places another scene file under `parent` and selects it.
    pub fn place_scene(&mut self, parent: NodeId, path: &str) -> Result<NodeId, SceneError> {
        if self.path.as_deref() == Some(path) {
            return Err(SceneError::Cycle { path: path.to_string(), chain: vec![path.to_string(), path.to_string()] });
        }
        let node = self.library.instantiate(&self.assets, path)?;
        Ok(self.add_child(parent, node))
    }

    /// Removes a node and everything under it. The root cannot be removed.
    pub fn delete(&mut self, id: NodeId) -> bool {
        if id == self.root || !self.scene.contains(id) {
            return false;
        }
        let parent = self.scene.parent(id);
        self.scene.remove(id);
        if self.selection.is_some_and(|s| !self.scene.contains(s)) {
            self.selection = parent;
        }
        true
    }

    /// Makes a copy of a node next to it and selects the copy.
    pub fn duplicate(&mut self, id: NodeId) -> Option<NodeId> {
        let parent = self.scene.parent(id)?;
        let text = self.scene.copy_to_ron(id).ok()?;
        let copy = self.library.load_text(&self.assets, &text).ok()?;
        Some(self.add_child(parent, copy))
    }

    /// Asset paths of every file under the assets folder with one of the
    /// given extensions, sorted.
    pub fn files(&self, extensions: &[&str]) -> Vec<String> {
        let mut found = Vec::new();
        if let Some(folder) = self.assets_folder() {
            collect_files(folder, folder, extensions, &mut found);
        }
        found.sort();
        found
    }
}

fn collect_files(root: &Path, folder: &Path, extensions: &[&str], found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, extensions, found);
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| extensions.contains(&e)) {
            if let Ok(relative) = path.strip_prefix(root) {
                found.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brandon_scene::{keep_unknown_behaviours, RawBehaviour, Sprite};

    fn project(name: &str, files: &[(&str, &str)]) -> Assets {
        keep_unknown_behaviours(true);
        let dir = std::env::temp_dir().join(format!("brandon-editor-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (file, text) in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        Assets::at(dir)
    }

    const MAIN: &str = r#"(name: "Main", children: [
        (name: "Ship", transform: (position: (10, 20)), behaviours: {"Ship": (speed: 460)}),
        (instance: "scenes/rock.scene", transform: (position: (5, 5))),
    ])"#;
    const ROCK: &str = r#"(name: "Rock", sprite: (texture: "rock.png"), children: [(name: "Dust")])"#;

    #[test]
    fn open_edit_undo_redo_save() {
        let assets = project("edit", &[("scenes/main.scene", MAIN), ("scenes/rock.scene", ROCK), ("rock.png", "")]);
        let mut doc = Document::new(assets);
        assert!(!doc.is_dirty() && doc.path.is_none());
        assert!(doc.save().is_err(), "a new scene has no file name");

        doc.open("scenes/main.scene").unwrap();
        assert!(!doc.is_dirty() && !doc.can_undo());
        let ship = doc.scene.find("Ship").unwrap();
        doc.selection = Some(ship);

        // A change becomes an undo step only when committed.
        doc.scene.get_mut(ship).unwrap().transform.position.x = 99.0;
        assert!(doc.commit());
        assert!(!doc.commit(), "nothing changed since");
        assert!(doc.is_dirty() && doc.can_undo());

        // Behaviours the editor has no code for are kept and can be edited.
        let ship_node = doc.scene.get_mut(ship).unwrap();
        ship_node.behaviours_of_mut::<RawBehaviour>().next().unwrap().ron = "(speed: 500)".to_string();
        doc.commit();

        assert!(doc.undo());
        let ship = doc.selection.expect("the selection follows the node through undo");
        let node = doc.scene.get(ship).unwrap();
        assert_eq!(node.name, "Ship");
        assert_eq!(node.behaviours_of::<RawBehaviour>().next().unwrap().ron, "(speed: 460)");
        assert_eq!(node.transform.position.x, 99.0);
        assert!(doc.undo());
        assert_eq!(doc.scene.get(doc.selection.unwrap()).unwrap().transform.position.x, 10.0);
        assert!(!doc.undo() && !doc.is_dirty());
        assert!(doc.redo() && doc.redo() && !doc.redo());
        assert!(doc.is_dirty());

        let file = doc.save().unwrap();
        assert!(!doc.is_dirty());
        let written = std::fs::read_to_string(file).unwrap();
        assert!(written.contains("\"Ship\": (speed: 500)") && written.contains("99.0"), "{written}");
        assert!(written.contains("instance: \"scenes/rock.scene\"") && !written.contains("Dust"), "{written}");

        // What was saved opens again and is the same scene.
        let before = doc.scene.to_ron(doc.root).unwrap();
        doc.open("scenes/main.scene").unwrap();
        assert_eq!(doc.scene.to_ron(doc.root).unwrap(), before);
    }

    #[test]
    fn tree_changes() {
        let assets = project("tree", &[("scenes/main.scene", MAIN), ("scenes/rock.scene", ROCK)]);
        let mut doc = Document::new(assets);
        doc.open("scenes/main.scene").unwrap();
        let ship = doc.scene.find("Ship").unwrap();
        let rock = doc.scene.find("Rock").unwrap();
        let dust = doc.scene.find("Dust").unwrap();
        assert!(doc.inside_placed_scene(dust) && !doc.inside_placed_scene(rock) && !doc.inside_placed_scene(ship));
        assert_eq!(doc.tree_path(dust), [1, 0]);
        assert_eq!(doc.node_at(&[1, 0]), Some(dust));
        assert_eq!(doc.node_at(&[7]), None);

        let flame = doc.add_child(ship, Node::new("Flame").with(Sprite::new("flame.png")));
        assert_eq!(doc.selection, Some(flame));
        doc.commit();

        // Copies land next to the original. A placed scene stays a reference.
        let ship_copy = doc.duplicate(ship).unwrap();
        assert_eq!(doc.scene.parent(ship_copy), Some(doc.root));
        assert_eq!(doc.scene.children(ship_copy).len(), 1);
        let rock_copy = doc.duplicate(rock).unwrap();
        assert_eq!(doc.scene.get(rock_copy).unwrap().instance_of(), Some("scenes/rock.scene"));
        assert_eq!(doc.scene.to_ron(doc.root).unwrap().matches("instance:").count(), 2);
        assert_eq!(doc.duplicate(doc.root), None);

        let placed = doc.place_scene(doc.root, "scenes/rock.scene").unwrap();
        assert_eq!(doc.scene.get(placed).unwrap().name, "Rock");
        assert!(doc.place_scene(doc.root, "scenes/main.scene").is_err(), "a scene cannot contain itself");
        assert!(doc.place_scene(doc.root, "scenes/nope.scene").is_err());

        assert!(!doc.delete(doc.root));
        doc.selection = Some(flame);
        assert!(doc.delete(ship));
        assert_eq!(doc.selection, Some(doc.root), "selection moves up when its node goes away");
        doc.commit();
        assert!(doc.undo());
        assert!(doc.scene.find("Flame").is_some());
    }

    #[test]
    fn lists_project_files() {
        let assets = project("files", &[("scenes/main.scene", MAIN), ("scenes/rock.scene", ROCK), ("art/ship.png", ""), ("a.jpg", ""), ("notes.txt", "")]);
        let doc = Document::new(assets);
        assert_eq!(doc.files(&["scene"]), ["scenes/main.scene", "scenes/rock.scene"]);
        assert_eq!(doc.files(&["png", "jpg"]), ["a.jpg", "art/ship.png"]);
    }
}
