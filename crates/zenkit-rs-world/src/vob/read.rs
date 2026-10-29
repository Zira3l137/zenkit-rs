use std::collections::HashMap;
use std::ops::Deref;
use std::ops::DerefMut;

use tracing::warn;
use zenkit_rs_archive::Archive;
use zenkit_rs_archive::ObjectHeader;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::ResultExt;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::math::Aabb;
use zenkit_rs_core::math::Mat3;
use zenkit_rs_core::text;
use zenkit_rs_material::AlphaFunction;

use super::Animate;
use super::AnimationMode;
use super::CameraKeyFrame;
use super::CodeMaster;
use super::Container;
use super::CutsceneCamera;
use super::Decal;
use super::Door;
use super::Earthquake;
use super::Fire;
use super::Interactive;
use super::Item;
use super::LensFlare;
use super::Light;
use super::MessageFilter;
use super::Mob;
use super::Mover;
use super::MoverController;
use super::ParticleEffectController;
use super::ShadowType;
use super::Sound;
use super::SoundDaytime;
use super::SpriteAlignment;
use super::TouchDamage;
use super::Trigger;
use super::TriggerChangeLevel;
use super::TriggerList;
use super::TriggerScript;
use super::TriggerUntouch;
use super::TriggerWorldStart;
use super::Visual;
use super::VisualKind;
use super::Vob;
use super::VobId;
use super::VobKind;
use super::Vobs;
use super::ZoneFarPlane;
use super::ZoneFog;
use super::ZoneMusic;

pub(crate) const FORMAT: &str = "world";

/// What an object read earlier was turned into, so that references to it can be resolved.
#[derive(Debug, Clone)]
pub(crate) enum Shared {
    Vob(VobId),
    Visual(VisualKind),
}

/// The start of the next object, with references and null objects already dealt with.
pub(crate) enum Next<'a> {
    Null,
    /// A reference, with what it points to if that was read before.
    Reference(Option<Shared>),
    New(ObjectHeader<'a>),
}

/// The state of reading the objects of one archive.
///
/// Archives store every object once; later uses of it are references by its index. So
/// everything that can be referenced is remembered in `objects`, and VOBs are collected in one
/// flat list that references point into.
pub(crate) struct Ctx<'a, 'b> {
    archive: &'b mut Archive<'a>,
    pub(crate) game: GameVersion,
    pub(crate) vobs: Vec<Vob>,
    pub(crate) objects: HashMap<u32, Shared>,
}

// Every reader in this crate reads values straight from the archive, so the context stands in
// for it.
impl<'a> Deref for Ctx<'a, '_> {
    type Target = Archive<'a>;

    fn deref(&self) -> &Archive<'a> {
        self.archive
    }
}

impl<'a> DerefMut for Ctx<'a, '_> {
    fn deref_mut(&mut self) -> &mut Archive<'a> {
        self.archive
    }
}

impl<'a, 'b> Ctx<'a, 'b> {
    pub(crate) fn new(archive: &'b mut Archive<'a>, game: GameVersion) -> Result<Self> {
        if archive.header().save_game {
            return Err(Error::unsupported(FORMAT, "save-game archives"));
        }
        Ok(Self { archive, game, vobs: Vec::new(), objects: HashMap::new() })
    }

    pub(crate) fn finish(self) -> Vobs {
        Vobs(self.vobs)
    }

    /// Opens the next object, which must be there. References and null objects are read whole.
    pub(crate) fn begin(&mut self, what: &str) -> Result<Next<'a>> {
        let offset = self.offset();
        let Some(object) = self.begin_object()? else {
            let reason = format!("expected {what} at offset {offset}");
            return Err(Error::malformed(FORMAT, reason));
        };

