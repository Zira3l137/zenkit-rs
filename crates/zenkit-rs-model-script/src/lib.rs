//! Model scripts (`.MDS` source, `.MSB` compiled): which meshes and skeleton a model uses, and
//! every animation it can play, with the sounds, particle effects and gameplay events tied to
//! their frames.
//!
//! The game ships compiled `.MSB` files; mods often ship the `.MDS` source instead. Both are read
//! into the same [`ModelScript`], which keeps the script's statements, and each animation's
//! events, in the order they are written.

mod binary;
mod source;

use bitflags::bitflags;
use zenkit_rs_core::Asset;
use zenkit_rs_core::Date;
use zenkit_rs_core::Result;

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
    const LETTERS: [(u8, Self); 6] = [
        (b'M', Self::MOVE),
        (b'R', Self::ROTATE),
        (b'E', Self::QUEUE),
        (b'F', Self::FLY),
        (b'I', Self::IDLE),
        (b'P', Self::IN_PLACE),
    ];

    /// The flags in the spelling most of the game's scripts use: a lone flag followed by a dot,
    /// like `M.`, no flags at all a single dot, and several flags their letters, like `MI`.
    fn to_text(self) -> String {
        let mut text: String = (Self::LETTERS.iter())
            .filter(|(_, flag)| self.contains(*flag))
            .map(|&(letter, _)| char::from(letter))
            .collect();
        if text.len() < 2 {
            text.push('.');
        }
        text
    }

    /// `text` if it is a spelling of these flags that scripts can hold, like `M`, `..` or `M:`,
    /// all of which the game's scripts use; otherwise [`to_text`](Self::to_text). Compiled
    /// scripts can also hold no text at all for no flags, which a source script cannot.
    fn spelled(self, text: &str, compiled: bool) -> String {
        if compiled && text.is_empty() && self.is_empty() {
            return String::new();
        }
        let letters = text.strip_suffix(':').unwrap_or(text);
        let valid = !letters.is_empty()
            && letters.bytes().all(|c| {
                c == b'.' || Self::LETTERS.iter().any(|(l, _)| *l == c.to_ascii_uppercase())
            })
            && Self::parse(text) == self;
        if valid { text.to_owned() } else { self.to_text() }
    }

    /// Parses the letters of a flags field like `MI` or `M.`. Any other character, usually `.`
    /// for "not set", is ignored.
    fn parse(text: &str) -> Self {
        (text.bytes())
            .filter_map(|c| Self::LETTERS.iter().find(|(l, _)| *l == c.to_ascii_uppercase()))
            .map(|&(_, flag)| flag)
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

    fn to_text(self) -> &'static str {
        match self {
            Self::Forward => "F",
            Self::Backward => "R",
        }
    }

    /// `text` if it is a spelling of this direction: a letter in either case, or nothing for
    /// forward, as an alias that leaves its direction out has; otherwise
    /// [`to_text`](Self::to_text).
    fn spelled(self, text: &str) -> &str {
        let valid = matches!(text, "" | "F" | "f" | "R" | "r") && Self::parse(text) == self;
        if valid { text } else { self.to_text() }
    }
}

/// What an [`EventTag`] tells the game to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EventType {
    /// A type this crate does not know. The event keeps its name and arguments regardless.
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

    /// The type an event name stands for, ignoring ASCII case like the engine does.
    pub fn from_name(name: &str) -> Self {
        (Self::NAMES.iter())
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map_or(Self::Unknown, |&(_, kind)| kind)
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
    /// No weapon, an empty argument.
    #[default]
    None,
}

impl FightMode {
    fn parse(name: &str) -> Self {
        match name.trim().to_ascii_uppercase().as_str() {
            "FIST" => Self::Fist,
            "1H" | "1HS" => Self::OneHanded,
            "2H" | "2HS" => Self::TwoHanded,
            "BOW" => Self::Bow,
            "CBOW" => Self::Crossbow,
            "MAG" => Self::Magic,
            _ => Self::None,
        }
    }
}

