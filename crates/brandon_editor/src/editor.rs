//! The editor window: a top bar, the scene tree, the viewport and the
//! inspector. Everything else opens on demand from the search box (Ctrl+K).

use crate::document::Document;
use crate::glass;
use crate::inspector;
use crate::theme;
use crate::viewport::{self, p, v, Drag, Gfx, ViewCamera};
use crate::widgets::{self, Icon, Tone};
use brandon_core::Color;
use brandon_scene::{Node, NodeId, RawBehaviour, Sprite, Text};
use egui::{Key, KeyboardShortcut, Modifiers, Pos2, Rect, RichText, Sense, Ui};
use glam::Vec2;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// How long a status message stays.
const STATUS_TIME: Duration = Duration::from_secs(6);
/// The height of the glass groups along the top.
const TOOLBAR_HEIGHT: f32 = 40.0;

const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const REDO_Y: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const SEARCH: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);
const SEARCH_P: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::P);
const DUPLICATE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
const PLAY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F5);

/// One thing the search box can do.
#[derive(Clone, Debug, PartialEq)]
enum Action {
    Save,
    Undo,
    Redo,
    Play,
    Stop,
    AddNode,
    Duplicate,
    Delete,
    FrameSelection,
    FrameAll,
    Open(String),
    Place(String),
    Select(NodeId),
    /// Opens the search box itself.
    Search,
}

struct Palette {
    query: String,
    selected: usize,
    /// Only show scene files, for the "Open" button.
    scenes_only: bool,
    focus: bool,
}

/// Which side of a floating panel can be dragged to resize it.
#[derive(Clone, Copy)]
enum Edge {
    Left,
    Right,
}

/// A row being dragged in the scene tree.
#[derive(Clone, Copy)]
struct TreeDrag(NodeId);

pub struct Editor {
    pub doc: Document,
    pub camera: ViewCamera,
    /// The part of the window no panel covers, last frame, in points. The
    /// camera centres on it.
    pub viewport: Rect,
    /// The whole window, which the scene fills behind the panels.
    pub canvas: Rect,
    tree_width: f32,
    inspector_width: f32,
    /// The size of the game's window, drawn as an outline in the viewport.
    pub screen: Vec2,
    project: PathBuf,
    drag: Drag,
    /// Something changed that is not an undo step yet.
    changed: bool,
    status: Option<(String, Instant, bool)>,
    palette: Option<Palette>,
    images: Vec<String>,
    scenes: Vec<String>,
    behaviour_names: Vec<String>,
    game: Option<Child>,
    frame_all: bool,
    pointer_world: Option<Vec2>,
    pub quit: bool,
}

