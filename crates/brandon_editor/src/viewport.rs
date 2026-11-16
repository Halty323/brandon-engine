//! The viewport: the editor's own camera, the grid, picking nodes with the
//! mouse, and the handles that move, rotate and scale them.

use crate::theme;
use brandon::draw::{local_bounds, Textures};
use brandon_core::{Assets, Color};
use brandon_render::{Filter, Layer, Quad, Renderer, TextRenderer};
use brandon_scene::{NodeId, Scene, Shape};
use egui::{Pos2, Rect, Stroke};
use glam::{Affine2, Mat2, Vec2};

/// Access to the renderer, for measuring nodes.
pub struct Gfx<'a> {
    pub renderer: &'a mut Renderer,
    pub text: &'a mut TextRenderer,
    pub textures: &'a mut Textures,
}

/// How the viewport looks at the scene. Independent of any camera node.
#[derive(Clone, Copy, Debug)]
pub struct ViewCamera {
    /// The world position shown in the middle of the viewport.
    pub center: Vec2,
    /// Screen points per world unit.
    pub zoom: f32,
}

impl Default for ViewCamera {
    fn default() -> Self {
        ViewCamera {
            center: Vec2::new(480.0, 270.0),
            zoom: 1.0,
        }
    }
}

pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 16.0;

impl ViewCamera {
    /// World units to screen points, for a viewport occupying `rect`.
    pub fn view(&self, rect: Rect) -> Affine2 {
        Affine2::from_translation(v(rect.center()))
            * Affine2::from_scale(Vec2::splat(self.zoom))
            * Affine2::from_translation(-self.center)
    }

    /// Zooms by `factor`, keeping the world point under `anchor` in place.
    pub fn zoom_at(&mut self, rect: Rect, anchor: Pos2, factor: f32) {
        let before = self.view(rect).inverse().transform_point2(v(anchor));
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.view(rect).inverse().transform_point2(v(anchor));
        self.center += before - after;
    }

    /// Shows the world rectangle `(min, max)` as large as fits, with a margin.
    pub fn frame(&mut self, rect: Rect, min: Vec2, max: Vec2) {
        let size = (max - min).max(Vec2::splat(1.0));
        let room = Vec2::new(rect.width(), rect.height()) * 0.8;
        self.center = (min + max) * 0.5;
        self.zoom = (room.x / size.x)
            .min(room.y / size.y)
            .clamp(MIN_ZOOM, MAX_ZOOM);
    }
}

pub fn v(p: Pos2) -> Vec2 {
    Vec2::new(p.x, p.y)
}

pub fn p(v: Vec2) -> Pos2 {
    Pos2::new(v.x, v.y)
}

// --- Grid -------------------------------------------------------------------

fn line(
    origin: Vec2,
    x_axis: Vec2,
    y_axis: Vec2,
    color: Color,
    white: brandon_render::Texture,
) -> Quad {
    Quad {
        origin,
        x_axis,
        y_axis,
        uv: [0.0, 0.0, 1.0, 1.0],
        color,
        texture: white,
        filter: Filter::Smooth,
        z: 0,
    }
}

