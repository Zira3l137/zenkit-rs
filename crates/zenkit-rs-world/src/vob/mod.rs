//! VOBs, short for virtual objects: everything placed in a world besides its static mesh, from
//! lights and sounds to doors, triggers and cutscene cameras.
//!
//! The engine models VOBs as a class hierarchy: `oCMobDoor` is an `oCMobInter`, which is an
//! `oCMOB`, which is a `zCVob`. Here every VOB is a [`Vob`] holding what all of them share, and
//! its [`kind`](Vob::kind) holds what its class adds. A class that extends another one keeps that
//! part as a field, so a [`Door`] has an [`interactive`](Door::interactive) part, which has a
//! [`mob`](Interactive::mob) part. Accessors like [`VobKind::mob`] find such a part whatever the
//! exact class is.
//!
//! VOBs refer to each other, and one VOB can sit in several places at once: a camera's key frames
//! are inside the camera and in the VOB tree too. So VOBs are stored flat, in [`Vobs`], and refer
//! to each other by [`VobId`].

mod camera;
mod light;
mod misc;
mod mob;
mod read;
mod sound;
mod trigger;
mod zone;

use std::ops::Index;

use zenkit_rs_archive::Archive;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::math::Aabb;
use zenkit_rs_core::math::Mat3;
use zenkit_rs_core::math::Vec2;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::raw_enum;
use zenkit_rs_material::AlphaFunction;

pub use camera::CameraKeyFrame;
pub use camera::CameraLerp;
pub use camera::CameraLoop;
pub use camera::CameraMotion;
pub use camera::CameraReference;
pub use camera::CutsceneCamera;
pub use light::Light;
pub use light::LightQuality;
pub use light::LightType;
pub use misc::Animate;
pub use misc::CodeMaster;
pub use misc::Earthquake;
pub use misc::Item;
pub use misc::LensFlare;
pub use misc::MessageFilter;
pub use misc::MessageFilterAction;
pub use misc::MoverController;
pub use misc::MoverMessage;
pub use misc::ParticleEffectController;
pub use misc::TouchCollision;
pub use misc::TouchDamage;
pub use mob::Container;
pub use mob::Door;
pub use mob::Fire;
pub use mob::Interactive;
pub use mob::Mob;
pub use mob::SoundMaterial;
pub(crate) use read::Ctx;
pub use sound::Sound;
pub use sound::SoundDaytime;
pub use sound::SoundMode;
pub use sound::SoundVolume;
pub use trigger::Mover;
pub use trigger::MoverBehavior;
pub use trigger::MoverKeyFrame;
pub use trigger::MoverLerp;
pub use trigger::MoverSpeed;
pub use trigger::Trigger;
pub use trigger::TriggerBatchMode;
pub use trigger::TriggerChangeLevel;
pub use trigger::TriggerList;
pub use trigger::TriggerScript;
pub use trigger::TriggerTarget;
pub use trigger::TriggerUntouch;
pub use trigger::TriggerWorldStart;
pub use zone::ZoneFarPlane;
pub use zone::ZoneFog;
pub use zone::ZoneMusic;

raw_enum! {
    pub enum ShadowType: u8 = None {
        None = 0,
        /// A dark circle at the VOB's feet.
        Blob = 1,
    }
}

raw_enum! {
    /// How a VOB turns to face the camera, as grass and other flat sprites do.
    pub enum SpriteAlignment: u8 = None {
        None = 0,
        Yaw = 1,
        Full = 2,
    }
}

raw_enum! {
    /// How a VOB sways in the wind. Gothic 2 only.
    pub enum AnimationMode: u8 = None {
        None = 0,
        /// Strong wind, mostly for grass and small plants.
        Wind = 1,
        /// Light wind, mostly for trees.
        WindAlt = 2,
    }
}

/// Points to one VOB in a [`Vobs`] collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VobId(u32);

impl VobId {
    /// The VOB's position in its [`Vobs`].
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Every VOB read from one archive, in the order they were read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Vobs(Vec<Vob>);

impl Vobs {
    /// Reads the VOB object that starts at the archive's current position, together with every
    /// VOB inside it, like a camera's key frames. Gives `None` if the object is null or of a class
    /// this crate does not know.
    ///
    /// This is for archives holding a single VOB.
    pub fn read(archive: &mut Archive, game: GameVersion) -> Result<Option<(Self, VobId)>> {
        let mut ctx = Ctx::new(archive, game)?;
        let id = ctx.vob()?;
        Ok(id.map(|id| (ctx.finish(), id)))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, id: VobId) -> Option<&Vob> {
        self.0.get(id.index())
    }

    /// Every VOB with its id, in the order they were read.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (VobId, &Vob)> + '_ {
        self.0.iter().enumerate().map(|(i, vob)| (VobId(i as u32), vob))
    }

    /// Walks the trees below `roots` depth first, each VOB before its children.
    pub fn walk<'a>(&'a self, roots: &[VobId]) -> Walk<'a> {
        Walk { vobs: self, stack: roots.iter().rev().copied().collect() }
    }
}

