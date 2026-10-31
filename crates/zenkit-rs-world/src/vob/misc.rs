use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::raw_enum;

use super::Ctx;
use super::VobId;
use super::WriteCtx;

raw_enum! {
    /// What a [`MessageFilter`] sends on when it receives a message.
    pub enum MessageFilterAction: u8 = None {
        None = 0,
        Trigger = 1,
        Untrigger = 2,
        Enable = 3,
        Disable = 4,
        Toggle = 5,
    }
}

raw_enum! {
    /// What a [`MoverController`] tells its movers to do.
    pub enum MoverMessage: u8 = FixedDirect {
        /// Jump to [`key`](MoverController::key).
        FixedDirect = 0,
        /// Move to [`key`](MoverController::key) through the key frames in between.
        FixedOrder = 1,
        Next = 2,
        Previous = 3,
    }
}

raw_enum! {
    pub enum TouchCollision: u8 = None {
        None = 0,
        Box = 1,
        Point = 2,
    }
}

/// A VOB whose visual animates, like a model playing its default animation.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Animate {
    pub start_on: bool,
    /// Whether it is running now. Save games only.
    pub is_running: Option<bool>,
}

impl Animate {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let start_on = r.bool()?;
        let is_running = if r.save { Some(r.bool()?) } else { None };
        Ok(Self { start_on, is_running })
    }

    pub(crate) fn write(&self, w: &mut WriteCtx) {
        w.bool("startOn", self.start_on);
    }
}

/// An item lying in the world.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Item {
    /// The script instance that defines the item, like `ITMW_1H_AXE_01`.
    pub instance: String,
    /// Save games only.
    pub state: Option<ItemState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemState {
    pub amount: i32,
    pub flags: i32,
}

impl Item {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let instance = r.name()?;
        let state =
            if r.save { Some(ItemState { amount: r.int()?, flags: r.int()? }) } else { None };
        Ok(Self { instance, state })
    }

    pub(crate) fn write(&self, w: &mut WriteCtx) {
        w.name("itemInstance", &self.instance);
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LensFlare {
    /// The lens flare effect, defined in `LENSFLARE.ZEN`.
    pub fx: String,
}

impl LensFlare {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self { fx: r.name()? })
    }

    pub(crate) fn write(&self, w: &mut WriteCtx) {
        w.name("lensflareFX", &self.fx);
    }
}

/// Plays a particle effect.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParticleEffectController {
    /// The effect's `.PFX` file.
    pub effect: String,
    pub kill_when_done: bool,
    pub initially_running: bool,
}

impl ParticleEffectController {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self { effect: r.name()?, kill_when_done: r.bool()?, initially_running: r.bool()? })
    }

    pub(crate) fn write(&self, w: &mut WriteCtx) {
        w.name("pfxName", &self.effect);
        w.bool("killVobWhenDone", self.kill_when_done);
        w.bool("pfxStartOn", self.initially_running);
    }
}

/// Turns the trigger and untrigger messages it receives into other messages for its target.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MessageFilter {
    pub target: String,
    pub on_trigger: MessageFilterAction,
    pub on_untrigger: MessageFilterAction,
}

impl MessageFilter {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self {
            target: r.name()?,
            on_trigger: MessageFilterAction::from_raw(r.enumeration()?),
            on_untrigger: MessageFilterAction::from_raw(r.enumeration()?),
        })
    }

    pub(crate) fn write(&self, w: &mut WriteCtx) {
        w.name("triggerTarget", &self.target);
        w.enumeration("onTrigger", self.on_trigger.raw());
        w.enumeration("onUntrigger", self.on_untrigger.raw());
    }
}

/// Triggers its target once all of its slaves have been triggered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CodeMaster {
    pub target: String,
    /// Whether the slaves must be triggered in order.
    pub ordered: bool,
    /// Whether triggering a slave out of order fails at once. Only used when
    /// [`ordered`](Self::ordered) is set.
    pub first_false_is_failure: bool,
    /// What to trigger when the slaves are triggered in the wrong order.
    pub failure_target: String,
    /// Whether an untrigger message resets the slaves triggered so far.
    pub untriggered_cancels: bool,
    /// The names of the VOBs to wait for.
    pub slaves: Vec<String>,
    /// Gothic 2 save games only.
    pub state: Option<CodeMasterState>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CodeMasterState {
    pub triggered_count: u8,
    /// One entry per slave: the VOB that triggered it, if it has been.
    pub triggered: Vec<Option<VobId>>,
}