/// A soft, dark wallpaper with a few coloured glows, so the glass panels
/// over it pick up some colour. Straight-alpha sRGB pixels.
pub fn wallpaper_image(width: u32, height: u32) -> Vec<u8> {
    fn to_linear(c: u8) -> f32 {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    fn to_srgb(c: f32) -> u8 {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.0031308 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    }
    let base = [0x0b, 0x0d, 0x12].map(to_linear);
    // (x, y, colour, strength, reach), in parts of the window.
    let glows: [(f32, f32, [u8; 3], f32, f32); 4] = [
        (0.12, 0.08, [0x4a, 0x4c, 0xd6], 0.24, 0.55),
        (0.92, 0.90, [0x14, 0x8a, 0x9c], 0.17, 0.55),
        (0.80, 0.12, [0x8a, 0x3c, 0xc0], 0.15, 0.42),
        (0.28, 0.98, [0x1f, 0x52, 0xb8], 0.15, 0.45),
    ];
    let aspect = width as f32 / height.max(1) as f32;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let u = (x as f32 + 0.5) / width as f32;
            let v = (y as f32 + 0.5) / height as f32;
            let mut color = base;
            for (gx, gy, tint, strength, reach) in glows {
                let dx = (u - gx) * aspect;
                let dy = v - gy;
                let falloff = (-(dx * dx + dy * dy) / (reach * reach)).exp() * strength;
                let tint = tint.map(to_linear);
                for i in 0..3 {
                    color[i] += tint[i] * falloff;
                }
            }
            pixels.extend_from_slice(&[
                to_srgb(color[0]),
                to_srgb(color[1]),
                to_srgb(color[2]),
                255,
            ]);
        }
    }
    pixels
}

/// The wallpaper, stretched over the whole window.
pub fn wallpaper_layer(texture: brandon_render::Texture, size: Vec2) -> Layer {
    Layer {
        view: Affine2::IDENTITY,
        quads: vec![Quad {
            origin: Vec2::ZERO,
            x_axis: Vec2::new(size.x, 0.0),
            y_axis: Vec2::new(0.0, size.y),
            uv: [0.0, 0.0, 1.0, 1.0],
            color: Color::WHITE,
            texture,
            filter: Filter::Smooth,
            z: 0,
        }],
    }
}

/// The grid behind the scene, the world axes, and the outline of the game's
/// screen. Lines are one screen pixel thick at any zoom.
pub fn grid_layer(
    camera: &ViewCamera,
    rect: Rect,
    view: Affine2,
    screen: Vec2,
    renderer: &Renderer,
) -> Layer {
    let white = renderer.white();
    let inverse = view.inverse();
    let top_left = inverse.transform_point2(v(rect.min));
    let bottom_right = inverse.transform_point2(v(rect.max));
    let thickness = 1.0 / camera.zoom;
    let mut quads = Vec::new();

    // A step that is at least 20 points apart on screen.
    let mut step = 16.0f32;
    while step * camera.zoom < 20.0 {
        step *= 2.0;
    }
    while step * camera.zoom > 80.0 && step > 1.0 {
        step /= 2.0;
    }
    // The game's screen, as a darker sheet on the wallpaper.
    quads.push(line(
        Vec2::ZERO,
        Vec2::new(screen.x, 0.0),
        Vec2::new(0.0, screen.y),
        Color::rgba(0.0, 0.0, 0.0, 0.55),
        white,
    ));

    let minor = Color::rgba(1.0, 1.0, 1.0, 0.008);
    let major = Color::rgba(1.0, 1.0, 1.0, 0.022);
    let height = bottom_right.y - top_left.y;
    let width = bottom_right.x - top_left.x;
    let first = (top_left.x / step).floor() as i64;
    let last = (bottom_right.x / step).ceil() as i64;
    for i in first..=last {
        let x = i as f32 * step;
        let color = if i % 4 == 0 { major } else { minor };
        quads.push(line(
            Vec2::new(x - thickness * 0.5, top_left.y),
            Vec2::new(thickness, 0.0),
            Vec2::new(0.0, height),
            color,
            white,
        ));
    }
    let first = (top_left.y / step).floor() as i64;
    let last = (bottom_right.y / step).ceil() as i64;
    for i in first..=last {
        let y = i as f32 * step;
        let color = if i % 4 == 0 { major } else { minor };
        quads.push(line(
            Vec2::new(top_left.x, y - thickness * 0.5),
            Vec2::new(width, 0.0),
            Vec2::new(0.0, thickness),
            color,
            white,
        ));
    }

    // The game's screen: where (0, 0) to the window size ends up without a camera.
    let edge = Color::rgba(1.0, 1.0, 1.0, 0.16);
    let t = thickness * 1.5;
    quads.push(line(
        Vec2::new(0.0, -t * 0.5),
        Vec2::new(screen.x, 0.0),
        Vec2::new(0.0, t),
        edge,
        white,
    ));
    quads.push(line(
        Vec2::new(0.0, screen.y - t * 0.5),
        Vec2::new(screen.x, 0.0),
        Vec2::new(0.0, t),
        edge,
        white,
    ));
    quads.push(line(
        Vec2::new(-t * 0.5, 0.0),
        Vec2::new(t, 0.0),
        Vec2::new(0.0, screen.y),
        edge,
        white,
    ));
    quads.push(line(
        Vec2::new(screen.x - t * 0.5, 0.0),
        Vec2::new(t, 0.0),
        Vec2::new(0.0, screen.y),
        edge,
        white,
    ));

    Layer { view, quads }
}