impl Index<VobId> for Vobs {
    type Output = Vob;

    fn index(&self, id: VobId) -> &Vob {
        &self.0[id.index()]
    }
}

/// See [`Vobs::walk`].
#[derive(Debug, Clone)]
pub struct Walk<'a> {
    vobs: &'a Vobs,
    stack: Vec<VobId>,
}

impl<'a> Iterator for Walk<'a> {
    type Item = (VobId, &'a Vob);

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        let vob = &self.vobs[id];
        self.stack.extend(vob.children.iter().rev());
        Some((id, vob))
    }
}

/// What every VOB has, whatever its class.
#[derive(Debug, Clone, PartialEq)]
pub struct Vob {
    pub kind: VobKind,
    /// Not unique; scripts and other VOBs use it to find this one.
    pub name: String,
    /// The Spacer template this VOB was made from.
    pub preset_name: String,
    pub bbox: Aabb,
    pub position: Vec3,
    pub rotation: Mat3,
    pub visual: Option<Visual>,
    pub show_visual: bool,
    pub camera_alignment: SpriteAlignment,
    /// Whether the Spacer keeps other VOBs from being placed inside this one. The game ignores it.
    pub cd_static: bool,
    /// Whether the player, NPCs and items collide with this VOB.
    pub cd_dynamic: bool,
    /// Whether this VOB is baked into the world's lightmaps.
    pub is_static: bool,
    pub dynamic_shadow: ShadowType,
    /// Not stored by every encoding; true when the archive does not say.
    pub physics_enabled: bool,
    /// Gothic 2 only, like the rest of the fields down to [`far_clip_scale`](Self::far_clip_scale).
    pub anim_mode: AnimationMode,
    pub anim_strength: f32,
    /// The depth bias the engine passes to Direct3D when drawing the visual.
    pub bias: i32,
    /// A development flag that release builds of the game ignore.
    pub ambient: bool,
    /// How far away the VOB is still drawn, between 0 and 2; 2 is the full distance of the
    /// world's far plane, 0 hides the VOB but keeps its collision.
    pub far_clip_scale: f32,
    /// The VOBs below this one in the VOB tree.
    pub children: Vec<VobId>,
}

impl Default for Vob {
    fn default() -> Self {
        Self {
            kind: VobKind::default(),
            name: String::new(),
            preset_name: String::new(),
            bbox: Aabb::default(),
            position: Vec3::ZERO,
            rotation: Mat3::IDENTITY,
            visual: None,
            show_visual: false,
            camera_alignment: SpriteAlignment::None,
            cd_static: true,
            cd_dynamic: true,
            is_static: false,
            dynamic_shadow: ShadowType::None,
            physics_enabled: true,
            anim_mode: AnimationMode::None,
            anim_strength: 0.0,
            bias: 0,
            ambient: false,
            far_clip_scale: 2.0,
            children: Vec::new(),
        }
    }
}

/// What a VOB's class adds, named after the engine class it was read from.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum VobKind {
    /// `zCVob`: a plain VOB, usually just showing its visual.
    #[default]
    Vob,
    /// `zCVobLevelCompo`: groups the VOBs of a part of the world. One of them carries the world
    /// mesh as its visual.
    Level,
    /// `zCVobSpot`: a named position, used by scripts to place things.
    Spot,
    /// `zCVobStartpoint`: where the player starts in this world.
    StartPoint,
    /// `zCVobStair`: marks stairs for NPC path finding. Gothic 1 only.
    Stair,
    /// `zCVobScreenFX`: holds the state of screen effects in save games.
    ScreenEffect,
    /// `zCVobAnimate`
    Animate(Animate),
    /// `oCItem`
    Item(Item),
    /// `zCVobLensFlare`
    LensFlare(LensFlare),
    /// `zCPFXControler`
    ParticleEffect(ParticleEffectController),
    /// `zCMessageFilter`
    MessageFilter(MessageFilter),
    /// `zCCodeMaster`
    CodeMaster(CodeMaster),
    /// `zCMoverControler`
    MoverController(MoverController),
    /// `oCTouchDamage`
    TouchDamage(TouchDamage),
    /// `zCEarthquake`
    Earthquake(Earthquake),
    /// `zCVobLight`
    Light(Light),
    /// `zCVobSound`
    Sound(Sound),
    /// `zCVobSoundDaytime`
    SoundDaytime(SoundDaytime),
    /// `oCZoneMusic`
    ZoneMusic(ZoneMusic),
    /// `oCZoneMusicDefault`: the music of every part of the world without a zone of its own.
    ZoneMusicDefault(ZoneMusic),
    /// `zCZoneZFog`
    ZoneFog(ZoneFog),
    /// `zCZoneZFogDefault`
    ZoneFogDefault(ZoneFog),
    /// `zCZoneVobFarPlane`
    ZoneFarPlane(ZoneFarPlane),
    /// `zCZoneVobFarPlaneDefault`
    ZoneFarPlaneDefault(ZoneFarPlane),
    /// `oCMOB`
    Mob(Mob),
    /// `oCMobInter`
    Interactive(Interactive),
    /// `oCMobBed`
    Bed(Interactive),
    /// `oCMobLadder`
    Ladder(Interactive),
    /// `oCMobSwitch`
    Switch(Interactive),
    /// `oCMobWheel`
    Wheel(Interactive),
    /// `oCMobFire`
    Fire(Fire),
    /// `oCMobContainer`
    Container(Container),
    /// `oCMobDoor`
    Door(Door),
    /// `zCTrigger`
    Trigger(Trigger),
    /// `oCCSTrigger`: starts the cutscene named by its [`target`](Trigger::target).
    CutsceneTrigger(Trigger),
    /// `zCTriggerList`
    TriggerList(TriggerList),
    /// `oCTriggerScript`
    TriggerScript(TriggerScript),
    /// `oCTriggerChangeLevel`
    TriggerChangeLevel(TriggerChangeLevel),
    /// `zCMover`
    Mover(Box<Mover>),
    /// `zCTriggerWorldStart`
    TriggerWorldStart(TriggerWorldStart),
    /// `zCTriggerUntouch`
    TriggerUntouch(TriggerUntouch),
    /// `zCCSCamera`
    CutsceneCamera(CutsceneCamera),
    /// `zCCamTrj_KeyFrame`
    CameraKeyFrame(CameraKeyFrame),
}

