//! The Brandon editor.
//!
//! ```sh
//! brandon-editor [game folder] [scene]
//! ```
//!
//! The game folder is the one holding the game's `Cargo.toml` and its
//! `assets` folder. It defaults to the current folder. The scene is an asset
//! path such as `scenes/main.scene`; without one, the first scene found is
//! opened.

mod document;
mod editor;
mod fields;
mod glass;
mod inspector;
mod theme;
mod ui_render;
mod viewport;
mod widgets;

use brandon::draw::{build_frame, Textures};
use brandon_core::{Assets, Color};
use brandon_render::{Renderer, TextRenderer, Texture};
use document::Document;
use editor::Editor;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ui_render::UiTextures;
use viewport::Gfx;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct Graphics {
    renderer: Renderer,
    text: TextRenderer,
    textures: Textures,
    ui_textures: UiTextures,
    /// The soft coloured backdrop behind the scene, which the glass panels
    /// pick up colour from.
    wallpaper: Texture,
    egui_state: egui_winit::State,
    window: Arc<Window>,
}

/// Saves one frame as a PNG and quits, for automated checks:
/// `BRANDON_CAPTURE=out.png BRANDON_CAPTURE_FRAME=10`.
struct Capture {
    path: String,
    frame: u64,
}

struct App {
    editor: Editor,
    egui: egui::Context,
    graphics: Option<Graphics>,
    capture: Option<Capture>,
    frames: u64,
    error: Option<String>,
    next_wake: Option<Instant>,
}

fn main() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
    // The editor does not have the game's code, so it keeps the game's
    // behaviours as data instead of refusing to open scenes that use them.
    brandon_scene::keep_unknown_behaviours(true);

    let mut args = std::env::args().skip(1);
    let project = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let scene = args.next();
    let project = project.canonicalize().unwrap_or(project);
    let assets_folder = project.join("assets");
    if !assets_folder.is_dir() {
        eprintln!(
            "There is no assets folder in {}. Run the editor from a game's folder, or pass it: brandon-editor path/to/game",
            project.display()
        );
        std::process::exit(1);
    }

    let mut editor = Editor::new(Document::new(Assets::at(&assets_folder)), project);
    let first_scene = scene.or_else(|| {
        let scenes = editor.doc.files(&["scene"]);
        scenes
            .iter()
            .find(|s| s.ends_with("main.scene"))
            .or(scenes.first())
            .cloned()
    });
    if let Some(path) = first_scene {
        editor.open(&path);
    }

    let capture = std::env::var("BRANDON_CAPTURE")
        .ok()
        .filter(|p| !p.is_empty())
        .map(|path| Capture {
            path,
            frame: std::env::var("BRANDON_CAPTURE_FRAME")
                .ok()
                .and_then(|f| f.parse().ok())
                .unwrap_or(10),
        });
    if let Ok(size) = std::env::var("BRANDON_EDITOR_SCREEN") {
        if let Some((w, h)) = size
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        {
            editor.screen = glam::Vec2::new(w, h);
        }
    }

    let egui = egui::Context::default();
    theme::apply(&egui);
    let mut app = App {
        editor,
        egui,
        graphics: None,
        capture,
        frames: 0,
        error: None,
        next_wake: None,
    };

    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("Brandon editor could not open a window: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = event_loop.run_app(&mut app) {
        app.error.get_or_insert(error.to_string());
    }
    app.graphics = None;
    if Renderer::validation_errors() > 0 {
        log::error!(
            "the Vulkan validation layers reported {} error(s)",
            Renderer::validation_errors()
        );
    }
    if let Some(error) = app.error {
        eprintln!("Brandon editor stopped: {error}");
        std::process::exit(1);
    }
}