/// A gameplay event on one frame of an animation (`*eventTag`), or for every animation of the
/// model (`modelTag`).
///
/// Its name and arguments are kept as written, so that it is written back the same; the methods
/// read them by [`kind`](Self::kind). Item events name a slot and then an item,
/// [`SwapMesh`](EventType::SwapMesh) two slots, [`HitLimb`](EventType::HitLimb) up to four
/// slots, and combat events a list of frames.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventTag {
    pub frame: i32,
    /// Like `DEF_CREATE_ITEM`.
    pub name: String,
    /// Compiled scripts store four, the unused ones empty; empty ones at the end are left out.
    pub arguments: Vec<String>,
    /// `ATTACH`, which only source scripts store.
    pub attached: bool,
}

impl EventTag {
    pub fn kind(&self) -> EventType {
        EventType::from_name(&self.name)
    }

    /// An argument, or the empty string if there are fewer.
    pub fn argument(&self, index: usize) -> &str {
        self.arguments.get(index).map_or("", String::as_str)
    }

    /// The frames combat events list in their first argument, like `"8 15"`.
    pub fn frames(&self) -> Vec<i32> {
        self.argument(0).split_ascii_whitespace().filter_map(|f| f.parse().ok()).collect()
    }

    /// The stance a [`SetFightMode`](EventType::SetFightMode) event switches to.
    pub fn fight_mode(&self) -> FightMode {
        FightMode::parse(self.argument(0))
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
    /// How far away it is heard, in centimetres.
    pub range: f32,
    /// Played on a free sound slot of the model rather than cutting off the one playing.
    pub empty_slot: bool,
}

impl Default for SoundEffect {
    fn default() -> Self {
        Self { frame: 0, name: String::new(), range: 1000.0, empty_slot: false }
    }
}

/// Starts a morph mesh animation, usually on the head (`*eventMMStartAni`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MorphAnimation {
    pub frame: i32,
    pub animation: String,
    /// The node holding the morph mesh. Empty for the default one, `BIP01 HEAD`.
    pub node: String,
    /// `I:`, said to blend the morph animation with the one playing.
    pub intensity: f32,
    /// `H:`, how long the animation is held, in seconds.
    pub hold_time: f32,
}

/// Shakes the camera (`*eventCamTremor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CameraTremor {
    pub frame: i32,
    /// How far away it is felt, in centimetres.
    pub range: i32,
    /// In milliseconds.
    pub duration: i32,
    pub min_amplifier: i32,
    pub max_amplifier: i32,
}

/// A chunk of a compiled script this crate does not know, kept so that it is written back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawChunk {
    pub id: u16,
    pub data: Vec<u8>,
}

/// Something an animation does on one of its frames.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Tag(EventTag),
    Sound(SoundEffect),
    GroundSound(SoundEffect),
    Particle(ParticleEffect),
    ParticleStop(ParticleEffectStop),
    Morph(MorphAnimation),
    Tremor(CameraTremor),
    /// An event of a compiled script this crate does not know, like `*eventSwapMesh`.
    Unknown(RawChunk),
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
    /// The flags as written, which scripts spell in several ways, like `M`, `M.` or `M:`. It is
    /// written back while it still spells [`flags`](Self::flags); otherwise `flags` are written
    /// in the spelling most scripts use.
    pub flags_text: String,
    /// The source file of the animation's frames.
    pub model: String,
    pub direction: Direction,
    pub first_frame: i32,
    pub last_frame: i32,
    pub fps: f32,
    /// `SPD:`, zero when not given.
    pub speed: f32,
    pub collision_volume_scale: f32,
    /// In the order they are written.
    pub events: Vec<Event>,
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
    /// The flags as written, which scripts spell in several ways, like `M`, `M.` or `M:`. It is
    /// written back while it still spells [`flags`](Self::flags); otherwise `flags` are written
    /// in the spelling most scripts use.
    pub flags_text: String,
    /// The animation this one plays.
    pub alias: String,
    pub direction: Direction,
    /// The direction as written, `F`, `R`, or empty, as many of the game's aliases leave it out.
    /// It is written back while it still spells [`direction`](Self::direction).
    pub direction_text: String,
    pub events: Vec<Event>,
}