        if object.is_reference() {
            self.end(&object)?;
            let target = self.objects.get(&object.index).cloned();
            if target.is_none() {
                warn!("{what} refers to object {}, which was not read before", object.index);
            }
            return Ok(Next::Reference(target));
        }
        if object.is_null() {
            self.end(&object)?;
            return Ok(Next::Null);
        }
        Ok(Next::New(object))
    }

    /// Closes an object, skipping whatever was left unread in it.
    pub(crate) fn end(&mut self, object: &ObjectHeader) -> Result<()> {
        if !self.end_object()? {
            warn!("object {} {:?} has unread fields", object.index, text::latin1(object.class));
            self.skip_object()?;
        }
        Ok(())
    }

    /// Reads a VOB object: a new one, a reference to one read before, or null.
    pub(crate) fn vob(&mut self) -> Result<Option<VobId>> {
        let object = match self.begin("a VOB")? {
            Next::New(object) => object,
            Next::Null | Next::Reference(None) => return Ok(None),
            Next::Reference(Some(Shared::Vob(id))) => return Ok(Some(id)),
            Next::Reference(Some(other)) => {
                warn!("expected a reference to a VOB, found one to {other:?}");
                return Ok(None);
            }
        };

        let class = text::latin1(object.class);
        let Some(read_kind) = kind_reader(object.class) else {
            warn!("skipping VOB {} of unknown class {class:?}", object.index);
            self.skip_object()?;
            return Ok(None);
        };

        // Registered before reading, since objects inside may refer back to the VOB they are in.
        let id = VobId(self.vobs.len() as u32);
        self.vobs.push(Vob::default());
        self.objects.insert(object.index, Shared::Vob(id));

        let vob =
            read_vob(self, read_kind).context(|| format!("VOB {} {class:?}", object.index))?;
        self.vobs[id.index()] = vob;
        self.end(&object)?;
        Ok(Some(id))
    }

    /// Reads a visual object, which several VOBs may share.
    fn visual(&mut self) -> Result<Option<VisualKind>> {
        let object = match self.begin("a visual")? {
            Next::New(object) => object,
            Next::Null | Next::Reference(None) => return Ok(None),
            Next::Reference(Some(Shared::Visual(kind))) => return Ok(Some(kind)),
            Next::Reference(Some(other)) => {
                warn!("expected a reference to a visual, found one to {other:?}");
                return Ok(None);
            }
        };

        let kind = match object.class {
            b"zCMesh" => VisualKind::Mesh,
            b"zCProgMeshProto" => VisualKind::MultiResolutionMesh,
            b"zCParticleFX" => VisualKind::ParticleEffect,
            b"zCAICamera" => VisualKind::Camera,
            b"zCModel" => VisualKind::Model,
            b"zCMorphMesh" => VisualKind::MorphMesh,
            b"zCDecal" => VisualKind::Decal(read_decal(self)?),
            other => {
                warn!("skipping visual of unknown class {:?}", text::latin1(other));
                self.skip_object()?;
                return Ok(Some(VisualKind::Unknown));
            }
        };

        self.objects.insert(object.index, Shared::Visual(kind.clone()));
        self.end(&object)?;
        Ok(Some(kind))
    }

    /// Skips an object this crate does not read yet, warning unless it is null.
    fn skip_unread(&mut self, what: &str) -> Result<()> {
        if let Next::New(object) = self.begin(what)? {
            warn!("skipping {what} of class {:?}", text::latin1(object.class));
            self.skip_object()?;
        }
        Ok(())
    }
}

type ReadKind = fn(&mut Ctx) -> Result<VobKind>;

