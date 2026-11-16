//! Turns a scene into quads for the renderer.
//!
//! Games do not need this module: [`Game`](crate::Game) draws the scene for
//! you. It is public for tools that draw scenes themselves, such as the
//! editor.

use brandon_core::{Assets, Color, ImageData};
use brandon_render::{
    Filter, Frame, Layer, Quad, RenderError, Renderer, TextLayout, TextRenderer, Texture,
};
use brandon_scene::{Align, Node, Scene, Sprite, Visible};
use glam::{Affine2, Vec2};
use std::collections::HashMap;
use std::rc::Rc;

/// Loads textures on first use and remembers them.
pub struct Textures {
    loaded: HashMap<String, Texture>,
    missing: Option<Texture>,
}

impl Default for Textures {
    fn default() -> Self {
        Self::new()
    }
}

impl Textures {
    pub fn new() -> Self {
        Textures {
            loaded: HashMap::new(),
            missing: None,
        }
    }

    /// The texture for an asset path. If it cannot be loaded, the problem is
    /// logged once and a bright checkerboard is shown instead.
    pub fn get(&mut self, renderer: &mut Renderer, assets: &Assets, path: &str) -> Texture {
        if let Some(texture) = self.loaded.get(path) {
            return *texture;
        }
        let texture = assets
            .load_image(path)
            .map_err(|e| e.to_string())
            .and_then(|image: ImageData| {
                renderer
                    .create_texture(image.width, image.height, &image.rgba)
                    .map_err(|e| e.to_string())
            })
            .unwrap_or_else(|problem| {
                log::error!("{problem}");
                self.missing(renderer)
            });
        self.loaded.insert(path.to_string(), texture);
        texture
    }

    fn missing(&mut self, renderer: &mut Renderer) -> Texture {
        if let Some(texture) = self.missing {
            return texture;
        }
        let (m, k) = ([255, 0, 255, 255], [0, 0, 0, 255]);
        let pixels: Vec<u8> = [m, k, k, m].concat();
        let texture = renderer
            .create_texture(2, 2, &pixels)
            .unwrap_or_else(|_| renderer.white());
        self.missing = Some(texture);
        texture
    }

    /// Is this the stand-in shown for images that could not be loaded?
    pub fn is_missing(&self, texture: Texture) -> bool {
        self.missing == Some(texture)
    }

    /// Forgets every loaded image, so files changed on disk are read again.
    pub fn clear(&mut self, renderer: &mut Renderer) {
        for (_, texture) in self.loaded.drain() {
            if Some(texture) != self.missing {
                renderer.destroy_texture(texture);
            }
        }
    }
}

struct PendingText {
    layout: Rc<TextLayout>,
    world: Affine2,
    color: Color,
    z: i32,
    overlay: bool,
    /// Where in the layer's quads the glyphs belong, to keep tree order.
    at: usize,
}

/// The size a sprite is drawn at, given its texture.
fn sprite_size(sprite: &Sprite, texture: Texture, missing: bool) -> Vec2 {
    let has_image = sprite.texture.is_some() && !missing;
    sprite
        .size
        .or(sprite.region.filter(|_| has_image).map(|r| r.size()))
        .or(has_image.then_some(texture.size()))
        .unwrap_or(Sprite::DEFAULT_SIZE)
}

/// The area a node's sprite and text cover, in the node's own space, as
/// `(top-left corner, size)`. `None` if the node draws nothing.
pub fn local_bounds(
    node: &Node,
    renderer: &mut Renderer,
    text: &mut TextRenderer,
    textures: &mut Textures,
    assets: &Assets,
) -> Option<(Vec2, Vec2)> {
    let mut bounds: Option<(Vec2, Vec2)> = None;
    let mut include = |min: Vec2, max: Vec2| {
        bounds = Some(match bounds {
            Some((lo, hi)) => (lo.min(min), hi.max(max)),
            None => (min, max),
        });
    };
    if let Some(sprite) = &node.sprite {
        let (texture, missing) = match &sprite.texture {
            Some(path) => {
                let texture = textures.get(renderer, assets, path);
                (texture, textures.is_missing(texture))
            }
            None => (renderer.white(), false),
        };
        let size = sprite_size(sprite, texture, missing);
        let corner = -sprite.anchor * size;
        include(corner, corner + size);
    }
    if let Some(label) = &node.text {
        let align = align_factor(label.align);
        let layout = text.layout(&label.text, label.size, 1.0, align);
        let left = -layout.width * align;
        include(
            Vec2::new(left, 0.0),
            Vec2::new(left + layout.width, layout.height),
        );
    }
    bounds.map(|(min, max)| (min, max - min))
}