/// A transition that only blends from one animation to the next (`aniBlend`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimationBlend {
    pub name: String,
    pub next: String,
    pub blend_in: f32,
    pub blend_out: f32,
    pub events: Vec<Event>,
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
    /// The flags as written, which scripts spell in several ways, like `M`, `M.` or `M:`. It is
    /// written back while it still spells [`flags`](Self::flags); otherwise `flags` are written
    /// in the spelling most scripts use.
    pub flags_text: String,
    /// The common prefix of the animations being mixed.
    pub model: String,
    pub last_frame: i32,
    pub events: Vec<Event>,
}

/// One statement of a script's `aniEnum` block.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Animation(Animation),
    Alias(AnimationAlias),
    Blend(AnimationBlend),
    Combination(AnimationCombination),
    /// Turns off the animation of this name (`aniDisable`).
    Disable(String),
    /// An event for every animation of the model, in practice the bones that can hit in combat:
    /// `modelTag("DEF_HIT_LIMB" "ZS_RIGHTHAND")`.
    ModelTag(EventTag),
    /// A statement of a compiled script this crate does not know, like `aniMaxFPS`.
    Unknown(RawChunk),
}

impl Statement {
    /// The events of an animation, alias, blend or combination; empty for other statements.
    pub fn events(&self) -> &[Event] {
        match self {
            Self::Animation(a) => &a.events,
            Self::Alias(a) => &a.events,
            Self::Blend(b) => &b.events,
            Self::Combination(c) => &c.events,
            Self::Disable(_) | Self::ModelTag(_) | Self::Unknown(_) => &[],
        }
    }

    pub(crate) fn events_mut(&mut self) -> Option<&mut Vec<Event>> {
        match self {
            Self::Animation(a) => Some(&mut a.events),
            Self::Alias(a) => Some(&mut a.events),
            Self::Blend(b) => Some(&mut b.events),
            Self::Combination(c) => Some(&mut c.events),
            Self::Disable(_) | Self::ModelTag(_) | Self::Unknown(_) => None,
        }
    }
}

/// The source file a compiled script was made from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScriptSource {
    /// Like `\_WORK\DATA\ANIMS\HUMANS.MDS`.
    pub path: String,
    pub date: Date,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelScript {
    /// The name in `Model("HUMANS")`. Compiled scripts do not store it, so theirs is the name of
    /// their source file.
    pub name: String,
    /// `None` if the script does not name one.
    pub skeleton: Option<Skeleton>,
    /// The meshes that can be put on the skeleton (`registerMesh`).
    pub meshes: Vec<String>,
    /// The `aniEnum` block, in the order it is written, which the engine reads it in.
    pub statements: Vec<Statement>,
    /// Compiled scripts only.
    pub source: Option<ScriptSource>,
}

