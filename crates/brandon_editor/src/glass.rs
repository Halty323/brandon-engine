//! Frosted glass for the editor's panels, toolbars and menus.
//!
//! Anything egui fills with one of the marker colours below is turned into
//! glass after the UI is laid out: a blurred, slightly more vivid copy of
//! the scene behind it, bent at the edges like the rim of a lens, with a
//! tint for legibility and a thin rim of light. The blurred copy comes from
//! the renderer's backdrop texture.

use egui::epaint::{ClippedShape, Mesh, RectShape, Shape, Vertex, WHITE_UV};
use egui::{Color32, Pos2, Rect, TextureId, Vec2};
use std::sync::Arc;

/// The texture egui meshes use to mean "the blurred scene behind".
pub const BACKDROP: TextureId = TextureId::User(0xB4C);

/// Fill a frame with this to make it regular glass: panels and menus.
pub const GLASS: Color32 = Color32::from_rgb(0x11, 0x22, 0x33);
/// Fill a frame with this for lighter glass: floating controls.
pub const GLASS_CLEAR: Color32 = Color32::from_rgb(0x11, 0x22, 0x44);

/// How one kind of glass looks.
struct Style {
    /// Black laid over the backdrop, for legibility.
    shade: f32,
    /// White laid over it, at the top and at the bottom.
    sheen: (f32, f32),
    /// How wide the bent rim is, in points.
    lens: f32,
    /// How far the rim reaches outside the shape for what it shows, as a
    /// share of its width.
    bend: f32,
    /// Brightness of the rim of light: everywhere, facing the light, and
    /// facing away from it.
    rim: (f32, f32, f32),
    /// The soft glow just inside the rim, facing the light.
    glow: f32,
}

const REGULAR: Style = Style {
    shade: 0.46,
    sheen: (0.085, 0.035),
    lens: 14.0,
    bend: 0.9,
    rim: (0.10, 0.42, 0.16),
    glow: 0.07,
};

const CLEAR: Style = Style {
    shade: 0.22,
    sheen: (0.12, 0.05),
    lens: 10.0,
    bend: 1.0,
    rim: (0.12, 0.50, 0.20),
    glow: 0.09,
};

/// Where the light comes from: above, a little to the left.
const LIGHT: Vec2 = Vec2::new(-0.45, -0.89);

/// Which glass a fill colour asks for, and how opaque it is (popups fade
/// in, which scales the colour).
fn glass_of(fill: Color32) -> Option<(&'static Style, f32)> {
    let a = fill.a();
    if a == 0 {
        return None;
    }
    let near = |value: u8, marker: u8| {
        (value as f32 - marker as f32 * a as f32 / 255.0).abs() <= 1.0 + 255.0 / a as f32 * 0.02
    };
    let [r, g, b, _] = fill.to_array();
    if !(near(r, 0x11) && near(g, 0x22)) {
        return None;
    }
    let opacity = a as f32 / 255.0;
    if near(b, 0x33) {
        Some((&REGULAR, opacity))
    } else if near(b, 0x44) {
        Some((&CLEAR, opacity))
    } else {
        None
    }
}

/// Replaces glass-coloured rectangles with glass. `screen` is the whole
/// window in points, and `pixel` the size of one physical pixel in points.
pub fn apply(shapes: Vec<ClippedShape>, screen: Rect, pixel: f32) -> Vec<ClippedShape> {
    let mut out = Vec::with_capacity(shapes.len() + 32);
    for clipped in shapes {
        let ClippedShape {
            clip_rect,
            shape,
            transform_after_tessellation: transform,
        } = clipped;
        let mut push = |shape: Shape| {
            out.push(ClippedShape {
                clip_rect,
                shape,
                transform_after_tessellation: transform,
            })
        };
        expand(shape, &mut push, screen, pixel);
    }
    out
}

fn expand(shape: Shape, push: &mut dyn FnMut(Shape), screen: Rect, pixel: f32) {
    match shape {
        Shape::Vec(shapes) => {
            for shape in shapes {
                expand(shape, push, screen, pixel);
            }
        }
        Shape::Rect(rect) => match glass_of(rect.fill) {
            Some((style, opacity)) => {
                for shape in glass(&rect, style, opacity, screen, pixel) {
                    push(shape);
                }
            }
            None => push(Shape::Rect(rect)),
        },
        shape => push(shape),
    }
}

/// The outline of a rounded rectangle: for each point, the centre of its
/// corner's arc and the direction out of the shape. The point itself, moved
/// in by `d`, is `centre + normal * (radius - d)`.
struct Outline {
    points: Vec<(Pos2, Vec2)>,
    radius: f32,
    center: Pos2,
}

