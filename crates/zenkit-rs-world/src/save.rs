//! What save games store about a world besides its VOBs.

use tracing::warn;
use zenkit_rs_archive::ObjectHeader;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::text;

use crate::vob::Ctx;
use crate::vob::Next;
use crate::vob::VobId;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorldState {
    /// The NPCs in the world, which are [`Npc`](crate::vob::Npc)s. They are not in the VOB tree.
    pub npcs: Vec<VobId>,
    /// NPCs waiting to be put into the world.
    pub spawns: Vec<Spawn>,
    pub spawning_enabled: bool,
    /// Gothic 2 only.
    pub spawn_flags: i32,
    pub cutscene_player: Option<CutscenePlayer>,
    pub sky: Option<SkyController>,
}

impl WorldState {
    /// Reads what follows the world's sections in a save game.
    pub(crate) fn read_npcs(&mut self, r: &mut Ctx) -> Result<()> {
        let count = r.int()?;
        for _ in 0..count {
            self.npcs.extend(r.vob()?);
        }

        let count = r.int()?;
        for i in 0..count {
            let npc = r.vob()?;
            let (position, timer) = (r.vec3()?, r.float()?);
            match npc {
                Some(npc) => self.spawns.push(Spawn { npc, position, timer }),
                None => warn!("skipping spawn {i}, which has no NPC"),
            }
        }

        self.spawning_enabled = r.bool()?;
        if r.game == GameVersion::Gothic2 {
            self.spawn_flags = r.int()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    /// An [`Npc`](crate::vob::Npc).
    pub npc: VobId,
    pub position: Vec3,
    pub timer: f32,
}

/// Plays the cutscenes running in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CutscenePlayer {
    pub last_process_day: i32,
    pub last_process_hour: i32,
}

impl CutscenePlayer {
    pub(crate) fn read(r: &mut Ctx) -> Result<Option<Self>> {
        let Some(object) = expect(r, "a cutscene player", b"oCCSPlayer:zCCSPlayer")? else {
            return Ok(None);
        };
        let player = Self { last_process_day: r.int()?, last_process_hour: r.int()? };
        let count = r.int()?;
        for _ in 0..count {
            // Cutscenes are not read yet; the samples have none running.
            r.skip_unread("cutscene context")?;
        }
        r.end(&object)?;
        Ok(Some(player))
    }
}

/// The weather and time of day.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SkyController {
    pub master_time: f32,
    pub rain_weight: f32,
    pub rain_start: f32,
    pub rain_stop: f32,
    pub rain_sct_timer: f32,
    pub rain_snd_vol: f32,
    pub day_ctr: f32,
    /// Gothic 2 only, like the fields after it.
    pub fade_scale: f32,
    pub render_lightning: bool,
    pub is_raining: bool,
    pub rain_ctr: i32,
}

impl SkyController {
    pub(crate) fn read(r: &mut Ctx) -> Result<Option<Self>> {
        let Some(object) = expect(r, "a sky controller", b"zCSkyControler_Outdoor")? else {
            return Ok(None);
        };
        let mut sky = Self {
            master_time: r.float()?,
            rain_weight: r.float()?,
            rain_start: r.float()?,
            rain_stop: r.float()?,
            rain_sct_timer: r.float()?,
            rain_snd_vol: r.float()?,
            day_ctr: r.float()?,
            ..Self::default()
        };
        if r.game == GameVersion::Gothic2 {
            sky.fade_scale = r.float()?;
            sky.render_lightning = r.bool()?;
            sky.is_raining = r.bool()?;
            sky.rain_ctr = r.int()?;
        }
        r.end(&object)?;
        Ok(Some(sky))
    }
}

/// Opens an object of `class`, or skips whatever is there instead.
fn expect<'a>(r: &mut Ctx<'a, '_>, what: &str, class: &[u8]) -> Result<Option<ObjectHeader<'a>>> {
    match r.begin(what)? {
        Next::New(object) if object.class == class => Ok(Some(object)),
        Next::New(object) => {
            warn!("skipping {what} of class {:?}", text::latin1(object.class));
            r.skip_object()?;
            Ok(None)
        }
        _ => Ok(None),
    }
}
