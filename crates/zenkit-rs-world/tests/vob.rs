//! VOBs read on their own, from ZenKit's samples. Their key tables were cut off to keep them small;
//! binsafe archives only use those for error messages.

use std::path::Path;

use zenkit_rs_archive::Archive;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::GameVersion::Gothic1;
use zenkit_rs_core::GameVersion::Gothic2;
use zenkit_rs_core::math::Color;
use zenkit_rs_core::math::Mat3;
use zenkit_rs_core::math::Mat4;
use zenkit_rs_core::math::Quat;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::math::Vec4;
use zenkit_rs_material::AlphaFunction;
use zenkit_rs_world::vob::*;

fn read(game: GameVersion, class: &str) -> (Vobs, VobId) {
    let dir = if game == Gothic1 { "G1" } else { "G2" };
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../samples/{dir}/VOb/{class}.zen"));
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut archive = Archive::new(&data).unwrap();
    let read = Vobs::read(&mut archive, game).unwrap().expect("a VOB");
    assert!(archive.is_eof(), "{class}");
    read
}

/// Reads a sample holding a single VOB and gives what its class adds.
fn kind(game: GameVersion, class: &str) -> VobKind {
    let (vobs, id) = read(game, class);
    assert_eq!(vobs.len(), 1);
    vobs[id].kind.clone()
}

macro_rules! expect {
    ($value:expr, $pattern:pat => $out:expr) => {
        match $value {
            $pattern => $out,
            other => panic!("expected {}, found {other:?}", stringify!($pattern)),
        }
    };
}

#[test]
fn vob_g1() {
    let (vobs, id) = read(Gothic1, "zCVob");
    let vob = &vobs[id];
    assert_eq!(vob.kind, VobKind::Vob);
    assert_eq!(vob.bbox.min, Vec3::new(-18966.623, -236.70769, 4_373.235));
    assert_eq!(vob.bbox.max, Vec3::new(-18772.623, -42.707687, 4_567.235));
    assert_eq!(vob.position, Vec3::new(-18869.623, -139.70769, 4_470.235));
    assert_eq!(vob.rotation, Mat3::IDENTITY);
    assert!(vob.show_visual);
    assert_eq!(vob.camera_alignment, SpriteAlignment::None);
    assert!(!vob.cd_static && !vob.cd_dynamic && !vob.is_static && !vob.physics_enabled);
    assert_eq!(vob.dynamic_shadow, ShadowType::None);
    assert_eq!(vob.anim_mode, AnimationMode::None);
    assert_eq!((vob.bias, vob.ambient, vob.anim_strength), (0, false, 0.0));
    assert_eq!(vob.far_clip_scale, 2.0);
    assert_eq!((vob.preset_name.as_str(), vob.name.as_str()), ("", ""));
    let visual = vob.visual.as_ref().unwrap();
    assert_eq!(visual.name, "FIRE.pfx");
    assert_eq!(visual.kind, VisualKind::ParticleEffect);
    assert!(vob.children.is_empty());
}

#[test]
fn vob_g2() {
    let (vobs, id) = read(Gothic2, "zCVob");
    let vob = &vobs[id];
    assert_eq!(vob.bbox.min, Vec3::new(30897.104, 4760.2495, -14865.572));
    assert_eq!(vob.bbox.max, Vec3::new(30929.83, 4836.1753, -14817.313));
    assert_eq!(vob.position, Vec3::new(30913.467, 4798.975, -14841.443));
    assert_eq!(
        vob.rotation,
        Mat3::from_cols(
            Vec3::new(0.920_505_3, 0.0, -0.390_731_28),
            Vec3::Y,
            Vec3::new(0.390_731_28, 0.0, 0.920_505_3),
        )
    );
    assert!(vob.show_visual && vob.is_static);
    assert!(!vob.cd_static && !vob.cd_dynamic && !vob.physics_enabled);
    assert_eq!(vob.anim_mode, AnimationMode::None);
    assert_eq!((vob.bias, vob.ambient, vob.anim_strength), (0, false, 0.0));
    assert_eq!(vob.far_clip_scale, 1.0);
    let visual = vob.visual.as_ref().unwrap();
    assert_eq!(visual.name, "OW_MISC_WALL_TORCH_01.3DS");
    assert_eq!(visual.kind, VisualKind::MultiResolutionMesh);
}

#[test]
fn animate() {
    let animate = |game| expect!(kind(game, "zCVobAnimate"), VobKind::Animate(a) => a);
    assert!(!animate(Gothic1).start_on);
    assert!(animate(Gothic2).start_on);
}

