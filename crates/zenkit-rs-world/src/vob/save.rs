//! What save games add to every VOB: its AI, its event manager and its physics state.

use tracing::warn;
use zenkit_rs_core::Result;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::text;

use super::Ctx;
use super::Next;
use super::VobId;
use crate::cutscene::CutsceneContext;

/// The state save games store for every VOB.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VobState {
    pub sleep_mode: u8,
    pub next_on_timer: f32,
    /// Only for VOBs with [`physics_enabled`](super::Vob::physics_enabled).
    pub rigid_body: Option<RigidBody>,
}

impl VobState {
    pub(crate) fn read(r: &mut Ctx, physics_enabled: bool) -> Result<Self> {
        let mut state = Self { sleep_mode: r.byte()?, next_on_timer: r.float()?, rigid_body: None };
        if physics_enabled {
            state.rigid_body = Some(RigidBody {
                velocity: r.vec3()?,
                mode: r.byte()?,
                gravity_enabled: r.bool()?,
                gravity_scale: r.float()?,
                slide_direction: r.vec3()?,
            });
        }
        Ok(state)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RigidBody {
    pub velocity: Vec3,
    pub mode: u8,
    pub gravity_enabled: bool,
    pub gravity_scale: f32,
    pub slide_direction: Vec3,
}

/// Queues the messages sent to a VOB, like an NPC's orders in a cutscene.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventManager {
    pub cleared: bool,
    pub active: bool,
    /// The cutscene the VOB is playing.
    pub cutscene: Option<Box<CutsceneContext>>,
}

impl EventManager {
    pub(crate) fn read(r: &mut Ctx) -> Result<Option<Self>> {
        let object = match r.begin("an event manager")? {
            Next::New(object) if object.class == b"zCEventManager" => object,
            Next::New(object) => {
                warn!("skipping event manager of class {:?}", text::latin1(object.class));
                r.skip_object()?;
                return Ok(None);
            }
            _ => return Ok(None),
        };
        let manager = Self {
            cleared: r.bool()?,
            active: r.bool()?,
            cutscene: CutsceneContext::read(r)?.map(Box::new),
        };
        r.end(&object)?;
        Ok(Some(manager))
    }
}

/// What drives a VOB in a save game.
#[derive(Debug, Clone, PartialEq)]
pub enum Ai {
    /// `oCAIHuman`: drives an NPC.
    Human(AiHuman),
    /// `oCAIVobMove`: moves an item someone threw or dropped.
    Move(AiMove),
}

impl Ai {
    pub(crate) fn read(r: &mut Ctx) -> Result<Option<Self>> {
        let object = match r.begin("an AI")? {
            Next::New(object) => object,
            Next::Null | Next::Reference(_) => return Ok(None),
        };
        let ai = match object.class {
            b"oCAIHuman:oCAniCtrl_Human:zCAIPlayer" => Self::Human(AiHuman::read(r)?),
            b"oCAIVobMove" => Self::Move(AiMove { vob: r.vob()?, owner: r.vob()? }),
            other => {
                warn!("skipping AI of class {:?}", text::latin1(other));
                r.skip_object()?;
                return Ok(None);
            }
        };
        r.end(&object)?;
        Ok(Some(ai))
    }
}

/// The animation state of an NPC.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiHuman {
    pub water_level: i32,
    pub floor_y: f32,
    pub water_y: f32,
    pub ceil_y: f32,
    pub feet_y: f32,
    pub head_y: f32,
    pub fall_dist_y: f32,
    pub fall_start_y: f32,
    /// The NPC this AI drives.
    pub npc: Option<VobId>,
    pub walk_mode: i32,
    pub weapon_mode: i32,
    pub weapon_mode_last: i32,
    pub weapon_mode_select: i32,
    pub change_weapon: bool,
    pub action_mode: i32,
}

impl AiHuman {
    fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self {
            water_level: r.int()?,
            floor_y: r.float()?,
            water_y: r.float()?,
            ceil_y: r.float()?,
            feet_y: r.float()?,
            head_y: r.float()?,
            fall_dist_y: r.float()?,
            fall_start_y: r.float()?,
            npc: r.vob()?,
            walk_mode: r.int()?,
            weapon_mode: r.int()?,
            weapon_mode_last: r.int()?,
            weapon_mode_select: r.int()?,
            change_weapon: r.bool()?,
            action_mode: r.int()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AiMove {
    /// The VOB being moved, which is usually the one this AI belongs to.
    pub vob: Option<VobId>,
    /// The NPC who threw or dropped it.
    pub owner: Option<VobId>,
}
