//! Model scripts (`.MDS` source, `.MSB` compiled): which meshes and skeleton a model uses, and
//! every animation it can play, with the sounds, particle effects and gameplay events tied to
//! their frames.
//!
//! The game ships compiled `.MSB` files; mods often ship the `.MDS` source instead. Both are read
//! into the same [`ModelScript`].

mod binary;
mod source;

use bitflags::bitflags;

use tracing::warn;

use crate::Asset;
use crate::Result;

const FORMAT: &str = "model script";

/// The skeleton a model is built on (`meshAndTree`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Skeleton {
    /// The source file the skeleton was compiled from, like `HUM_BODY_NAKED0.ASC`.
    pub name: String,
    /// The skeleton file's mesh is not used; one of [`ModelScript::meshes`] is.
    pub disable_mesh: bool,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct AnimationFlags: u8 {
        /// The animation moves the model through the world.
        const MOVE = 1 << 0;
        /// The animation turns the model.
        const ROTATE = 1 << 1;
        /// Queued after the animation currently playing on its layer instead of replacing it.
        const QUEUE = 1 << 2;
        const FLY = 1 << 3;
        const IDLE = 1 << 4;
        const IN_PLACE = 1 << 5;
    }
}

impl AnimationFlags {
    /// Parses the letters of a flags field like `MI` or `M.`. Any other character, usually `.`
    /// for "not set", is ignored.
    fn parse(text: &str) -> Self {
        text.bytes()
            .map(|c| match c.to_ascii_uppercase() {
                b'M' => Self::MOVE,
                b'R' => Self::ROTATE,
                b'E' => Self::QUEUE,
                b'F' => Self::FLY,
                b'I' => Self::IDLE,
                b'P' => Self::IN_PLACE,
                _ => Self::empty(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    #[default]
    Forward,
    Backward,
}

impl Direction {
    fn parse(text: &str) -> Self {
        if text.starts_with(['R', 'r']) { Self::Backward } else { Self::Forward }
    }
}

/// What an [`EventTag`] tells the game to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EventType {
    /// A type this crate does not know. The event is kept so that its frame is not lost.
    #[default]
    Unknown,
    CreateItem,
    InsertItem,
    RemoveItem,
    DestroyItem,
    PlaceItem,
    ExchangeItem,
    SetFightMode,
    PlaceMunition,
    RemoveMunition,
    DrawSound,
    UndrawSound,
    SwapMesh,
    DrawTorch,
    InventoryTorch,
    DropTorch,
    HitLimb,
    HitDirection,
    DamageMultiplier,
    ParryFrame,
    OptimalFrame,
    HitEnd,
    ComboWindow,
}

impl EventType {
    const NAMES: [(&str, Self); 24] = [
        ("DEF_CREATE_ITEM", Self::CreateItem),
        ("DEF_INSERT_ITEM", Self::InsertItem),
        ("DEF_REMOVE_ITEM", Self::RemoveItem),
        ("DEF_DESTROY_ITEM", Self::DestroyItem),
        ("DEF_PLACE_ITEM", Self::PlaceItem),
        ("DEF_EXCHANGE_ITEM", Self::ExchangeItem),
        ("DEF_FIGHTMODE", Self::SetFightMode),
        ("DEF_PLACE_MUNITION", Self::PlaceMunition),
        ("DEF_REMOVE_MUNITION", Self::RemoveMunition),
        ("DEF_DRAWSOUND", Self::DrawSound),
        ("DEF_UNDRAWSOUND", Self::UndrawSound),
        ("DEF_SWAPMESH", Self::SwapMesh),
        ("DEF_DRAWTORCH", Self::DrawTorch),
        ("DEF_INV_TORCH", Self::InventoryTorch),
        ("DEF_DROP_TORCH", Self::DropTorch),
        ("DEF_HIT_LIMB", Self::HitLimb),
        ("HIT_LIMB", Self::HitLimb),
        ("DEF_HIT_DIR", Self::HitDirection),
        ("DEF_DIR", Self::HitDirection),
        ("DEF_DAM_MULTIPLY", Self::DamageMultiplier),
        ("DEF_PAR_FRAME", Self::ParryFrame),
        ("DEF_OPT_FRAME", Self::OptimalFrame),
        ("DEF_HIT_END", Self::HitEnd),
        ("DEF_WINDOW", Self::ComboWindow),
    ];

    fn parse(name: &str) -> Self {
        Self::NAMES
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|&(_, kind)| kind)
            .unwrap_or_else(|| {
                warn!("unknown event type {name:?}");
                Self::Unknown
            })
    }
}

/// The weapon stance a [`EventType::SetFightMode`] event switches to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FightMode {
    Fist,
    OneHanded,
    TwoHanded,
    Bow,
    Crossbow,
    Magic,
    #[default]
    None,
}

impl FightMode {
    fn parse(name: &str) -> Self {
        match name.to_ascii_uppercase().as_str() {
            "FIST" => Self::Fist,
            "1H" => Self::OneHanded,
            "2H" => Self::TwoHanded,
            "BOW" => Self::Bow,
            "CBOW" => Self::Crossbow,
            "MAG" => Self::Magic,
            _ => Self::None,
        }
    }
}

/// A gameplay event on one frame of an animation (`*eventTag`).
///
/// Which fields are set depends on [`kind`](Self::kind): item events name a `slot` and an `item`,
/// [`SwapMesh`](EventType::SwapMesh) swaps `slot` with `slot2`, combat events list `frames`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventTag {
    pub frame: i32,
    pub kind: EventType,
    pub slot: String,
    pub slot2: String,
    pub item: String,
    pub frames: Vec<i32>,
    pub fight_mode: FightMode,
    pub attached: bool,
}