impl Editor {
    pub fn new(doc: Document, project: PathBuf) -> Self {
        let mut editor = Editor {
            doc,
            camera: ViewCamera::default(),
            viewport: Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)),
            canvas: Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)),
            tree_width: 270.0,
            inspector_width: 320.0,
            screen: Vec2::new(1280.0, 720.0),
            project,
            drag: Drag::None,
            changed: false,
            status: None,
            palette: None,
            images: Vec::new(),
            scenes: Vec::new(),
            behaviour_names: Vec::new(),
            game: None,
            frame_all: true,
            pointer_world: None,
            quit: false,
        };
        editor.refresh_files();
        editor
    }

    /// Re-reads the lists of scenes, images and behaviour names.
    fn refresh_files(&mut self) {
        self.scenes = self.doc.files(&["scene"]);
        self.images = self.doc.files(&["png", "jpg", "jpeg"]);
        let mut names = Vec::new();
        for scene in &self.scenes {
            let Ok(text) = self.doc.assets.read_text(scene) else {
                continue;
            };
            let Ok(node) = Node::from_ron(&text) else {
                continue;
            };
            let mut tree = brandon_scene::Scene::new();
            tree.spawn(node);
            for id in tree.ids() {
                if let Some(node) = tree.get(id) {
                    names.extend(node.behaviours_of::<RawBehaviour>().map(|b| b.name.clone()));
                }
            }
        }
        names.sort();
        names.dedup();
        self.behaviour_names = names;
    }

    fn say(&mut self, text: impl Into<String>) {
        self.status = Some((text.into(), Instant::now(), false));
    }

    fn warn(&mut self, text: impl Into<String>) {
        self.status = Some((text.into(), Instant::now(), true));
    }

    pub fn view(&self) -> glam::Affine2 {
        self.camera.view(self.viewport)
    }

    pub fn title(&self) -> String {
        let name = self.doc.path.as_deref().unwrap_or("Untitled");
        let dirty = if self.doc.is_dirty() { " •" } else { "" };
        format!("{name}{dirty} — Brandon")
    }

    /// Does the game need watching (it is running)?
    pub fn busy(&self) -> bool {
        self.game.is_some()
    }

    // --- Actions ------------------------------------------------------------

    pub fn open(&mut self, path: &str) {
        if self.doc.is_dirty() {
            if let Err(error) = self.doc.save() {
                self.warn(format!(
                    "Not opened: the current scene could not be saved ({error})"
                ));
                return;
            }
        }
        match self.doc.open(path) {
            Ok(()) => {
                self.say(format!("Opened {path}"));
                self.frame_all = true;
                self.drag = Drag::None;
                self.refresh_files();
            }
            Err(error) => self.warn(error.to_string()),
        }
    }

    fn save(&mut self) {
        self.flush();
        match self.doc.save() {
            Ok(file) => {
                self.say(format!("Saved {}", file.display()));
                self.refresh_files();
            }
            Err(error) => self.warn(format!("Not saved: {error}")),
        }
    }

    fn undo(&mut self) {
        self.flush();
        if self.doc.undo() {
            self.say("Undone");
        }
    }

    fn redo(&mut self) {
        if self.doc.redo() {
            self.say("Redone");
        }
    }

    /// Turns pending changes into an undo step.
    fn flush(&mut self) {
        if self.changed {
            self.doc.commit();
            self.changed = false;
        }
    }

    fn play(&mut self) {
        if self.game.is_some() {
            return;
        }
        if self.doc.is_dirty() || self.changed {
            self.save();
            if self.doc.is_dirty() {
                return;
            }
        }
        if !self.project.join("Cargo.toml").is_file() {
            self.warn(format!(
                "No Cargo.toml in {}, so there is no game to run",
                self.project.display()
            ));
            return;
        }
        match Command::new("cargo")
            .arg("run")
            .current_dir(&self.project)
            .spawn()
        {
            Ok(child) => {
                self.game = Some(child);
                self.say("Building and starting the game…");
            }
            Err(error) => self.warn(format!("Could not start cargo: {error}")),
        }
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.game.take() {
            let _ = child.kill();
            let _ = child.wait();
            self.say("Game stopped");
        }
    }

    fn poll_game(&mut self) {
        let Some(child) = &mut self.game else { return };
        match child.try_wait() {
            Ok(Some(status)) => {
                self.game = None;
                if status.success() {
                    self.say("Game closed");
                } else {
                    self.warn("The game stopped with an error. See the terminal for details.");
                }
            }
            Ok(None) => {}
            Err(_) => self.game = None,
        }
    }

    fn selected_editable(&self) -> Option<NodeId> {
        self.doc
            .selection
            .filter(|id| self.doc.scene.contains(*id) && !self.doc.inside_placed_scene(*id))
    }

    /// Where new nodes go: under the selection, or under the root.
    fn insertion_parent(&self) -> NodeId {
        self.selected_editable()
            .filter(|id| {
                self.doc
                    .scene
                    .get(*id)
                    .is_some_and(|n| n.instance_of().is_none() || *id == self.doc.root)
            })
            .unwrap_or(self.doc.root)
    }

    fn add_node(&mut self, parent: NodeId, node: Node) {
        self.flush();
        // New nodes appear in the middle of the view, in the parent's space.
        let parent_world = self.doc.scene.world_transform(parent);
        let local = parent_world.inverse().transform_point2(self.camera.center);
        self.doc
            .add_child(parent, node.at(local.x.round(), local.y.round()));
        self.doc.commit();
    }

    fn delete(&mut self) {
        let Some(id) = self.selected_editable() else {
            return;
        };
        self.flush();
        if self.doc.delete(id) {
            self.doc.commit();
        }
    }

    fn duplicate(&mut self) {
        let Some(id) = self.selected_editable() else {
            return;
        };
        self.flush();
        if self.doc.duplicate(id).is_some() {
            self.doc.commit();
        }
    }

    fn place(&mut self, path: &str) {
        self.flush();
        let parent = self.insertion_parent();
        match self.doc.place_scene(parent, path) {
            Ok(id) => {
                let world = self.camera.center;
                let local = self
                    .doc
                    .scene
                    .world_transform(parent)
                    .inverse()
                    .transform_point2(world);
                if let Some(node) = self.doc.scene.get_mut(id) {
                    node.transform.position = local.round();
                }
                self.doc.commit();
            }
            Err(error) => self.warn(error.to_string()),
        }
    }

    fn run(&mut self, action: Action, gfx: &mut Gfx) {
        match action {
            Action::Save => self.save(),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Play => self.play(),
            Action::Stop => self.stop(),
            Action::AddNode => self.add_node(self.insertion_parent(), Node::new("Node")),
            Action::Duplicate => self.duplicate(),
            Action::Delete => self.delete(),
            Action::FrameSelection => self.frame_selection(gfx),
            Action::FrameAll => self.frame_all = true,
            Action::Open(path) => self.open(&path),
            Action::Place(path) => self.place(&path),
            Action::Search => self.open_palette(false),
            Action::Select(id) => {
                self.doc.selection = Some(id);
                self.frame_selection(gfx);
            }
        }
    }

    fn frame_selection(&mut self, gfx: &mut Gfx) {
        let Some(id) = self.doc.selection else { return };
        let world = self.doc.scene.world_transform(id);
        let (min, max) = match viewport::node_bounds(&self.doc.scene, id, gfx, &self.doc.assets) {
            Some(bounds) => {
                let corners = viewport::world_corners(world, bounds);
                let min = corners
                    .iter()
                    .fold(Vec2::splat(f32::INFINITY), |a, c| a.min(*c));
                let max = corners
                    .iter()
                    .fold(Vec2::splat(f32::NEG_INFINITY), |a, c| a.max(*c));
                (min, max)
            }
            None => (
                world.translation - Vec2::splat(100.0),
                world.translation + Vec2::splat(100.0),
            ),
        };
        let zoom = self.camera.zoom;
        self.camera.frame(self.viewport, min, max);
        // Do not zoom in further than needed for small things.
        self.camera.zoom = self.camera.zoom.min(zoom.max(2.0));
    }

    // --- The window -----------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui, gfx: &mut Gfx) {
        self.poll_game();
        self.shortcuts(ui, gfx);
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        let gap = theme::GAP;
        self.tree_width = self
            .tree_width
            .clamp(200.0, (screen.width() * 0.3).max(200.0));
        self.inspector_width = self
            .inspector_width
            .clamp(260.0, (screen.width() * 0.35).max(260.0));
        let tree = Rect::from_min_size(
            screen.min + egui::vec2(gap, gap),
            egui::vec2(self.tree_width, screen.height() - gap * 2.0),
        );
        let inspector = Rect::from_min_size(
            Pos2::new(
                screen.right() - gap - self.inspector_width,
                screen.top() + gap,
            ),
            egui::vec2(self.inspector_width, screen.height() - gap * 2.0),
        );
        // The part of the window no panel covers: the camera centres on it.
        let free = Rect::from_min_max(
            Pos2::new(tree.right(), screen.top() + gap + TOOLBAR_HEIGHT),
            Pos2::new(inspector.left(), screen.bottom()),
        );
        self.canvas = screen;
        self.viewport = free;

        // The scene fills the window; everything else floats over it.
        egui::Area::new(egui::Id::unique("viewport"))
            .order(egui::Order::Background)
            .fixed_pos(screen.min)
            .constrain(false)
            .show(&ctx, |ui| {
                ui.set_min_size(screen.size());
                self.viewport_panel(ui, gfx);
            });
        self.floating_panel(&ctx, "tree", tree, Edge::Right, |editor, ui| {
            editor.tree_panel(ui)
        });
        self.floating_panel(&ctx, "inspector", inspector, Edge::Left, |editor, ui| {
            editor.inspector_panel(ui)
        });
        self.toolbar(
            &ctx,
            Rect::from_min_max(
                Pos2::new(tree.right() + gap, screen.top() + gap),
                Pos2::new(inspector.left() - gap, screen.top() + gap + TOOLBAR_HEIGHT),
            ),
        );
        self.status(&ctx, free, screen.bottom() - gap);
        self.palette_window(&ctx, gfx);

        // Group everything that changed while nothing is held into one undo step.
        let idle = !ctx.egui_is_using_pointer()
            && !ctx.input(|i| i.pointer.any_down())
            && !ctx.text_edit_focused();
        if idle {
            self.flush();
        }
    }

    fn shortcuts(&mut self, ui: &mut Ui, gfx: &mut Gfx) {
        let ctx = ui.ctx().clone();
        // A clicked button keeps keyboard focus too, so only a text field
        // being typed in should take the keys away from the editor.
        let typing = ctx.text_edit_focused();
        let popup_open = egui::Popup::is_any_open(&ctx);
        let mut actions = Vec::new();
        ctx.input_mut(|i| {
            if i.consume_shortcut(&SAVE) {
                actions.push(Action::Save);
            }
            // While typing, the text field undoes its own edits.
            if !typing {
                if i.consume_shortcut(&REDO) || i.consume_shortcut(&REDO_Y) {
                    actions.push(Action::Redo);
                }
                if i.consume_shortcut(&UNDO) {
                    actions.push(Action::Undo);
                }
            }
            if i.consume_shortcut(&SEARCH) || i.consume_shortcut(&SEARCH_P) {
                actions.push(Action::Search);
            }
            if i.consume_shortcut(&PLAY) {
                actions.push(if self.game.is_some() {
                    Action::Stop
                } else {
                    Action::Play
                });
            }
            if !typing && self.palette.is_none() {
                if i.consume_shortcut(&DUPLICATE) {
                    actions.push(Action::Duplicate);
                }
                if i.consume_key(Modifiers::NONE, Key::Delete)
                    || i.consume_key(Modifiers::NONE, Key::Backspace)
                {
                    actions.push(Action::Delete);
                }
                if i.consume_key(Modifiers::NONE, Key::F) {
                    actions.push(Action::FrameSelection);
                }
                if i.consume_key(Modifiers::NONE, Key::Home)
                    || i.consume_key(Modifiers::SHIFT, Key::F)
                {
                    actions.push(Action::FrameAll);
                }
                // Escape closes an open menu first.
                if !popup_open && i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.doc.selection = None;
                }
            }
        });
        // Arrow keys nudge the selection.
        if !typing && self.palette.is_none() {
            if let Some(id) = self.selected_editable().filter(|id| *id != self.doc.root) {
                let (nudge, big) = ctx.input(|i| {
                    let mut d = Vec2::ZERO;
                    for (key, dir) in [
                        (Key::ArrowLeft, Vec2::NEG_X),
                        (Key::ArrowRight, Vec2::X),
                        (Key::ArrowUp, Vec2::NEG_Y),
                        (Key::ArrowDown, Vec2::Y),
                    ] {
                        if i.key_pressed(key) {
                            d += dir;
                        }
                    }
                    (d, i.modifiers.shift)
                });
                if nudge != Vec2::ZERO {
                    if let Some(node) = self.doc.scene.get_mut(id) {
                        node.transform.position += nudge * if big { 10.0 } else { 1.0 };
                        self.changed = true;
                    }
                }
            }
        }
        for action in actions {
            self.run(action, gfx);
        }
    }

    /// A glass panel floating over the scene, which can be made wider or
    /// narrower by dragging its inner edge.
    fn floating_panel(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        rect: Rect,
        edge: Edge,
        add: impl FnOnce(&mut Editor, &mut Ui),
    ) {
        let id = egui::Id::unique(name);
        egui::Area::new(id)
            .order(egui::Order::Middle)
            .fixed_pos(rect.min)
            .constrain(false)
            .show(ctx, |ui| {
                // Clicks on the panel's empty parts stay with the panel.
                ui.interact(rect, id.with("background"), Sense::click_and_drag());
                let radius = theme::PANEL_RADIUS;
                ui.painter()
                    .add(theme::panel_shadow().as_shape(rect, radius));
                ui.painter()
                    .add(egui::epaint::RectShape::filled(rect, radius, glass::GLASS));
                let inner = rect.shrink2(egui::vec2(10.0, 12.0));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_clip_rect(rect.shrink(2.0));
                add(self, &mut child);

                // The inner edge can be dragged to resize.
                let x = match edge {
                    Edge::Left => rect.left(),
                    Edge::Right => rect.right(),
                };
                let handle = Rect::from_center_size(
                    Pos2::new(x, rect.center().y),
                    egui::vec2(10.0, rect.height() - 2.0 * radius as f32),
                );
                let response = ui
                    .interact(handle, id.with("resize"), Sense::drag())
                    .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                if response.dragged() {
                    let delta = response.drag_delta().x;
                    match edge {
                        Edge::Right => self.tree_width += delta,
                        Edge::Left => self.inspector_width -= delta,
                    }
                }
                ui.allocate_rect(rect, Sense::hover());
            });
    }

    /// One glass capsule holding a few controls, anchored at `pivot`.
    fn glass_group<R>(
        ctx: &egui::Context,
        name: &str,
        anchor: egui::Align2,
        at: Pos2,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        egui::Area::new(egui::Id::unique(name))
            .order(egui::Order::Middle)
            .pivot(anchor)
            .fixed_pos(at)
            .constrain(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(glass::GLASS_CLEAR)
                    .corner_radius(TOOLBAR_HEIGHT as u8 / 2)
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 22,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(70),
                    })
                    .inner_margin(egui::Margin::same(5))
                    .show(ui, |ui| {
                        ui.set_height(TOOLBAR_HEIGHT - 10.0);
                        ui.horizontal_centered(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            add(ui)
                        })
                        .inner
                    })
                    .inner
            })
            .inner
    }

    fn open_palette(&mut self, scenes_only: bool) {
        self.refresh_files();
        self.palette = Some(Palette {
            query: String::new(),
            selected: 0,
            scenes_only,
            focus: true,
        });
    }

    fn toolbar(&mut self, ctx: &egui::Context, rect: Rect) {
        // Search, in the middle.
        let search_width = (rect.width() - 260.0).clamp(160.0, 340.0);
        let clicked = Self::glass_group(
            ctx,
            "search",
            egui::Align2::CENTER_TOP,
            Pos2::new(rect.center().x - 60.0, rect.top()),
            |ui| {
                let (row, response) = ui.allocate_exact_size(
                    egui::vec2(search_width, TOOLBAR_HEIGHT - 10.0),
                    Sense::click(),
                );
                let painter = ui.painter();
                if response.hovered() {
                    painter.rect_filled(row, 15.0, theme::FILL_SOFT);
                }
                widgets::paint_icon(
                    painter,
                    Icon::Search,
                    Rect::from_center_size(
                        Pos2::new(row.left() + 18.0, row.center().y),
                        egui::Vec2::splat(15.0),
                    ),
                    theme::SECONDARY,
                );
                painter.text(
                    Pos2::new(row.left() + 34.0, row.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Search nodes, scenes and commands",
                    egui::FontId::proportional(13.0),
                    theme::SECONDARY,
                );
                painter.text(
                    Pos2::new(row.right() - 14.0, row.center().y),
                    egui::Align2::RIGHT_CENTER,
                    "Ctrl K",
                    egui::FontId::new(12.0, theme::semibold()),
                    theme::TERTIARY,
                );
                response.clicked()
            },
        );
        if clicked {
            self.open_palette(false);
        }

        // History and saving, then Play, on the right.
        let running = self.game.is_some();
        let play = Self::glass_group(
            ctx,
            "play",
            egui::Align2::RIGHT_TOP,
            Pos2::new(rect.right(), rect.top()),
            |ui| {
                let (icon, label, tone, hint) = if running {
                    (Icon::Stop, "Stop", Tone::Danger, "Stop the game (F5)")
                } else {
                    (
                        Icon::Play,
                        "Play",
                        Tone::Primary,
                        "Save and run the game (F5)",
                    )
                };
                widgets::capsule(ui, Some(icon), label, tone, true)
                    .on_hover_text(hint)
                    .clicked()
            },
        );
        if play {
            if running {
                self.stop();
            } else {
                self.play();
            }
        }
        let can_undo = self.doc.can_undo() || self.changed;
        let can_redo = self.doc.can_redo();
        let can_save = self.doc.path.is_some();
        let (undo, redo, save) = Self::glass_group(
            ctx,
            "history",
            egui::Align2::RIGHT_TOP,
            Pos2::new(rect.right() - 102.0, rect.top()),
            |ui| {
                let undo =
                    widgets::icon_button(ui, Icon::Undo, 30.0, can_undo, "Undo (Ctrl+Z)").clicked();
                let redo =
                    widgets::icon_button(ui, Icon::Redo, 30.0, can_redo, "Redo (Ctrl+Shift+Z)")
                        .clicked();
                let save = widgets::capsule(ui, None, "Save", Tone::Bare, can_save)
                    .on_hover_text("Ctrl+S")
                    .clicked();
                (undo, redo, save)
            },
        );
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if save {
            self.save();
        }
    }

    /// A small pill at the bottom with the zoom and the cursor's position,
    /// and the latest message above it for a few seconds.
    fn status(&mut self, ctx: &egui::Context, free: Rect, bottom: f32) {
        let zoom = format!("{:.0}%", self.camera.zoom * 100.0);
        let pointer = self
            .pointer_world
            .map(|w| format!("{:.0}, {:.0}", w.x, w.y));
        let screen_label = format!("{:.0} × {:.0}", self.screen.x, self.screen.y);
        let screen = &mut self.screen;
        let mut frame_all = false;
        Self::glass_group(
            ctx,
            "status",
            egui::Align2::CENTER_BOTTOM,
            Pos2::new(free.center().x, bottom),
            |ui| {
                ui.add_space(6.0);
                if widgets::capsule(ui, None, &zoom, Tone::Bare, true)
                    .on_hover_text("Zoom. Click to see everything.")
                    .clicked()
                {
                    frame_all = true;
                }
                if let Some(pointer) = &pointer {
                    ui.label(RichText::new(pointer).color(theme::SECONDARY).size(12.0));
                    ui.add_space(6.0);
                }
                let button = widgets::capsule(ui, None, &screen_label, Tone::Bare, true)
                    .on_hover_text("The size of the game's window");
                egui::Popup::from_toggle_button_response(&button)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.set_min_width(180.0);
                        ui.label(
                            RichText::new("Game window")
                                .family(theme::semibold())
                                .color(theme::SECONDARY),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut screen.x)
                                    .speed(1.0)
                                    .range(16.0..=8192.0)
                                    .prefix("w  "),
                            );
                            ui.add(
                                egui::DragValue::new(&mut screen.y)
                                    .speed(1.0)
                                    .range(16.0..=8192.0)
                                    .prefix("h  "),
                            );
                        });
                    });
            },
        );
        if frame_all {
            self.frame_all = true;
        }

        let Some((text, when, warning)) = self.status.clone() else {
            return;
        };
        let age = when.elapsed();
        if age >= STATUS_TIME {
            return;
        }
        // Fade out over the last half second.
        let left = (STATUS_TIME - age).as_secs_f32();
        let opacity = (left / 0.5).min(1.0);
        ctx.request_repaint_after(Duration::from_millis(50));
        egui::Area::new(egui::Id::unique("toast"))
            .order(egui::Order::Middle)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(Pos2::new(free.center().x, bottom - TOOLBAR_HEIGHT - 8.0))
            .constrain(false)
            .interactable(false)
            .show(ctx, |ui| {
                ui.multiply_opacity(opacity);
                egui::Frame::new()
                    .fill(glass::GLASS_CLEAR)
                    .corner_radius(17)
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 22,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(60),
                    })
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let color = if warning { theme::WARN } else { theme::GREEN };
                            let (dot, _) =
                                ui.allocate_exact_size(egui::vec2(8.0, 16.0), Sense::hover());
                            ui.painter().circle_filled(dot.center(), 3.5, color);
                            ui.label(RichText::new(text).color(theme::TEXT).size(12.5));
                        });
                    });
            });
    }

    // --- Scene tree -----------------------------------------------------------

    fn tree_panel(&mut self, ui: &mut Ui) {
        // The open scene's name, which also switches scenes.
        ui.horizontal(|ui| {
            let name = self
                .doc
                .path
                .as_deref()
                .and_then(|p| p.rsplit('/').next())
                .map(|n| n.trim_end_matches(".scene").to_string())
                .unwrap_or_else(|| "Untitled".to_string());
            let dirty = self.doc.is_dirty() || self.changed;
            let width = ui.available_width() - 40.0;
            let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 40.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 12.0, theme::FILL_SOFT);
            }
            let painter = ui.painter();
            painter.text(
                Pos2::new(rect.left() + 10.0, rect.top() + 12.0),
                egui::Align2::LEFT_CENTER,
                "BRANDON",
                egui::FontId::new(10.0, theme::semibold()),
                theme::TERTIARY,
            );
            let title = painter.layout_no_wrap(
                name,
                egui::FontId::new(16.0, theme::semibold()),
                theme::TEXT,
            );
            let title_width = title.size().x;
            painter.galley(
                Pos2::new(rect.left() + 10.0, rect.top() + 18.0),
                title,
                theme::TEXT,
            );
            let mut x = rect.left() + 10.0 + title_width + 6.0;
            if dirty {
                painter.circle_filled(Pos2::new(x + 3.0, rect.top() + 28.0), 3.5, theme::SECONDARY);
                x += 10.0;
            }
            widgets::paint_icon(
                painter,
                Icon::ChevronUpDown,
                Rect::from_center_size(
                    Pos2::new(x + 8.0, rect.top() + 28.0),
                    egui::Vec2::splat(13.0),
                ),
                theme::TERTIARY,
            );
            if response
                .on_hover_text(self.doc.path.as_deref().unwrap_or("Not saved yet"))
                .clicked()
            {
                self.open_palette(true);
            }
            let add = widgets::icon_button(ui, Icon::Plus, 30.0, true, "Add a node");
            egui::Popup::menu(&add).show(|ui| {
                widgets::menu_style(ui);
                self.add_menu_items(ui)
            });
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let root = self.doc.root;
                self.tree_row(ui, root, 0);
                // Space below the tree to drop onto the root.
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), Sense::click());
                if response.clicked() {
                    self.doc.selection = None;
                }
                if let Some(dragged) = response.dnd_release_payload::<TreeDrag>() {
                    self.move_node(dragged.0, root, usize::MAX);
                } else if response.dnd_hover_payload::<TreeDrag>().is_some() {
                    ui.painter().hline(
                        rect.x_range(),
                        rect.top() + 2.0,
                        egui::Stroke::new(2.0, theme::ACCENT),
                    );
                }
            });
    }

    fn add_menu_items(&mut self, ui: &mut Ui) {
        ui.set_min_width(210.0);
        let parent = self.insertion_parent();
        let mut add: Option<Node> = None;
        for (icon, label, make) in [
            (Icon::Node, "Empty node", 0),
            (Icon::Image, "Sprite", 1),
            (Icon::Text, "Text", 2),
            (Icon::Camera, "Camera", 3),
        ] {
            if widgets::menu_item(ui, Some(icon), label, "").clicked() {
                add = Some(match make {
                    0 => Node::new("Node"),
                    1 => Node::new("Sprite").with(Sprite::rect(64.0, 64.0, Color::hex(0x0a84ff))),
                    2 => Node::new("Text").with(Text::new("Text").with_size(32.0)),
                    _ => Node::new("Camera").with(brandon_scene::Camera::new()),
                });
            }
        }
        if let Some(node) = add {
            self.add_node(parent, node);
            ui.close();
        }
        let placeable: Vec<String> = self
            .scenes
            .iter()
            .filter(|s| Some(s.as_str()) != self.doc.path.as_deref())
            .cloned()
            .collect();
        if !placeable.is_empty() {
            widgets::menu_separator(ui);
            widgets::menu_heading(ui, "Place a scene");
            for scene in placeable {
                if widgets::menu_item(ui, Some(Icon::Placed), &scene, "").clicked() {
                    self.place(&scene);
                    ui.close();
                }
            }
        }
    }

    fn move_node(&mut self, id: NodeId, parent: NodeId, index: usize) {
        if id == self.doc.root || self.doc.inside_placed_scene(id) {
            return;
        }
        let into_placed = parent != self.doc.root
            && (self.doc.inside_placed_scene(parent)
                || self
                    .doc
                    .scene
                    .get(parent)
                    .is_some_and(|n| n.instance_of().is_some()));
        if into_placed {
            self.warn(
                "A placed scene's insides belong to its own file. Open that scene to change them.",
            );
            return;
        }
        self.flush();
        // Keep the node where it is on screen.
        let world = self.doc.scene.world_transform(id);
        let mut index = index;
        if self.doc.scene.parent(id) == Some(parent) {
            let current = self
                .doc
                .scene
                .children(parent)
                .iter()
                .position(|c| *c == id)
                .unwrap_or(0);
            if index != usize::MAX && current < index {
                index -= 1;
            }
        }
        if self.doc.scene.move_to(id, Some(parent), index) {
            let parent_world = self.doc.scene.world_transform(parent);
            let local = parent_world.inverse() * world;
            let (scale, rotation, translation) = local.to_scale_angle_translation();
            if let Some(node) = self.doc.scene.get_mut(id) {
                if scale.is_finite() && translation.is_finite() {
                    node.transform.position = translation;
                    node.transform.rotation = rotation;
                    node.transform.scale = scale;
                }
            }
            self.doc.scene.refresh_transforms();
            self.doc.commit();
        } else {
            self.warn("A node cannot go inside itself");
        }
    }

    fn tree_row(&mut self, ui: &mut Ui, id: NodeId, depth: usize) {
        let Some(node) = self.doc.scene.get(id) else {
            return;
        };
        let children: Vec<NodeId> = self.doc.scene.children(id).to_vec();
        let name = if node.name.is_empty() {
            "(unnamed)".to_string()
        } else {
            node.name.clone()
        };
        let placed = node.instance_of().is_some() && id != self.doc.root;
        let inside = self.doc.inside_placed_scene(id);
        let icon = if id == self.doc.root {
            Icon::Scene
        } else {
            inspector::node_icon(node)
        };
        let row = TreeRow {
            icon,
            name,
            placed: placed || inside,
            hidden: !node.visible,
            behaviours: node.behaviour_count(),
            selected: self.doc.selection == Some(id),
        };

        let draw_row = |ui: &mut Ui, editor: &mut Editor| {
            let response = tree_row_widget(ui, &row);
            if response.clicked() {
                editor.doc.selection = Some(id);
            }
            if id != editor.doc.root && !inside && response.drag_started() {
                response.dnd_set_drag_payload(TreeDrag(id));
            }
            // Dropping: top quarter goes before, bottom quarter after, the middle inside.
            if let Some(pointer) = ui.ctx().pointer_interact_pos() {
                if response.dnd_hover_payload::<TreeDrag>().is_some() {
                    let rect = response.rect;
                    let zone = (pointer.y - rect.top()) / rect.height().max(1.0);
                    let stroke = egui::Stroke::new(2.0, theme::ACCENT);
                    if zone < 0.25 && id != editor.doc.root {
                        ui.painter().hline(rect.x_range(), rect.top(), stroke);
                    } else if zone > 0.75 && id != editor.doc.root {
                        ui.painter().hline(rect.x_range(), rect.bottom(), stroke);
                    } else {
                        ui.painter()
                            .rect_stroke(rect, 9.0, stroke, egui::StrokeKind::Inside);
                    }
                }
                if let Some(dragged) = response.dnd_release_payload::<TreeDrag>() {
                    let rect = response.rect;
                    let zone = (pointer.y - rect.top()) / rect.height().max(1.0);
                    let parent = editor.doc.scene.parent(id);
                    let position = parent
                        .map(|p| {
                            editor
                                .doc
                                .scene
                                .children(p)
                                .iter()
                                .position(|c| *c == id)
                                .unwrap_or(0)
                        })
                        .unwrap_or(0);
                    match parent {
                        Some(parent) if zone < 0.25 => {
                            editor.move_node(dragged.0, parent, position)
                        }
                        Some(parent) if zone > 0.75 => {
                            editor.move_node(dragged.0, parent, position + 1)
                        }
                        _ => editor.move_node(dragged.0, id, usize::MAX),
                    }
                }
            }
            response.context_menu(|ui| {
                widgets::menu_style(ui);
                editor.tree_context_menu(ui, id, placed, inside)
            });
        };

        if children.is_empty() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(ui.spacing().indent);
                draw_row(ui, self);
            });
        } else {
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id(("tree", id)),
                depth < 2 && !placed,
            );
            let header = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                state.show_toggle_button(ui, |ui, openness, response| {
                    let color = if response.hovered() {
                        theme::TEXT
                    } else {
                        theme::TERTIARY
                    };
                    widgets::paint_icon(
                        ui.painter(),
                        Icon::Chevron(openness),
                        Rect::from_center_size(response.rect.center(), egui::Vec2::splat(13.0)),
                        color,
                    );
                });
                draw_row(ui, self);
            });
            state.show_body_indented(&header.response, ui, |ui| {
                for child in children {
                    self.tree_row(ui, child, depth + 1);
                }
            });
        }
    }

    fn tree_context_menu(&mut self, ui: &mut Ui, id: NodeId, placed: bool, inside: bool) {
        ui.set_min_width(200.0);
        self.doc.selection = Some(id);
        if placed {
            if let Some(path) = self
                .doc
                .scene
                .get(id)
                .and_then(|n| n.instance_of())
                .map(str::to_string)
            {
                if widgets::menu_item(ui, Some(Icon::Placed), &format!("Open {path}"), "").clicked()
                {
                    self.open(&path);
                    ui.close();
                    return;
                }
                widgets::menu_separator(ui);
            }
        }
        if inside {
            widgets::menu_heading(ui, "Part of a placed scene");
            return;
        }
        if !placed {
            widgets::submenu_item(ui, Some(Icon::Plus), "Add child", |ui| {
                self.add_menu_items(ui)
            });
        }
        if id != self.doc.root {
            if widgets::menu_item(ui, Some(Icon::Overlay), "Duplicate", "Ctrl D").clicked() {
                self.duplicate();
                ui.close();
            }
            let visible = self.doc.scene.get(id).is_some_and(|n| n.visible);
            let (icon, label) = if visible {
                (Icon::EyeClosed, "Hide")
            } else {
                (Icon::Eye, "Show")
            };
            if widgets::menu_item(ui, Some(icon), label, "").clicked() {
                if let Some(node) = self.doc.scene.get_mut(id) {
                    node.visible = !visible;
                    self.changed = true;
                }
                ui.close();
            }
            widgets::menu_separator(ui);
            if widgets::menu_item_danger(ui, "Delete", "Del").clicked() {
                self.delete();
                ui.close();
            }
        }
    }

    // --- Inspector ------------------------------------------------------------

    fn inspector_panel(&mut self, ui: &mut Ui) {
        let Some(id) = self.doc.selection.filter(|id| self.doc.scene.contains(*id)) else {
            ui.add_space(ui.available_height() * 0.36);
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(40.0), Sense::hover());
                widgets::paint_icon(ui.painter(), Icon::Scene, rect, theme::TERTIARY);
                ui.add_space(10.0);
                ui.label(
                    RichText::new("No Selection")
                        .family(theme::semibold())
                        .size(17.0)
                        .color(theme::SECONDARY),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Click something in the scene\nor in the list on the left.")
                        .color(theme::TERTIARY)
                        .size(12.5),
                );
            });
            return;
        };
        let read_only = self.doc.inside_placed_scene(id);
        let mut requests = inspector::Requests::default();
        let images = std::mem::take(&mut self.images);
        let names = std::mem::take(&mut self.behaviour_names);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width() - 4.0);
                if let Some(node) = self.doc.scene.get_mut(id) {
                    let cx = inspector::Context {
                        images: &images,
                        behaviour_names: &names,
                        read_only,
                    };
                    if inspector::show(ui, node, &cx, &mut requests) {
                        self.changed = true;
                    }
                }
            });
        self.images = images;
        self.behaviour_names = names;
        if self.changed {
            self.doc.scene.refresh_transforms();
        }
        if let Some(path) = requests.open_scene {
            self.open(&path);
        }
    }

    // --- Viewport -------------------------------------------------------------

    fn viewport_panel(&mut self, ui: &mut Ui, gfx: &mut Gfx) {
        // The scene reaches under the panels, but the camera centres on the
        // part they leave free.
        let rect = self.canvas;
        let free = self.viewport;
        if self.frame_all {
            self.frame_all = false;
            match viewport::scene_extent(&self.doc.scene, self.doc.root, gfx, &self.doc.assets) {
                Some((min, max)) => {
                    let min = min.min(Vec2::ZERO);
                    let max = max.max(self.screen);
                    self.camera.frame(free, min, max);
                }
                None => self.camera.frame(free, Vec2::ZERO, self.screen),
            }
        }
        let response = ui.interact(
            rect,
            ui.scope_id().with("viewport"),
            Sense::click_and_drag(),
        );
        let view = self.view();
        let to_world = |pos: Pos2| view.inverse().transform_point2(v(pos));
        self.pointer_world = response.hover_pos().map(to_world);

        // Zoom with the wheel, around the cursor.
        if let Some(hover) = response.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            let pinch = ui.input(|i| i.zoom_delta());
            let factor = (scroll * 0.0018).exp() * pinch;
            if (factor - 1.0).abs() > 1e-4 {
                self.camera.zoom_at(free, hover, factor);
            }
        }

        let ctrl = ui.input(|i| i.modifiers.command);
        let selected = self
            .doc
            .selection
            .filter(|id| self.doc.scene.contains(*id) && *id != self.doc.root);
        let handles = selected.map(|id| {
            let world = self.doc.scene.world_transform(id);
            let bounds = viewport::node_bounds(&self.doc.scene, id, gfx, &self.doc.assets);
            viewport::handles(world, bounds, self.view())
        });
        let editable = selected.filter(|id| !self.doc.inside_placed_scene(*id));

        // Starting a drag.
        let middle = ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
        if response.drag_started() {
            let press = ui
                .input(|i| i.pointer.press_origin())
                .unwrap_or(rect.center());
            let near = |point: Pos2| point.distance(press) <= viewport::HANDLE_RADIUS + 3.0;
            self.drag = Drag::Pan;
            if !middle {
                match (editable, handles) {
                    (Some(id), Some(h)) if near(h.rotate) => {
                        let start = self.doc.scene.get(id).map_or(0.0, |n| n.transform.rotation);
                        let d = press - h.origin;
                        self.drag = Drag::Rotate {
                            id,
                            start,
                            start_angle: d.y.atan2(d.x),
                        };
                    }
                    (Some(id), Some(h)) if near(h.scale) => {
                        let start = self
                            .doc
                            .scene
                            .get(id)
                            .map_or(Vec2::ONE, |n| n.transform.scale);
                        self.drag = Drag::Scale {
                            id,
                            start,
                            start_distance: press.distance(h.origin).max(1.0),
                        };
                    }
                    _ => {
                        let world = to_world(press);
                        let hit = viewport::pick(
                            &self.doc.scene,
                            self.doc.root,
                            world,
                            gfx,
                            &self.doc.assets,
                        );
                        if let Some(hit) = hit {
                            self.doc.selection = Some(hit);
                            if !self.doc.inside_placed_scene(hit) {
                                let start = self
                                    .doc
                                    .scene
                                    .get(hit)
                                    .map_or(Vec2::ZERO, |n| n.transform.position);
                                let parent_world = self
                                    .doc
                                    .scene
                                    .parent(hit)
                                    .map(|p| self.doc.scene.world_transform(p))
                                    .unwrap_or(glam::Affine2::IDENTITY);
                                self.drag = Drag::Move {
                                    id: hit,
                                    start,
                                    grab: world,
                                    parent_inverse: parent_world.matrix2.inverse(),
                                };
                            }
                        }
                    }
                }
            }
        }

        // Continuing it.
        if response.dragged() {
            let pointer = response.interact_pointer_pos().unwrap_or(rect.center());
            match self.drag {
                Drag::None => {}
                Drag::Pan => {
                    self.camera.center -= v(Pos2::ZERO + response.drag_delta()) / self.camera.zoom;
                }
                Drag::Move {
                    id,
                    start,
                    grab,
                    parent_inverse,
                } => {
                    let delta = parent_inverse * (to_world(pointer) - grab);
                    let mut position = start + delta;
                    if ctrl {
                        position = Vec2::new(
                            viewport::snap(position.x, 8.0),
                            viewport::snap(position.y, 8.0),
                        );
                    }
                    if let Some(node) = self.doc.scene.get_mut(id) {
                        if node.transform.position != position {
                            node.transform.position = position;
                            self.changed = true;
                        }
                    }
                }
                Drag::Rotate {
                    id,
                    start,
                    start_angle,
                } => {
                    if let Some(h) = handles {
                        let d = pointer - h.origin;
                        let mut rotation = start + (d.y.atan2(d.x) - start_angle);
                        if ctrl {
                            rotation = viewport::snap(rotation, 15f32.to_radians());
                        }
                        if let Some(node) = self.doc.scene.get_mut(id) {
                            node.transform.rotation = rotation;
                            self.changed = true;
                        }
                    }
                }
                Drag::Scale {
                    id,
                    start,
                    start_distance,
                } => {
                    if let Some(h) = handles {
                        let mut factor = pointer.distance(h.origin) / start_distance;
                        if ctrl {
                            factor = viewport::snap(factor, 0.1).max(0.1);
                        }
                        if let Some(node) = self.doc.scene.get_mut(id) {
                            node.transform.scale = start * factor.max(0.001);
                            self.changed = true;
                        }
                    }
                }
            }
            if !matches!(self.drag, Drag::Pan | Drag::None) {
                self.doc.scene.refresh_transforms();
            }
        }
        if response.drag_stopped() {
            self.drag = Drag::None;
        }

        // A click without a drag selects what is under the cursor.
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.doc.selection = viewport::pick(
                    &self.doc.scene,
                    self.doc.root,
                    to_world(pos),
                    gfx,
                    &self.doc.assets,
                );
            }
        }
        if response.double_clicked() {
            self.frame_selection(gfx);
        }

        // Cursor feedback.
        if let (Some(h), Some(hover)) = (handles, response.hover_pos()) {
            if editable.is_some() {
                if hover.distance(h.rotate) <= viewport::HANDLE_RADIUS + 3.0 {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Alias);
                } else if hover.distance(h.scale) <= viewport::HANDLE_RADIUS + 3.0 {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                }
            }
        }
        match self.drag {
            Drag::Pan => ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing),
            Drag::Move { .. } => ui.ctx().set_cursor_icon(egui::CursorIcon::Move),
            _ => {}
        }

        // Handles, drawn with the current transforms.
        let painter = ui.painter_at(rect);
        if let Some(id) = self
            .doc
            .selection
            .filter(|id| self.doc.scene.contains(*id) && *id != self.doc.root)
        {
            let world = self.doc.scene.world_transform(id);
            let bounds = viewport::node_bounds(&self.doc.scene, id, gfx, &self.doc.assets);
            let h = viewport::handles(world, bounds, self.view());
            if self.doc.inside_placed_scene(id) {
                if let Some(corners) = h.outline {
                    let mut points = corners.to_vec();
                    points.push(corners[0]);
                    painter.add(egui::Shape::dashed_line(
                        &points,
                        egui::Stroke::new(1.5, theme::PLACED),
                        6.0,
                        4.0,
                    ));
                }
            } else {
                viewport::paint_handles(
                    &painter,
                    &h,
                    self.doc
                        .scene
                        .get(id)
                        .is_some_and(|n| n.instance_of().is_some()),
                );
            }
        }

        // The origin label, so (0, 0) can be found.
        let origin = p(self.view().transform_point2(Vec2::ZERO));
        if rect.contains(origin) {
            painter.text(
                origin + egui::vec2(6.0, 4.0),
                egui::Align2::LEFT_TOP,
                "0, 0",
                egui::FontId::proportional(10.5),
                theme::TERTIARY,
            );
        }

        if self.doc.scene.children(self.doc.root).is_empty() {
            painter.text(
                free.center(),
                egui::Align2::CENTER_CENTER,
                "An empty scene. Add something with + in the scene list,\nor press Ctrl+K to open a scene.",
                egui::FontId::proportional(14.0),
                theme::SECONDARY,
            );
        }
    }

    // --- Search -----------------------------------------------------------------

    fn actions_matching(
        &self,
        query: &str,
        scenes_only: bool,
    ) -> Vec<(Action, String, &'static str)> {
        let mut all: Vec<(Action, String, &'static str)> = Vec::new();
        for scene in &self.scenes {
            all.push((
                Action::Open(scene.clone()),
                format!("Open {scene}"),
                "Scene",
            ));
        }
        if !scenes_only {
            let has_selection = self
                .selected_editable()
                .is_some_and(|id| id != self.doc.root);
            all.push((Action::Save, "Save".into(), "Ctrl+S"));
            if self.game.is_some() {
                all.push((Action::Stop, "Stop the game".into(), "F5"));
            } else {
                all.push((Action::Play, "Play the game".into(), "F5"));
            }
            all.push((Action::Undo, "Undo".into(), "Ctrl+Z"));
            all.push((Action::Redo, "Redo".into(), "Ctrl+Shift+Z"));
            all.push((Action::AddNode, "Add an empty node".into(), "Command"));
            if has_selection {
                all.push((
                    Action::Duplicate,
                    "Duplicate the selection".into(),
                    "Ctrl+D",
                ));
                all.push((Action::Delete, "Delete the selection".into(), "Del"));
            }
            all.push((Action::FrameSelection, "Frame the selection".into(), "F"));
            all.push((Action::FrameAll, "Frame everything".into(), "Home"));
            for scene in &self.scenes {
                if Some(scene.as_str()) != self.doc.path.as_deref() {
                    all.push((
                        Action::Place(scene.clone()),
                        format!("Place {scene} here"),
                        "Scene",
                    ));
                }
            }
            for id in self.doc.scene.ids() {
                if let Some(node) = self.doc.scene.get(id) {
                    if id != self.doc.root && !node.name.is_empty() {
                        all.push((Action::Select(id), node.name.clone(), "Node"));
                    }
                }
            }
        }
        let words: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        all.retain(|(_, label, kind)| {
            let haystack = format!("{} {}", label.to_lowercase(), kind.to_lowercase());
            words.iter().all(|w| haystack.contains(w.as_str()))
        });
        all
    }

    fn palette_window(&mut self, ctx: &egui::Context, gfx: &mut Gfx) {
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let matches = self.actions_matching(&palette.query, palette.scenes_only);
        let mut chosen: Option<Action> = None;
        let mut close = false;

        // Anchored near the top, so the box stays put while the list shrinks.
        let id = egui::Id::unique("brandon-search");
        let area = egui::Modal::default_area(id.with("area"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 96.0));
        let modal = egui::Modal::new(id)
            .area(area)
            .backdrop_color(egui::Color32::from_black_alpha(70))
            .frame(
                egui::Frame::new()
                    .fill(glass::GLASS)
                    .corner_radius(22)
                    .shadow(theme::panel_shadow())
                    .inner_margin(egui::Margin::same(8)),
            );
        let response = modal.show(ctx, |ui| {
            ui.set_width(560.0);
            let hint = if palette.scenes_only {
                "Open a scene"
            } else {
                "Search nodes, scenes and commands"
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (icon, _) = ui.allocate_exact_size(egui::vec2(34.0, 38.0), Sense::hover());
                widgets::paint_icon(
                    ui.painter(),
                    Icon::Search,
                    Rect::from_center_size(icon.center(), egui::Vec2::splat(18.0)),
                    theme::SECONDARY,
                );
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut palette.query)
                        .hint_text(RichText::new(hint).color(theme::TERTIARY))
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::proportional(18.0))
                        .margin(egui::Margin::symmetric(0, 8)),
                );
                if palette.focus {
                    edit.request_focus();
                    palette.focus = false;
                }
                if edit.changed() {
                    palette.selected = 0;
                }
            });
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    palette.selected = (palette.selected + 1).min(matches.len().saturating_sub(1));
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    palette.selected = palette.selected.saturating_sub(1);
                }
                if i.consume_key(Modifiers::NONE, Key::Enter) {
                    chosen = matches.get(palette.selected).map(|m| m.0.clone());
                }
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    close = true;
                }
            });
            let line = ui.available_rect_before_wrap();
            ui.painter().hline(
                (line.left() - 8.0)..=(line.right() + 8.0),
                line.top() + 2.0,
                egui::Stroke::new(1.0, theme::SEPARATOR),
            );
            ui.add_space(8.0);
            if matches.is_empty() {
                ui.add_space(10.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No Results").color(theme::SECONDARY));
                });
                ui.add_space(12.0);
                return;
            }
            egui::ScrollArea::vertical()
                .max_height(380.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (index, (action, label, kind)) in matches.iter().enumerate() {
                        let selected = index == palette.selected;
                        let row = palette_row(ui, action_icon(action), label, kind, selected);
                        if selected && row.rect.height() > 0.0 {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            chosen = Some(action.clone());
                        }
                    }
                });
        });
        if response.should_close() {
            close = true;
        }
        if let Some(action) = chosen {
            self.run(action, gfx);
        } else if !close {
            self.palette = Some(palette);
        }
    }
}

