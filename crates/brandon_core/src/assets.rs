use std::path::{Path, PathBuf};

/// Something went wrong loading an asset.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("asset `{path}` was not found (looked in: {})", searched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))]
    NotFound {
        path: String,
        searched: Vec<PathBuf>,
    },
    #[error("could not read `{path}`: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("could not decode image `{path}`: {message}")]
    Image { path: String, message: String },
}

/// A decoded image: 8-bit RGBA, rows from top to bottom.
#[derive(Clone, Debug)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Finds and reads asset files.
///
/// Asset paths are written relative to the game's `assets` folder, with
/// forward slashes, for example `"sprites/player.png"`.
#[derive(Clone, Debug)]
pub struct Assets {
    roots: Vec<PathBuf>,
}

impl Assets {
    /// Looks for a folder called `folder` next to where the game is run from,
    /// in the crate folder when run with `cargo run`, and next to the
    /// executable.
    pub fn discover(folder: impl AsRef<Path>) -> Self {
        let folder = folder.as_ref();
        let mut roots = Vec::new();
        if folder.is_absolute() {
            roots.push(folder.to_path_buf());
        } else {
            if let Ok(cwd) = std::env::current_dir() {
                roots.push(cwd.join(folder));
            }
            if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
                roots.push(PathBuf::from(manifest).join(folder));
            }
            if let Some(exe_dir) = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
            {
                roots.push(exe_dir.join(folder));
            }
        }
        roots.dedup();
        Self { roots }
    }

    /// Uses exactly this folder.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            roots: vec![root.into()],
        }
    }

    /// The folders that are searched, in order.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Finds the file for an asset path.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, AssetError> {
        let direct = Path::new(path);
        if direct.is_absolute() && direct.is_file() {
            return Ok(direct.to_path_buf());
        }
        self.roots
            .iter()
            .map(|root| root.join(path))
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| AssetError::NotFound {
                path: path.to_string(),
                searched: self.roots.clone(),
            })
    }

    /// Reads a whole asset file.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, AssetError> {
        let file = self.resolve(path)?;
        std::fs::read(&file).map_err(|source| AssetError::Io {
            path: path.to_string(),
            source,
        })
    }

    /// Reads an asset file as text.
    pub fn read_text(&self, path: &str) -> Result<String, AssetError> {
        let file = self.resolve(path)?;
        std::fs::read_to_string(&file).map_err(|source| AssetError::Io {
            path: path.to_string(),
            source,
        })
    }

    /// Reads and decodes an image (PNG or JPEG).
    pub fn load_image(&self, path: &str) -> Result<ImageData, AssetError> {
        let bytes = self.read(path)?;
        let decoded = image::load_from_memory(&bytes)
            .map_err(|e| AssetError::Image {
                path: path.to_string(),
                message: e.to_string(),
            })?
            .into_rgba8();
        Ok(ImageData {
            width: decoded.width(),
            height: decoded.height(),
            rgba: decoded.into_raw(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_asset_names_the_searched_folders() {
        let assets = Assets::at("/definitely/not/here");
        let err = assets.read("x.png").unwrap_err().to_string();
        assert!(
            err.contains("x.png") && err.contains("/definitely/not/here"),
            "{err}"
        );
    }
}
