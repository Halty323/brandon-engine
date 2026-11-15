use crate::{Node, NodeId, Scene, Shape};
use glam::{Affine2, Vec2};

/// A shape placed in the world.
enum Placed {
    Circle { center: Vec2, radius: f32 },
    /// A convex quad: a rectangle after rotation and scaling.
    Quad([Vec2; 4]),
}

fn place(shape: &Shape, world: &Affine2) -> Placed {
    match *shape {
        Shape::Circle { radius } => {
            let scale = world.matrix2.x_axis.length().max(world.matrix2.y_axis.length());
            Placed::Circle { center: world.translation, radius: radius * scale }
        }
        Shape::Rect { size } => {
            let h = size * 0.5;
            Placed::Quad([
                world.transform_point2(Vec2::new(-h.x, -h.y)),
                world.transform_point2(Vec2::new(h.x, -h.y)),
                world.transform_point2(Vec2::new(h.x, h.y)),
                world.transform_point2(Vec2::new(-h.x, h.y)),
            ])
        }
    }
}

fn project(points: &[Vec2; 4], axis: Vec2) -> (f32, f32) {
    points.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
        let d = p.dot(axis);
        (lo.min(d), hi.max(d))
    })
}

fn quads_overlap(a: &[Vec2; 4], b: &[Vec2; 4]) -> bool {
    for quad in [a, b] {
        for i in 0..4 {
            let axis = (quad[(i + 1) % 4] - quad[i]).perp();
            if axis == Vec2::ZERO {
                continue;
            }
            let (a_lo, a_hi) = project(a, axis);
            let (b_lo, b_hi) = project(b, axis);
            if a_hi < b_lo || b_hi < a_lo {
                return false;
            }
        }
    }
    true
}

fn point_in_quad(point: Vec2, quad: &[Vec2; 4]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let cross = (quad[(i + 1) % 4] - quad[i]).perp_dot(point - quad[i]);
        if cross != 0.0 {
            if sign != 0.0 && cross.signum() != sign {
                return false;
            }
            sign = cross.signum();
        }
    }
    true
}

fn distance_to_segment(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 { ((point - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
    point.distance(a + ab * t)
}

fn circle_quad_overlap(center: Vec2, radius: f32, quad: &[Vec2; 4]) -> bool {
    point_in_quad(center, quad) || (0..4).any(|i| distance_to_segment(center, quad[i], quad[(i + 1) % 4]) <= radius)
}

fn overlap(a: &Placed, b: &Placed) -> bool {
    match (a, b) {
        (Placed::Circle { center: c1, radius: r1 }, Placed::Circle { center: c2, radius: r2 }) => {
            c1.distance_squared(*c2) <= (r1 + r2) * (r1 + r2)
        }
        (Placed::Circle { center, radius }, Placed::Quad(q)) | (Placed::Quad(q), Placed::Circle { center, radius }) => {
            circle_quad_overlap(*center, *radius, q)
        }
        (Placed::Quad(a), Placed::Quad(b)) => quads_overlap(a, b),
    }
}

fn contains_point(shape: &Placed, point: Vec2) -> bool {
    match shape {
        Placed::Circle { center, radius } => center.distance_squared(point) <= radius * radius,
        Placed::Quad(q) => point_in_quad(point, q),
    }
}

impl Scene {
    /// The nodes whose [`Shape`] overlaps the shape of `node`.
    ///
    /// `node` is tested where it is right now. Other nodes are tested where
    /// they were at the start of the frame. Returns nothing if `node` has no
    /// shape.
    pub fn overlapping(&self, node: &Node) -> Vec<NodeId> {
        let Some(shape) = &node.shape else { return Vec::new() };
        let mine = place(shape, &self.world_transform_of(node));
        let mut hits = Vec::new();
        for id in self.ids() {
            if id == node.id() {
                continue;
            }
            let Some(other) = self.get(id) else { continue };
            let Some(other_shape) = &other.shape else { continue };
            if overlap(&mine, &place(other_shape, &self.world_transform(id))) {
                hits.push(id);
            }
        }
        hits
    }

    /// The nodes whose [`Shape`] contains a world position, for example the
    /// mouse position.
    pub fn shapes_at(&self, point: Vec2) -> Vec<NodeId> {
        self.ids()
            .into_iter()
            .filter(|id| {
                self.get(*id)
                    .and_then(|n| n.shape.as_ref())
                    .is_some_and(|shape| contains_point(&place(shape, &self.world_transform(*id)), point))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(scene: &Scene, ids: Vec<NodeId>) -> Vec<String> {
        ids.into_iter().map(|id| scene.get(id).unwrap().name.clone()).collect()
    }

    #[test]
    fn circles_and_rects() {
        let mut scene = Scene::new();
        let a = scene.spawn(Node::new("a").with(Shape::circle(10.0)));
        scene.spawn(Node::new("touching circle").at(18.0, 0.0).with(Shape::circle(10.0)));
        scene.spawn(Node::new("far circle").at(30.0, 0.0).with(Shape::circle(10.0)));
        scene.spawn(Node::new("rect").at(0.0, 14.0).with(Shape::rect(100.0, 10.0)));
        scene.spawn(Node::new("far rect").at(0.0, 30.0).with(Shape::rect(100.0, 10.0)));
        scene.spawn(Node::new("no shape").at(0.0, 0.0));
        let node = scene.get(a).unwrap();
        assert_eq!(names(&scene, scene.overlapping(node)), ["touching circle", "rect"]);
    }

    #[test]
    fn rotated_rects() {
        let mut scene = Scene::new();
        // Two thin bars. Unrotated they are far apart along y; rotating one
        // by a quarter turn makes them cross.
        let a = scene.spawn(Node::new("a").with(Shape::rect(100.0, 4.0)));
        let b = scene.spawn(Node::new("b").at(0.0, 30.0).with(Shape::rect(100.0, 4.0)));
        assert!(scene.overlapping(scene.get(a).unwrap()).is_empty());
        scene.get_mut(b).unwrap().transform.rotation = std::f32::consts::FRAC_PI_2;
        scene.refresh_transforms();
        assert_eq!(scene.overlapping(scene.get(a).unwrap()), [b]);
    }

    #[test]
    fn circle_inside_big_rect_and_scaled_parent() {
        let mut scene = Scene::new();
        let parent = scene.spawn(Node::new("parent").scaled(10.0));
        let big = scene.spawn_child(parent, Node::new("big").with(Shape::rect(10.0, 10.0)));
        let dot = scene.spawn(Node::new("dot").at(40.0, -40.0).with(Shape::circle(1.0)));
        assert_eq!(scene.overlapping(scene.get(dot).unwrap()), [big]);
        assert_eq!(scene.shapes_at(Vec2::new(49.0, 49.0)), [big]);
        assert!(scene.shapes_at(Vec2::new(51.0, 0.0)).is_empty());
    }
}