impl Outline {
    fn new(rect: Rect, radius: f32) -> Self {
        let radius = radius
            .min(rect.width() / 2.0)
            .min(rect.height() / 2.0)
            .max(0.0);
        let steps = ((radius * 0.5) as usize).clamp(3, 18);
        let corners = [
            (
                Pos2::new(rect.max.x - radius, rect.min.y + radius),
                -90.0f32,
            ),
            (Pos2::new(rect.max.x - radius, rect.max.y - radius), 0.0),
            (Pos2::new(rect.min.x + radius, rect.max.y - radius), 90.0),
            (Pos2::new(rect.min.x + radius, rect.min.y + radius), 180.0),
        ];
        let mut points = Vec::with_capacity(corners.len() * (steps + 1));
        for (center, start) in corners {
            for step in 0..=steps {
                let angle = (start + 90.0 * step as f32 / steps as f32).to_radians();
                points.push((center, Vec2::new(angle.cos(), angle.sin())));
            }
        }
        Outline {
            points,
            radius,
            center: rect.center(),
        }
    }

    fn at(&self, index: usize, inset: f32) -> Pos2 {
        let (center, normal) = self.points[index];
        center + normal * (self.radius - inset)
    }

    fn normal(&self, index: usize) -> Vec2 {
        self.points[index].1
    }
}

/// One ring of a mesh: every outline point moved in by `inset`, with a
/// colour and a texture position for each.
struct Ring<'a> {
    inset: f32,
    color: &'a dyn Fn(usize, Pos2) -> Color32,
    uv: &'a dyn Fn(usize, Pos2) -> Pos2,
}

/// A mesh made of rings from the outside in, joined into a band each, and
/// optionally filled in the middle.
fn ring_mesh(outline: &Outline, rings: &[Ring], fill_center: bool, texture: TextureId) -> Mesh {
    let n = outline.points.len();
    let mut mesh = Mesh::with_texture(texture);
    for ring in rings {
        for i in 0..n {
            let pos = outline.at(i, ring.inset);
            mesh.vertices.push(Vertex {
                pos,
                uv: (ring.uv)(i, pos),
                color: (ring.color)(i, pos),
            });
        }
    }
    for r in 0..rings.len().saturating_sub(1) {
        let a = (r * n) as u32;
        let b = ((r + 1) * n) as u32;
        for i in 0..n as u32 {
            let j = (i + 1) % n as u32;
            mesh.indices
                .extend_from_slice(&[a + i, a + j, b + j, a + i, b + j, b + i]);
        }
    }
    if fill_center {
        if let Some(last) = rings.last() {
            let center = outline.center;
            let index = mesh.vertices.len() as u32;
            mesh.vertices.push(Vertex {
                pos: center,
                uv: (last.uv)(usize::MAX, center),
                color: (last.color)(usize::MAX, center),
            });
            let ring = ((rings.len() - 1) * n) as u32;
            for i in 0..n as u32 {
                let j = (i + 1) % n as u32;
                mesh.indices.extend_from_slice(&[ring + i, ring + j, index]);
            }
        }
    }
    mesh
}