// --- Measuring and picking ----------------------------------------------------

/// The area a node covers in its own space, as `(min, max)`: what it draws,
/// or its shape when it draws nothing.
pub fn node_bounds(
    scene: &Scene,
    id: NodeId,
    gfx: &mut Gfx,
    assets: &Assets,
) -> Option<(Vec2, Vec2)> {
    let node = scene.get(id)?;
    if let Some((min, size)) = local_bounds(node, gfx.renderer, gfx.text, gfx.textures, assets) {
        return Some((min, min + size));
    }
    match node.shape? {
        Shape::Circle { radius } => Some((Vec2::splat(-radius), Vec2::splat(radius))),
        Shape::Rect { size } => Some((-size * 0.5, size * 0.5)),
    }
}

/// The corners of a node's bounds, in world space.
pub fn world_corners(world: Affine2, (min, max): (Vec2, Vec2)) -> [Vec2; 4] {
    [
        world.transform_point2(min),
        world.transform_point2(Vec2::new(max.x, min.y)),
        world.transform_point2(max),
        world.transform_point2(Vec2::new(min.x, max.y)),
    ]
}

/// The topmost visible node under a world position. Nodes inside a placed
/// scene count as the placed scene itself.
pub fn pick(
    scene: &Scene,
    root: NodeId,
    point: Vec2,
    gfx: &mut Gfx,
    assets: &Assets,
) -> Option<NodeId> {
    let mut candidates = Vec::new();
    scene.for_each_visible(|node, at| candidates.push((node.id(), at.world, at.z)));
    // Drawn last is on top: sort by draw order and search from the end.
    let order: Vec<usize> = {
        let mut order: Vec<usize> = (0..candidates.len()).collect();
        order.sort_by_key(|i| candidates[*i].2);
        order
    };
    for index in order.into_iter().rev() {
        let (id, world, _) = candidates[index];
        if id == root {
            continue;
        }
        let Some((min, max)) = node_bounds(scene, id, gfx, assets) else {
            continue;
        };
        if world.matrix2.determinant().abs() < 1e-9 {
            continue;
        }
        let local = world.inverse().transform_point2(point);
        if local.x >= min.x && local.x <= max.x && local.y >= min.y && local.y <= max.y {
            return Some(outermost_placed(scene, root, id));
        }
    }
    None
}

/// The outermost node, between `id` and the root, that was placed from
/// another scene file. `id` itself if there is none.
pub fn outermost_placed(scene: &Scene, root: NodeId, id: NodeId) -> NodeId {
    let mut found = id;
    let mut cursor = scene.parent(id);
    while let Some(ancestor) = cursor {
        if ancestor == root {
            break;
        }
        if scene
            .get(ancestor)
            .is_some_and(|n| n.instance_of().is_some())
        {
            found = ancestor;
        }
        cursor = scene.parent(ancestor);
    }
    found
}

