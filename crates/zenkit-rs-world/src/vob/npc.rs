use tracing::warn;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::raw_enum;

use super::Ctx;
use super::FORMAT;
use super::Next;
use super::VobId;
use super::VobKind;

raw_enum! {
    /// Who an NPC passes a piece of [`News`] on to.
    pub enum NewsSpread: u8 = DontSpread {
        DontSpread = 0,
        FriendlyTowardsVictim = 1,
        FriendlyTowardsWitness = 2,
        FriendlyTowardsOffender = 3,
        SameGuildVictim = 4,
    }
}

/// An NPC in a save game, with everything the scripts and the AI know about it.
///
/// Worlds made in the Spacer have no NPCs; the scripts spawn them when a new game starts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Npc {
    /// The script instance, like `VLK_412_HARAD`.
    pub instance: String,
    pub model_scale: Vec3,
    pub model_fatness: f32,
    /// Model scripts applied over the base one, like `HUMANS_RELAXED.MDS`.
    pub overlays: Vec<String>,
    pub flags: i32,
    pub guild: i32,
    /// The guild the NPC really belongs to, when it pretends to be in another one.
    pub guild_true: i32,
    pub level: i32,
    pub xp: i32,
    pub xp_next_level: i32,
    /// Learning points.
    pub lp: i32,
    pub talents: Vec<Talent>,
    pub fight_tactic: i32,
    pub fight_mode: i32,
    pub wounded: bool,
    pub mad: bool,
    pub mad_time: i32,
    pub player: bool,
    pub attributes: [i32; 8],
    /// Gothic 2 only.
    pub hit_chances: [i32; 4],
    pub missions: [i32; 5],
    pub start_ai_state: String,
    /// The script's `aivar` array: 50 values in Gothic 1, 100 in Gothic 2.
    pub ai_vars: Vec<i32>,
    pub script_waypoint: String,
    pub attitude: i32,
    pub attitude_temp: i32,
    pub name_nr: i32,
    pub news: Vec<News>,
    pub carry_vob: Option<VobId>,
    pub enemy: Option<VobId>,
    pub move_lock: bool,
    /// Items the NPC carries but that were never created, as `INSTANCE:AMOUNT` pairs.
    pub packed: Vec<String>,
    pub items: Vec<InventoryItem>,
    pub slots: Vec<Slot>,
    pub current_state: AiState,
    pub next_state: AiState,
    pub last_ai_state: i32,
    pub has_routine: bool,
    pub routine_changed: bool,
    pub routine_overlay: bool,
    pub routine_overlay_count: i32,
    pub walk_mode_routine: i32,
    pub weapon_mode_routine: bool,
    pub start_new_routine: bool,
    pub ai_state_driven: i32,
    pub ai_state_position: Vec3,
    pub current_routine: String,
    pub respawn: bool,
    pub respawn_time: i32,
    pub protection: [i32; 8],
    /// Gothic 2 only, like [`npc_type`](Self::npc_type) and [`spell_mana`](Self::spell_mana).
    pub bs_interruptable_override: i32,
    pub npc_type: i32,
    pub spell_mana: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Talent {
    pub talent: i32,
    pub value: i32,
    pub skill: i32,
}

/// Something an NPC saw happen and may tell others about.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct News {
    pub told: bool,
    pub spread_time: f32,
    pub spread_type: NewsSpread,
    /// What happened: 200 for a murder, 195 an attack, 190 a theft, 185 a defeat, 180 a show of
    /// nerve, 175 interference, 170 having defeated someone.
    pub news_id: i32,
    pub gossip: bool,
    pub guild_victim: bool,
    pub witness_name: String,
    pub offender_name: String,
    pub victim_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryItem {
    /// An [`Item`](super::Item).
    pub item: VobId,
    /// The quick slot the item sits in, for items flagged as having one.
    pub short_key: Option<i32>,
}

/// A place on the body an item can be put, like a hand or the belt.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Slot {
    pub name: String,
    /// What the slot holds, if anything.
    pub item: Option<VobId>,
    pub in_inventory: bool,
}

/// A state of the NPC's script AI, like `ZS_Smith_Fire`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiState {
    pub valid: bool,
    pub name: String,
    /// The script function's index.
    pub index: i32,
    pub is_routine: bool,
}

impl AiState {
    fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self { valid: r.bool()?, name: r.name()?, index: r.int()?, is_routine: r.bool()? })
    }
}

/// The item flag saying an inventory item has a short key stored after it.
const ITEM_HAS_SHORT_KEY: i32 = 0x200;

impl Npc {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let gothic2 = r.game == GameVersion::Gothic2;
        let mut npc = Self {
            instance: r.name()?,
            model_scale: r.vec3()?,
            model_fatness: r.float()?,
            ..Self::default()
        };

        let count = r.int()?;
        for _ in 0..count {
            npc.overlays.push(r.name()?);
        }