impl EventTag {
    /// Sorts an event's free-form arguments into fields. Source and compiled scripts store the
    /// same two leading arguments, so both readers build events through here.
    fn new(frame: i32, kind: &str, a: Option<String>, b: Option<String>, attached: bool) -> Self {
        let kind = EventType::parse(kind);
        let mut event = Self { frame, kind, attached, ..Self::default() };
        let a = a.unwrap_or_default();

        use EventType::*;
        match kind {
            CreateItem | ExchangeItem => {
                event.slot = a;
                event.item = b.unwrap_or_default();
            }
            SwapMesh => {
                event.slot = a;
                event.slot2 = b.unwrap_or_default();
            }
            InsertItem | RemoveItem | DestroyItem | PlaceMunition | RemoveMunition | DrawSound
            | UndrawSound | InventoryTorch | HitLimb | HitDirection => event.slot = a,
            SetFightMode => event.fight_mode = FightMode::parse(&a),
            DamageMultiplier | ParryFrame | OptimalFrame | HitEnd | ComboWindow => {
                event.frames = a.split_ascii_whitespace().filter_map(|f| f.parse().ok()).collect();
            }
            Unknown | PlaceItem | DrawTorch | DropTorch => {}
        }
        event
    }
}

/// A particle effect started on one frame (`*eventPFX`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParticleEffect {
    pub frame: i32,
    /// Lets a later [`ParticleEffectStop`] refer to this effect.
    pub index: i32,
    pub name: String,
    /// The node the effect is placed at.
    pub position: String,
    /// The effect follows the node instead of staying where it started.
    pub attached: bool,
}

/// Stops the [`ParticleEffect`] with the same `index` (`*eventPFXStop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParticleEffectStop {
    pub frame: i32,
    pub index: i32,
}

/// A sound played on one frame (`*eventSFX`, or `*eventSFXGrnd` for sounds that depend on the
/// ground material, like footsteps).
#[derive(Debug, Clone, PartialEq)]
pub struct SoundEffect {
    pub frame: i32,
    pub name: String,
    pub range: f32,
    pub empty_slot: bool,
}

impl Default for SoundEffect {
    fn default() -> Self {
        Self { frame: 0, name: String::new(), range: 1000.0, empty_slot: false }
    }
}

/// Starts a morph mesh animation, usually on the head (`*eventMMStartAni`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MorphAnimation {
    pub frame: i32,
    pub animation: String,
    /// The node holding the morph mesh. Empty for the default one.
    pub node: String,
}

/// Shakes the camera (`*eventCamTremor`). The meaning of the four values is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CameraTremor {
    pub frame: i32,
    pub values: [i32; 4],
}

