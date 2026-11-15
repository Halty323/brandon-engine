//! The game loop: window events in, frames out.

use crate::draw::{build_frame, Textures};
use crate::game::{Error, Game, Setup};
use crate::keys::{button_from_winit, key_from_winit};
use brandon_core::{Assets, Color, Input, Random, Time};
use brandon_render::{Renderer, TextRenderer};
use brandon_scene::{Requests, Scene, SceneLibrary, View};
use glam::Vec2;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

/// Everything that exists only while there is a window.
struct Graphics {
    // Dropped in this order: the renderer goes before its window.
    renderer: Renderer,
    text: TextRenderer,
    textures: Textures,
    window: Arc<Window>,
}

/// A request to save one frame as a PNG and quit, for automated checks.
///
/// Set `BRANDON_CAPTURE=out.png` (and optionally `BRANDON_CAPTURE_FRAME=120`).
/// While capturing, every frame advances time by exactly 1/60 s, so the
/// picture is the same on every run.
struct Capture {
    path: String,
    frame: u64,
}

struct App {
    title: String,
    size: (u32, u32),
    resizable: bool,
    background: Color,
    start_scene: Option<String>,
    setup: Option<Setup>,
    assets: Assets,
    library: SceneLibrary,
    scene: Scene,
    input: Input,
    time: Time,
    random: Random,
    last_frame: Option<Instant>,
    graphics: Option<Graphics>,
    capture: Option<Capture>,
    error: Option<Error>,
}

pub(crate) fn run(game: Game) -> Result<(), Error> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();

    let capture = std::env::var("BRANDON_CAPTURE")
        .ok()
        .filter(|p| !p.is_empty())
        .map(|path| Capture {
            path,
            frame: std::env::var("BRANDON_CAPTURE_FRAME")
                .ok()
                .and_then(|f| f.parse().ok())
                .unwrap_or(60),
        });
    let mut app = App {
        title: game.title,
        size: (game.width, game.height),
        resizable: game.resizable,
        background: game.background,
        start_scene: game.start_scene,
        setup: game.setup,
        assets: Assets::discover(&game.assets_folder),
        library: SceneLibrary::new(),
        scene: Scene::new(),
        input: Input::default(),
        time: Time::default(),
        // Capturing must give the same picture every run.
        random: if capture.is_some() {
            Random::from_seed(1)
        } else {
            Random::new()
        },
        last_frame: None,
        graphics: None,
        capture,
        error: None,
    };

    let event_loop = EventLoop::new().map_err(|e| Error::Window(e.to_string()))?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop
        .run_app(&mut app)
        .map_err(|e| Error::Window(e.to_string()))?;
    // Let go of the renderer before the event loop and its display connection.
    app.graphics = None;
    let validation_errors = Renderer::validation_errors();
    if validation_errors > 0 {
        log::error!(
            "the Vulkan validation layers reported {validation_errors} error(s) during this run"
        );
    }
    match app.error.take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