/// The world-space box around every visible node, for "frame all".
pub fn scene_extent(
    scene: &Scene,
    root: NodeId,
    gfx: &mut Gfx,
    assets: &Assets,
) -> Option<(Vec2, Vec2)> {
    let mut nodes = Vec::new();
    scene.for_each_visible(|node, at| nodes.push((node.id(), at.world)));
    let mut extent: Option<(Vec2, Vec2)> = None;
    for (id, world) in nodes {
        if id == root {
            continue;
        }
        let corners = match node_bounds(scene, id, gfx, assets) {
            Some(bounds) => world_corners(world, bounds).to_vec(),
            None => vec![world.translation],
        };
        for corner in corners {
            extent = Some(match extent {
                Some((lo, hi)) => (lo.min(corner), hi.max(corner)),
                None => (corner, corner),
            });
        }
    }
    extent
}

// --- Handles ------------------------------------------------------------------

/// Where the handles of the selected node are on screen.
#[derive(Clone, Copy, Debug)]
pub struct Handles {
    pub origin: Pos2,
    pub rotate: Pos2,
    pub scale: Pos2,
    pub outline: Option<[Pos2; 4]>,
}

pub const HANDLE_RADIUS: f32 = 7.0;

pub fn handles(world: Affine2, bounds: Option<(Vec2, Vec2)>, view: Affine2) -> Handles {
    let origin = view.transform_point2(world.translation);
    let screen_world = view * world;
    let up = screen_world
        .transform_vector2(Vec2::NEG_Y)
        .normalize_or(Vec2::NEG_Y);
    let outline = bounds.map(|b| world_corners(world, b).map(|c| p(view.transform_point2(c))));
    let reach = match outline {
        Some(corners) => corners
            .iter()
            .map(|c| v(*c).distance(origin))
            .fold(0.0f32, f32::max),
        None => 0.0,
    };
    let rotate = origin + up * (reach.max(24.0) + 26.0);
    let scale = match outline {
        Some(corners) => v(corners[2]),
        None => {
            origin
                + screen_world
                    .transform_vector2(Vec2::ONE)
                    .normalize_or(Vec2::ONE)
                    * 36.0
        }
    };
    Handles {
        origin: p(origin),
        rotate: p(rotate),
        scale: p(scale),
        outline,
    }
}

pub fn paint_handles(painter: &egui::Painter, handles: &Handles, placed: bool) {
    let color = if placed { theme::PLACED } else { theme::ACCENT };
    let shadow = egui::Color32::from_black_alpha(70);
    let ring = Stroke::new(2.0, egui::Color32::WHITE);
    if let Some(corners) = handles.outline {
        let mut points = corners.to_vec();
        points.push(corners[0]);
        painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
    }
    painter.line_segment(
        [handles.origin, handles.rotate],
        Stroke::new(1.5, egui::Color32::from_white_alpha(150)),
    );
    painter.circle_filled(
        handles.rotate + egui::vec2(0.0, 1.5),
        HANDLE_RADIUS + 1.0,
        shadow,
    );
    painter.circle(handles.rotate, HANDLE_RADIUS - 0.5, theme::ROTATE, ring);
    let half = egui::Vec2::splat(HANDLE_RADIUS - 1.0);
    let square = Rect::from_center_size(handles.scale, half * 2.0);
    painter.rect_filled(
        square.translate(egui::vec2(0.0, 1.5)).expand(1.5),
        4.0,
        shadow,
    );
    painter.rect(square, 3.0, theme::SCALE, ring, egui::StrokeKind::Outside);
    painter.circle_filled(handles.origin + egui::vec2(0.0, 1.0), 5.5, shadow);
    painter.circle(
        handles.origin,
        4.0,
        egui::Color32::WHITE,
        Stroke::new(2.0, color),
    );
}