impl CodeMaster {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let mut master = Self {
            target: r.name()?,
            ordered: r.bool()?,
            first_false_is_failure: r.bool()?,
            failure_target: r.name()?,
            untriggered_cancels: r.bool()?,
            slaves: Vec::new(),
            state: None,
        };
        let count = r.byte()?;
        for _ in 0..count {
            master.slaves.push(r.name()?);
        }

        if r.save && r.game == GameVersion::Gothic2 {
            let mut state = CodeMasterState { triggered_count: r.byte()?, triggered: Vec::new() };
            for _ in 0..count {
                state.triggered.push(r.vob()?);
            }
            master.state = Some(state);
        }
        Ok(master)
    }

    pub(crate) fn write(&self, w: &mut WriteCtx) {
        w.name("triggerTarget", &self.target);
        w.bool("orderRelevant", self.ordered);
        w.bool("firstFalseIsFailure", self.first_false_is_failure);
        w.name("triggerTargetFailure", &self.failure_target);
        w.bool("untriggerCancels", self.untriggered_cancels);
        w.byte_count("numSlaves", self.slaves.len(), "code master slaves");
        for (i, slave) in self.slaves.iter().take(255).enumerate() {
            w.name(&format!("slaveVobName{i}"), slave);
        }
    }
}

/// Sends its target movers to a key frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MoverController {
    pub target: String,
    pub message: MoverMessage,
    pub key: i32,
}

impl MoverController {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self {
            target: r.name()?,
            message: MoverMessage::from_raw(r.enumeration()?),
            key: r.int()?,
        })
    }

    pub(crate) fn write(&self, w: &mut WriteCtx) {
        w.name("triggerTarget", &self.target);
        w.enumeration("moverMessage", self.message.raw());
        w.int("gotoFixedKey", self.key);
    }
}

/// Hurts whatever touches it, like the barrier around the Gothic 1 colony.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TouchDamage {
    pub damage: f32,
    pub barrier: bool,
    pub blunt: bool,
    pub edge: bool,
    pub fire: bool,
    pub fly: bool,
    pub magic: bool,
    pub point: bool,
    pub fall: bool,
    /// In seconds; 0 hurts only once.
    pub repeat_delay: f32,
    pub volume_scale: f32,
    pub collision: TouchCollision,
}

impl TouchDamage {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self {
            damage: r.float()?,
            barrier: r.bool()?,
            blunt: r.bool()?,
            edge: r.bool()?,
            fire: r.bool()?,
            fly: r.bool()?,
            magic: r.bool()?,
            point: r.bool()?,
            fall: r.bool()?,
            repeat_delay: r.float()?,
            volume_scale: r.float()?,
            collision: TouchCollision::from_raw(r.enumeration()?),
        })
    }

    pub(crate) fn write(&self, w: &mut WriteCtx) {
        w.float("damage", self.damage);
        w.bool("Barrier", self.barrier);
        w.bool("Blunt", self.blunt);
        w.bool("Edge", self.edge);
        w.bool("Fire", self.fire);
        w.bool("Fly", self.fly);
        w.bool("Magic", self.magic);
        w.bool("Point", self.point);
        w.bool("Fall", self.fall);
        w.float("damageRepeatDelaySec", self.repeat_delay);
        w.float("damageVolDownScale", self.volume_scale);
        w.enumeration("damageCollType", self.collision.raw());
    }
}

/// Shakes the camera.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Earthquake {
    pub radius: f32,
    /// In seconds.
    pub duration: f32,
    pub amplitude: Vec3,
}

impl Earthquake {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self { radius: r.float()?, duration: r.float()?, amplitude: r.vec3()? })
    }

    pub(crate) fn write(&self, w: &mut WriteCtx) {
        w.float("radius", self.radius);
        w.float("timeSec", self.duration);
        w.vec3("amplitudeCM", self.amplitude);
    }
}