macro_rules! statements {
    ($(#[$doc:meta])* $name:ident, $variant:ident, $ty:ty) => {
        $(#[$doc])*
        pub fn $name(&self) -> impl Iterator<Item = &$ty> + '_ {
            self.statements.iter().filter_map(|s| match s {
                Statement::$variant(x) => Some(x),
                _ => None,
            })
        }
    };
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

    statements!(animations, Animation, Animation);
    statements!(aliases, Alias, AnimationAlias);
    statements!(blends, Blend, AnimationBlend);
    statements!(combinations, Combination, AnimationCombination);
    statements!(model_tags, ModelTag, EventTag);

    /// The names of the animations turned off (`aniDisable`).
    pub fn disabled_animations(&self) -> impl Iterator<Item = &str> + '_ {
        self.statements.iter().filter_map(|s| match s {
            Statement::Disable(name) => Some(name.as_str()),
            _ => None,
        })
    }

    /// Finds an animation by name, ignoring ASCII case like the engine does.
    pub fn animation(&self, name: &str) -> Option<&Animation> {
        self.animations().find(|a| a.name.eq_ignore_ascii_case(name))
    }

    /// Writes the script as source, a `.MDS` file, laid out like the game's: one statement per
    /// line, and an animation's events in braces below it, with Windows line endings.
    ///
    /// Values left at their defaults are left out, like `FPS:25`. Fails if the script holds
    /// chunks of a compiled script this crate does not know, which have no source form, if a text
    /// has a quote, a line break or characters beyond Latin-1, or if a number is not finite.
    pub fn to_mds(&self) -> Result<Vec<u8>> {
        source::write(self)
    }

    /// Writes the script compiled, as a `.MSB` file.
    ///
    /// A script read from source has no [`source`](Self::source), and is written with an empty
    /// source path and date. `ATTACH` on tag events is not stored. Fails if a tag has more than
    /// four arguments, or a text has a line break or characters beyond Latin-1.
    pub fn to_msb(&self) -> Result<Vec<u8>> {
        binary::write(self)
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
    fn flags_are_written_like_the_games() {
        use AnimationFlags as F;
        for (flags, text) in [
            (F::empty(), "."),
            (F::MOVE, "M."),
            (F::MOVE | F::FLY, "MF"),
            (F::IDLE | F::MOVE, "MI"),
            (F::all(), "MREFIP"),
        ] {
            assert_eq!(flags.to_text(), text);
            assert_eq!(F::parse(text), flags);
        }
    }

    #[test]
    fn flags_keep_their_spelling_while_it_fits() {
        use AnimationFlags as F;
        for text in ["M", "M.", "M:", "..", "mi", "IM"] {
            assert_eq!(F::parse(text).spelled(text, false), text);
        }
        assert_eq!(F::MOVE.spelled("", true), "M.");
        assert_eq!(F::MOVE.spelled("MI", false), "M.", "a spelling of other flags");
        assert_eq!(F::MOVE.spelled("M X", false), "M.", "not one keyword");
        assert_eq!(F::empty().spelled(":", false), ".");
        assert_eq!(F::empty().spelled("", true), "", "compiled scripts can hold no flags");
        assert_eq!(F::empty().spelled("", false), ".");

        use Direction::*;
        assert_eq!(
            (Forward.spelled(""), Forward.spelled("f"), Backward.spelled("r")),
            ("", "f", "r")
        );
        assert_eq!(
            (Backward.spelled(""), Forward.spelled("R"), Forward.spelled("FW")),
            ("R", "F", "F")
        );
    }

    #[test]
    fn events_read_their_arguments_by_kind() {
        let tag = |name: &str, args: &[&str]| EventTag {
            name: name.into(),
            arguments: args.iter().map(|a| a.to_string()).collect(),
            ..EventTag::default()
        };
        let e = tag("def_create_item", &["ZS_LEFTHAND", "ITFO_APPLE"]);
        assert_eq!(
            (e.kind(), e.argument(0), e.argument(1)),
            (EventType::CreateItem, "ZS_LEFTHAND", "ITFO_APPLE")
        );
        assert_eq!(e.argument(2), "");
        assert_eq!(tag("DEF_WINDOW", &["8\t15"]).frames(), [8, 15]);
        assert_eq!(tag("DEF_FIGHTMODE", &["2hs"]).fight_mode(), FightMode::TwoHanded);
        assert_eq!(tag("DEF_FIGHTMODE", &[""]).fight_mode(), FightMode::None);
        assert_eq!(tag("DEF_SOMETHING_NEW", &["x"]).kind(), EventType::Unknown);
    }
}