        npc.flags = r.int()?;
        npc.guild = r.int()?;
        npc.guild_true = r.int()?;
        npc.level = r.int()?;
        npc.xp = r.int()?;
        npc.xp_next_level = r.int()?;
        npc.lp = r.int()?;

        let count = r.int()?;
        for _ in 0..count {
            // Talents the NPC does not have are stored as null objects.
            if let Next::New(object) = r.begin("a talent")? {
                npc.talents.push(Talent { talent: r.int()?, value: r.int()?, skill: r.int()? });
                r.end(&object)?;
            }
        }

        npc.fight_tactic = r.int()?;
        npc.fight_mode = r.int()?;
        npc.wounded = r.bool()?;
        npc.mad = r.bool()?;
        npc.mad_time = r.int()?;
        npc.player = r.bool()?;
        for attribute in &mut npc.attributes {
            *attribute = r.int()?;
        }
        if gothic2 {
            for chance in &mut npc.hit_chances {
                *chance = r.int()?;
            }
        }
        for mission in &mut npc.missions {
            *mission = r.int()?;
        }

        npc.start_ai_state = r.name()?;
        let count = if gothic2 { 100 } else { 50 };
        npc.ai_vars = ints(&r.raw(count * 4)?, count)?;
        npc.script_waypoint = r.name()?;
        npc.attitude = r.int()?;
        npc.attitude_temp = r.int()?;
        npc.name_nr = r.int()?;
        // The spells, which nobody knows how to read.
        r.raw(4)?;

        let count = r.int()?;
        for _ in 0..count {
            npc.news.push(News {
                told: r.bool()?,
                spread_time: r.float()?,
                spread_type: NewsSpread::from_raw(r.int()? as u32),
                news_id: r.int()?,
                gossip: r.bool()?,
                guild_victim: r.bool()?,
                witness_name: r.name()?,
                offender_name: r.name()?,
                victim_name: r.name()?,
            });
        }

        npc.carry_vob = r.vob()?;
        npc.enemy = r.vob()?;
        npc.move_lock = r.bool()?;

        if gothic2 {
            // ZenKit splits this wrongly: it keeps only the part before the first `;`.
            let packed = r.name()?;
            npc.packed = packed.split(';').take(9).map(String::from).collect();
        } else {
            for _ in 0..9 {
                npc.packed.push(r.name()?);
            }
        }

        let count = r.int()?;
        for i in 0..count {
            let Some(item) = r.vob()? else {
                // Whether a short key follows depends on the item's flags, so the rest cannot
                // be read without it.
                let reason = format!("inventory item {i} is missing");
                return Err(Error::malformed(FORMAT, reason));
            };
            let flags = match &r.vobs[item.index()].kind {
                VobKind::Item(item) => item.state.map_or(0, |s| s.flags),
                other => {
                    warn!("inventory item {i} is not an item but {other:?}");
                    0
                }
            };
            let short_key = if flags & ITEM_HAS_SHORT_KEY != 0 { Some(r.int()?) } else { None };
            npc.items.push(InventoryItem { item, short_key });
        }

        let count = r.int()?;
        for _ in 0..count {
            let used = r.bool()?;
            let name = r.name()?;
            let mut slot = Slot { name, item: None, in_inventory: false };
            if used {
                slot.item = r.vob()?;
                slot.in_inventory = r.bool()?;
            }
            npc.slots.push(slot);
        }

        npc.current_state = AiState::read(r)?;
        npc.next_state = AiState::read(r)?;
        npc.last_ai_state = r.int()?;
        npc.has_routine = r.bool()?;
        npc.routine_changed = r.bool()?;
        npc.routine_overlay = r.bool()?;
        npc.routine_overlay_count = r.int()?;
        npc.walk_mode_routine = r.int()?;
        npc.weapon_mode_routine = r.bool()?;
        npc.start_new_routine = r.bool()?;
        npc.ai_state_driven = r.int()?;
        npc.ai_state_position = r.vec3()?;
        npc.current_routine = r.name()?;
        npc.respawn = r.bool()?;
        npc.respawn_time = r.int()?;
        let protection = ints(&r.raw(32)?, 8)?;
        npc.protection.copy_from_slice(&protection);

        if gothic2 {
            npc.bs_interruptable_override = r.int()?;
            npc.npc_type = r.int()?;
            npc.spell_mana = r.int()?;
        }
        Ok(npc)
    }
}

fn ints(raw: &[u8], count: usize) -> Result<Vec<i32>> {
    let mut r = Reader::new(raw);
    (0..count).map(|_| r.i32()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_raw_values_fail() {
        assert_eq!(ints(&[1, 0, 0, 0, 2, 0, 0, 0], 2).unwrap(), [1, 2]);
        assert!(ints(&[1, 0, 0], 1).is_err());
    }
}
