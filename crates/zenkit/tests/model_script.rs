#![cfg(feature = "model-script")]

use std::path::Path;

use zenkit::model_script::AnimationFlags;
use zenkit::model_script::Direction;
use zenkit::model_script::EventType;
use zenkit::model_script::ModelScript;
use zenkit::model_script::Skeleton;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples").join(name)).unwrap()
}

#[test]
fn line_endings() {
    for eol in ["\n", "\r\n"] {
        let src = [
            "Model (\"test\") {",
            "registerMesh(\"Some File(1).asc\")",
            "registerMesh(\"MissingQuote.asc",
            "registerMesh(\"MissingQuoteAndParen.asc)",
            "registerMesh(\"After.asc\")",
            "}",
            "",
        ]
        .join(eol);
        let script = ModelScript::from_bytes(src.as_bytes()).unwrap();
        assert_eq!(
            script.meshes,
            ["Some File(1).asc", "MissingQuote.asc", "MissingQuoteAndParen.asc", "After.asc"],
            "{eol:?}"
        );
    }
}

#[test]
fn source() {
    let script = ModelScript::from_bytes(&sample("waran.mds")).unwrap();

    assert_eq!(
        script.skeleton,
        Some(Skeleton { name: "TestModelMesh.asc".into(), disable_mesh: true })
    );
    assert_eq!(
        script.meshes,
        [
            "AnotherTestModelMesh1.asc",
            "AnotherTestModelMesh2.asc",
            "AnotherTestModelMesh3.asc",
            "AnotherTestModelMesh4.asc",
            "AnotherTestModelMesh5.asc",
            "AnotherTestModelMesh(6).asc",
        ]
    );

    assert_eq!(script.animations.len(), 3);
    let a = &script.animations[0];
    assert_eq!(a.name, "aniName1");
    assert_eq!(a.layer, 111);
    assert_eq!(a.next, "aniNext1");
    assert_eq!(a.blend_in, 4.2);
    assert_eq!(a.blend_out, 0.5);
    assert_eq!(a.flags, AnimationFlags::MOVE | AnimationFlags::IDLE);
    assert_eq!(a.model, "aniModel1");
    assert_eq!(a.direction, Direction::Forward);
    assert_eq!((a.first_frame, a.last_frame), (221, -331));
    assert_eq!(a.fps, 25.0);
    assert_eq!(a.speed, 0.0);
    assert_eq!(a.collision_volume_scale, 0.2);
    assert!(a.particles.is_empty() && a.particle_stops.is_empty());
    assert!(a.sounds.is_empty() && a.ground_sounds.is_empty());
    assert!(a.events.is_empty() && a.morphs.is_empty() && a.tremors.is_empty());

    let a = &script.animations[1];
    assert_eq!(a.name, "aniName2");
    assert_eq!(a.layer, 112);
    assert_eq!(a.next, "aniNext2");
    assert_eq!((a.blend_in, a.blend_out), (9.0, 0.0));
    assert_eq!(a.flags, AnimationFlags::MOVE);
    assert_eq!(a.model, "aniModel2");
    assert_eq!(a.direction, Direction::Backward);
    assert_eq!((a.first_frame, a.last_frame), (222, 332));
    assert_eq!((a.fps, a.collision_volume_scale), (25.0, 1.0));

    assert_eq!(a.events.len(), 3);
    assert_eq!(a.events[0].frame, 0);
    assert_eq!(a.events[0].kind, EventType::DropTorch);
    assert!(a.events[0].attached);
    assert_eq!(a.events[1].frame, 1);
    assert_eq!(a.events[1].kind, EventType::ComboWindow);
    assert_eq!(a.events[1].frames, [1, 2, 3, 4, 5]);
    assert!(!a.events[1].attached);
    assert_eq!(a.events[2].frame, 0);
    assert_eq!(a.events[2].kind, EventType::CreateItem);
    assert_eq!(a.events[2].slot, "eventSlot");
    assert_eq!(a.events[2].item, "eventItem");
    assert!(!a.events[2].attached);

    assert_eq!(a.sounds.len(), 3);
    assert_eq!((a.sounds[0].frame, a.sounds[0].name.as_str()), (3, "sfxName1"));
    assert!(a.sounds[0].empty_slot);
    assert_eq!(a.sounds[0].range, 1000.0);
    assert_eq!((a.sounds[1].frame, a.sounds[1].name.as_str()), (4, "sfxName2"));
    assert_eq!(a.sounds[1].range, 67.4);
    assert!(!a.sounds[1].empty_slot);
    assert_eq!(a.sounds[2].range, 98.1);
    assert!(a.sounds[2].empty_slot);

    assert_eq!(a.ground_sounds.len(), 1);
    assert_eq!(a.ground_sounds[0].frame, 5);
    assert_eq!(a.ground_sounds[0].name, "sfxGrndName");
    assert!(!a.ground_sounds[0].empty_slot);

    assert_eq!(a.particles.len(), 3);
    let p = &a.particles;
    assert_eq!((p[0].frame, p[0].index, p[0].attached), (6, 0, true));
    assert_eq!((p[0].name.as_str(), p[0].position.as_str()), ("pfxName1", "pfxPosition1"));
    assert_eq!((p[1].frame, p[1].index, p[1].attached), (7, 991, false));
    assert_eq!((p[1].name.as_str(), p[1].position.as_str()), ("pfxName2", "pfxPosition2"));
    assert_eq!((p[2].frame, p[2].index, p[2].attached), (9, 991, true));
    assert_eq!((p[2].name.as_str(), p[2].position.as_str()), ("pfxName3", "pfxPosition3"));

    assert_eq!(a.particle_stops.len(), 1);
    assert_eq!((a.particle_stops[0].frame, a.particle_stops[0].index), (8, 992));

    assert_eq!(a.morphs.len(), 2);
    assert_eq!((a.morphs[0].frame, a.morphs[0].animation.as_str()), (9, "mmAni1"));
    assert!(a.morphs[0].node.is_empty());
    assert_eq!((a.morphs[1].frame, a.morphs[1].animation.as_str()), (10, "mmAni2"));
    assert_eq!(a.morphs[1].node, "mmNode");

    assert_eq!(a.tremors.len(), 1);
    assert_eq!(a.tremors[0].frame, 11);
    assert_eq!(a.tremors[0].values, [881, 882, 883, 884]);

    // Its events have no braces around them.
    let a = &script.animations[2];
    assert_eq!(a.name, "aniName3");
    assert_eq!(a.layer, 113);
    assert_eq!(a.next, "aniNext3");
    assert_eq!((a.blend_in, a.blend_out), (8.0, 0.0));
    assert_eq!(a.flags, AnimationFlags::MOVE);
    assert_eq!(a.model, "aniModel3");
    assert_eq!(a.direction, Direction::Forward);
    assert_eq!((a.first_frame, a.last_frame), (223, 333));
    assert_eq!(a.events.len(), 1);
    assert_eq!(a.events[0].kind, EventType::CreateItem);
    assert_eq!(a.events[0].slot, "eventSlot");
    assert_eq!(a.events[0].item, "eventItem");
    assert_eq!(a.ground_sounds.len(), 1);
    assert_eq!(a.ground_sounds[0].frame, 5);
    assert_eq!(a.ground_sounds[0].name, "sfxGrndName");

    let names: Vec<_> = script.blends.iter().map(|b| (b.name.as_str(), b.next.as_str())).collect();
    assert_eq!(
        names,
        [("blendName1", "blendNext1"), ("blendName2", "blendNext2"), ("blendName3", "blendNext3")]
    );
    assert_eq!((script.blends[2].blend_in, script.blends[2].blend_out), (223.1, 333.1));

    assert_eq!(script.aliases.len(), 2);
    let a = &script.aliases[0];
    assert_eq!((a.name.as_str(), a.layer, a.next.as_str()), ("aliasName1", 114, "aliasNext1"));
    assert_eq!((a.blend_in, a.blend_out), (100.1, 200.2));
    assert_eq!(a.flags, AnimationFlags::ROTATE | AnimationFlags::QUEUE);
    assert_eq!((a.alias.as_str(), a.direction), ("aliasAlias1", Direction::Forward));
    let a = &script.aliases[1];
    assert_eq!((a.name.as_str(), a.layer, a.next.as_str()), ("aliasName2", 115, "aliasNext2"));
    assert_eq!((a.blend_in, a.blend_out), (101.1, 201.2));
    assert_eq!(a.flags, AnimationFlags::FLY);
    assert_eq!((a.alias.as_str(), a.direction), ("aliasAlias2", Direction::Backward));

    assert_eq!(script.combinations.len(), 2);
    let c = &script.combinations[0];
    assert_eq!((c.name.as_str(), c.layer, c.next.as_str()), ("combName1", 116, "combNext1"));
    assert_eq!((c.blend_in, c.blend_out), (102.1, 202.2));
    assert_eq!(c.flags, AnimationFlags::MOVE);
    assert_eq!((c.model.as_str(), c.last_frame), ("combModel1", 226));
    let c = &script.combinations[1];
    assert_eq!((c.name.as_str(), c.layer, c.next.as_str()), ("combName2", 117, "combNext2"));
    assert_eq!((c.blend_in, c.blend_out), (103.1, 203.2));
    assert_eq!(c.flags, AnimationFlags::IDLE);
    assert_eq!((c.model.as_str(), c.last_frame), ("combModel2", 227));

    assert_eq!(script.disabled_animations, ["disable1", "disable2"]);
    assert_eq!(script.hit_limbs, ["tag1", "tag2"]);
}

