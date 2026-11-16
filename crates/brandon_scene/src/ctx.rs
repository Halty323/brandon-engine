use crate::{Node, NodeId, Scene, SceneError, SceneLibrary};
use brandon_core::{Assets, Input, Random, Time};
use glam::{Affine2, Vec2};

/// What the window currently shows.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Window size in pixels.
    pub size: Vec2,
    /// Transform from world space to window pixels.
    pub screen_from_world: Affine2,
}

impl Default for View {
    fn default() -> Self {
        View {
            size: Vec2::new(1280.0, 720.0),
            screen_from_world: Affine2::IDENTITY,
        }
    }
}

/// Things behaviours asked the engine to do after the current update.
#[derive(Debug, Default)]
pub struct Requests {
    pub quit: bool,
    pub change_scene: Option<String>,
}

/// Everything a behaviour can reach while it runs.
pub struct Ctx<'a> {
    /// The scene tree. The node being updated is lent to the behaviour as its
    /// `node` argument, so `scene.get(node.id())` returns `None` meanwhile.
    pub scene: &'a mut Scene,
    /// Keyboard and mouse.
    pub input: &'a Input,
    pub time: &'a Time,
    /// Random numbers for gameplay.
    pub random: &'a mut Random,
    pub assets: &'a Assets,
    pub library: &'a SceneLibrary,
    pub view: &'a View,
    pub(crate) requests: &'a mut Requests,
}

impl Ctx<'_> {
    /// Seconds since the previous frame. Multiply speeds by this.
    pub fn dt(&self) -> f32 {
        self.time.delta()
    }

    /// Seconds since the game started.
    pub fn elapsed(&self) -> f32 {
        self.time.elapsed()
    }

    /// Window size in pixels.
    pub fn screen_size(&self) -> Vec2 {
        self.view.size
    }

    /// Converts a window position in pixels to a world position.
    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        self.view
            .screen_from_world
            .inverse()
            .transform_point2(screen)
    }

    /// Converts a world position to a window position in pixels.
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        self.view.screen_from_world.transform_point2(world)
    }

    /// The mouse position in world space.
    pub fn mouse_world(&self) -> Vec2 {
        self.screen_to_world(self.input.mouse_position())
    }

    /// Adds a node at the top level of the scene.
    pub fn spawn(&mut self, node: Node) -> NodeId {
        self.scene.spawn(node)
    }

    /// Adds a node under `parent`.
    pub fn spawn_child(&mut self, parent: NodeId, node: Node) -> NodeId {
        self.scene.spawn_child(parent, node)
    }

    /// Creates a fresh copy of a scene file, ready to adjust and spawn:
    ///
    /// ```ignore
    /// ctx.spawn(ctx.instantiate("scenes/rock.scene").at(x, y));
    /// ```
    ///
    /// If the file cannot be loaded, the problem is logged and an empty node
    /// is returned, so a missing file does not crash the game.
    pub fn instantiate(&self, path: &str) -> Node {
        self.library.instantiate_or_placeholder(self.assets, path)
    }

    /// Like [`instantiate`](Self::instantiate), but returns the error.
    pub fn try_instantiate(&self, path: &str) -> Result<Node, SceneError> {
        self.library.instantiate(self.assets, path)
    }

    /// Removes a node and everything under it. A behaviour may remove its own
    /// node.
    pub fn destroy(&mut self, id: NodeId) {
        self.scene.remove(id);
    }

    /// Replaces the whole scene with a scene file once this frame's update is
    /// done.
    pub fn change_scene(&mut self, path: impl Into<String>) {
        self.requests.change_scene = Some(path.into());
    }

    /// Closes the game once this frame is done.
    pub fn quit(&mut self) {
        self.requests.quit = true;
    }
}

impl Scene {
    /// Runs one frame of every behaviour, parents before their children.
    ///
    /// Nodes spawned during the update start running on the next frame.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        input: &Input,
        time: &Time,
        random: &mut Random,
        assets: &Assets,
        library: &SceneLibrary,
        view: &View,
        requests: &mut Requests,
    ) {
        self.refresh_transforms();
        for id in self.ids() {
            if !self.has_behaviours(id) {
                continue;
            }
            let Some(mut node) = self.take(id) else {
                continue;
            };
            let count = node.behaviours.len();
            for index in 0..count {
                let Some(mut behaviour) = node.behaviours[index].behaviour.take() else {
                    continue;
                };
                let mut ctx = Ctx {
                    scene: self,
                    input,
                    time,
                    random,
                    assets,
                    library,
                    view,
                    requests,
                };
                if !node.behaviours[index].ready {
                    node.behaviours[index].ready = true;
                    behaviour.ready(&mut node, &mut ctx);
                }
                if ctx.scene.contains(id) {
                    behaviour.update(&mut node, &mut ctx);
                }
                let slot = &mut node.behaviours[index];
                if !slot.removed {
                    slot.behaviour = Some(behaviour);
                }
                if !self.contains(id) {
                    break;
                }
            }
            node.behaviours.retain(|slot| !slot.removed);
            self.put_back(node);
        }
    }
}