#[test]
fn zones() {
    let far = |game| expect!(kind(game, "zCZoneVobFarPlane"), VobKind::ZoneFarPlane(z) => z);
    assert_eq!(far(Gothic1), ZoneFarPlane { vob_far_plane_z: 9000.0, inner_range_percentage: 0.7 });
    assert_eq!(far(Gothic2), ZoneFarPlane { vob_far_plane_z: 6500.0, inner_range_percentage: 0.7 });

    let fog = |game| expect!(kind(game, "zCZoneZFog"), VobKind::ZoneFog(z) => z);
    let g1 = fog(Gothic1);
    assert_eq!((g1.range_center, g1.inner_range_percentage), (8000.0, 0.85));
    assert_eq!(g1.color, Color::new(120, 120, 120, 20));
    assert!(!g1.fade_out_sky && !g1.override_color);
    let g2 = fog(Gothic2);
    assert_eq!((g2.range_center, g2.inner_range_percentage), (16000.0, 0.7));
    assert_eq!(g2.color, Color::new(120, 120, 120, 255));
    assert!(!g2.fade_out_sky && !g2.override_color);

    let music = |game| expect!(kind(game, "oCZoneMusic"), VobKind::ZoneMusic(z) => z);
    for (game, reverb) in [(Gothic1, -9.0), (Gothic2, -3.219)] {
        let zone = music(game);
        assert_eq!(
            zone,
            ZoneMusic {
                enabled: true,
                priority: 1,
                ellipsoid: false,
                reverb,
                volume: 1.0,
                looping: true
            }
        );
    }
}

#[test]
fn lens_flare_and_item() {
    let flare = expect!(kind(Gothic1, "zCVobLensFlare"), VobKind::LensFlare(f) => f);
    assert_eq!(flare.fx, "TORCHFX01");

    let item = |game| expect!(kind(game, "oCItem"), VobKind::Item(i) => i.instance);
    assert_eq!(item(Gothic1), "ITMW_1H_AXE_01");
    assert_eq!(item(Gothic2), "ITPL_BLUEPLANT");
}

#[test]
fn particle_effects() {
    let pfx = |game| expect!(kind(game, "zCPFXControler"), VobKind::ParticleEffect(p) => p);
    let g1 = pfx(Gothic1);
    assert_eq!(g1.effect, "CS_miltenfog.PFX");
    assert!(g1.kill_when_done && g1.initially_running);
    let g2 = pfx(Gothic2);
    assert_eq!(g2.effect, "STARGATE_EDGES.PFX");
    assert!(!g2.kill_when_done && !g2.initially_running);
}

#[test]
fn mobs() {
    let g1 = expect!(kind(Gothic1, "oCMOB"), VobKind::Mob(m) => m);
    assert_eq!(g1, Mob { hit_points: 10, ..Mob::default() });
    let g2 = expect!(kind(Gothic2, "oCMOB"), VobKind::Mob(m) => m);
    assert_eq!(g2, Mob { focus_name: "MOBNAME_GRAVE_18".into(), hit_points: 10, ..Mob::default() });

    let inter = |game| expect!(kind(game, "oCMobInter"), VobKind::Interactive(i) => i);
    let g1 = inter(Gothic1);
    assert_eq!(g1.state_count, 1);
    assert_eq!(g1.target, "OW_ORC_MAINGATE_01");
    assert_eq!(g1.condition_function, "MC_ORCCITY_GATE");
    assert_eq!((g1.item.as_str(), g1.on_state_change_function.as_str()), ("", ""));
    assert!(!g1.rewind);
    let g2 = inter(Gothic2);
    assert_eq!(g2.state_count, 1);
    assert_eq!(g2.on_state_change_function, "PRAYIDOL");
    assert_eq!((g2.target.as_str(), g2.condition_function.as_str()), ("", ""));

    for (game, tree) in [(Gothic1, "FIRETREE_MEDIUM.ZEN"), (Gothic2, "FIRETREE_LAMP.ZEN")] {
        let kind = kind(game, "oCMobFire");
        assert!(kind.mob().is_some() && kind.interactive().is_some());
        let fire = expect!(kind, VobKind::Fire(f) => f);
        assert_eq!((fire.slot.as_str(), fire.vob_tree.as_str()), ("BIP01 FIRE", tree));
    }

    for (game, contents) in [(Gothic1, ""), (Gothic2, "ItMi_Gold:35")] {
        let container = expect!(kind(game, "oCMobContainer"), VobKind::Container(c) => c);
        assert!(!container.locked);
        assert_eq!((container.key.as_str(), container.pick_string.as_str()), ("", ""));
        assert_eq!(container.contents, contents);
    }

    for game in [Gothic1, Gothic2] {
        let door = expect!(kind(game, "oCMobDoor"), VobKind::Door(d) => d);
        assert!(!door.locked);
        assert_eq!((door.key.as_str(), door.pick_string.as_str()), ("", ""));
    }
}