/// How to read what a class adds to a VOB, or `None` for classes this crate does not know.
fn kind_reader(class: &[u8]) -> Option<ReadKind> {
    let read: ReadKind = match class {
        b"zCVob" => |_| Ok(VobKind::Vob),
        b"zCVobLevelCompo:zCVob" => |_| Ok(VobKind::Level),
        b"zCVobSpot:zCVob" => |_| Ok(VobKind::Spot),
        b"zCVobStartpoint:zCVob" => |_| Ok(VobKind::StartPoint),
        b"zCVobStair:zCVob" => |_| Ok(VobKind::Stair),
        b"zCVobScreenFX:zCVob" => |_| Ok(VobKind::ScreenEffect),
        b"zCVobAnimate:zCVob" => |r| Animate::read(r).map(VobKind::Animate),
        b"oCItem:zCVob" => |r| Item::read(r).map(VobKind::Item),
        b"zCVobLensFlare:zCVob" => |r| LensFlare::read(r).map(VobKind::LensFlare),
        b"zCPFXControler:zCVob" => {
            |r| ParticleEffectController::read(r).map(VobKind::ParticleEffect)
        }
        b"zCMessageFilter:zCVob" => |r| MessageFilter::read(r).map(VobKind::MessageFilter),
        b"zCCodeMaster:zCVob" => |r| CodeMaster::read(r).map(VobKind::CodeMaster),
        b"zCMoverControler:zCVob" => |r| MoverController::read(r).map(VobKind::MoverController),
        b"oCTouchDamage:zCTouchDamage:zCVob" => |r| TouchDamage::read(r).map(VobKind::TouchDamage),
        b"zCEarthquake:zCVob" => |r| Earthquake::read(r).map(VobKind::Earthquake),
        b"zCVobLight:zCVob" => |r| Light::read(r).map(VobKind::Light),
        b"zCVobSound:zCVob" => |r| Sound::read(r).map(VobKind::Sound),
        b"zCVobSoundDaytime:zCVobSound:zCVob" => {
            |r| SoundDaytime::read(r).map(VobKind::SoundDaytime)
        }
        b"oCZoneMusic:zCVob" => |r| ZoneMusic::read(r).map(VobKind::ZoneMusic),
        b"oCZoneMusicDefault:oCZoneMusic:zCVob" => {
            |r| ZoneMusic::read(r).map(VobKind::ZoneMusicDefault)
        }
        b"zCZoneZFog:zCVob" => |r| ZoneFog::read(r).map(VobKind::ZoneFog),
        b"zCZoneZFogDefault:zCZoneZFog:zCVob" => |r| ZoneFog::read(r).map(VobKind::ZoneFogDefault),
        b"zCZoneVobFarPlane:zCVob" => |r| ZoneFarPlane::read(r).map(VobKind::ZoneFarPlane),
        b"zCZoneVobFarPlaneDefault:zCZoneVobFarPlane:zCVob" => {
            |r| ZoneFarPlane::read(r).map(VobKind::ZoneFarPlaneDefault)
        }
        b"oCMOB:zCVob" => |r| Mob::read(r).map(VobKind::Mob),
        b"oCMobInter:oCMOB:zCVob" => |r| Interactive::read(r).map(VobKind::Interactive),
        b"oCMobBed:oCMobInter:oCMOB:zCVob" => |r| Interactive::read(r).map(VobKind::Bed),
        b"oCMobLadder:oCMobInter:oCMOB:zCVob" => |r| Interactive::read(r).map(VobKind::Ladder),
        b"oCMobSwitch:oCMobInter:oCMOB:zCVob" => |r| Interactive::read(r).map(VobKind::Switch),
        b"oCMobWheel:oCMobInter:oCMOB:zCVob" => |r| Interactive::read(r).map(VobKind::Wheel),
        b"oCMobFire:oCMobInter:oCMOB:zCVob" => |r| Fire::read(r).map(VobKind::Fire),
        b"oCMobContainer:oCMobInter:oCMOB:zCVob" => |r| Container::read(r).map(VobKind::Container),
        b"oCMobDoor:oCMobInter:oCMOB:zCVob" => |r| Door::read(r).map(VobKind::Door),
        b"zCTrigger:zCVob" => |r| Trigger::read(r).map(VobKind::Trigger),
        b"oCCSTrigger:zCTrigger:zCVob" => |r| Trigger::read(r).map(VobKind::CutsceneTrigger),
        b"zCTriggerList:zCTrigger:zCVob" => |r| TriggerList::read(r).map(VobKind::TriggerList),
        b"oCTriggerScript:zCTrigger:zCVob" => {
            |r| TriggerScript::read(r).map(VobKind::TriggerScript)
        }
        b"oCTriggerChangeLevel:zCTrigger:zCVob" => {
            |r| TriggerChangeLevel::read(r).map(VobKind::TriggerChangeLevel)
        }
        b"zCMover:zCTrigger:zCVob" => |r| Mover::read(r).map(|m| VobKind::Mover(Box::new(m))),
        b"zCTriggerWorldStart:zCVob" => {
            |r| TriggerWorldStart::read(r).map(VobKind::TriggerWorldStart)
        }
        b"zCTriggerUntouch:zCVob" => |r| TriggerUntouch::read(r).map(VobKind::TriggerUntouch),
        b"zCCSCamera:zCVob" => |r| CutsceneCamera::read(r).map(VobKind::CutsceneCamera),
        b"zCCamTrj_KeyFrame:zCVob" => |r| CameraKeyFrame::read(r).map(VobKind::CameraKeyFrame),
        _ => return None,
    };
    Some(read)
}

fn read_vob(r: &mut Ctx, read_kind: ReadKind) -> Result<Vob> {
    let mut vob = read_base(r)?;
    vob.kind = read_kind(r)?;
    Ok(vob)
}

