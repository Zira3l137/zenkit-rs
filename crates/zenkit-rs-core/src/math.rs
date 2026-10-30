//! Math types shared by every format.
//!
//! Vectors and matrices come from [`glam`], which is re-exported so that your code and this crate
//! always agree on its version.

use crate::Error;
use crate::Result;
use crate::io::Reader;
use crate::io::Writer;

pub use glam;
pub use glam::Mat3;
pub use glam::Mat4;
pub use glam::Quat;
pub use glam::Vec2;
pub use glam::Vec3;
pub use glam::Vec4;

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

impl Aabb {
    /// Reads `min` then `max`.
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self { min: r.vec3()?, max: r.vec3()? })
    }

    pub fn write(&self, w: &mut Writer) {
        w.vec3(self.min);
        w.vec3(self.max);
    }
}

/// An oriented bounding box, with an optional hierarchy of tighter boxes inside it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Obb {
    pub center: Vec3,
    pub axes: [Vec3; 3],
    /// Half the box's extent along each of [`axes`](Self::axes).
    pub half_width: Vec3,
    pub children: Vec<Obb>,
}

impl Obb {
    const MAX_DEPTH: usize = 64;

    /// The smallest axis-aligned box that contains this one.
    ///
    /// ZenKit's version starts its maximum at the smallest positive float rather than the most
    /// negative one, so boxes lying entirely below zero come out wrong there.
    pub fn to_aabb(&self) -> Aabb {
        let mut aabb = Aabb { min: Vec3::INFINITY, max: Vec3::NEG_INFINITY };
        for corner in 0..8 {
            let sign = |bit: u32| if corner & bit == 0 { -1.0 } else { 1.0 };
            let point = self.center
                + self.axes[0] * self.half_width.x * sign(4)
                + self.axes[1] * self.half_width.y * sign(2)
                + self.axes[2] * self.half_width.z * sign(1);
            aabb.min = aabb.min.min(point);
            aabb.max = aabb.max.max(point);
        }
        aabb
    }

    /// Reads a box and, recursively, its children.
    pub fn read(r: &mut Reader) -> Result<Self> {
        Self::read_nested(r, 0)
    }

    fn read_nested(r: &mut Reader, depth: usize) -> Result<Self> {
        if depth > Self::MAX_DEPTH {
            return Err(Error::malformed("bounding box", "children nest too deeply"));
        }

        let center = r.vec3()?;
        let axes = [r.vec3()?, r.vec3()?, r.vec3()?];
        let half_width = r.vec3()?;

        let count = r.u16()? as usize;
        let mut children = Vec::with_capacity(count.min(r.remaining() / 62));
        for _ in 0..count {
            children.push(Self::read_nested(r, depth + 1)?);
        }

        Ok(Self { center, axes, half_width, children })
    }

    /// Writes the box and its children. Fails if a box has more children than its `u16` count
    /// can hold.
    pub fn write(&self, w: &mut Writer) -> Result<()> {
        w.vec3(self.center);
        self.axes.iter().for_each(|&axis| w.vec3(axis));
        w.vec3(self.half_width);
        let count = u16::try_from(self.children.len()).map_err(|_| {
            let reason = format!("{} children, at most {} fit", self.children.len(), u16::MAX);
            Error::unwritable("bounding box", reason)
        })?;
        w.u16(count);
        self.children.iter().try_for_each(|child| child.write(w))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obb_below_zero_converts_correctly() {
        let obb = Obb {
            center: Vec3::splat(-10.0),
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            half_width: Vec3::new(1.0, 2.0, 3.0),
            children: Vec::new(),
        };
        let aabb = obb.to_aabb();
        assert_eq!(aabb.min, Vec3::new(-11.0, -12.0, -13.0));
        assert_eq!(aabb.max, Vec3::new(-9.0, -8.0, -7.0));
    }

    #[test]
    fn obb_round_trips() {
        let leaf = |x| Obb { center: Vec3::splat(x), half_width: Vec3::ONE, ..Obb::default() };
        let obb = Obb {
            center: Vec3::new(1.0, 2.0, 3.0),
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            half_width: Vec3::new(4.0, 5.0, 6.0),
            children: vec![leaf(1.0), Obb { children: vec![leaf(2.0)], ..leaf(3.0) }],
        };
        let mut w = Writer::new();
        obb.write(&mut w).unwrap();
        let data = w.into_inner();
        assert_eq!(data.len(), 62 * 4);
        let mut r = Reader::new(&data);
        assert_eq!(Obb::read(&mut r).unwrap(), obb);
        assert!(r.is_eof());
    }

    #[test]
    fn deeply_nested_obb_is_rejected() {
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend([0u8; 60]);
            data.extend(1u16.to_le_bytes());
        }
        assert!(Obb::read(&mut Reader::new(&data)).is_err());
    }
}
