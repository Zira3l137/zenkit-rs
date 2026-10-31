use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Deref;
use std::ops::DerefMut;

use zenkit_rs_archive::ArchiveWriter;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::io::Writer;

use super::Decal;
use super::Visual;
use super::VisualKind;
use super::Vob;
use super::VobId;
use super::VobKind;
use super::Vobs;

/// The state of writing the objects of one archive: which VOBs and meshes are written already,
/// so that later uses of them become references, as the engine writes them.
pub(crate) struct WriteCtx<'a, 'b> {
    w: &'b mut ArchiveWriter,
    pub(crate) game: GameVersion,
    vobs: &'a Vobs,
    written: HashMap<VobId, u32>,
    meshes: HashMap<&'a str, u32>,
}

impl Deref for WriteCtx<'_, '_> {
    type Target = ArchiveWriter;

    fn deref(&self) -> &ArchiveWriter {
        self.w
    }
}

impl DerefMut for WriteCtx<'_, '_> {
    fn deref_mut(&mut self) -> &mut ArchiveWriter {
        self.w
    }
}

impl<'a, 'b> WriteCtx<'a, 'b> {
    pub(crate) fn new(w: &'b mut ArchiveWriter, vobs: &'a Vobs, game: GameVersion) -> Self {
        if w.header().save_game {
            w.fail("save games cannot be written yet");
        }
        Self { w, game, vobs, written: HashMap::new(), meshes: HashMap::new() }
    }

    pub(crate) fn gothic2(&self) -> bool {
        self.game == GameVersion::Gothic2
    }

    /// Writes a VOB in the field `name`: null for `None`, a reference if it was written before,
    /// and otherwise the whole VOB with every VOB inside it.
    pub(crate) fn vob(&mut self, name: &str, id: Option<VobId>) {
        let Some(id) = id else {
            self.null_object(name);
            return;
        };
        if let Some(&index) = self.written.get(&id) {
            self.reference(name, index);
            return;
        }
        let vobs = self.vobs;
        let Some(vob) = vobs.get(id) else {
            self.fail(format!("VOB {} is not in the world's VOBs", id.index()));
            self.null_object(name);
            return;
        };

        let (class, [g1, g2]) = class(&vob.kind);
        let version = if self.gothic2() { g2 } else { g1 };
        let index = self.begin_object(name, class, version);
        self.written.insert(id, index);
        write_base(self, vob);
        write_kind(self, &vob.kind);
        self.end_object();
    }

    /// Writes the VOB trees below `roots`, the counterpart of [`Ctx::tree`](super::Ctx::tree):
    /// their count, then each VOB followed by its number of children and their trees.
    ///
    /// The counts are numbered through the whole tree, `childs0` for the roots, as the engine
    /// names them. A VOB found in the tree a second time is written as a reference without
    /// children, which also stops a VOB that is its own ancestor from repeating forever.
    pub(crate) fn tree(&mut self, roots: &[VobId]) {
        let vobs = self.vobs;
        self.int("childs0", roots.len() as i32);

        let mut in_tree = HashSet::new();
        let mut next = 1;
        let mut stack: Vec<VobId> = roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            self.vob("%", Some(id));
            let children = match vobs.get(id) {
                Some(vob) if in_tree.insert(id) => &vob.children[..],
                _ => &[],
            };
            self.int(&format!("childs{next}"), children.len() as i32);
            next += 1;
            stack.extend(children.iter().rev());
        }
    }

    /// Writes a count the engine stores in a byte, failing if it does not fit.
    pub(crate) fn byte_count(&mut self, key: &str, len: usize, what: &str) {
        match u8::try_from(len) {
            Ok(len) => self.byte(key, len),
            Err(_) => self.fail(format!("{len} {what}, at most 255 fit")),
        }
    }
}