impl App {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let attributes = Window::default_attributes()
            .with_title(self.editor.title())
            .with_inner_size(LogicalSize::new(1440, 900))
            .with_min_inner_size(LogicalSize::new(800, 500));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| e.to_string())?,
        );
        let size = window.inner_size();
        let mut renderer =
            Renderer::new(window.clone(), size.width, size.height).map_err(|e| e.to_string())?;
        // Small and smooth: it is stretched over the window and only holds
        // gentle gradients.
        let (w, h) = (192, 120);
        let wallpaper = renderer
            .create_texture(w, h, &viewport::wallpaper_image(w, h))
            .map_err(|e| e.to_string())?;
        let egui_state = egui_winit::State::new(
            self.egui.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(4096),
        );
        self.graphics = Some(Graphics {
            renderer,
            text: TextRenderer::new(),
            textures: Textures::new(),
            ui_textures: UiTextures::default(),
            wallpaper,
            egui_state,
            window,
        });
        Ok(())
    }

    /// Draws the scene, the grid and the editor's UI.
    fn draw(
        editor: &mut Editor,
        egui: &egui::Context,
        g: &mut Graphics,
        shapes: Vec<egui::epaint::ClippedShape>,
        ppp: f32,
        textures_delta: &mut egui::epaint::textures::TexturesDelta,
        capturing: bool,
    ) -> Result<(), String> {
        let size = g.window.inner_size();
        let points = glam::Vec2::new(size.width as f32, size.height as f32) / ppp;
        editor.doc.scene.refresh_transforms();
        let view = editor.view();
        let mut frame = build_frame(
            &editor.doc.scene,
            &mut g.renderer,
            &mut g.text,
            &mut g.textures,
            &editor.doc.assets,
            Color::hex(0x101217),
            points,
            ppp,
            Some(view),
        )
        .map_err(|e| e.to_string())?;
        let grid = viewport::grid_layer(
            &editor.camera,
            editor.canvas,
            view,
            editor.screen,
            &g.renderer,
        );
        frame.layers.insert(0, grid);
        frame
            .layers
            .insert(0, viewport::wallpaper_layer(g.wallpaper, points));

        g.ui_textures
            .update(&mut g.renderer, textures_delta)
            .map_err(|e| e.to_string())?;
        // Panels marked as glass become blurred, lit glass over the scene.
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(points.x, points.y));
        let shapes = glass::apply(shapes, screen, 1.0 / ppp);
        let primitives = egui.tessellate(shapes, ppp);
        frame.ui = g.ui_textures.meshes(primitives, g.renderer.backdrop());
        if capturing {
            g.renderer.request_capture();
        }
        g.renderer.render(&frame).map_err(|e| e.to_string())?;
        g.ui_textures.free(&mut g.renderer, textures_delta);
        Ok(())
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let Some(g) = &mut self.graphics else {
            return Ok(());
        };
        let size = g.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        self.frames += 1;

        let input = g.egui_state.take_egui_input(&g.window);
        let editor = &mut self.editor;
        let mut gfx = Gfx {
            renderer: &mut g.renderer,
            text: &mut g.text,
            textures: &mut g.textures,
        };
        let output = self.egui.run_ui(input, |ui| editor.ui(ui, &mut gfx));
        let mut textures_delta = output.textures_delta;
        let result = Self::draw(
            &mut self.editor,
            &self.egui,
            g,
            output.shapes,
            output.pixels_per_point,
            &mut textures_delta,
            self.capture
                .as_ref()
                .is_some_and(|c| self.frames >= c.frame),
        );
        // egui insists every texture change is handled, even after an error.
        textures_delta.clear();
        result?;
        g.egui_state
            .handle_platform_output(&g.window, output.platform_output);
        g.window.set_title(&self.editor.title());

        let capturing = self
            .capture
            .as_ref()
            .is_some_and(|c| self.frames >= c.frame);

        if capturing {
            if let (Some(image), Some(capture)) = (g.renderer.take_capture(), &self.capture) {
                image::save_buffer(
                    &capture.path,
                    &image.rgba,
                    image.width,
                    image.height,
                    image::ColorType::Rgba8,
                )
                .map_err(|e| e.to_string())?;
                println!("Saved frame {} to {}", self.frames, capture.path);
                event_loop.exit();
            }
        }
        if self.editor.quit {
            event_loop.exit();
        }

        // Draw again soon if egui asks for it, or while capturing.
        let delay = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay)
            .unwrap_or(Duration::MAX);
        if self.capture.is_some() || delay.is_zero() {
            g.window.request_redraw();
            self.next_wake = None;
        } else {
            let mut wait = delay.min(Duration::from_secs(1));
            if self.editor.busy() {
                wait = wait.min(Duration::from_millis(250));
            }
            self.next_wake = Some(Instant::now() + wait);
        }
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.graphics.is_none() {
            if let Err(error) = self.start(event_loop) {
                self.error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window: WindowId,
        event: WindowEvent,
    ) {
        let Some(g) = &mut self.graphics else { return };
        let response = g.egui_state.on_window_event(&g.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => g.renderer.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.frame(event_loop) {
                    self.error = Some(error);
                    event_loop.exit();
                }
                return;
            }
            _ => {}
        }
        if response.repaint {
            g.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.next_wake {
            Some(when) if when <= Instant::now() => {
                self.next_wake = None;
                if let Some(g) = &self.graphics {
                    g.window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Some(when) => event_loop.set_control_flow(ControlFlow::WaitUntil(when)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}
