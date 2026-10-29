//! Cutscenes running in a save game, like a conversation the player was in.

use tracing::warn;
use zenkit_rs_core::Result;
use zenkit_rs_core::text;
use zenkit_rs_cutscene::CutsceneBlock;

use crate::vob::Ctx;
use crate::vob::Next;
use crate::vob::Shared;
use crate::vob::VobId;

/// A cutscene being played: its block, its settings and the VOBs it involves.
///
/// Several VOBs can play the same cutscene, and each of them holds a copy of it. A copy cannot
/// hold itself, so a VOB inside a cutscene that refers back to the cutscene reads as having none.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CutsceneContext {
    pub block: CutsceneBlock,
    pub props: Option<CutsceneProps>,
    pub role_count: i32,
    pub role_vob_count: i32,
    pub npc: Option<VobId>,
    pub main_role: Option<VobId>,
    pub is_cutscene: bool,
    pub reference: i32,
    pub actual_block: i32,
    pub alt_stage: Option<VobId>,
    pub stage: Option<VobId>,
    pub was_triggered: bool,
}

impl CutsceneContext {
    /// Reads a cutscene object, or a reference to one read before.
    pub(crate) fn read(r: &mut Ctx) -> Result<Option<Self>> {
        let object = match r.begin("a cutscene")? {
            Next::New(object) if object.class == b"zCCSCutsceneContext:zCCutscene:zCCSBlock" => {
                object
            }
            Next::New(object) => {
                warn!("skipping cutscene of class {:?}", text::latin1(object.class));
                r.skip_object()?;
                return Ok(None);
            }
            Next::Reference(Some(Shared::Cutscene(context))) => return Ok(Some(*context)),
            Next::Reference(Some(other)) => {
                warn!("expected a reference to a cutscene, found one to {other:?}");
                return Ok(None);
            }
            Next::Null | Next::Reference(None) => return Ok(None),
        };

        let Some(block) = CutsceneBlock::read_fields(r)? else {
            r.skip_object()?;
            return Ok(None);
        };
        let context = Self {
            block,
            props: CutsceneProps::read(r)?,
            role_count: r.int()?,
            role_vob_count: r.int()?,
            npc: r.vob()?,
            main_role: r.vob()?,
            is_cutscene: r.bool()?,
            reference: r.int()?,
            actual_block: r.int()?,
            alt_stage: r.vob()?,
            stage: r.vob()?,
            was_triggered: r.bool()?,
        };
        r.objects.insert(object.index, Shared::Cutscene(Box::new(context.clone())));
        r.end(&object)?;
        Ok(Some(context))
    }
}

/// How a cutscene runs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CutsceneProps {
    pub name: String,
    pub global: bool,
    pub looping: bool,
    pub has_to_be_triggered: bool,
    pub distance: f32,
    pub range: f32,
    pub locked_block_count: i32,
    pub run_behaviour: u32,
    pub run_behaviour_value: i32,
    pub stage_name: String,
    /// The script function called when the cutscene stops.
    pub script_function_on_stop: String,
}

impl CutsceneProps {
    fn read(r: &mut Ctx) -> Result<Option<Self>> {
        let object = match r.begin("cutscene properties")? {
            Next::New(object) if object.class == b"zCCSProps" => object,
            Next::New(object) => {
                warn!("skipping cutscene properties of class {:?}", text::latin1(object.class));
                r.skip_object()?;
                return Ok(None);
            }
            _ => return Ok(None),
        };
        let props = Self {
            name: r.name()?,
            global: r.bool()?,
            looping: r.bool()?,
            has_to_be_triggered: r.bool()?,
            distance: r.float()?,
            range: r.float()?,
            locked_block_count: r.int()?,
            run_behaviour: r.enumeration()?,
            run_behaviour_value: r.int()?,
            stage_name: r.name()?,
            script_function_on_stop: r.name()?,
        };
        r.end(&object)?;
        Ok(Some(props))
    }
}