fn glass(rect: &RectShape, style: &Style, opacity: f32, screen: Rect, pixel: f32) -> Vec<Shape> {
    let bounds = rect.rect;
    if !bounds.is_positive() {
        return Vec::new();
    }
    let outline = Outline::new(bounds, rect.corner_radius.nw as f32);
    let radius = outline.radius;
    let feather = pixel;
    let size = screen.size().max(Vec2::splat(1.0));
    let to_uv = |pos: Pos2| {
        Pos2::new(
            (pos.x - screen.min.x) / size.x,
            (pos.y - screen.min.y) / size.y,
        )
    };
    let white =
        |a: f32| Color32::from_white_alpha((a * opacity * 255.0).round().clamp(0.0, 255.0) as u8);
    let clear = |_: usize, _: Pos2| Color32::TRANSPARENT;
    let mut shapes = Vec::with_capacity(3);

    // The blurred scene, bent at the rim: near the edge it shows what lies
    // a little outside the shape, squeezed in, as a thick lens would.
    let lens = style.lens.min(radius).max(0.0);
    let bend = lens * style.bend;
    let solid = move |_: usize, _: Pos2| white(1.0);
    let edge = &outline;
    let bent = |inset: f32| {
        move |i: usize, pos: Pos2| {
            if i == usize::MAX || lens <= 0.0 {
                return to_uv(pos);
            }
            let t = 1.0 - (inset / lens).clamp(0.0, 1.0);
            to_uv(pos + edge.normal(i) * bend * t * t * t)
        }
    };
    let (uv_a, uv_b, uv_c, uv_d, uv_e) = (
        bent(0.0),
        bent(lens * 0.25),
        bent(lens * 0.5),
        bent(lens * 0.75),
        bent(lens),
    );
    let mut rings = vec![
        Ring {
            inset: -feather,
            color: &clear,
            uv: &uv_a,
        },
        Ring {
            inset: 0.0,
            color: &solid,
            uv: &uv_a,
        },
    ];
    if lens > 0.0 {
        rings.extend([
            Ring {
                inset: lens * 0.25,
                color: &solid,
                uv: &uv_b,
            },
            Ring {
                inset: lens * 0.5,
                color: &solid,
                uv: &uv_c,
            },
            Ring {
                inset: lens * 0.75,
                color: &solid,
                uv: &uv_d,
            },
            Ring {
                inset: lens,
                color: &solid,
                uv: &uv_e,
            },
        ]);
    }
    shapes.push(Shape::Mesh(Arc::new(ring_mesh(
        &outline, &rings, true, BACKDROP,
    ))));

    // Shade for legibility, and a sheen that is brighter at the top.
    let top = bounds.top();
    let height = bounds.height().max(1.0);
    let (sheen_top, sheen_bottom) = style.sheen;
    let shade = style.shade;
    let tint = move |_: usize, pos: Pos2| {
        let t = ((pos.y - top) / height).clamp(0.0, 1.0);
        let sheen = sheen_top + (sheen_bottom - sheen_top) * t;
        // Black at `shade`, then white at `sheen` over it, as one colour.
        let alpha = 1.0 - (1.0 - shade) * (1.0 - sheen);
        let a = (alpha * opacity * 255.0).round().clamp(0.0, 255.0) as u8;
        let w = (sheen * opacity * 255.0).round().clamp(0.0, 255.0) as u8;
        Color32::from_rgba_premultiplied(w, w, w, a)
    };
    let uv = |_: usize, _: Pos2| WHITE_UV;
    let rings = [
        Ring {
            inset: -feather,
            color: &clear,
            uv: &uv,
        },
        Ring {
            inset: 0.0,
            color: &tint,
            uv: &uv,
        },
    ];
    shapes.push(Shape::Mesh(Arc::new(ring_mesh(
        &outline,
        &rings,
        true,
        TextureId::default(),
    ))));

    // A thin rim of light, brightest where the edge faces the light, and a
    // soft glow just inside it.
    let (base, toward, away) = style.rim;
    let facing = |i: usize| {
        let n = outline.normal(i);
        let d = n.dot(LIGHT.normalized());
        (d.max(0.0).powf(2.0), (-d).max(0.0).powf(3.0))
    };
    let rim = |i: usize, _: Pos2| {
        let (lit, back) = facing(i);
        white(base + toward * lit + away * back)
    };
    let glow_strength = style.glow;
    let glow = |i: usize, _: Pos2| white(glow_strength * facing(i).0);
    let width = pixel.max(0.75);
    let reach = (radius * 0.6).clamp(2.0, 10.0);
    let rings = [
        Ring {
            inset: -feather,
            color: &clear,
            uv: &uv,
        },
        Ring {
            inset: 0.0,
            color: &rim,
            uv: &uv,
        },
        Ring {
            inset: width,
            color: &rim,
            uv: &uv,
        },
        Ring {
            inset: width + feather,
            color: &glow,
            uv: &uv,
        },
        Ring {
            inset: reach,
            color: &clear,
            uv: &uv,
        },
    ];
    shapes.push(Shape::Mesh(Arc::new(ring_mesh(
        &outline,
        &rings,
        false,
        TextureId::default(),
    ))));
    shapes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_are_recognised_even_while_fading() {
        assert!(glass_of(GLASS).is_some());
        assert!(glass_of(GLASS_CLEAR).is_some());
        assert!(glass_of(Color32::from_rgb(0x11, 0x22, 0x55)).is_none());
        assert!(glass_of(Color32::BLACK).is_none());
        let faded = GLASS.gamma_multiply(0.5);
        let (_, opacity) = glass_of(faded).expect("still glass");
        assert!((opacity - 0.5).abs() < 0.02);
    }

    #[test]
    fn glass_replaces_only_marked_rectangles() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let marked = RectShape::filled(
            Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(200.0, 300.0)),
            20,
            GLASS,
        );
        let plain = RectShape::filled(screen, 0, Color32::RED);
        let shapes = vec![
            ClippedShape::new(screen, Shape::Rect(plain)),
            ClippedShape::new(screen, Shape::Vec(vec![Shape::Rect(marked)])),
        ];
        let out = apply(shapes, screen, 1.0);
        assert_eq!(out.len(), 4, "the plain rectangle, then three glass layers");
        assert!(matches!(out[0].shape, Shape::Rect(_)));
        let Shape::Mesh(base) = &out[1].shape else {
            panic!("the backdrop comes first");
        };
        assert_eq!(base.texture_id, BACKDROP);
        // Texture positions map the window from 0 to 1.
        for v in &base.vertices {
            assert!(v.uv.x > -0.1 && v.uv.x < 0.4 && v.uv.y > -0.1 && v.uv.y < 0.6);
        }
        assert!(base
            .indices
            .iter()
            .all(|i| (*i as usize) < base.vertices.len()));
    }

    #[test]
    fn outlines_go_round_clockwise() {
        let outline = Outline::new(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 50.0)),
            10.0,
        );
        let first = outline.at(0, 0.0);
        assert!((first - Pos2::new(90.0, 0.0)).length() < 1e-3, "{first:?}");
        // Every point lies on or inside the rectangle.
        for i in 0..outline.points.len() {
            let p = outline.at(i, 0.0);
            assert!(p.x >= -1e-3 && p.x <= 100.001 && p.y >= -1e-3 && p.y <= 50.001);
        }
    }
}