/// The icon beside a search result.
fn action_icon(action: &Action) -> Icon {
    match action {
        Action::Save => Icon::Save,
        Action::Undo => Icon::Undo,
        Action::Redo => Icon::Redo,
        Action::Play => Icon::Play,
        Action::Stop => Icon::Stop,
        Action::AddNode => Icon::Plus,
        Action::Duplicate => Icon::Overlay,
        Action::Delete => Icon::Close,
        Action::FrameSelection | Action::FrameAll => Icon::Frame,
        Action::Open(_) => Icon::Scene,
        Action::Place(_) => Icon::Placed,
        Action::Select(_) => Icon::Node,
        Action::Search => Icon::Search,
    }
}

/// One search result: an icon in a small tile, the label, and what kind of
/// thing it is on the right. The chosen row is filled with the accent colour.
fn palette_row(ui: &mut Ui, icon: Icon, label: &str, kind: &str, selected: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 12.0, theme::ACCENT);
    } else if response.hovered() {
        painter.rect_filled(rect, 12.0, theme::FILL_SOFT);
    }
    let tile = Rect::from_center_size(
        Pos2::new(rect.left() + 20.0, rect.center().y),
        egui::Vec2::splat(24.0),
    );
    let (tile_fill, icon_color) = if selected {
        (egui::Color32::from_white_alpha(46), egui::Color32::WHITE)
    } else if icon == Icon::Placed {
        (theme::PLACED.gamma_multiply(0.22), theme::PLACED)
    } else {
        (theme::FILL_SOFT, theme::TEXT)
    };
    painter.rect_filled(tile, 7.0, tile_fill);
    widgets::paint_icon(painter, icon, tile.shrink(5.0), icon_color);
    let text = if selected {
        egui::Color32::WHITE
    } else {
        theme::TEXT
    };
    painter.text(
        Pos2::new(rect.left() + 42.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.0),
        text,
    );
    painter.text(
        Pos2::new(rect.right() - 12.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        kind,
        egui::FontId::proportional(12.0),
        if selected {
            egui::Color32::from_white_alpha(190)
        } else {
            theme::TERTIARY
        },
    );
    response
}