/// Reads the `zCVob` part every VOB starts with.
fn read_base(r: &mut Ctx) -> Result<Vob> {
    let mut vob = Vob::default();
    let gothic2 = r.game == GameVersion::Gothic2;

    let mut visual_name = String::new();
    let mut has_visual = true;
    let mut has_ai = true;

    let packed = r.int()? != 0;
    if packed {
        // The Spacer's default: the transform and flags squeezed into one raw value, and the
        // names stored only when they are set.
        let raw = r.raw(if gothic2 { 83 } else { 74 })?;
        let mut b = Reader::new(&raw);
        vob.bbox = Aabb::read(&mut b)?;
        vob.position = b.vec3()?;
        vob.rotation = mat3(&mut b)?;

        let bits = b.u8()?;
        vob.show_visual = bits & 1 != 0;
        vob.camera_alignment = SpriteAlignment::from_raw(u32::from(bits >> 1 & 0b11));
        vob.cd_static = bits & 1 << 3 != 0;
        vob.cd_dynamic = bits & 1 << 4 != 0;
        vob.is_static = bits & 1 << 5 != 0;
        vob.dynamic_shadow = ShadowType::from_raw(u32::from(bits >> 6));

        let flags = if gothic2 { b.u16()? } else { u16::from(b.u8()?) };
        let has = |bit: u16| flags & 1 << bit != 0;
        has_visual = has(3);
        has_ai = has(4);
        vob.physics_enabled = has(6);
        if gothic2 {
            // One bit higher than ZenKit reads them: bit 7 is a flag of unknown meaning, as
            // comparing packed VOBs of a save with the same world saved unpacked shows.
            vob.anim_mode = AnimationMode::from_raw(u32::from(flags >> 8 & 0b11));
            vob.bias = i32::from(flags >> 10 & 0b1_1111);
            vob.ambient = has(15);
            vob.anim_strength = b.f32()?;
            vob.far_clip_scale = b.f32()?;
        }

        if has(0) {
            vob.preset_name = r.name()?;
        }
        if has(1) {
            vob.name = r.name()?;
        }
        if has(2) {
            visual_name = r.name()?;
        }
    } else {
        vob.preset_name = r.name()?;
        vob.bbox = r.aabb()?;
        vob.rotation = r.mat3()?;
        vob.position = r.vec3()?;
        vob.name = r.name()?;
        visual_name = r.name()?;
        vob.show_visual = r.bool()?;
        vob.camera_alignment = SpriteAlignment::from_raw(r.enumeration()?);
        if gothic2 {
            vob.anim_mode = AnimationMode::from_raw(r.enumeration()?);
            vob.anim_strength = r.float()?;
            vob.far_clip_scale = r.float()?;
        }
        vob.cd_static = r.bool()?;
        vob.cd_dynamic = r.bool()?;
        vob.is_static = r.bool()?;
        vob.dynamic_shadow = ShadowType::from_raw(r.enumeration()?);
        if gothic2 {
            vob.bias = r.int()?;
            vob.ambient = r.bool()?;
        }
    }

    let kind = if has_visual { r.visual()? } else { None };
    vob.visual = match kind {
        Some(kind) => Some(Visual { name: visual_name, kind }),
        None if !visual_name.is_empty() => {
            Some(Visual { name: visual_name, kind: VisualKind::Unknown })
        }
        None => None,
    };

    // Only save games give VOBs an AI.
    if has_ai {
        r.skip_unread("AI")?;
    }

    Ok(vob)
}

fn read_decal(r: &mut Ctx) -> Result<Decal> {
    // The decal's own copy of the visual name, which the engine ignores.
    r.string()?;
    let mut decal = Decal {
        dimension: r.vec2()?,
        offset: r.vec2()?,
        two_sided: r.bool()?,
        alpha_func: AlphaFunction::from_raw(r.enumeration()?),
        texture_anim_fps: r.float()?,
        ..Decal::default()
    };
    if r.game == GameVersion::Gothic2 {
        decal.alpha_weight = r.byte()?;
        decal.ignore_daylight = r.bool()?;
    }
    Ok(decal)
}

/// A 3×3 matrix stored row by row.
pub(crate) fn mat3(r: &mut Reader) -> Result<Mat3> {
    let mut values = [0.0; 9];
    for value in &mut values {
        *value = r.f32()?;
    }
    Ok(Mat3::from_cols_array(&values).transpose())
}
