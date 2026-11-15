use glam::{Affine2, Vec2};
use serde::{Deserialize, Serialize};

/// Where a node is, relative to its parent.
///
/// `x` grows to the right and `y` grows downwards, like pixels on a screen.
/// Positive rotation turns clockwise.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Transform {
    #[serde(skip_serializing_if = "is_zero")]
    pub position: Vec2,
    /// Rotation in radians.
    #[serde(skip_serializing_if = "is_zero_f32")]
    pub rotation: f32,
    #[serde(skip_serializing_if = "is_one")]
    pub scale: Vec2,
    /// Draw order. Higher values are drawn on top. Added to the parent's `z`.
    #[serde(skip_serializing_if = "is_zero_i32")]
    pub z: i32,
}

fn is_zero(v: &Vec2) -> bool {
    *v == Vec2::ZERO
}
fn is_one(v: &Vec2) -> bool {
    *v == Vec2::ONE
}
fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}
fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        position: Vec2::ZERO,
        rotation: 0.0,
        scale: Vec2::ONE,
        z: 0,
    };

    /// A transform at a position.
    pub fn at(x: f32, y: f32) -> Self {
        Transform {
            position: Vec2::new(x, y),
            ..Self::IDENTITY
        }
    }

    /// The transform as a matrix.
    pub fn to_affine(&self) -> Affine2 {
        Affine2::from_scale_angle_translation(self.scale, self.rotation, self.position)
    }

    /// Is this the identity transform?
    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// The direction this transform's local "right" (`+x`) points in.
    pub fn right(&self) -> Vec2 {
        Vec2::from_angle(self.rotation)
    }

    /// The direction this transform's local "up" (`-y`) points in.
    pub fn up(&self) -> Vec2 {
        -Vec2::from_angle(self.rotation).perp()
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions() {
        let t = Transform::IDENTITY;
        assert!(t.right().abs_diff_eq(Vec2::X, 1e-6));
        assert!(t.up().abs_diff_eq(Vec2::NEG_Y, 1e-6));
        let quarter = Transform {
            rotation: std::f32::consts::FRAC_PI_2,
            ..Transform::IDENTITY
        };
        // A clockwise quarter turn makes "right" point down the screen.
        assert!(quarter.right().abs_diff_eq(Vec2::Y, 1e-6));
        assert!(quarter
            .to_affine()
            .transform_vector2(Vec2::X)
            .abs_diff_eq(Vec2::Y, 1e-6));
    }
}