/// The engine class of a VOB kind and its class version in Gothic 1 and Gothic 2.
///
/// The versions are the ones the games' own worlds carry, where ZenKit's samples show them.
/// Classes no sample holds take ZenKit's, or their base class's, which ZenKit uses too.
fn class(kind: &VobKind) -> (&'static str, [u16; 2]) {
    const VOB: [u16; 2] = [12289, 52224];
    const INTERACTIVE: [u16; 2] = [35585, 35585];
    const TRIGGER_SUBCLASS: [u16; 2] = [24577, 24577];
    match kind {
        VobKind::Vob => ("zCVob", VOB),
        VobKind::Level => ("zCVobLevelCompo:zCVob", VOB),
        VobKind::Spot => ("zCVobSpot:zCVob", VOB),
        VobKind::StartPoint => ("zCVobStartpoint:zCVob", VOB),
        VobKind::Stair => ("zCVobStair:zCVob", VOB),
        VobKind::ScreenEffect => ("zCVobScreenFX:zCVob", VOB),
        VobKind::Animate(_) => ("zCVobAnimate:zCVob", VOB),
        VobKind::Item(_) => ("oCItem:zCVob", [0, 0]),
        VobKind::LensFlare(_) => ("zCVobLensFlare:zCVob", [64704, 193]),
        VobKind::ParticleEffect(_) => ("zCPFXControler:zCVob", VOB),
        VobKind::MessageFilter(_) => ("zCMessageFilter:zCVob", [0, 0]),
        VobKind::CodeMaster(_) => ("zCCodeMaster:zCVob", [0, 0]),
        VobKind::MoverController(_) => ("zCMoverControler:zCVob", [0, 0]),
        VobKind::TouchDamage(_) => ("oCTouchDamage:zCTouchDamage:zCVob", [36865, 36865]),
        VobKind::Earthquake(_) => ("zCEarthquake:zCVob", [52224, 52224]),
        VobKind::Light(_) => ("zCVobLight:zCVob", [46080, 39168]),
        VobKind::Sound(_) => ("zCVobSound:zCVob", [12289, 12289]),
        VobKind::SoundDaytime(_) => ("zCVobSoundDaytime:zCVobSound:zCVob", [12289, 12289]),
        VobKind::ZoneMusic(_) => ("oCZoneMusic:zCVob", [0, 0]),
        VobKind::ZoneMusicDefault(_) => ("oCZoneMusicDefault:oCZoneMusic:zCVob", [0, 0]),
        VobKind::ZoneFog(_) => ("zCZoneZFog:zCVob", [64704, 5505]),
        VobKind::ZoneFogDefault(_) => ("zCZoneZFogDefault:zCZoneZFog:zCVob", [64704, 5505]),
        VobKind::ZoneFarPlane(_) => ("zCZoneVobFarPlane:zCVob", VOB),
        VobKind::ZoneFarPlaneDefault(_) => {
            ("zCZoneVobFarPlaneDefault:zCZoneVobFarPlane:zCVob", VOB)
        }
        VobKind::Mob(_) => ("oCMOB:zCVob", [47105, 47105]),
        VobKind::Interactive(_) => ("oCMobInter:oCMOB:zCVob", INTERACTIVE),
        VobKind::Bed(_) => ("oCMobBed:oCMobInter:oCMOB:zCVob", [61440, 61440]),
        VobKind::Ladder(_) => ("oCMobLadder:oCMobInter:oCMOB:zCVob", INTERACTIVE),
        VobKind::Switch(_) => ("oCMobSwitch:oCMobInter:oCMOB:zCVob", INTERACTIVE),
        VobKind::Wheel(_) => ("oCMobWheel:oCMobInter:oCMOB:zCVob", INTERACTIVE),
        VobKind::Fire(_) => ("oCMobFire:oCMobInter:oCMOB:zCVob", [18433, 18433]),
        VobKind::Container(_) => ("oCMobContainer:oCMobInter:oCMOB:zCVob", [64513, 64513]),
        VobKind::Door(_) => ("oCMobDoor:oCMobInter:oCMOB:zCVob", [64513, 64513]),
        VobKind::Trigger(_) => ("zCTrigger:zCVob", [47105, 47105]),
        VobKind::CutsceneTrigger(_) => ("oCCSTrigger:zCTrigger:zCVob", TRIGGER_SUBCLASS),
        VobKind::TriggerList(_) => ("zCTriggerList:zCTrigger:zCVob", [59776, 5505]),
        VobKind::TriggerScript(_) => ("oCTriggerScript:zCTrigger:zCVob", TRIGGER_SUBCLASS),
        VobKind::TriggerChangeLevel(_) => {
            ("oCTriggerChangeLevel:zCTrigger:zCVob", TRIGGER_SUBCLASS)
        }
        VobKind::Mover(_) => ("zCMover:zCTrigger:zCVob", [39936, 39936]),
        VobKind::TriggerWorldStart(_) => ("zCTriggerWorldStart:zCVob", [52224, 52224]),
        VobKind::TriggerUntouch(_) => ("zCTriggerUntouch:zCVob", [193, 193]),
        VobKind::CutsceneCamera(_) => ("zCCSCamera:zCVob", [30720, 33793]),
        VobKind::CameraKeyFrame(_) => ("zCCamTrj_KeyFrame:zCVob", [12289, 31489]),
        VobKind::Npc(_) => ("oCNpc:zCVob", [52224, 29697]),
    }
}

