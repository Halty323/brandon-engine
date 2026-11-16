//! Turns egui's output into meshes for Brandon's renderer.

use brandon_render::{Filter, Mesh, MeshVertex, RenderError, Renderer, Texture};
use egui::epaint::textures::TexturesDelta;
use egui::epaint::{ImageDelta, Primitive, TextureId};
use egui::{ClippedPrimitive, TextureFilter};
use std::collections::HashMap;

struct UiTexture {
    texture: Texture,
    width: usize,
    height: usize,
    /// A copy of the pixels, so partial updates can be applied.
    pixels: Vec<u8>,
    filter: Filter,
}

/// Keeps egui's textures on the graphics card.
#[derive(Default)]
pub struct UiTextures {
    textures: HashMap<TextureId, UiTexture>,
}

impl UiTextures {
    /// Applies new and changed textures. Call before drawing.
    pub fn update(
        &mut self,
        renderer: &mut Renderer,
        delta: &mut TexturesDelta,
    ) -> Result<(), RenderError> {
        let mut result = Ok(());
        for (id, images) in delta.set.drain() {
            for image in &images {
                if result.is_ok() {
                    result = self.set(renderer, id, image);
                }
            }
        }
        result
    }

    /// Frees textures egui no longer needs. Call after drawing.
    pub fn free(&mut self, renderer: &mut Renderer, delta: &mut TexturesDelta) {
        for id in delta.free.drain() {
            if let Some(old) = self.textures.remove(&id) {
                renderer.destroy_texture(old.texture);
            }
        }
    }

    fn set(
        &mut self,
        renderer: &mut Renderer,
        id: TextureId,
        delta: &ImageDelta,
    ) -> Result<(), RenderError> {
        let egui::ImageData::Color(image) = &delta.image;
        let [w, h] = image.size;
        let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
        let filter = match delta.options.magnification {
            TextureFilter::Nearest => Filter::Pixelated,
            TextureFilter::Linear => Filter::Smooth,
        };
        match (delta.pos, self.textures.get_mut(&id)) {
            (Some([x, y]), Some(existing)) => {
                for row in 0..h {
                    let target = ((y + row) * existing.width + x) * 4;
                    let source = row * w * 4;
                    let Some(dst) = existing.pixels.get_mut(target..target + w * 4) else {
                        continue;
                    };
                    dst.copy_from_slice(&bytes[source..source + w * 4]);
                }
                renderer.update_texture(existing.texture, &existing.pixels)?;
            }
            (Some(_), None) => log::warn!("egui updated part of a texture that does not exist"),
            (None, Some(existing)) if existing.width == w && existing.height == h => {
                existing.pixels = bytes;
                existing.filter = filter;
                renderer.update_texture(existing.texture, &existing.pixels)?;
            }
            (None, _) => {
                if let Some(old) = self.textures.remove(&id) {
                    renderer.destroy_texture(old.texture);
                }
                let texture = renderer.create_premultiplied_texture(w as u32, h as u32, &bytes)?;
                self.textures.insert(
                    id,
                    UiTexture {
                        texture,
                        width: w,
                        height: h,
                        pixels: bytes,
                        filter,
                    },
                );
            }
        }
        Ok(())
    }

    /// Converts tessellated egui output into meshes. Positions stay in egui
    /// points, which is what the editor's frame uses as its unit.
    pub fn meshes(&self, primitives: Vec<ClippedPrimitive>) -> Vec<Mesh> {
        let mut meshes = Vec::with_capacity(primitives.len());
        for primitive in primitives {
            let Primitive::Mesh(mesh) = primitive.primitive else {
                continue;
            };
            let Some(texture) = self.textures.get(&mesh.texture_id) else {
                continue;
            };
            let clip = primitive.clip_rect;
            meshes.push(Mesh {
                vertices: mesh
                    .vertices
                    .iter()
                    .map(|v| MeshVertex {
                        position: [v.pos.x, v.pos.y],
                        uv: [v.uv.x, v.uv.y],
                        color: v.color.to_array(),
                    })
                    .collect(),
                indices: mesh.indices,
                texture: texture.texture,
                filter: texture.filter,
                clip: Some([clip.min.x, clip.min.y, clip.width(), clip.height()]),
            });
        }
        meshes
    }
}