#[test]
fn lights() {
    let g1 = expect!(kind(Gothic1, "zCVobLight"), VobKind::Light(l) => l);
    let [a, b, c, d] = [
        Color::new(211, 147, 107, 255),
        Color::new(223, 173, 117, 255),
        Color::new(225, 197, 100, 255),
        Color::new(227, 209, 106, 255),
    ];
    assert_eq!(
        g1,
        Light {
            preset: String::new(),
            light_type: LightType::Point,
            range: 2000.0,
            color: b,
            cone_angle: 0.0,
            is_static: false,
            quality: LightQuality::Low,
            lensflare_fx: String::new(),
            on: true,
            range_animation_scale: Vec::new(),
            range_animation_fps: 0.0,
            range_animation_smooth: true,
            color_animation_list: vec![
                a, b, a, b, c, b, d, b, a, b, c, d, b, a, c, b, c, a, b, d, c, a, c, b, c, d, b, a,
                b, a, c, d, b,
            ],
            color_animation_fps: 11.000_007,
            color_animation_smooth: false,
            can_move: true,
        }
    );

    let g2 = expect!(kind(Gothic2, "zCVobLight"), VobKind::Light(l) => l);
    assert_eq!(
        g2,
        Light {
            preset: "NW_STANDART_FIRE_STATIC".into(),
            light_type: LightType::Point,
            range: 400.0,
            color: Color::new(100, 71, 60, 255),
            is_static: true,
            quality: LightQuality::Low,
            ..Light::default()
        }
    );
}

#[test]
fn sounds() {
    let g1 = expect!(kind(Gothic1, "zCVobSound"), VobKind::Sound(s) => s);
    assert_eq!(
        g1,
        Sound {
            volume: 100.0,
            mode: SoundMode::Loop,
            random_delay: 5.0,
            random_delay_var: 2.0,
            initially_playing: true,
            ambient_3d: false,
            obstruction: false,
            cone_angle: 0.0,
            volume_type: SoundVolume::Spherical,
            radius: 1500.0,
            sound_name: "FIRE_MEDIUM".into(),
        }
    );
    let g2 = expect!(kind(Gothic2, "zCVobSound"), VobKind::Sound(s) => s);
    assert_eq!((g2.volume, g2.mode), (80.0, SoundMode::Random));
    assert_eq!((g2.random_delay, g2.random_delay_var, g2.radius), (30.0, 20.0, 3000.0));
    assert!(g2.initially_playing && !g2.ambient_3d && !g2.obstruction);
    assert_eq!(g2.sound_name, "OW_CROW");

    for (game, start, end, name) in [
        (Gothic1, 8.0, 18.0, "INSECTS_AND_NIGHTINGALES"),
        (Gothic2, 5.0, 21.0, "InsectsFrogs_Night"),
    ] {
        let kind = kind(game, "zCVobSoundDaytime");
        assert!(kind.sound().is_some());
        let daytime = expect!(kind, VobKind::SoundDaytime(s) => s);
        assert_eq!((daytime.start_time, daytime.end_time), (start, end));
        assert_eq!(daytime.sound_name2, name);
    }
}