fn write_kind(w: &mut WriteCtx, kind: &VobKind) {
    match kind {
        VobKind::Vob
        | VobKind::Level
        | VobKind::Spot
        | VobKind::StartPoint
        | VobKind::Stair
        | VobKind::ScreenEffect => {}
        VobKind::Animate(animate) => animate.write(w),
        VobKind::Item(item) => item.write(w),
        VobKind::LensFlare(flare) => flare.write(w),
        VobKind::ParticleEffect(effect) => effect.write(w),
        VobKind::MessageFilter(filter) => filter.write(w),
        VobKind::CodeMaster(master) => master.write(w),
        VobKind::MoverController(controller) => controller.write(w),
        VobKind::TouchDamage(damage) => damage.write(w),
        VobKind::Earthquake(quake) => quake.write(w),
        VobKind::Light(light) => light.write(w),
        VobKind::Sound(sound) => sound.write(w),
        VobKind::SoundDaytime(sound) => sound.write(w),
        VobKind::ZoneMusic(zone) | VobKind::ZoneMusicDefault(zone) => zone.write(w),
        VobKind::ZoneFog(zone) | VobKind::ZoneFogDefault(zone) => zone.write(w),
        VobKind::ZoneFarPlane(zone) | VobKind::ZoneFarPlaneDefault(zone) => zone.write(w),
        VobKind::Mob(mob) => mob.write(w),
        VobKind::Interactive(i)
        | VobKind::Bed(i)
        | VobKind::Ladder(i)
        | VobKind::Switch(i)
        | VobKind::Wheel(i) => i.write(w),
        VobKind::Fire(fire) => fire.write(w),
        VobKind::Container(container) => container.write(w),
        VobKind::Door(door) => door.write(w),
        VobKind::Trigger(trigger) | VobKind::CutsceneTrigger(trigger) => trigger.write(w),
        VobKind::TriggerList(list) => list.write(w),
        VobKind::TriggerScript(script) => script.write(w),
        VobKind::TriggerChangeLevel(change) => change.write(w),
        VobKind::Mover(mover) => mover.write(w),
        VobKind::TriggerWorldStart(trigger) => trigger.write(w),
        VobKind::TriggerUntouch(trigger) => trigger.write(w),
        VobKind::CutsceneCamera(camera) => camera.write(w),
        VobKind::CameraKeyFrame(frame) => frame.write(w),
        VobKind::Npc(_) => {
            w.fail("NPCs are only stored in save games, which cannot be written yet")
        }
    }
}

