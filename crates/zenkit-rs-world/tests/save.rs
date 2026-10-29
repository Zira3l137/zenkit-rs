//! Save games. The small samples are single objects cut out of ZenKit's sample saves, each with
//! everything it refers to inside it.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use zenkit_rs_archive::Archive;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::GameVersion::Gothic1;
use zenkit_rs_core::GameVersion::Gothic2;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_world::World;
use zenkit_rs_world::vob::Ai;
use zenkit_rs_world::vob::CameraState;
use zenkit_rs_world::vob::EventManager;
use zenkit_rs_world::vob::ItemState;
use zenkit_rs_world::vob::MoverBehavior;
use zenkit_rs_world::vob::MoverState;
use zenkit_rs_world::vob::TriggerState;
use zenkit_rs_world::vob::VobId;
use zenkit_rs_world::vob::VobKind;
use zenkit_rs_world::vob::Vobs;

fn read(game: GameVersion, class: &str) -> (Vobs, VobId) {
    let dir = if game == Gothic1 { "G1" } else { "G2" };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../samples/{dir}/SaveVOb/{class}.zen"));
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut archive = Archive::new(&data).unwrap();
    assert!(archive.header().save_game);
    let read = Vobs::read(&mut archive, game).unwrap().expect("a VOB");
    assert!(archive.is_eof(), "{class}");
    read
}

fn item(vobs: &Vobs, id: VobId) -> (&str, ItemState) {
    match &vobs[id].kind {
        VobKind::Item(item) => (item.instance.as_str(), item.state.unwrap()),
        other => panic!("expected an item, found {other:?}"),
    }
}

#[test]
fn gothic1_player() {
    let (vobs, id) = read(Gothic1, "oCNpc");
    let player = &vobs[id];
    assert_eq!(player.name, "PC_THIEF");
    assert_eq!(player.event_manager, Some(EventManager { cleared: false, active: true }));
    let state = player.state.unwrap();
    assert_eq!((state.sleep_mode, state.next_on_timer, state.rigid_body), (1, f32::MAX, None));

    let Some(Ai::Human(ai)) = &player.ai else { panic!("{:?}", player.ai) };
    assert_eq!(ai.npc, Some(id), "the AI refers back to the NPC it is in");
    assert_eq!((ai.floor_y, ai.head_y, ai.walk_mode), (5331.0728, 5500.9834, 1));

    let VobKind::Npc(npc) = &player.kind else { panic!("{:?}", player.kind) };
    assert_eq!(npc.instance, "PC_THIEF");
    assert_eq!((npc.level, npc.xp), (999, 0));
    assert_eq!(npc.attributes, [340, 340, 0, 0, 70, 90, 0, 0]);
    assert_eq!(npc.hit_chances, [0; 4], "Gothic 1 has none");
    assert_eq!(npc.protection, [0, 40, 40, 1000, 0, 0, 5, 0]);
    assert_eq!(npc.talents.len(), 8);
    assert_eq!(npc.ai_vars.len(), 50);
    assert_eq!(npc.packed[1], "ITAMARROW:100");
    assert_eq!(npc.packed.len(), 9);
    assert_eq!(npc.current_routine, "RTN_START_1");
    assert_eq!(npc.current_state.name, "ZS_SITAROUND");
    assert!(npc.current_state.valid && npc.current_state.is_routine);

    let items: Vec<_> = npc.items.iter().map(|i| item(&vobs, i.item).0).collect();
    assert_eq!(items, ["ITMW_1H_SWORD_04", "DIEGOS_BOGEN", "STT_ARMOR_H"]);
    assert_eq!(item(&vobs, npc.items[0].item).1, ItemState { amount: 1, flags: 1082146818 });

    // Slots hold references to items in the inventory, which resolve to the same VOBs.
    let slots: Vec<_> = npc.slots.iter().filter_map(|s| Some((s.name.as_str(), s.item?))).collect();
    let sword = npc.items[0].item;
    assert_eq!(slots[0], ("ZS_SWORD", sword));
    assert_eq!(slots.len(), 3);
    assert_eq!(npc.slots.len(), 8);
}

#[test]
fn gothic2_player() {
    let (vobs, id) = read(Gothic2, "oCNpc");
    let player = &vobs[id];
    assert_eq!(player.event_manager, Some(EventManager { cleared: false, active: false }));

    let VobKind::Npc(npc) = &player.kind else { panic!("{:?}", player.kind) };
    assert_eq!(npc.instance, "PC_HERO");
    assert_eq!((npc.level, npc.xp, npc.xp_next_level), (0, 490, 500));
    assert_eq!(npc.attributes, [40, 40, 10, 10, 10, 10, 0, 0]);
    assert_eq!(npc.model_scale, Vec3::ONE);
    assert_eq!(npc.ai_vars.len(), 100);
    assert_eq!(npc.talents.len(), 4, "null talents are left out");
    assert_eq!((npc.talents[0].talent, npc.talents[1].talent), (1, 2));
    assert!(npc.packed[0].starts_with("ITMW_1H_MISC_AXE:1,ITMW_1H_VLK_SWORD:2,"));

    let items: Vec<_> = npc.items.iter().map(|i| item(&vobs, i.item).0).collect();
    assert_eq!(items, ["ITMW_ADDON_KNIFE01", "ITAR_BAU_L"]);
    assert!(npc.items.iter().all(|i| i.short_key.is_none()));
}