#[test]
fn compiled() {
    let script = ModelScript::from_bytes(&sample("waran.msb")).unwrap();

    assert_eq!(script.skeleton, Some(Skeleton { name: "WAR_BODY".into(), disable_mesh: true }));
    assert_eq!(script.meshes, ["WAR_BODY.ASC", "WAR_FIRE_BODY.ASC"]);

    assert_eq!(script.animations.len(), 62);
    let a = &script.animations[0];
    assert_eq!(a.name, "T_FISTRUN_2_FISTRUNL");
    assert_eq!(a.layer, 1);
    assert_eq!(a.next, "S_FISTRUNL");
    assert_eq!((a.blend_in, a.blend_out), (0.0, 0.0));
    assert_eq!(a.flags, AnimationFlags::MOVE);
    assert_eq!(a.model, "WARAN_RUN_KM01.ASC");
    assert_eq!(a.direction, Direction::Forward);
    assert_eq!((a.first_frame, a.last_frame), (1, 8));
    assert_eq!((a.fps, a.speed, a.collision_volume_scale), (25.0, 0.0, 1.0));
    assert_eq!(a.ground_sounds.len(), 1);
    assert_eq!((a.ground_sounds[0].frame, a.ground_sounds[0].name.as_str()), (5, "SNEAK"));
    assert!(a.sounds.is_empty() && a.particles.is_empty() && a.particle_stops.is_empty());
    assert!(a.morphs.is_empty() && a.events.is_empty() && a.tremors.is_empty());

    assert_eq!(script.aliases.len(), 38);
    let a = &script.aliases[0];
    assert_eq!((a.name.as_str(), a.layer, a.next.as_str()), ("S_FISTRUN", 1, "S_FISTRUN"));
    assert_eq!((a.blend_in, a.blend_out), (0.0, 0.0));
    assert_eq!(a.flags, AnimationFlags::MOVE | AnimationFlags::IDLE);
    assert_eq!((a.alias.as_str(), a.direction), ("S_FISTWALK", Direction::Forward));

    assert_eq!(script.blends.len(), 17);
    let b = &script.blends[0];
    assert_eq!((b.name.as_str(), b.next.as_str()), ("T_FISTRUNR_2_FISTRUN", "S_FISTRUN"));
    assert_eq!((b.blend_in, b.blend_out), (0.0, 0.0));

    assert_eq!(script.combinations.len(), 1);
    let c = &script.combinations[0];
    assert_eq!((c.name.as_str(), c.layer, c.next.as_str()), ("T_LOOK", 9, "T_LOOK"));
    assert_eq!((c.blend_in, c.blend_out), (0.3, 0.3));
    assert_eq!(c.flags, AnimationFlags::MOVE);
    assert_eq!((c.model.as_str(), c.last_frame), ("C_LOOK_", 9));

    assert!(script.disabled_animations.is_empty());

    let events = &script.animations[47].events;
    assert_eq!(events.len(), 4);
    assert_eq!(events[0].kind, EventType::HitLimb);
    assert_eq!(events[0].slot, "BIP01 TONGUE04");
    assert_eq!(events[3].frame, 0);
    assert_eq!(events[3].kind, EventType::ComboWindow);
    assert_eq!(events[3].frames, [8, 15]);
}

#[test]
fn truncated_scripts_never_panic() {
    let source = sample("waran.mds");
    for len in 0..source.len() {
        let _ = ModelScript::from_bytes(&source[..len]);
    }
    let compiled = sample("waran.msb");
    for len in (0..compiled.len()).step_by(7) {
        let _ = ModelScript::from_bytes(&compiled[..len]);
    }
}