/// What one row of the scene tree shows.
struct TreeRow {
    icon: Icon,
    name: String,
    /// Part of, or the root of, a scene placed from another file.
    placed: bool,
    hidden: bool,
    behaviours: usize,
    selected: bool,
}

/// One row of the scene tree: an icon, the name, and a small count of
/// behaviours. Painted by hand so it lines up exactly.
fn tree_row_widget(ui: &mut Ui, row: &TreeRow) -> egui::Response {
    let height = 28.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        Sense::click_and_drag(),
    );
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if row.selected {
            painter.rect_filled(rect, 9.0, theme::ACCENT);
        } else if response.hovered() {
            painter.rect_filled(rect, 9.0, theme::FILL_SOFT);
        }
        let dim = |c: egui::Color32| {
            if row.hidden {
                c.gamma_multiply(0.45)
            } else {
                c
            }
        };
        let (icon_color, text_color) = if row.selected {
            (egui::Color32::WHITE, egui::Color32::WHITE)
        } else if row.placed {
            (dim(theme::PLACED), dim(theme::TEXT))
        } else {
            (dim(theme::ACCENT), dim(theme::TEXT))
        };
        widgets::paint_icon(
            painter,
            row.icon,
            Rect::from_center_size(
                Pos2::new(rect.left() + 14.0, rect.center().y),
                egui::Vec2::splat(15.0),
            ),
            icon_color,
        );
        let text_left = rect.left() + 30.0;
        let galley = painter.layout_no_wrap(
            row.name.clone(),
            egui::FontId::proportional(13.0),
            text_color,
        );
        let pill_room = if row.behaviours > 0 { 30.0 } else { 0.0 };
        let max_width = (rect.right() - text_left - pill_room - 6.0).max(10.0);
        let clip = Rect::from_min_max(
            Pos2::new(text_left, rect.top()),
            Pos2::new(text_left + max_width, rect.bottom()),
        );
        painter
            .with_clip_rect(clip.intersect(painter.clip_rect()))
            .galley(
                Pos2::new(text_left, rect.center().y - galley.size().y / 2.0),
                galley.clone(),
                text_color,
            );
        if row.behaviours > 0 {
            let x = (text_left + galley.size().x + 8.0).min(rect.right() - pill_room);
            let pill =
                Rect::from_min_size(Pos2::new(x, rect.center().y - 8.0), egui::vec2(22.0, 16.0));
            let (fill, color) = if row.selected {
                (egui::Color32::from_white_alpha(56), egui::Color32::WHITE)
            } else {
                (theme::FILL, theme::SECONDARY)
            };
            painter.rect_filled(pill, 8.0, fill);
            painter.text(
                pill.center(),
                egui::Align2::CENTER_CENTER,
                row.behaviours.to_string(),
                egui::FontId::new(10.5, theme::semibold()),
                color,
            );
        }
    }
    match row.behaviours {
        0 => response,
        1 => response.on_hover_text("1 behaviour"),
        n => response.on_hover_text(format!("{n} behaviours")),
    }
}