fn align_factor(align: Align) -> f32 {
    match align {
        Align::Left => 0.0,
        Align::Center => 0.5,
        Align::Right => 1.0,
    }
}

fn sprite_quad(sprite: &Sprite, texture: Texture, missing: bool, at: &Visible) -> Quad {
    let texture_size = texture.size();
    let region = sprite
        .region
        .filter(|_| sprite.texture.is_some() && !missing);
    let size = sprite_size(sprite, texture, missing);
    let mut uv = match region {
        Some(r) => [
            r.x / texture_size.x,
            r.y / texture_size.y,
            (r.x + r.w) / texture_size.x,
            (r.y + r.h) / texture_size.y,
        ],
        None => [0.0, 0.0, 1.0, 1.0],
    };
    if sprite.flip_x {
        uv.swap(0, 2);
    }
    if sprite.flip_y {
        uv.swap(1, 3);
    }
    Quad {
        origin: at.world.transform_point2(-sprite.anchor * size),
        x_axis: at.world.transform_vector2(Vec2::new(size.x, 0.0)),
        y_axis: at.world.transform_vector2(Vec2::new(0.0, size.y)),
        uv,
        color: if missing { Color::WHITE } else { sprite.color },
        texture,
        filter: if sprite.smooth && !missing {
            Filter::Smooth
        } else {
            Filter::Pixelated
        },
        z: at.z,
    }
}

/// Builds the frame to draw: the world as seen by the camera, then overlays
/// fixed to the window.
///
/// With `view_override`, the scene's own camera is ignored and everything,
/// overlays included, is drawn through that view. The editor uses this to
/// look at a scene from anywhere.
#[allow(clippy::too_many_arguments)]
pub fn build_frame(
    scene: &Scene,
    renderer: &mut Renderer,
    text: &mut TextRenderer,
    textures: &mut Textures,
    assets: &Assets,
    clear: Color,
    window_size: Vec2,
    pixels_per_unit: f32,
    view_override: Option<Affine2>,
) -> Result<Frame, RenderError> {
    let view = view_override.unwrap_or_else(|| scene.view_transform(window_size));
    let overlay_view = view_override.unwrap_or(Affine2::IDENTITY);
    let overlay_zoom = overlay_view.matrix2.x_axis.length();
    let view_zoom = view.matrix2.x_axis.length();
    let mut world_quads: Vec<Quad> = Vec::new();
    let mut overlay_quads: Vec<Quad> = Vec::new();
    let mut texts: Vec<PendingText> = Vec::new();

    scene.for_each_visible(|node, at| {
        let quads = if at.overlay {
            &mut overlay_quads
        } else {
            &mut world_quads
        };
        if let Some(sprite) = &node.sprite {
            let (texture, missing) = match &sprite.texture {
                Some(path) => {
                    let texture = textures.get(renderer, assets, path);
                    (texture, textures.is_missing(texture))
                }
                None => (renderer.white(), false),
            };
            quads.push(sprite_quad(sprite, texture, missing, &at));
        }
        if let Some(label) = &node.text {
            // Rasterise glyphs at the size they appear on screen, so text
            // stays sharp under scaling and zoom.
            let zoom = if at.overlay { overlay_zoom } else { view_zoom };
            let scale = quantise(pixels_per_unit * at.world.matrix2.y_axis.length() * zoom);
            let align = align_factor(label.align);
            texts.push(PendingText {
                layout: text.layout(&label.text, label.size, scale, align),
                world: at.world,
                color: label.color,
                z: at.z,
                overlay: at.overlay,
                at: quads.len(),
            });
        }
    });

    // Every glyph is in the atlas now, so its final texture coordinates are known.
    let atlas = text.prepare(renderer)?;
    // Insert from the back so earlier insertion points stay valid.
    for pending in texts.iter().rev() {
        let quads = if pending.overlay {
            &mut overlay_quads
        } else {
            &mut world_quads
        };
        let glyphs = pending.layout.glyphs.iter().map(|glyph| Quad {
            origin: pending.world.transform_point2(Vec2::new(glyph.x, glyph.y)),
            x_axis: pending.world.transform_vector2(Vec2::new(glyph.w, 0.0)),
            y_axis: pending.world.transform_vector2(Vec2::new(0.0, glyph.h)),
            uv: text.uv(glyph),
            color: pending.color,
            texture: atlas,
            filter: Filter::Smooth,
            z: pending.z,
        });
        quads.splice(pending.at..pending.at, glyphs);
    }

    Ok(Frame {
        clear,
        size: window_size,
        layers: vec![
            Layer {
                view,
                quads: world_quads,
            },
            Layer {
                view: overlay_view,
                quads: overlay_quads,
            },
        ],
        ui: Vec::new(),
    })
}

