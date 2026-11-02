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
use super::WriteCtx;

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
    /// One entry per talent number; talents the NPC does not have are `None`.
    pub talents: Vec<Option<Talent>>,
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
    /// The engine's own structure behind the NPC's spells, kept as stored, since what it holds is
    /// not known: four bytes.
    pub spells: Vec<u8>,
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
            let talent = match r.begin("a talent")? {
                Next::New(object) => {
                    let talent = Talent { talent: r.int()?, value: r.int()?, skill: r.int()? };
                    r.end(&object)?;
                    Some(talent)
                }
                _ => None,
            };
            npc.talents.push(talent);
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
        npc.spells = r.raw(4)?[..4].to_vec();

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

impl Npc {
    pub(crate) fn write(&self, w: &mut WriteCtx) {
        let gothic2 = w.gothic2();
        w.name("npcInstance", &self.instance);
        w.vec3("modelScale", self.model_scale);
        w.float("modelFatness", self.model_fatness);
        w.int("numOverlays", self.overlays.len() as i32);
        for overlay in &self.overlays {
            w.name("overlay", overlay);
        }

        w.int("flags", self.flags);
        w.int("guild", self.guild);
        w.int("guildTrue", self.guild_true);
        w.int("level", self.level);
        w.int("xp", self.xp);
        w.int("xpnl", self.xp_next_level);
        w.int("lp", self.lp);

        w.int("numTalents", self.talents.len() as i32);
        let version = if gothic2 { 64513 } else { 0 };
        for talent in &self.talents {
            match talent {
                Some(talent) => {
                    w.begin_object("%", "oCNpcTalent", version);
                    w.int("talent", talent.talent);
                    w.int("value", talent.value);
                    w.int("skill", talent.skill);
                    w.end_object();
                }
                None => w.null_object("%"),
            }
        }

        w.int("fightTactic", self.fight_tactic);
        w.int("fightMode", self.fight_mode);
        w.bool("wounded", self.wounded);
        w.bool("mad", self.mad);
        w.int("madTime", self.mad_time);
        w.bool("player", self.player);
        for (i, &attribute) in self.attributes.iter().enumerate() {
            w.int(&format!("atr{i}"), attribute);
        }
        if gothic2 {
            // Numbered from 1, unlike every other list here.
            for (i, &chance) in self.hit_chances.iter().enumerate() {
                w.int(&format!("hc{}", i + 1), chance);
            }
        }
        for (i, &mission) in self.missions.iter().enumerate() {
            w.int(&format!("mission{i}"), mission);
        }

        w.name("startAIState", &self.start_ai_state);
        let count = if gothic2 { 100 } else { 50 };
        if self.ai_vars.len() != count {
            w.fail(format!(
                "NPC {:?} has {} script variables, not {count}",
                self.instance,
                self.ai_vars.len()
            ));
        }
        w.raw("scriptVars", &raw_ints(&self.ai_vars));
        w.name("scriptWp", &self.script_waypoint);
        w.int("attitude", self.attitude);
        w.int("tmpAttitude", self.attitude_temp);
        w.int("nameNr", self.name_nr);
        let mut spells = self.spells.clone();
        spells.resize(4, 0);
        w.raw("spells", &spells);

        w.int("NumOfEntries", self.news.len() as i32);
        for news in &self.news {
            w.bool("told", news.told);
            w.float("spread_time", news.spread_time);
            w.int("spreadType", news.spread_type.raw() as i32);
            w.int("news_id", news.news_id);
            w.bool("gossip", news.gossip);
            w.bool("guildvictim", news.guild_victim);
            w.name("witnessName", &news.witness_name);
            w.name("offenderName", &news.offender_name);
            w.name("victimName", &news.victim_name);
        }

        w.vob("carryVob", self.carry_vob);
        w.vob("enemy", self.enemy);
        w.bool("moveLock", self.move_lock);

        if gothic2 {
            w.name("packed", &self.packed.join(";"));
        } else {
            if self.packed.len() != 9 {
                let count = self.packed.len();
                w.fail(format!("NPC {:?} has {count} packed lists, not 9", self.instance));
            }
            for packed in &self.packed {
                w.name("packed", packed);
            }
        }

        w.int("itemCount", self.items.len() as i32);
        for (i, item) in self.items.iter().enumerate() {
            w.vob(&format!("item{i}"), Some(item.item));
            let flags = match w.vobs().get(item.item).map(|vob| &vob.kind) {
                Some(VobKind::Item(item)) => item.state.map_or(0, |s| s.flags),
                _ => 0,
            };
            // The reader can only tell a short key is there from the item's flags.
            match (flags & ITEM_HAS_SHORT_KEY != 0, item.short_key) {
                (true, key) => w.int(&format!("shortKey{i}"), key.unwrap_or(0)),
                (false, None) => {}
                (false, Some(_)) => w.fail(format!(
                    "NPC {:?}'s item {i} has a short key, but not the flag that announces it",
                    self.instance
                )),
            }
        }

        w.int("numInvSlots", self.slots.len() as i32);
        for slot in &self.slots {
            let used = slot.item.is_some();
            w.bool("used", used);
            w.name("name", &slot.name);
            if used {
                w.vob("vob", slot.item);
                w.bool("inInv", slot.in_inventory);
            }
        }

        for (prefix, state) in [("curState", &self.current_state), ("nextState", &self.next_state)]
        {
            w.bool(&format!("{prefix}.valid"), state.valid);
            w.name(&format!("{prefix}.name"), &state.name);
            w.int(&format!("{prefix}.prgIndex"), state.index);
            w.bool(&format!("{prefix}.isRtnState"), state.is_routine);
        }
        w.int("lastAIState", self.last_ai_state);
        w.bool("hasRoutine", self.has_routine);
        w.bool("rtnChanged", self.routine_changed);
        w.bool("rtnOverlay", self.routine_overlay);
        w.int("rtnOverlayCount", self.routine_overlay_count);
        w.int("walkmode_routine", self.walk_mode_routine);
        w.bool("weaponmode_routine", self.weapon_mode_routine);
        w.bool("startNewRoutine", self.start_new_routine);
        w.int("aiStateDriven", self.ai_state_driven);
        w.vec3("aiStatePos", self.ai_state_position);
        w.name("curRoutine", &self.current_routine);
        w.bool("respawn", self.respawn);
        w.int("respawnTime", self.respawn_time);
        w.raw("protection", &raw_ints(&self.protection));

        if gothic2 {
            w.int("bsInterruptableOverride", self.bs_interruptable_override);
            w.int("npcType", self.npc_type);
            w.int("spellMana", self.spell_mana);
        }
    }
}

fn raw_ints(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
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