#[test]
fn container_items() {
    let (vobs, id) = read(Gothic2, "oCMobContainer");
    let VobKind::Container(container) = &vobs[id].kind else { panic!() };
    assert_eq!(container.contents, "ItMi_Gold:35");
    assert_eq!(container.items.len(), 1);
    assert_eq!(
        item(&vobs, container.items[0]),
        ("ITMI_GOLD", ItemState { amount: 35, flags: 2097153 })
    );
}

#[test]
fn mover_state() {
    let (vobs, id) = read(Gothic1, "zCMover");
    let VobKind::Mover(mover) = &vobs[id].kind else { panic!() };
    assert_eq!(mover.behavior, MoverBehavior::TriggerControl);
    assert_eq!(mover.keyframes.len(), 2);
    assert_eq!(
        mover.trigger.state,
        Some(TriggerState {
            next_time_triggerable: 0.0,
            other_vob: None,
            count_can_be_activated: -1,
            is_enabled: true,
        })
    );
    assert_eq!(
        mover.state,
        Some(MoverState { move_speed_unit: 0.128_094_87, mover_state: 2, ..MoverState::default() })
    );
    assert_eq!(mover.sfx_open_start, "");
}

#[test]
fn camera_and_code_master_state() {
    let (vobs, id) = read(Gothic2, "zCCSCamera");
    let VobKind::CutsceneCamera(camera) = &vobs[id].kind else { panic!() };
    assert_eq!(camera.state, Some(CameraState::default()));
    assert_eq!((camera.trajectory.len(), camera.targets.len()), (2, 1));
    assert!(vobs.iter().all(|(_, vob)| vob.state.is_some()));

    let (vobs, id) = read(Gothic2, "zCCodeMaster");
    let VobKind::CodeMaster(master) = &vobs[id].kind else { panic!() };
    let state = master.state.as_ref().unwrap();
    assert_eq!((state.triggered_count, state.triggered.len()), (0, master.slaves.len()));
}

/// Collects everything a subscriber writes.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Reads a world and gives what it logged.
fn read_logged(data: &[u8]) -> (World, String) {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .finish();
    let world = tracing::subscriber::with_default(subscriber, || World::from_bytes(data)).unwrap();
    let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    (world, log)
}

fn count_kinds(world: &World) -> HashMap<&'static str, usize> {
    let mut kinds = HashMap::new();
    for (_, vob) in world.vobs.iter() {
        let kind = match vob.kind {
            VobKind::Npc(_) => "npc",
            VobKind::Item(_) => "item",
            VobKind::Light(_) => "light",
            VobKind::Container(_) => "container",
            _ => continue,
        };
        *kinds.entry(kind).or_default() += 1;
    }
    kinds
}

/// The complete worlds of ZenKit's sample saves, if `ZENKIT_SAMPLES` points at a ZenKit
/// checkout's `tests/samples`. Binsafe archives check the type of every value, so reading them
/// without errors or leftovers means every class reads exactly what the games wrote.
#[test]
fn zenkit_sample_saves() {
    let Some(root) = std::env::var_os("ZENKIT_SAMPLES").map(PathBuf::from) else {
        eprintln!("ZENKIT_SAMPLES is not set, skipping");
        return;
    };

    let (world, log) = read_logged(&std::fs::read(root.join("G1/Save/WORLD.SAV")).unwrap());
    assert_eq!(world.game, Gothic1);
    assert!(!log.contains("unread"), "{log}");
    let state = world.state.as_ref().unwrap();
    assert_eq!((state.npcs.len(), state.spawns.len()), (1029, 634));
    assert!(state.spawning_enabled);
    assert_eq!(state.sky.unwrap().master_time, 0.837_731);
    assert_eq!(state.cutscene_player.unwrap().last_process_hour, 8);
    assert_eq!(world.vobs.len(), 17136);
    let kinds = count_kinds(&world);
    assert_eq!((kinds["npc"], kinds["item"], kinds["light"]), (1031, 2811, 1077));
    assert_eq!((world.way_net.points.len(), world.way_net.ways.len()), (2784, 3500));
    let VobKind::Npc(player) = &world.vobs[world.roots[0]].kind else { panic!() };
    assert_eq!(player.instance, "PC_THIEF");

    let (world, log) = read_logged(&std::fs::read(root.join("G2/Save/NEWWORLD.SAV")).unwrap());
    assert_eq!(world.game, Gothic2);
    assert!(!log.contains("unread"), "{log}");
    let state = world.state.as_ref().unwrap();
    assert_eq!((state.npcs.len(), state.spawns.len()), (1001, 713));
    let sky = state.sky.unwrap();
    assert_eq!((sky.day_ctr, sky.fade_scale), (1.0, 1.0));
    let kinds = count_kinds(&world);
    assert_eq!((kinds["npc"], kinds["item"], kinds["light"]), (1043, 3681, 3401));
    assert_eq!(kinds["container"], 159);
    assert_eq!((world.way_net.points.len(), world.way_net.ways.len()), (2959, 3402));

    let moving = world.vobs.iter().filter(|(_, vob)| matches!(vob.ai, Some(Ai::Move(_)))).count();
    assert_eq!(moving, 3);
}