/// What a drag in the viewport is doing.
#[derive(Clone, Copy, Debug, Default)]
pub enum Drag {
    #[default]
    None,
    Pan,
    Move {
        id: NodeId,
        start: Vec2,
        grab: Vec2,
        parent_inverse: Mat2,
    },
    Rotate {
        id: NodeId,
        start: f32,
        start_angle: f32,
    },
    Scale {
        id: NodeId,
        start: Vec2,
        start_distance: f32,
    },
}

/// Rounds to the nearest multiple of `step`.
pub fn snap(value: f32, step: f32) -> f32 {
    (value / step).round() * step
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_zooms_around_the_cursor() {
        let rect = Rect::from_min_size(Pos2::new(200.0, 40.0), egui::vec2(800.0, 600.0));
        let mut camera = ViewCamera {
            center: Vec2::new(100.0, 100.0),
            zoom: 1.0,
        };
        let view = camera.view(rect);
        assert_eq!(
            view.transform_point2(Vec2::new(100.0, 100.0)),
            Vec2::new(600.0, 340.0)
        );

        let cursor = Pos2::new(300.0, 100.0);
        let under = camera.view(rect).inverse().transform_point2(v(cursor));
        camera.zoom_at(rect, cursor, 3.0);
        assert_eq!(camera.zoom, 3.0);
        let still_under = camera.view(rect).inverse().transform_point2(v(cursor));
        assert!(under.abs_diff_eq(still_under, 1e-3));

        camera.zoom_at(rect, cursor, 1000.0);
        assert_eq!(camera.zoom, MAX_ZOOM);
    }

    #[test]
    fn framing_fits_the_box() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 500.0));
        let mut camera = ViewCamera::default();
        camera.frame(rect, Vec2::new(0.0, 0.0), Vec2::new(200.0, 100.0));
        assert_eq!(camera.center, Vec2::new(100.0, 50.0));
        assert_eq!(camera.zoom, 4.0, "the box fills 80% of the viewport");
    }

    #[test]
    fn handles_sit_around_the_node() {
        let world = Affine2::from_translation(Vec2::new(100.0, 100.0));
        let h = handles(
            world,
            Some((Vec2::splat(-10.0), Vec2::splat(10.0))),
            Affine2::IDENTITY,
        );
        assert_eq!(h.origin, Pos2::new(100.0, 100.0));
        assert_eq!(h.scale, Pos2::new(110.0, 110.0));
        assert!(h.rotate.x == 100.0 && h.rotate.y < 70.0, "{:?}", h.rotate);
        let turned =
            Affine2::from_angle_translation(std::f32::consts::FRAC_PI_2, Vec2::new(100.0, 100.0));
        let h = handles(turned, None, Affine2::IDENTITY);
        assert!(
            h.rotate.x > 140.0 && (h.rotate.y - 100.0).abs() < 1e-3,
            "{:?}",
            h.rotate
        );
    }

    #[test]
    fn placed_scenes_count_as_one_node() {
        let dir =
            std::env::temp_dir().join(format!("brandon-viewport-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("rock.scene"),
            r#"(name: "Rock", children: [(name: "Dust", children: [(name: "Speck")])])"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("main.scene"),
            r#"(name: "Main", children: [(name: "Free"), (instance: "rock.scene")])"#,
        )
        .unwrap();
        let assets = Assets::at(&dir);
        let mut scene = Scene::new();
        let root = scene.spawn(
            brandon_scene::SceneLibrary::new()
                .load(&assets, "main.scene")
                .unwrap(),
        );
        let rock = scene.find("Rock").unwrap();
        assert_eq!(
            outermost_placed(&scene, root, scene.find("Speck").unwrap()),
            rock
        );
        assert_eq!(outermost_placed(&scene, root, rock), rock);
        let free = scene.find("Free").unwrap();
        assert_eq!(outermost_placed(&scene, root, free), free);
    }

    #[test]
    fn snapping() {
        assert_eq!(snap(17.0, 8.0), 16.0);
        assert_eq!(snap(-13.0, 8.0), -16.0);
    }
}
