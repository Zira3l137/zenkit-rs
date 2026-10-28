//! Materials (`zCMaterial`), which describe how a surface is textured and how it behaves.
//!
//! Materials never come on their own: meshes and worlds embed them as archive objects, and the
//! rare `.MAT` file is just a list of them.

use crate::archive::{Archive, parse};
use crate::macros::raw_enum;
use crate::math::{Color, Vec2};
use crate::{GameVersion, Result};

/// The class version Gothic 1 writes. Gothic 2 writes `0x9C03`.
const VERSION_G1: u16 = 17408;

raw_enum! {
    /// What a surface is made of, which decides footstep sounds and hit effects.
    pub enum MaterialGroup: u8 = Undefined {
        Undefined = 0,
        Metal = 1,
        Stone = 2,
        Wood = 3,
        Earth = 4,
        Water = 5,
        Snow = 6,
        None = 0xFF,
    }
}

raw_enum! {
    pub enum AlphaFunction: u8 = None {
        Default = 0,
        None = 1,
        Blend = 2,
        Add = 3,
        Subtract = 4,
        Multiply = 5,
        MultiplyAlt = 6,
    }
}

raw_enum! {
    pub enum AnimationMapping: u8 = None {
        None = 0,
        Linear = 1,
    }
}

raw_enum! {
    pub enum WaveMode: u8 = None {
        None = 0,
        GroundAmbient = 1,
        Ground = 2,
        WallAmbient = 3,
        Wall = 4,
        Environment = 5,
        WindAmbient = 6,
        Wind = 7,
    }
}

raw_enum! {
    pub enum WaveSpeed: u8 = None {
        None = 0,
        Slow = 1,
        Normal = 2,
        Fast = 3,
    }
}

/// A material. Fields from [`detail_object_scale`](Self::detail_object_scale) down to
/// [`alpha_func`](Self::alpha_func) only exist in Gothic 2 and keep their defaults otherwise.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Material {
    pub name: String,
    pub group: MaterialGroup,
    pub color: Color,
    /// In degrees.
    pub smooth_angle: f32,
    pub texture: String,
    /// The texture's size in pixels, as the engine stored it when the material was made.
    pub texture_scale: Vec2,
    pub texture_anim_fps: f32,
    pub texture_anim_map_mode: AnimationMapping,
    pub texture_anim_map_dir: Vec2,
    pub disable_collision: bool,
    pub disable_lightmap: bool,
    pub dont_collapse: bool,
    pub detail_object: String,
    pub detail_object_scale: f32,
    pub force_occluder: bool,
    pub environment_mapping: bool,
    pub environment_mapping_strength: f32,
    pub wave_mode: WaveMode,
    pub wave_speed: WaveSpeed,
    pub wave_max_amplitude: f32,
    pub wave_grid_size: f32,
    pub ignore_sun: bool,
    pub alpha_func: AlphaFunction,
    pub default_mapping: Vec2,
}

impl Material {
    /// Reads a whole `zCMaterial` object, telling the game version from its class version.
    pub fn read(archive: &mut Archive) -> Result<Self> {
        let object = archive.expect_object(b"zCMaterial")?;
        let game = match object.version {
            VERSION_G1 => GameVersion::Gothic1,
            _ => GameVersion::Gothic2,
        };

        let material = Self::read_fields(archive, game)?;
        if !archive.end_object()? {
            log::warn!("material {:?} has unread fields", material.name);
            archive.skip_object()?;
        }
        Ok(material)
    }

    /// Reads a material list: a count, then a name and a material for each. This is the layout
    /// of `.MAT` files and of the material section in meshes.
    pub fn read_list(archive: &mut Archive) -> Result<Vec<Self>> {
        let count = archive.int()?.max(0) as usize;
        let mut materials = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            // The name is stored again inside the object; this copy is ignored, as in the engine.
            archive.string()?;
            materials.push(Self::read(archive)?);
        }
        Ok(materials)
    }

    /// Reads the fields of a material whose object header has already been read.
    pub fn read_fields(archive: &mut Archive, game: GameVersion) -> Result<Self> {
        let mut m = Self {
            name: archive.name()?,
            group: MaterialGroup::from_raw(archive.enumeration()?),
            color: archive.color()?,
            smooth_angle: archive.float()?,
            texture: archive.name()?,
            texture_scale: Vec2::from_array(parse::floats(archive.string()?)),
            texture_anim_fps: archive.float()?,
            texture_anim_map_mode: AnimationMapping::from_raw(archive.enumeration()?),
            texture_anim_map_dir: Vec2::from_array(parse::floats(archive.string()?)),
            disable_collision: archive.bool()?,
            disable_lightmap: archive.bool()?,
            dont_collapse: archive.bool()?,
            detail_object: archive.name()?,
            ..Self::default()
        };

        match game {
            GameVersion::Gothic1 => m.alpha_func = AlphaFunction::Default,
            GameVersion::Gothic2 => {
                m.detail_object_scale = archive.float()?;
                m.force_occluder = archive.bool()?;
                m.environment_mapping = archive.bool()?;
                m.environment_mapping_strength = archive.float()?;
                m.wave_mode = WaveMode::from_raw(archive.enumeration()?);
                m.wave_speed = WaveSpeed::from_raw(archive.enumeration()?);
                m.wave_max_amplitude = archive.float()?;
                m.wave_grid_size = archive.float()?;
                m.ignore_sun = archive.bool()?;
                m.alpha_func = AlphaFunction::from_raw(archive.enumeration()?);
            }
        }

        m.default_mapping = archive.vec2()?;
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_enum_values_fall_back_to_the_default() {
        assert_eq!(MaterialGroup::from_raw(3), MaterialGroup::Wood);
        assert_eq!(MaterialGroup::from_raw(0xFF), MaterialGroup::None);
        assert_eq!(MaterialGroup::from_raw(42), MaterialGroup::Undefined);
        assert_eq!(WaveSpeed::default(), WaveSpeed::None);
    }
}