#[test]
fn triggers() {
    let g1 = expect!(kind(Gothic1, "oCCSTrigger"), VobKind::CutsceneTrigger(t) => t);
    assert_eq!(
        g1,
        Trigger {
            target: "AMB_PSI_CS003.CS".into(),
            // ZenKit's flags 3 and filter flags 23.
            start_enabled: true,
            send_untrigger: false,
            react_to_on_trigger: true,
            react_to_on_touch: true,
            react_to_on_damage: true,
            respond_to_object: false,
            respond_to_pc: true,
            respond_to_npc: false,
            vob_target: String::new(),
            max_activation_count: -1,
            retrigger_delay: 0.0,
            damage_threshold: 0.0,
            fire_delay: 0.0,
        }
    );

    let g2 = expect!(kind(Gothic2, "zCTrigger"), VobKind::Trigger(t) => t);
    assert_eq!(g2, Trigger { start_enabled: true, max_activation_count: -1, ..Trigger::default() });

    let script = |game| expect!(kind(game, "oCTriggerScript"), VobKind::TriggerScript(t) => t);
    assert_eq!(script(Gothic1).function, "ON_NC_GATE_TRIGGER");
    assert_eq!(script(Gothic2).function, "EVT_CAVALORNSGOBBOS_FUNC");

    let change =
        |game| expect!(kind(game, "oCTriggerChangeLevel"), VobKind::TriggerChangeLevel(t) => t);
    let g1 = change(Gothic1);
    assert_eq!(
        (g1.level_name.as_str(), g1.start_vob.as_str()),
        ("ORCTEMPEL.ZEN", "ENTRANCE_ORCTEMPLE_SURFACE")
    );
    let g2 = change(Gothic2);
    assert_eq!(
        (g2.level_name.as_str(), g2.start_vob.as_str()),
        ("ADDON\\ADDONWORLD.ZEN", "START_ADDON")
    );

    let start = expect!(kind(Gothic2, "zCTriggerWorldStart"), VobKind::TriggerWorldStart(t) => t);
    assert_eq!(start.target, "EVT_TROLL_GRAVE_MOVER_01");
    assert!(start.fire_once);

    let untouch = expect!(kind(Gothic2, "zCTriggerUntouch"), VobKind::TriggerUntouch(t) => t);
    assert_eq!(untouch.target, "EVT_TROLL_GRAVE_TRIGGERLIST_01");
}

#[test]
fn trigger_lists() {
    let list = |game| expect!(kind(game, "zCTriggerList"), VobKind::TriggerList(t) => t);
    let target = |name: &str, delay| TriggerTarget { name: name.into(), delay };

    let g1 = list(Gothic1);
    assert_eq!(g1.mode, TriggerBatchMode::All);
    assert_eq!(
        g1.targets,
        [
            target("EVT_CASTLE_PLATE", 0.0),
            target("EVT_CASTLE_FLOOR_5", 6.0),
            target("EVT_CASTLE_FLOOR_4", 2.0),
            target("EVT_CASTLE_FLOOR_3", 1.0),
            target("EVT_CASTLE_FLOOR_2", 0.0),
            target("EVT_CASTLE_FLOOR_1", 4.0),
        ]
    );

    let g2 = list(Gothic2);
    assert_eq!(g2.mode, TriggerBatchMode::All);
    assert_eq!(
        g2.targets,
        [
            target("EVT_ADDON_TROLLPORTAL_MASTERTRIGGERLIST_ALPHA_01", 0.0),
            target("EVT_ADDON_TROLLPORTAL_TRIGGERSCRIPT_01", 0.0),
        ]
    );
}

#[test]
fn movers() {
    let mover = |game| expect!(kind(game, "zCMover"), VobKind::Mover(m) => *m);
    let key = |position: [f32; 3], [x, y, z, w]: [f32; 4]| MoverKeyFrame {
        position: Vec3::from_array(position),
        rotation: Quat::from_xyzw(x, y, z, w),
    };

    let g1 = mover(Gothic1);
    assert_eq!(g1.behavior, MoverBehavior::TriggerControl);
    assert_eq!((g1.touch_blocker_damage, g1.stay_open_time, g1.speed), (0.0, 2.0, 0.002_000_24));
    assert!(!g1.locked && !g1.auto_link && !g1.auto_rotate);
    assert_eq!((g1.lerp_mode, g1.speed_mode), (MoverLerp::Linear, MoverSpeed::SlowStartEnd));
    assert_eq!(
        g1.keyframes,
        [
            key(
                [-23325.2, 3438.9133, -21834.947],
                [0.091_305_84, 0.747_364_76, 0.649_674_6, 0.105_035_28]
            ),
            key(
                [-23325.154, 3438.9133, -21844.367],
                [0.138_834_36, -0.069_077_85, 0.987_856_1, -0.009_708_27]
            ),
        ]
    );

    let g2 = mover(Gothic2);
    assert_eq!(g2.behavior, MoverBehavior::Toggle);
    assert_eq!((g2.stay_open_time, g2.speed), (2.0, 0.05));
    assert!(!g2.locked && !g2.auto_link && !g2.auto_rotate);
    let rotation = [-0.000_760_567_7, 0.017_451_758, 0.008_693_331, 0.999_809_8];
    assert_eq!(
        g2.keyframes,
        [
            key([29785.96, 5140.82, -16279.848], rotation),
            key([29785.96, 5720.82, -16279.848], rotation)
        ]
    );

    for mover in [g1, g2] {
        assert_eq!(
            [&mover.sfx_open_start, &mover.sfx_open_end, &mover.sfx_moving],
            ["GATE_START", "GATE_STOP", "GATE_LOOP"]
        );
        assert_eq!([&mover.sfx_close_start, &mover.sfx_close_end], ["GATE_START", "GATE_STOP"]);
        assert_eq!([&mover.sfx_lock, &mover.sfx_unlock, &mover.sfx_use_locked], ["", "", ""]);
    }
}

