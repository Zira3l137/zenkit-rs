use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::math::Quat;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::raw_enum;

use super::Ctx;

raw_enum! {
    pub enum MoverBehavior: u8 = Toggle {
        /// Each trigger moves it to the other end.
        Toggle = 0,
        /// Moves to the end while triggered, and back once untriggered.
        TriggerControl = 1,
        /// Moves to the end, waits for [`stay_open_time`](Mover::stay_open_time) and moves back.
        OpenTime = 2,
        Loop = 3,
        /// Each trigger moves it one key frame on.
        SingleKeys = 4,
    }
}

raw_enum! {
    /// How a mover gets from one key frame to the next.
    pub enum MoverLerp: u8 = Curve {
        Linear = 0,
        Curve = 1,
    }
}

raw_enum! {
    pub enum MoverSpeed: u8 = Constant {
        Constant = 0,
        SlowStartEnd = 1,
        SlowStart = 2,
        SlowEnd = 3,
        SegmentSlowStartEnd = 4,
        SegmentSlowStart = 5,
        SegmentSlowEnd = 6,
    }
}

raw_enum! {
    /// Which of its targets a [`TriggerList`] fires.
    pub enum TriggerBatchMode: u8 = All {
        All = 0,
        /// One more each time it is triggered.
        Next = 1,
        Random = 2,
    }
}

/// An invisible volume that sends a trigger message to its target when touched, damaged or
/// triggered itself.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Trigger {
    /// The name of the VOB to trigger.
    pub target: String,
    pub start_enabled: bool,
    /// Whether it sends an untrigger message when the touch ends.
    pub send_untrigger: bool,
    pub react_to_on_trigger: bool,
    pub react_to_on_touch: bool,
    pub react_to_on_damage: bool,
    pub respond_to_object: bool,
    pub respond_to_pc: bool,
    pub respond_to_npc: bool,
    /// Only VOBs of this name set it off, when not empty.
    pub vob_target: String,
    /// -1 for no limit.
    pub max_activation_count: i32,
    /// In seconds, like the other delays.
    pub retrigger_delay: f32,
    pub damage_threshold: f32,
    pub fire_delay: f32,
}

impl Trigger {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let target = r.name()?;
        let flags = r.raw(1)?[0];
        let filter = r.raw(1)?[0];
        let bit = |byte: u8, n: u8| byte & 1 << n != 0;
        Ok(Self {
            target,
            start_enabled: bit(flags, 0),
            send_untrigger: bit(flags, 2),
            react_to_on_trigger: bit(filter, 0),
            react_to_on_touch: bit(filter, 1),
            react_to_on_damage: bit(filter, 2),
            respond_to_object: bit(filter, 3),
            respond_to_pc: bit(filter, 4),
            respond_to_npc: bit(filter, 5),
            vob_target: r.name()?,
            max_activation_count: r.int()?,
            retrigger_delay: r.float()?,
            damage_threshold: r.float()?,
            fire_delay: r.float()?,
        })
    }
}

/// One target of a [`TriggerList`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TriggerTarget {
    pub name: String,
    /// In seconds.
    pub delay: f32,
}

/// Triggers several targets, each after its own delay.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TriggerList {
    pub trigger: Trigger,
    pub mode: TriggerBatchMode,
    pub targets: Vec<TriggerTarget>,
}

impl TriggerList {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let trigger = Trigger::read(r)?;
        let mode = TriggerBatchMode::from_raw(r.enumeration()?);
        let count = r.byte()?;
        let mut targets = Vec::with_capacity(count.into());
        for _ in 0..count {
            targets.push(TriggerTarget { name: r.name()?, delay: r.float()? });
        }
        Ok(Self { trigger, mode, targets })
    }
}

/// Calls a script function when triggered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TriggerScript {
    pub trigger: Trigger,
    pub function: String,
}

impl TriggerScript {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self { trigger: Trigger::read(r)?, function: r.name()? })
    }
}

/// Takes the player to another world.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TriggerChangeLevel {
    pub trigger: Trigger,
    /// The world file, like `ORCTEMPEL.ZEN`.
    pub level_name: String,
    /// The VOB in that world to put the player at.
    pub start_vob: String,
}

impl TriggerChangeLevel {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self { trigger: Trigger::read(r)?, level_name: r.name()?, start_vob: r.name()? })
    }
}

/// Where a [`Mover`] is at one of its key frames.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MoverKeyFrame {
    pub position: Vec3,
    pub rotation: Quat,
}

/// A VOB that moves between key frames when triggered, like a gate or a lift.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mover {
    pub trigger: Trigger,
    pub behavior: MoverBehavior,
    pub touch_blocker_damage: f32,
    /// In seconds, for [`MoverBehavior::OpenTime`].
    pub stay_open_time: f32,
    pub locked: bool,
    pub auto_link: bool,
    /// Gothic 2 only.
    pub auto_rotate: bool,
    pub speed: f32,
    pub lerp_mode: MoverLerp,
    pub speed_mode: MoverSpeed,
    pub keyframes: Vec<MoverKeyFrame>,
    pub sfx_open_start: String,
    pub sfx_open_end: String,
    pub sfx_moving: String,
    pub sfx_close_start: String,
    pub sfx_close_end: String,
    pub sfx_lock: String,
    pub sfx_unlock: String,
    pub sfx_use_locked: String,
}

impl Mover {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let mut mover = Self {
            trigger: Trigger::read(r)?,
            behavior: MoverBehavior::from_raw(r.enumeration()?),
            touch_blocker_damage: r.float()?,
            stay_open_time: r.float()?,
            locked: r.bool()?,
            auto_link: r.bool()?,
            ..Self::default()
        };
        if r.game == GameVersion::Gothic2 {
            mover.auto_rotate = r.bool()?;
        }

        let count = usize::from(r.word()?);
        if count > 0 {
            mover.speed = r.float()?;
            mover.lerp_mode = MoverLerp::from_raw(r.enumeration()?);
            mover.speed_mode = MoverSpeed::from_raw(r.enumeration()?);

            let raw = r.raw(count * 28)?;
            let mut keys = Reader::new(&raw);
            for _ in 0..count {
                let position = keys.vec3()?;
                let [x, y, z, w] = [keys.f32()?, keys.f32()?, keys.f32()?, keys.f32()?];
                mover
                    .keyframes
                    .push(MoverKeyFrame { position, rotation: Quat::from_xyzw(x, y, z, w) });
            }
        }

        mover.sfx_open_start = r.name()?;
        mover.sfx_open_end = r.name()?;
        mover.sfx_moving = r.name()?;
        mover.sfx_close_start = r.name()?;
        mover.sfx_close_end = r.name()?;
        mover.sfx_lock = r.name()?;
        mover.sfx_unlock = r.name()?;
        mover.sfx_use_locked = r.name()?;
        Ok(mover)
    }
}

/// Triggers its target when the world is entered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TriggerWorldStart {
    pub target: String,
    /// Whether to fire only the first time the world is entered.
    pub fire_once: bool,
}

impl TriggerWorldStart {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self { target: r.name()?, fire_once: r.bool()? })
    }
}

/// Triggers its target when something stops touching it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TriggerUntouch {
    pub target: String,
}

impl TriggerUntouch {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self { target: r.name()? })
    }
}
