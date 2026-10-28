//! Math types shared by every format.
//!
//! Vectors and matrices come from [`glam`], which is re-exported so that your code and this crate
//! always agree on its version.

pub use glam::{self, Mat3, Mat4, Vec2, Vec3, Vec4};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// An axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}