#[test]
fn logic_vobs() {
    let filter = expect!(kind(Gothic2, "zCMessageFilter"), VobKind::MessageFilter(f) => f);
    assert_eq!(filter.target, "EVT_ADDON_TROLLPORTAL_CAMERA_01");
    assert_eq!(filter.on_trigger, MessageFilterAction::Untrigger);
    assert_eq!(filter.on_untrigger, MessageFilterAction::Untrigger);

    let master = expect!(kind(Gothic2, "zCCodeMaster"), VobKind::CodeMaster(m) => m);
    assert_eq!(master.target, "EVT_ORNAMENT_TRIGGER_BIGFARM_01");
    assert!(!master.ordered && !master.first_false_is_failure && !master.untriggered_cancels);
    assert_eq!(master.failure_target, "");
    assert_eq!(
        master.slaves,
        [
            "EVT_ORNAMENT_SWITCH_BIGFARM_01",
            "EVT_ORNAMENT_SWITCH_BIGFARM_02",
            "EVT_ORNAMENT_SWITCH_BIGFARM_03"
        ]
    );

    let quake = expect!(kind(Gothic2, "zCEarthquake"), VobKind::Earthquake(e) => e);
    assert_eq!((quake.radius, quake.duration), (1000.0, 5.0));
    assert_eq!(quake.amplitude, Vec3::new(2.0, 10.0, 2.0));

    let damage = expect!(kind(Gothic2, "oCTouchDamage"), VobKind::TouchDamage(d) => d);
    assert_eq!(
        damage,
        TouchDamage {
            damage: 1000.0,
            barrier: true,
            edge: true,
            volume_scale: 1.0,
            collision: TouchCollision::Box,
            ..TouchDamage::default()
        }
    );
}

#[test]
fn cutscene_camera_holds_its_key_frames() {
    let (vobs, id) = read(Gothic2, "zCCSCamera");
    let camera = expect!(&vobs[id].kind, VobKind::CutsceneCamera(c) => c);
    assert_eq!(camera.trajectory_for, CameraReference::World);
    assert_eq!(camera.target_trajectory_for, CameraReference::World);
    assert_eq!((camera.loop_mode, camera.lerp_mode), (CameraLoop::None, CameraLerp::Path));
    assert!(!camera.ignore_for_vob_rotation && !camera.ignore_for_vob_rotation_target);
    assert!(!camera.adapt && !camera.ease_first && !camera.ease_last);
    assert_eq!(camera.total_duration, 20.0);
    assert_eq!(camera.auto_focus_vob, "");
    assert!(!camera.auto_player_movable && !camera.auto_untrigger_last);
    assert_eq!(camera.auto_untrigger_last_delay, 0.0);
    assert_eq!((camera.trajectory.len(), camera.targets.len()), (2, 1));
    assert_eq!(vobs.len(), 4);

    let frame = |id| expect!(&vobs[id].kind, VobKind::CameraKeyFrame(f) => f.clone());
    let pose = |translation: [f32; 3]| Mat4::from_translation(Vec3::from_array(translation));
    let expected = |time, motion_type, original_pose| CameraKeyFrame {
        time,
        roll_angle: 0.0,
        fov_scale: 1.0,
        motion_type,
        motion_type_fov: CameraMotion::Smooth,
        motion_type_roll: CameraMotion::Smooth,
        motion_type_time_scale: CameraMotion::Smooth,
        tension: 0.0,
        bias: 0.0,
        continuity: 0.0,
        time_scale: 1.0,
        time_fixed: false,
        original_pose,
    };

    let first = Mat4::from_cols(
        Vec4::new(0.202_226_31, 3.006_479e-11, -0.979_338_8, 0.0),
        Vec4::new(0.008_059_132, 0.999_966_14, 0.001_664_151_8, 0.0),
        Vec4::new(0.979_305_7, -0.008_229_156, 0.202_219_46, 0.0),
        Vec4::new(81815.766, 3905.9504, 29227.188, 1.0),
    );
    assert_eq!(frame(camera.trajectory[0]), expected(0.0, CameraMotion::Slow, first));
    assert_eq!(
        frame(camera.trajectory[1]),
        expected(20.0, CameraMotion::Slow, pose([82642.9, 3977.6484, 28358.457]))
    );
    assert_eq!(
        frame(camera.targets[0]),
        expected(0.0, CameraMotion::Smooth, pose([82642.9, 3899.0, 29397.984]))
    );
}