impl App {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Error> {
        let attributes = Window::default_attributes()
            .with_title(self.title.clone())
            .with_inner_size(LogicalSize::new(self.size.0, self.size.1))
            .with_resizable(self.resizable);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| Error::Window(e.to_string()))?,
        );
        let pixels = window.inner_size();
        let renderer = Renderer::new(window.clone(), pixels.width, pixels.height)?;
        self.graphics = Some(Graphics {
            renderer,
            text: TextRenderer::new(),
            textures: Textures::new(),
            window,
        });

        if let Some(path) = self.start_scene.clone() {
            self.load_scene(&path)?;
        }
        if let Some(setup) = self.setup.take() {
            setup(&mut self.scene);
        }
        Ok(())
    }

    fn load_scene(&mut self, path: &str) -> Result<(), Error> {
        let root = self.library.load(&self.assets, path)?;
        self.scene.clear();
        self.scene.spawn(root);
        Ok(())
    }

    /// The window size in logical pixels, which is what the game sees.
    fn window_size(graphics: &Graphics) -> Vec2 {
        let size = graphics
            .window
            .inner_size()
            .to_logical::<f32>(graphics.window.scale_factor());
        Vec2::new(size.width.max(1.0), size.height.max(1.0))
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Error> {
        let Some(graphics) = &mut self.graphics else {
            return Ok(());
        };

        let now = Instant::now();
        let delta = match (&self.capture, self.last_frame) {
            (Some(_), _) => 1.0 / 60.0,
            (None, Some(last)) => now.duration_since(last).as_secs_f32(),
            (None, None) => 1.0 / 60.0,
        };
        self.last_frame = Some(now);
        self.time.advance(delta);

        // Update.
        let window_size = Self::window_size(graphics);
        self.scene.refresh_transforms();
        let view = View {
            size: window_size,
            screen_from_world: self.scene.view_transform(window_size),
        };
        let mut requests = Requests::default();
        self.scene.update(
            &self.input,
            &self.time,
            &mut self.random,
            &self.assets,
            &self.library,
            &view,
            &mut requests,
        );
        self.input.end_frame();
        if let Some(path) = requests.change_scene {
            if let Err(error) = self.load_scene(&path) {
                log::error!("could not change scene: {error}");
            }
        }
        if requests.quit {
            event_loop.exit();
        }

        // Draw.
        let Some(graphics) = &mut self.graphics else {
            return Ok(());
        };
        let pixels = graphics.window.inner_size();
        if pixels.width == 0 || pixels.height == 0 {
            std::thread::sleep(Duration::from_millis(20)); // Minimised: nothing to draw.
            return Ok(());
        }
        self.scene.refresh_transforms();
        let frame = build_frame(
            &self.scene,
            &mut graphics.renderer,
            &mut graphics.text,
            &mut graphics.textures,
            &self.assets,
            self.background,
            window_size,
            graphics.window.scale_factor() as f32,
        )?;
        let capturing = self
            .capture
            .as_ref()
            .is_some_and(|c| self.time.frame() >= c.frame);
        if capturing {
            graphics.renderer.request_capture();
        }
        graphics.renderer.render(&frame)?;

        if capturing {
            if let (Some(image), Some(capture)) = (graphics.renderer.take_capture(), &self.capture)
            {
                match image::save_buffer(
                    &capture.path,
                    &image.rgba,
                    image.width,
                    image.height,
                    image::ColorType::Rgba8,
                ) {
                    Ok(()) => println!("Saved frame {} to {}", self.time.frame(), capture.path),
                    Err(error) => log::error!("could not save the captured frame: {error}"),
                }
                event_loop.exit();
            } else if self.time.frame() > self.capture.as_ref().map_or(0, |c| c.frame) + 120 {
                log::error!("no frame could be captured");
                event_loop.exit();
            }
        }
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.graphics.is_none() {
            if let Err(error) = self.start(event_loop) {
                self.fail(event_loop, error);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window: WindowId,
        event: WindowEvent,
    ) {
        let Some(graphics) = &mut self.graphics else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => graphics.renderer.resize(size.width, size.height),
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = graphics.window.inner_size();
                graphics.renderer.resize(size.width, size.height);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(key) = key_from_winit(code) {
                        self.input
                            .set_key(key, event.state == ElementState::Pressed);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let logical = position.to_logical::<f32>(graphics.window.scale_factor());
                self.input
                    .set_mouse_position(Vec2::new(logical.x, logical.y));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(button) = button_from_winit(button) {
                    self.input
                        .set_mouse_button(button, state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y),
                    MouseScrollDelta::PixelDelta(p) => Vec2::new(p.x as f32, p.y as f32) / 40.0,
                };
                self.input.add_scroll(lines);
            }
            WindowEvent::Focused(false) => self.input.release_all(),
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.frame(event_loop) {
                    self.fail(event_loop, error);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(graphics) = &self.graphics {
            graphics.window.request_redraw();
        }
    }
}