/// Rounds a text scale to steps of a quarter, so smoothly zooming text does
/// not rasterise a new set of glyphs every frame.
fn quantise(scale: f32) -> f32 {
    if !scale.is_finite() || scale <= 0.0 {
        return 1.0;
    }
    ((scale * 4.0).round() / 4.0).clamp(0.25, 8.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brandon_core::Rect;

    fn texture_64x32() -> Texture {
        // Only the size matters here; the renderer is not involved.
        Texture::placeholder(64, 32)
    }

    fn at(x: f32, y: f32) -> Visible {
        Visible {
            world: Affine2::from_translation(Vec2::new(x, y)),
            z: 3,
            overlay: false,
        }
    }

    #[test]
    fn sprite_is_centred_and_sized_by_its_texture() {
        let quad = sprite_quad(
            &Sprite::new("a.png"),
            texture_64x32(),
            false,
            &at(100.0, 100.0),
        );
        assert_eq!(quad.origin, Vec2::new(68.0, 84.0));
        assert_eq!(
            (quad.x_axis, quad.y_axis),
            (Vec2::new(64.0, 0.0), Vec2::new(0.0, 32.0))
        );
        assert_eq!(quad.uv, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(quad.z, 3);
    }

    #[test]
    fn region_anchor_flip_and_size() {
        let sprite = Sprite::new("a.png")
            .with_region(Rect::new(32.0, 0.0, 32.0, 32.0))
            .with_anchor(0.0, 0.0);
        let quad = sprite_quad(&sprite, texture_64x32(), false, &at(10.0, 20.0));
        assert_eq!(quad.origin, Vec2::new(10.0, 20.0));
        assert_eq!(quad.x_axis, Vec2::new(32.0, 0.0), "sized by the region");
        assert_eq!(quad.uv, [0.5, 0.0, 1.0, 1.0]);

        let flipped = Sprite {
            flip_x: true,
            ..sprite.clone()
        }
        .with_size(10.0, 10.0);
        let quad = sprite_quad(&flipped, texture_64x32(), false, &at(0.0, 0.0));
        assert_eq!(quad.uv, [1.0, 0.0, 0.5, 1.0]);
        assert_eq!(quad.y_axis, Vec2::new(0.0, 10.0));
    }

    #[test]
    fn plain_rectangle_and_missing_texture() {
        let rect = sprite_quad(
            &Sprite::rect(20.0, 10.0, Color::RED),
            texture_64x32(),
            false,
            &at(0.0, 0.0),
        );
        assert_eq!(
            (rect.x_axis.x, rect.y_axis.y, rect.color),
            (20.0, 10.0, Color::RED)
        );

        let missing = sprite_quad(
            &Sprite::new("gone.png").with_color(Color::RED),
            texture_64x32(),
            true,
            &at(0.0, 0.0),
        );
        assert_eq!(missing.x_axis.x, Sprite::DEFAULT_SIZE.x);
        assert_eq!(
            (missing.color, missing.filter),
            (Color::WHITE, Filter::Pixelated)
        );
    }

    #[test]
    fn text_scale_is_quantised() {
        assert_eq!(quantise(1.1), 1.0);
        assert_eq!(quantise(1.2), 1.25);
        assert_eq!(quantise(0.0), 1.0);
        assert_eq!(quantise(100.0), 8.0);
    }
}