#[test]
fn decals_keep_their_settings() {
    // None of the samples has a decal, so this one is made from scratch.
    let mut data = b"ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 0\nEND\nobjects 2\nEND\n\n\
        [% zCVob 52224 0]\npack=int:0\npresetName=string:\n\
        bbox3DWS=rawFloat:0 0 0 1 1 1\n\
        trafoOSToWSRot=raw:0000803f0000000000000000000000000000803f000000000000000000000000000000000000803f\n\
        trafoOSToWSPos=vec3:1 2 3\nvobName=string:DECAL\nvisual=string:BLOOD.TGA\n\
        showVisual=bool:1\nvisualCamAlign=enum:2\nvisualAniMode=enum:1\nvisualAniModeStrength=float:0.5\n\
        vobFarClipZScale=float:1\ncdStatic=bool:0\ncdDyn=bool:0\nstaticVob=bool:0\ndynShadow=enum:1\n\
        zbias=int:3\nisAmbient=bool:1\n\
        [visual zCDecal 53505 1]\nname=string:BLOOD.TGA\ndecalDim=rawFloat:25 50\n\
        decalOffset=rawFloat:0 1\ndecal2Sided=bool:1\ndecalAlphaFunc=enum:2\ndecalTexAniFPS=float:4\n\
        decalAlphaWeight=int:200\nignoreDayLight=bool:1\n[]\n\
        [ai % 0 0]\n[]\n[]\n"
        .to_vec();

    let read = |data: &[u8]| {
        let mut archive = Archive::new(data).unwrap();
        let (vobs, id) = Vobs::read(&mut archive, Gothic2).unwrap().unwrap();
        vobs[id].clone()
    };

    let vob = read(&data);
    assert_eq!(vob.name, "DECAL");
    assert_eq!(vob.position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!((vob.camera_alignment, vob.anim_mode), (SpriteAlignment::Full, AnimationMode::Wind));
    assert_eq!((vob.anim_strength, vob.far_clip_scale, vob.bias), (0.5, 1.0, 3));
    assert_eq!(vob.dynamic_shadow, ShadowType::Blob);
    assert!(vob.show_visual && vob.ambient && vob.physics_enabled);
    let visual = vob.visual.unwrap();
    assert_eq!(visual.name, "BLOOD.TGA");
    let decal = expect!(visual.kind, VisualKind::Decal(d) => d);
    assert_eq!(decal.dimension.to_array(), [25.0, 50.0]);
    assert_eq!(decal.offset.to_array(), [0.0, 1.0]);
    assert!(decal.two_sided && decal.ignore_daylight);
    assert_eq!(decal.alpha_func, AlphaFunction::Blend);
    assert_eq!((decal.texture_anim_fps, decal.alpha_weight), (4.0, 200));

    // An unknown visual class still keeps the visual's name.
    let at = data.windows(7).position(|w| w == b"zCDecal").unwrap();
    data[at..at + 7].copy_from_slice(b"zCOther");
    let visual = read(&data).visual.unwrap();
    assert_eq!((visual.name.as_str(), visual.kind), ("BLOOD.TGA", VisualKind::Unknown));
}

#[test]
fn unknown_and_null_vobs_read_as_none() {
    for (class, version) in [("zCVobMagic:zCVob", "0"), ("%", "0")] {
        let data = format!(
            "ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 0\nEND\nobjects 1\nEND\n\n\
             [% {class} {version} 0]\nvalue=int:1\n[]\n"
        );
        let mut archive = Archive::new(data.as_bytes()).unwrap();
        assert!(Vobs::read(&mut archive, Gothic1).unwrap().is_none(), "{class}");
        assert!(archive.is_eof());
    }
}

#[test]
fn save_games_are_not_read_yet() {
    let data =
        b"ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 1\nEND\nobjects 0\nEND\n";
    let mut archive = Archive::new(data).unwrap();
    assert!(Vobs::read(&mut archive, Gothic1).is_err());
}