/// Writes the `zCVob` part packed, as the games' own worlds store it: the transform and flags in
/// one raw value, and the names only when they are set.
fn write_base<'a>(w: &mut WriteCtx<'a, '_>, vob: &'a Vob) {
    let gothic2 = w.gothic2();
    let mut raw = Writer::with_capacity(83);
    vob.bbox.write(&mut raw);
    raw.vec3(vob.position);
    vob.rotation.transpose().to_cols_array().iter().for_each(|&v| raw.f32(v));

    let bits = u8::from(vob.show_visual)
        | (vob.camera_alignment.raw() as u8 & 0b11) << 1
        | u8::from(vob.cd_static) << 3
        | u8::from(vob.cd_dynamic) << 4
        | u8::from(vob.is_static) << 5
        | (vob.dynamic_shadow.raw() as u8 & 0b11) << 6;
    raw.u8(bits);

    let visual_name = vob.visual.as_ref().map_or("", |v| v.name.as_str());
    let visual_object = vob.visual.as_ref().is_some_and(|v| v.kind != VisualKind::Unknown);
    // Bits 4 and 5 announce an AI and an event manager, which only save games store.
    let mut flags = u16::from(!vob.preset_name.is_empty())
        | u16::from(!vob.name.is_empty()) << 1
        | u16::from(!visual_name.is_empty()) << 2
        | u16::from(visual_object) << 3
        | u16::from(vob.physics_enabled) << 6;
    if gothic2 {
        if !(0..32).contains(&vob.bias) {
            w.fail(format!(
                "VOB {:?} has depth bias {}, packed VOBs fit 0 to 31",
                vob.name, vob.bias
            ));
        }
        flags |= (vob.anim_mode.raw() as u16 & 0b11) << 8
            | (vob.bias as u16 & 0b1_1111) << 10
            | u16::from(vob.ambient) << 15;
        raw.u16(flags);
        raw.f32(vob.anim_strength);
        raw.f32(vob.far_clip_scale);
    } else {
        raw.u8(flags as u8);
    }

    w.int("pack", 1);
    w.raw("dataRaw", raw.as_bytes());
    if !vob.preset_name.is_empty() {
        w.name("presetName", &vob.preset_name);
    }
    if !vob.name.is_empty() {
        w.name("vobName", &vob.name);
    }
    if !visual_name.is_empty() {
        w.name("visual", visual_name);
    }
    if let Some(visual) = vob.visual.as_ref().filter(|_| visual_object) {
        write_visual(w, visual);
    }
}

/// Writes a visual object. VOBs showing the same multi-resolution mesh share one object, as in the
/// games' worlds; every other kind of visual is an object of its own.
fn write_visual<'a>(w: &mut WriteCtx<'a, '_>, visual: &'a Visual) {
    if visual.kind == VisualKind::MultiResolutionMesh
        && let Some(&index) = w.meshes.get(visual.name.as_str())
    {
        w.reference("visual", index);
        return;
    }
    let (class, version) = match &visual.kind {
        VisualKind::Mesh => ("zCMesh", 0),
        VisualKind::MultiResolutionMesh => ("zCProgMeshProto", if w.gothic2() { 64513 } else { 0 }),
        VisualKind::ParticleEffect => ("zCParticleFX", 0),
        VisualKind::Camera => ("zCAICamera", 0),
        VisualKind::Model => ("zCModel", 0),
        VisualKind::MorphMesh => ("zCMorphMesh", 0),
        VisualKind::Decal(_) => ("zCDecal", 0),
        VisualKind::Unknown => return,
    };
    let index = w.begin_object("visual", class, version);
    if visual.kind == VisualKind::MultiResolutionMesh {
        w.meshes.insert(&visual.name, index);
    }
    if let VisualKind::Decal(decal) = &visual.kind {
        write_decal(w, &visual.name, decal);
    }
    w.end_object();
}

fn write_decal(w: &mut WriteCtx, name: &str, decal: &Decal) {
    w.name("name", name);
    w.vec2("decalDim", decal.dimension);
    w.vec2("decalOffset", decal.offset);
    w.bool("decal2Sided", decal.two_sided);
    w.enumeration("decalAlphaFunc", decal.alpha_func.raw());
    w.float("decalTexAniFPS", decal.texture_anim_fps);
    if w.gothic2() {
        w.byte("decalAlphaWeight", decal.alpha_weight);
        w.bool("ignoreDayLight", decal.ignore_daylight);
    }
}
