use crate::{RenderError, Renderer, Texture};
use cosmic_text::{
    Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// The font built into the engine: Inter Medium, under the SIL Open Font License.
const DEFAULT_FONT: &[u8] = include_bytes!("../assets/Inter-Medium.otf");

const ATLAS_WIDTH: u32 = 1024;
const ATLAS_START_HEIGHT: u32 = 512;
const ATLAS_MAX_HEIGHT: u32 = 4096;
/// Empty pixels around each glyph so smooth scaling never bleeds a neighbour in.
const PADDING: u32 = 1;
/// Layouts not used for this many frames are forgotten.
const KEEP_FRAMES: u64 = 120;
const LINE_HEIGHT: f32 = 1.25;

/// One glyph of a laid-out text.
#[derive(Clone, Copy, Debug)]
pub struct PlacedGlyph {
    /// Top-left corner relative to the text's anchor, in the units the text
    /// size was given in.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Where the glyph is in the atlas, in pixels: `[x, y, w, h]`.
    atlas: [u32; 4],
}

/// A text laid out and ready to draw.
#[derive(Clone, Debug, Default)]
pub struct TextLayout {
    pub glyphs: Vec<PlacedGlyph>,
    /// Width of the widest line.
    pub width: f32,
    /// Height of all lines.
    pub height: f32,
}

#[derive(Clone, Copy)]
struct AtlasGlyph {
    rect: [u32; 4],
    left: i32,
    top: i32,
}

#[derive(PartialEq, Eq, Hash)]
struct LayoutKey {
    text: String,
    size: u32,
    scale: u32,
    align: u32,
}

/// Shapes text and keeps the rasterised glyphs in one atlas texture.
pub struct TextRenderer {
    fonts: FontSystem,
    swash: SwashCache,
    family: String,
    pixels: Vec<u8>,
    height: u32,
    cursor: (u32, u32),
    row_height: u32,
    dirty: bool,
    texture: Option<Texture>,
    glyphs: HashMap<CacheKey, Option<AtlasGlyph>>,
    layouts: HashMap<LayoutKey, (Rc<TextLayout>, u64)>,
    frame: u64,
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextRenderer {
    /// A text renderer with the built-in font.
    pub fn new() -> Self {
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_source(cosmic_text::fontdb::Source::Binary(Arc::new(DEFAULT_FONT)));
        let family = db
            .faces()
            .next()
            .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
            .unwrap_or_else(|| "Inter".to_string());
        db.set_sans_serif_family(family.clone());
        TextRenderer {
            fonts: FontSystem::new_with_locale_and_db("en-US".to_string(), db),
            swash: SwashCache::new(),
            family,
            pixels: vec![0; (ATLAS_WIDTH * ATLAS_START_HEIGHT * 4) as usize],
            height: ATLAS_START_HEIGHT,
            cursor: (PADDING, PADDING),
            row_height: 0,
            dirty: false,
            texture: None,
            glyphs: HashMap::new(),
            layouts: HashMap::new(),
            frame: 0,
        }
    }

    /// Lays out `text` at a font size of `size` units.
    ///
    /// `scale` is how many pixels one unit covers on screen, so glyphs are
    /// rasterised at the size they are shown. `align` places each line
    /// relative to the anchor: `0.0` starts lines at the anchor, `0.5`
    /// centres them on it and `1.0` ends them at it.
    pub fn layout(&mut self, text: &str, size: f32, scale: f32, align: f32) -> Rc<TextLayout> {
        let key = LayoutKey {
            text: text.to_string(),
            size: size.to_bits(),
            scale: scale.to_bits(),
            align: align.to_bits(),
        };
        if let Some((layout, used)) = self.layouts.get_mut(&key) {
            *used = self.frame;
            return layout.clone();
        }
        let layout = Rc::new(self.shape(text, size, scale, align));
        self.layouts.insert(key, (layout.clone(), self.frame));
        layout
    }

    fn shape(&mut self, text: &str, size: f32, scale: f32, align: f32) -> TextLayout {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let pixels = size * scale;
        if text.is_empty() || !pixels.is_finite() || pixels < 0.5 {
            return TextLayout::default();
        }
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(pixels, pixels * LINE_HEIGHT));
        buffer.set_size(&mut self.fonts, None, None);
        let attrs = Attrs::new().family(Family::Name(&self.family));
        buffer.set_text(&mut self.fonts, text, &attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.fonts, false);

        // Collect first: rasterising needs `self` again.
        let mut pending = Vec::new();
        let mut layout = TextLayout::default();
        for run in buffer.layout_runs() {
            layout.width = layout.width.max(run.line_w / scale);
            layout.height = layout.height.max((run.line_top + run.line_height) / scale);
            let shift = -run.line_w * align;
            for glyph in run.glyphs {
                let physical = glyph.physical((shift, run.line_y), 1.0);
                pending.push((physical.cache_key, physical.x, physical.y));
            }
        }
        for (key, x, y) in pending {
            let Some(glyph) = self.glyph(key) else {
                continue;
            };
            layout.glyphs.push(PlacedGlyph {
                x: (x + glyph.left) as f32 / scale,
                y: (y - glyph.top) as f32 / scale,
                w: glyph.rect[2] as f32 / scale,
                h: glyph.rect[3] as f32 / scale,
                atlas: glyph.rect,
            });
        }
        layout
    }

    /// Finds a glyph in the atlas, rasterising it first if needed. `None`
    /// for glyphs with nothing to draw, such as spaces.
    fn glyph(&mut self, key: CacheKey) -> Option<AtlasGlyph> {
        if let Some(known) = self.glyphs.get(&key) {
            return *known;
        }
        let placed = self.rasterise(key);
        self.glyphs.insert(key, placed);
        placed
    }

    fn rasterise(&mut self, key: CacheKey) -> Option<AtlasGlyph> {
        let image = self.swash.get_image_uncached(&mut self.fonts, key)?;
        let (w, h) = (image.placement.width, image.placement.height);
        if w == 0 || h == 0 || w + 2 * PADDING > ATLAS_WIDTH || h + 2 * PADDING > ATLAS_MAX_HEIGHT {
            return None;
        }
        let (x, y) = self.allocate(w, h)?;
        for row in 0..h {
            for col in 0..w {
                let source = (row * w + col) as usize;
                let rgba = match image.content {
                    SwashContent::Mask => [255, 255, 255, image.data[source]],
                    SwashContent::Color | SwashContent::SubpixelMask => {
                        let p = &image.data[source * 4..source * 4 + 4];
                        [p[0], p[1], p[2], p[3]]
                    }
                };
                let target = (((y + row) * ATLAS_WIDTH + x + col) * 4) as usize;
                self.pixels[target..target + 4].copy_from_slice(&rgba);
            }
        }
        self.dirty = true;
        Some(AtlasGlyph {
            rect: [x, y, w, h],
            left: image.placement.left,
            top: image.placement.top,
        })
    }

    /// Finds room for a `w` by `h` glyph, growing the atlas when it is full.
    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if self.cursor.0 + w + PADDING > ATLAS_WIDTH {
            self.cursor = (PADDING, self.cursor.1 + self.row_height + PADDING);
            self.row_height = 0;
        }
        while self.cursor.1 + h + PADDING > self.height {
            if self.height >= ATLAS_MAX_HEIGHT {
                log::warn!(
                    "the glyph atlas is full and starts over; text may flicker for one frame"
                );
                self.pixels.fill(0);
                self.glyphs.clear();
                self.layouts.clear();
                self.cursor = (PADDING, PADDING);
                self.row_height = 0;
                continue;
            }
            self.height *= 2;
            self.pixels
                .resize((ATLAS_WIDTH * self.height * 4) as usize, 0);
        }
        let at = self.cursor;
        self.cursor.0 += w + PADDING;
        self.row_height = self.row_height.max(h);
        Some(at)
    }

    /// Uploads newly rasterised glyphs. Call once per frame, after every
    /// [`layout`](Self::layout) and before building quads. Returns the atlas
    /// texture.
    pub fn prepare(&mut self, renderer: &mut Renderer) -> Result<Texture, RenderError> {
        self.frame += 1;
        let frame = self.frame;
        self.layouts
            .retain(|_, (_, used)| frame - *used <= KEEP_FRAMES);

        let texture = match self.texture {
            Some(texture) if texture.height() == self.height => {
                if self.dirty {
                    renderer.update_texture(texture, &self.pixels)?;
                }
                texture
            }
            old => {
                if let Some(old) = old {
                    renderer.destroy_texture(old);
                }
                renderer.create_texture(ATLAS_WIDTH, self.height, &self.pixels)?
            }
        };
        self.texture = Some(texture);
        self.dirty = false;
        Ok(texture)
    }

    /// Texture coordinates `[u0, v0, u1, v1]` of a glyph in the atlas.
    pub fn uv(&self, glyph: &PlacedGlyph) -> [f32; 4] {
        let [x, y, w, h] = glyph.atlas;
        let (aw, ah) = (ATLAS_WIDTH as f32, self.height as f32);
        [
            x as f32 / aw,
            y as f32 / ah,
            (x + w) as f32 / aw,
            (y + h) as f32 / ah,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_text_with_the_built_in_font() {
        let mut text = TextRenderer::new();
        let layout = text.layout("Score 42", 24.0, 1.0, 0.0);
        assert_eq!(layout.glyphs.len(), 7, "the space has nothing to draw");
        assert!(
            layout.width > 60.0 && layout.width < 140.0,
            "width {}",
            layout.width
        );
        assert!((layout.height - 30.0).abs() < 0.01, "one line of 24 * 1.25");
        // Glyphs go left to right and stay inside the line.
        assert!(layout.glyphs.windows(2).all(|pair| pair[0].x < pair[1].x));
        assert!(layout
            .glyphs
            .iter()
            .all(|g| g.x >= -1.0 && g.y >= 0.0 && g.y + g.h <= layout.height));
        assert!(text.dirty);

        // The same text again is served from the cache.
        let again = text.layout("Score 42", 24.0, 1.0, 0.0);
        assert!(Rc::ptr_eq(&layout, &again));
    }

    #[test]
    fn alignment_and_lines() {
        let mut text = TextRenderer::new();
        let left = text.layout("wide line\nx", 20.0, 1.0, 0.0);
        let centered = text.layout("wide line\nx", 20.0, 1.0, 0.5);
        let right = text.layout("wide line\nx", 20.0, 1.0, 1.0);
        assert!((left.height - 50.0).abs() < 0.01, "two lines");
        let min_x = |l: &TextLayout| l.glyphs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
        let max_x = |l: &TextLayout| {
            l.glyphs
                .iter()
                .map(|g| g.x + g.w)
                .fold(f32::NEG_INFINITY, f32::max)
        };
        assert!(min_x(&left) >= -1.0);
        assert!(
            (min_x(&centered) + max_x(&centered)).abs() < 3.0,
            "centred around the anchor"
        );
        assert!(max_x(&right) <= 1.0);
        // The short second line is centred too, not left-aligned with the first.
        let last = centered.glyphs.last().unwrap();
        assert!((last.x + last.w * 0.5).abs() < 2.0);
    }

    #[test]
    fn scale_changes_pixels_not_layout_size() {
        let mut text = TextRenderer::new();
        let one = text.layout("Hi", 20.0, 1.0, 0.0);
        let two = text.layout("Hi", 20.0, 2.0, 0.0);
        assert!((one.width - two.width).abs() < 1.0);
        assert!(
            two.glyphs[0].atlas[3] > one.glyphs[0].atlas[3],
            "rasterised bigger at 2x"
        );
        assert!(text.layout("", 20.0, 1.0, 0.0).glyphs.is_empty());
    }

    #[test]
    fn atlas_grows_when_full() {
        let mut text = TextRenderer::new();
        let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        for size in [40.0, 60.0, 80.0, 100.0] {
            text.layout(alphabet, size, 1.0, 0.0);
        }
        assert!(text.height > ATLAS_START_HEIGHT);
        assert_eq!(text.pixels.len(), (ATLAS_WIDTH * text.height * 4) as usize);
        // No two glyphs share atlas space.
        let rects: Vec<[u32; 4]> = text.glyphs.values().flatten().map(|g| g.rect).collect();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let apart = a[0] + a[2] <= b[0]
                    || b[0] + b[2] <= a[0]
                    || a[1] + a[3] <= b[1]
                    || b[1] + b[3] <= a[1];
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }
}
