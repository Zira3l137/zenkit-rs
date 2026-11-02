//! Cutscenes running in a save game, like a conversation the player was in.

use tracing::warn;
use zenkit_rs_core::Result;
use zenkit_rs_core::text;
use zenkit_rs_cutscene::CutsceneBlock;

use crate::vob::Ctx;
use crate::vob::Next;
use crate::vob::Shared;
use crate::vob::VobId;
use crate::vob::WriteCtx;

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

    /// Writes a cutscene in the field `name`, or null for `None`.
    ///
    /// The engine writes a cutscene once and refers to it wherever else it is held, like in the
    /// cutscene player's list, which reading turns into copies. So a cutscene equal to one
    /// written before becomes a reference to it.
    pub(crate) fn write(context: Option<&Self>, w: &mut WriteCtx, name: &str) {
        let Some(context) = context else {
            w.null_object(name);
            return;
        };
        if let Some(&(_, index)) = w.cutscenes.iter().find(|(written, _)| written == context) {
            w.reference(name, index);
            return;
        }
        let index = w.begin_object(name, "zCCSCutsceneContext:zCCutscene:zCCSBlock", 0);
        w.cutscenes.push((context.clone(), index));
        context.block.write_fields(w);
        match &context.props {
            Some(props) => props.write(w),
            None => w.null_object("%"),
        }
        w.int("numOfRoles", context.role_count);
        w.int("numOfRoleVobs", context.role_vob_count);
        w.vob("%", context.npc);
        w.vob("mainRoleVob", context.main_role);
        w.bool("isCutscene", context.is_cutscene);
        w.int("reference", context.reference);
        w.int("actualBlock", context.actual_block);
        w.vob("alt_stage", context.alt_stage);
        w.vob("stage", context.stage);
        w.bool("wasTriggered", context.was_triggered);
        w.end_object();
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

    fn write(&self, w: &mut WriteCtx) {
        w.begin_object("%", "zCCSProps", 0);
        w.name("name", &self.name);
        w.bool("globalCutscene", self.global);
        w.bool("csLoop", self.looping);
        w.bool("hasToBeTriggerd", self.has_to_be_triggered);
        w.float("distance", self.distance);
        w.float("range", self.range);
        w.int("numLockedBlocks", self.locked_block_count);
        w.enumeration("runBehaviour", self.run_behaviour);
        w.int("runBehaviourValue", self.run_behaviour_value);
        w.name("StageName", &self.stage_name);
        w.name("scriptFuncOnStop", &self.script_function_on_stop);
        w.end_object();
    }
}
