use crate::app;
use brandon_core::Color;
use brandon_render::RenderError;
use brandon_scene::{Behaviour, Scene, SceneError};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// A reason a game could not start or had to stop.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    Scene(#[from] SceneError),
    #[error("window error: {0}")]
    Window(String),
}

pub(crate) type Setup = Box<dyn FnOnce(&mut Scene)>;

/// Describes a game: its window and what it starts with.
///
/// ```no_run
/// use brandon::prelude::*;
///
/// Game::new().title("Starfall").window_size(960, 540).start_scene("scenes/main.scene").run();
/// ```
pub struct Game {
    pub(crate) title: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) resizable: bool,
    pub(crate) background: Color,
    pub(crate) assets_folder: String,
    pub(crate) start_scene: Option<String>,
    pub(crate) setup: Option<Setup>,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Self {
        Game {
            title: "Brandon game".to_string(),
            width: 1280,
            height: 720,
            resizable: true,
            background: Color::hex(0x14161f),
            assets_folder: "assets".to_string(),
            start_scene: None,
            setup: None,
        }
    }

    /// The window title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The size of the window when it opens.
    pub fn window_size(mut self, width: u32, height: u32) -> Self {
        self.width = width.max(1);
        self.height = height.max(1);
        self
    }

    /// Whether the player can resize the window. On by default.
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// The colour behind everything.
    pub fn background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }

    /// The folder asset paths are relative to. The default is `assets`,
    /// looked for next to where the game runs from and in the crate folder.
    pub fn assets(mut self, folder: impl Into<String>) -> Self {
        self.assets_folder = folder.into();
        self
    }

    /// Makes a behaviour usable in scene files. Same as [`brandon::register`](crate::register).
    pub fn register<T>(self) -> Self
    where
        T: Behaviour + Serialize + DeserializeOwned + Clone,
    {
        brandon_scene::register::<T>();
        self
    }

    /// The scene file to load when the game starts.
    pub fn start_scene(mut self, path: impl Into<String>) -> Self {
        self.start_scene = Some(path.into());
        self
    }

    /// Builds the starting scene in code. Runs after the start scene, if
    /// there is one, has been loaded.
    pub fn setup(mut self, setup: impl FnOnce(&mut Scene) + 'static) -> Self {
        self.setup = Some(Box::new(setup));
        self
    }

    /// Runs the game until its window is closed. If the game cannot start,
    /// the problem is printed and the program exits.
    pub fn run(self) {
        if let Err(error) = self.try_run() {
            eprintln!("Brandon could not run the game: {error}");
            std::process::exit(1);
        }
    }

    /// Runs the game until its window is closed, returning any problem.
    pub fn try_run(self) -> Result<(), Error> {
        app::run(self)
    }
}
