use zenkit_rs_core::Result;
use zenkit_rs_core::raw_enum;

use super::Ctx;
use super::VobId;

raw_enum! {
    /// What a mob sounds like when hit or moved.
    pub enum SoundMaterial: u8 = Wood {
        Wood = 0,
        Stone = 1,
        Metal = 2,
        Leather = 3,
        Clay = 4,
        Glass = 5,
    }
}

/// A mob, short for movable object: a VOB the player can focus, like a grave or a stump.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mob {
    /// The script constant whose text is shown when the player focuses the mob.
    pub focus_name: String,
    pub hit_points: i32,
    pub damage: i32,
    pub movable: bool,
    pub takable: bool,
    pub focus_override: bool,
    pub material: SoundMaterial,
    /// The visual shown once the mob is destroyed.
    pub visual_destroyed: String,
    /// The script instance of the NPC that owns the mob.
    pub owner: String,
    pub owner_guild: String,
    pub destroyed: bool,
}

impl Mob {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self {
            focus_name: r.name()?,
            hit_points: r.int()?,
            damage: r.int()?,
            movable: r.bool()?,
            takable: r.bool()?,
            focus_override: r.bool()?,
            material: SoundMaterial::from_raw(r.enumeration()?),
            visual_destroyed: r.name()?,
            owner: r.name()?,
            owner_guild: r.name()?,
            destroyed: r.bool()?,
        })
    }
}

/// A mob the player can use, like a bench, a lever or an anvil.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Interactive {
    pub mob: Mob,
    pub state_count: i32,
    /// What to trigger when used.
    pub target: String,
    /// The item needed to use the mob.
    pub item: String,
    /// The script function that decides whether the mob can be used.
    pub condition_function: String,
    /// The script function called whenever the mob's state changes.
    pub on_state_change_function: String,
    pub rewind: bool,
}

impl Interactive {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self {
            mob: Mob::read(r)?,
            state_count: r.int()?,
            target: r.name()?,
            item: r.name()?,
            condition_function: r.name()?,
            on_state_change_function: r.name()?,
            rewind: r.bool()?,
        })
    }
}

/// A campfire, a torch holder or anything else that burns when used.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fire {
    pub interactive: Interactive,
    /// The model node the fire burns at.
    pub slot: String,
    /// The world file holding the fire's effects, like `FIRETREE_MEDIUM.ZEN`.
    pub vob_tree: String,
}

impl Fire {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self { interactive: Interactive::read(r)?, slot: r.name()?, vob_tree: r.name()? })
    }
}

/// A chest or anything else that holds items.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Container {
    pub interactive: Interactive,
    pub locked: bool,
    /// The item that unlocks it.
    pub key: String,
    /// The lock picking combination, as `L` and `R` for left and right.
    pub pick_string: String,
    /// The items inside, as comma-separated `INSTANCE:AMOUNT` pairs, like `ItMi_Gold:35`.
    pub contents: String,
    /// In save games, the items inside, which are [`Item`](super::Item)s. Their
    /// [`contents`](Self::contents) are only created the first time the container is opened.
    pub items: Vec<VobId>,
}

impl Container {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let mut container = Self {
            interactive: Interactive::read(r)?,
            locked: r.bool()?,
            key: r.name()?,
            pick_string: r.name()?,
            contents: r.name()?,
            items: Vec::new(),
        };
        if r.save {
            let count = r.int()?;
            for _ in 0..count {
                container.items.extend(r.vob()?);
            }
        }
        Ok(container)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Door {
    pub interactive: Interactive,
    pub locked: bool,
    /// The item that unlocks it.
    pub key: String,
    /// The lock picking combination, as `L` and `R` for left and right.
    pub pick_string: String,
}

impl Door {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self {
            interactive: Interactive::read(r)?,
            locked: r.bool()?,
            key: r.name()?,
            pick_string: r.name()?,
        })
    }
}
