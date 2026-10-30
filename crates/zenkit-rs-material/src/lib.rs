//! Materials (`zCMaterial`), which describe how a surface is textured and how it behaves.
//!
//! Materials never come on their own. When the engine compiles a model, it turns each material of
//! the `.3DS` or `.ASC` file into a `zCMaterial`, and the compiled mesh (`.MSH`, `.MRM`, `.MDM`)
//! keeps the whole material in a material list; worlds keep theirs in the world mesh. The fields a
//! modelling tool cannot express, like the [group](Material::group) that decides footstep sounds,
//! start out with defaults and are set in the Spacer.
//!
//! Materials are read from an [`Archive`] and written to an [`ArchiveWriter`]. The `.MAT` samples
//! in the tests are material lists cut out of compiled meshes, not files the game uses.

use tracing::warn;
use zenkit_rs_archive::Archive;
use zenkit_rs_archive::ArchiveWriter;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::ResultExt;
use zenkit_rs_core::math::Color;
use zenkit_rs_core::math::Vec2;
use zenkit_rs_core::raw_enum;
use zenkit_rs_core::text;

const VERSION_G1: u16 = 17408;
const VERSION_G2: u16 = 0x9C03;

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
            warn!("material {:?} has unread fields", material.name);
            archive.skip_object()?;
        }
        Ok(material)
    }

    /// Reads a material list: a count, then a name and a material for each. This is how
    /// compiled meshes store their materials.
    pub fn read_list(archive: &mut Archive) -> Result<Vec<Self>> {
        let count = archive.int()?.max(0) as usize;
        let mut materials = Vec::with_capacity(count.min(4096));
        for i in 0..count {
            materials.push(Self::read_named(archive).context(|| format!("material {i}"))?);
        }
        Ok(materials)
    }

    /// Reads a name followed by a whole material object, the way material lists store each entry.
    /// The name is stored again inside the object; this copy is ignored, as in the engine.
    pub fn read_named(archive: &mut Archive) -> Result<Self> {
        archive.string()?;
        Self::read(archive)
    }

    /// Reads the fields of a material whose object header has already been read.
    pub fn read_fields(archive: &mut Archive, game: GameVersion) -> Result<Self> {
        let mut m = Self {
            name: archive.name()?,
            group: MaterialGroup::from_raw(archive.enumeration()?),
            color: archive.color()?,
            smooth_angle: archive.float()?,
            texture: archive.name()?,
            texture_scale: Vec2::from_array(text::floats(archive.string()?)),
            texture_anim_fps: archive.float()?,
            texture_anim_map_mode: AnimationMapping::from_raw(archive.enumeration()?),
            texture_anim_map_dir: Vec2::from_array(text::floats(archive.string()?)),
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

    /// Writes a whole `zCMaterial` object, with the class version and fields of `game`.
    pub fn write(&self, w: &mut ArchiveWriter, game: GameVersion) {
        let version = match game {
            GameVersion::Gothic1 => VERSION_G1,
            GameVersion::Gothic2 => VERSION_G2,
        };
        w.begin_object("%", "zCMaterial", version);
        self.write_fields(w, game);
        w.end_object();
    }

    /// Writes a material list, the counterpart of [`read_list`](Self::read_list).
    pub fn write_list(materials: &[Self], w: &mut ArchiveWriter, game: GameVersion) {
        w.int("", materials.len() as i32);
        for material in materials {
            material.write_named(w, game);
        }
    }

    /// Writes the material's name followed by the whole material, the counterpart of
    /// [`read_named`](Self::read_named).
    pub fn write_named(&self, w: &mut ArchiveWriter, game: GameVersion) {
        w.name("", &self.name);
        self.write(w, game);
    }

    /// Writes the fields of a material whose object header has already been written.
    pub fn write_fields(&self, w: &mut ArchiveWriter, game: GameVersion) {
        w.name("name", &self.name);
        w.enumeration("matGroup", self.group.raw());
        w.color("color", self.color);
        w.float("smoothAngle", self.smooth_angle);
        w.name("texture", &self.texture);
        w.string("texScale", &pair(self.texture_scale));
        w.float("texAniFPS", self.texture_anim_fps);
        w.enumeration("texAniMapMode", self.texture_anim_map_mode.raw());
        w.string("texAniMapDir", &pair(self.texture_anim_map_dir));
        w.bool("noCollDet", self.disable_collision);
        w.bool("noLightmap", self.disable_lightmap);
        w.bool("losDontCollapse", self.dont_collapse);
        w.name("detailObject", &self.detail_object);

        if game == GameVersion::Gothic2 {
            w.float("detailObjectScale", self.detail_object_scale);
            w.bool("forceOccluder", self.force_occluder);
            w.bool("environmentalMapping", self.environment_mapping);
            w.float("environmentalMappingStrength", self.environment_mapping_strength);
            w.enumeration("waveMode", self.wave_mode.raw());
            w.enumeration("waveSpeed", self.wave_speed.raw());
            w.float("waveMaxAmplitude", self.wave_max_amplitude);
            w.float("waveGridSize", self.wave_grid_size);
            w.bool("ignoreSunLight", self.ignore_sun);
            w.enumeration("alphaFunc", self.alpha_func.raw());
        }

        w.vec2("defaultMapping", self.default_mapping);
    }
}

/// Two floats as text, the way the engine stores a texture's scale and animation direction. Eight
/// digits can round away the last bit of a float, as they do in the engine.
fn pair(v: Vec2) -> Vec<u8> {
    format!("{} {}", text::float_g(v.x, 8), text::float_g(v.y, 8)).into_bytes()
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