impl VobKind {
    /// The `oCMOB` part of any mob, from plain ones to doors and containers.
    pub fn mob(&self) -> Option<&Mob> {
        match self {
            Self::Mob(mob) => Some(mob),
            _ => self.interactive().map(|i| &i.mob),
        }
    }

    /// The `oCMobInter` part of any interactive mob.
    pub fn interactive(&self) -> Option<&Interactive> {
        match self {
            Self::Interactive(i)
            | Self::Bed(i)
            | Self::Ladder(i)
            | Self::Switch(i)
            | Self::Wheel(i) => Some(i),
            Self::Fire(fire) => Some(&fire.interactive),
            Self::Container(container) => Some(&container.interactive),
            Self::Door(door) => Some(&door.interactive),
            _ => None,
        }
    }

    /// The `zCTrigger` part of any trigger, movers included.
    pub fn trigger(&self) -> Option<&Trigger> {
        match self {
            Self::Trigger(trigger) | Self::CutsceneTrigger(trigger) => Some(trigger),
            Self::TriggerList(list) => Some(&list.trigger),
            Self::TriggerScript(script) => Some(&script.trigger),
            Self::TriggerChangeLevel(change) => Some(&change.trigger),
            Self::Mover(mover) => Some(&mover.trigger),
            _ => None,
        }
    }

    /// The `zCVobSound` part of any sound.
    pub fn sound(&self) -> Option<&Sound> {
        match self {
            Self::Sound(sound) => Some(sound),
            Self::SoundDaytime(daytime) => Some(&daytime.sound),
            _ => None,
        }
    }

    pub fn zone_music(&self) -> Option<&ZoneMusic> {
        match self {
            Self::ZoneMusic(zone) | Self::ZoneMusicDefault(zone) => Some(zone),
            _ => None,
        }
    }

    pub fn zone_fog(&self) -> Option<&ZoneFog> {
        match self {
            Self::ZoneFog(zone) | Self::ZoneFogDefault(zone) => Some(zone),
            _ => None,
        }
    }

    pub fn zone_far_plane(&self) -> Option<&ZoneFarPlane> {
        match self {
            Self::ZoneFarPlane(zone) | Self::ZoneFarPlaneDefault(zone) => Some(zone),
            _ => None,
        }
    }
}

/// How a VOB looks.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Visual {
    /// The file the visual was made from: a mesh's `.3DS`, a model's `.MDS`, a particle effect's
    /// `.PFX` or a decal's `.TGA`.
    pub name: String,
    pub kind: VisualKind,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum VisualKind {
    /// `zCMesh`
    Mesh,
    /// `zCProgMeshProto`
    MultiResolutionMesh,
    /// `zCParticleFX`
    ParticleEffect,
    /// `zCAICamera`
    Camera,
    /// `zCModel`
    Model,
    /// `zCMorphMesh`
    MorphMesh,
    /// `zCDecal`: a textured quad.
    Decal(Decal),
    /// The archive names a visual, but stores no object for it or one of a class this crate does
    /// not know.
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Decal {
    /// Width and height.
    pub dimension: Vec2,
    /// From the VOB's position.
    pub offset: Vec2,
    /// Whether the back is drawn too.
    pub two_sided: bool,
    pub alpha_func: AlphaFunction,
    pub texture_anim_fps: f32,
    /// Opacity from 0 to 255, used unless [`alpha_func`](Self::alpha_func) is `None`. Gothic 2
    /// only, like [`ignore_daylight`](Self::ignore_daylight).
    pub alpha_weight: u8,
    pub ignore_daylight: bool,
}