/// An animation played from a `.MAN` file (`ani`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Animation {
    pub name: String,
    pub layer: u32,
    /// The animation to play once this one ends.
    pub next: String,
    pub blend_in: f32,
    pub blend_out: f32,
    pub flags: AnimationFlags,
    /// The source file of the animation's frames.
    pub model: String,
    pub direction: Direction,
    pub first_frame: i32,
    pub last_frame: i32,
    pub fps: f32,
    /// Only stored in compiled scripts; zero in source scripts.
    pub speed: f32,
    pub collision_volume_scale: f32,
    pub events: Vec<EventTag>,
    pub particles: Vec<ParticleEffect>,
    pub particle_stops: Vec<ParticleEffectStop>,
    pub sounds: Vec<SoundEffect>,
    pub ground_sounds: Vec<SoundEffect>,
    pub morphs: Vec<MorphAnimation>,
    pub tremors: Vec<CameraTremor>,
}

/// Another name for an animation, optionally played backwards (`aniAlias`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimationAlias {
    pub name: String,
    pub layer: u32,
    pub next: String,
    pub blend_in: f32,
    pub blend_out: f32,
    pub flags: AnimationFlags,
    /// The animation this one plays.
    pub alias: String,
    pub direction: Direction,
}

/// A transition that only blends from one animation to the next (`aniBlend`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimationBlend {
    pub name: String,
    pub next: String,
    pub blend_in: f32,
    pub blend_out: f32,
}

/// An animation mixed from several others by direction, like looking around (`aniComb`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimationCombination {
    pub name: String,
    pub layer: u32,
    pub next: String,
    pub blend_in: f32,
    pub blend_out: f32,
    pub flags: AnimationFlags,
    /// The common prefix of the animations being mixed.
    pub model: String,
    pub last_frame: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelScript {
    /// `None` if the script does not name one.
    pub skeleton: Option<Skeleton>,
    /// The meshes that can be put on the skeleton (`registerMesh`).
    pub meshes: Vec<String>,
    /// Names of animations turned off (`aniDisable`).
    pub disabled_animations: Vec<String>,
    pub animations: Vec<Animation>,
    pub aliases: Vec<AnimationAlias>,
    pub blends: Vec<AnimationBlend>,
    pub combinations: Vec<AnimationCombination>,
    /// Bones that can be hit in combat (`modelTag("DEF_HIT_LIMB" ...)`).
    pub hit_limbs: Vec<String>,
}

impl ModelScript {
    /// Reads either form. Compiled scripts are chunked, so their first two bytes are a chunk ID.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        match data {
            [lo, hi, ..] if binary::is_chunk_id(u16::from_le_bytes([*lo, *hi])) => {
                binary::read(data)
            }
            _ => source::parse(data),
        }
    }

    /// Finds an animation by name, ignoring ASCII case like the engine does.
    pub fn animation(&self, name: &str) -> Option<&Animation> {
        self.animations.iter().find(|a| a.name.eq_ignore_ascii_case(name))
    }
}

impl Asset for ModelScript {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        ModelScript::from_bytes(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_ignore_placeholders() {
        assert_eq!(AnimationFlags::parse("MI"), AnimationFlags::MOVE | AnimationFlags::IDLE);
        assert_eq!(AnimationFlags::parse("M."), AnimationFlags::MOVE);
        assert_eq!(AnimationFlags::parse(".F"), AnimationFlags::FLY);
        assert_eq!(AnimationFlags::parse("."), AnimationFlags::empty());
    }

    #[test]
    fn events_sort_their_arguments() {
        let s = |v: &str| Some(v.to_owned());

        let e = EventTag::new(3, "DEF_CREATE_ITEM", s("ZS_LEFTHAND"), s("ITFO_APPLE"), false);
        assert_eq!((e.kind, e.slot.as_str()), (EventType::CreateItem, "ZS_LEFTHAND"));
        assert_eq!(e.item, "ITFO_APPLE");

        let e = EventTag::new(0, "DEF_WINDOW", s("8\t15"), None, false);
        assert_eq!(e.frames, [8, 15]);

        let e = EventTag::new(0, "DEF_FIGHTMODE", s("2h"), None, false);
        assert_eq!(e.fight_mode, FightMode::TwoHanded);

        let e = EventTag::new(7, "DEF_SOMETHING_NEW", s("x"), None, true);
        assert_eq!((e.kind, e.frame, e.attached), (EventType::Unknown, 7, true));
    }
}
