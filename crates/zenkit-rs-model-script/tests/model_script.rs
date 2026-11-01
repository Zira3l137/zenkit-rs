use std::path::Path;

use zenkit_rs_model_script::*;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)).unwrap()
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

/// The events of an animation, alias, blend or combination, by kind.
macro_rules! expect {
    ($value:expr, $pattern:pat => $out:expr) => {
        match $value {
            $pattern => $out,
            other => panic!("expected {}, found {other:?}", stringify!($pattern)),
        }
    };
}

fn tag(frame: i32, name: &str, arguments: &[&str], attached: bool) -> Event {
    Event::Tag(EventTag {
        frame,
        name: name.into(),
        arguments: arguments.iter().map(|a| a.to_string()).collect(),
        attached,
    })
}

fn sound(frame: i32, name: &str, range: f32, empty_slot: bool) -> SoundEffect {
    SoundEffect { frame, name: name.into(), range, empty_slot }
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
    assert_eq!(script.source, None);
    assert_eq!(script.name, "TestModel");

    let kinds: Vec<_> = script
        .statements
        .iter()
        .map(|s| match s {
            Statement::Animation(a) => format!("ani {}", a.name),
            Statement::Alias(a) => format!("alias {}", a.name),
            Statement::Blend(b) => format!("blend {}", b.name),
            Statement::Combination(c) => format!("comb {}", c.name),
            Statement::Disable(name) => format!("disable {name}"),
            Statement::ModelTag(t) => format!("tag {}", t.argument(0)),
            Statement::Unknown(c) => format!("unknown {:#x}", c.id),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "ani aniName1",
            "ani aniName2",
            "blend blendName1",
            "blend blendName2",
            "ani aniName3",
            "blend blendName3",
            "alias aliasName1",
            "alias aliasName2",
            "comb combName1",
            "comb combName2",
            "disable disable1",
            "disable disable2",
            "tag tag1",
            "tag tag2",
        ]
    );

    let animations: Vec<_> = script.animations().collect();
    let a = animations[0];
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
    assert!(a.events.is_empty());

    let a = animations[1];
    assert_eq!(a.name, "aniName2");
    assert_eq!(a.layer, 112);
    assert_eq!(a.next, "aniNext2");
    assert_eq!((a.blend_in, a.blend_out), (9.0, 0.0));
    assert_eq!(a.flags, AnimationFlags::MOVE);
    assert_eq!(a.model, "aniModel2");
    assert_eq!(a.direction, Direction::Backward);
    assert_eq!((a.first_frame, a.last_frame), (222, 332));
    assert_eq!((a.fps, a.collision_volume_scale), (25.0, 1.0));
    assert_eq!(
        a.events,
        [
            tag(0, "DEF_DROP_TORCH", &[], true),
            tag(1, "DEF_WINDOW", &["1 2 3 4 5"], false),
            tag(0, "DEF_CREATE_ITEM", &["eventSlot", "eventItem"], false),
            Event::Sound(sound(3, "sfxName1", 1000.0, true)),
            Event::Sound(sound(4, "sfxName2", 67.4, false)),
            Event::Sound(sound(4, "sfxName3", 98.1, true)),
            Event::GroundSound(sound(5, "sfxGrndName", 1000.0, false)),
            Event::Particle(ParticleEffect {
                frame: 6,
                index: 0,
                name: "pfxName1".into(),
                position: "pfxPosition1".into(),
                attached: true
            }),
            Event::Particle(ParticleEffect {
                frame: 7,
                index: 991,
                name: "pfxName2".into(),
                position: "pfxPosition2".into(),
                attached: false
            }),
            Event::Particle(ParticleEffect {
                frame: 9,
                index: 991,
                name: "pfxName3".into(),
                position: "pfxPosition3".into(),
                attached: true
            }),
            Event::ParticleStop(ParticleEffectStop { frame: 8, index: 992 }),
            Event::Morph(MorphAnimation {
                frame: 9,
                animation: "mmAni1".into(),
                ..Default::default()
            }),
            Event::Morph(MorphAnimation {
                frame: 10,
                animation: "mmAni2".into(),
                node: "mmNode".into(),
                ..Default::default()
            }),
            Event::Tremor(CameraTremor {
                frame: 11,
                range: 881,
                duration: 882,
                min_amplifier: 883,
                max_amplifier: 884
            }),
        ]
    );
    let window = expect!(&a.events[1], Event::Tag(t) => t);
    assert_eq!((window.kind(), window.frames()), (EventType::ComboWindow, vec![1, 2, 3, 4, 5]));

    // Its events have no braces around them.
    let a = animations[2];
    assert_eq!(a.name, "aniName3");
    assert_eq!(a.layer, 113);
    assert_eq!(a.next, "aniNext3");
    assert_eq!((a.blend_in, a.blend_out), (8.0, 0.0));
    assert_eq!(a.flags, AnimationFlags::MOVE);
    assert_eq!(a.model, "aniModel3");
    assert_eq!(a.direction, Direction::Forward);
    assert_eq!((a.first_frame, a.last_frame), (223, 333));
    assert_eq!(
        a.events,
        [
            tag(0, "DEF_CREATE_ITEM", &["eventSlot", "eventItem"], false),
            Event::GroundSound(sound(5, "sfxGrndName", 1000.0, false)),
        ]
    );

    let blends: Vec<_> = script.blends().collect();
    let names: Vec<_> = blends.iter().map(|b| (b.name.as_str(), b.next.as_str())).collect();
    assert_eq!(
        names,
        [("blendName1", "blendNext1"), ("blendName2", "blendNext2"), ("blendName3", "blendNext3")]
    );
    assert_eq!((blends[2].blend_in, blends[2].blend_out), (223.1, 333.1));
    assert_eq!(blends[2].events, [Event::Sound(sound(4, "sfxName2", 67.4, false))]);

    let aliases: Vec<_> = script.aliases().collect();
    assert_eq!(aliases.len(), 2);
    let a = aliases[0];
    assert_eq!((a.name.as_str(), a.layer, a.next.as_str()), ("aliasName1", 114, "aliasNext1"));
    assert_eq!((a.blend_in, a.blend_out), (100.1, 200.2));
    assert_eq!(a.flags, AnimationFlags::ROTATE | AnimationFlags::QUEUE);
    assert_eq!((a.alias.as_str(), a.direction), ("aliasAlias1", Direction::Forward));
    let a = aliases[1];
    assert_eq!((a.name.as_str(), a.layer, a.next.as_str()), ("aliasName2", 115, "aliasNext2"));
    assert_eq!((a.blend_in, a.blend_out), (101.1, 201.2));
    assert_eq!(a.flags, AnimationFlags::FLY);
    assert_eq!((a.alias.as_str(), a.direction), ("aliasAlias2", Direction::Backward));

    let combinations: Vec<_> = script.combinations().collect();
    assert_eq!(combinations.len(), 2);
    let c = combinations[0];
    assert_eq!((c.name.as_str(), c.layer, c.next.as_str()), ("combName1", 116, "combNext1"));
    assert_eq!((c.blend_in, c.blend_out), (102.1, 202.2));
    assert_eq!(c.flags, AnimationFlags::MOVE);
    assert_eq!((c.model.as_str(), c.last_frame), ("combModel1", 226));
    assert_eq!(c.events, [tag(0, "DEF_DROP_TORCH", &[], true)]);
    let c = combinations[1];
    assert_eq!((c.name.as_str(), c.layer, c.next.as_str()), ("combName2", 117, "combNext2"));
    assert_eq!((c.blend_in, c.blend_out), (103.1, 203.2));
    assert_eq!(c.flags, AnimationFlags::IDLE);
    assert_eq!((c.model.as_str(), c.last_frame), ("combModel2", 227));

    assert!(script.disabled_animations().eq(["disable1", "disable2"]));
    let limbs: Vec<_> = script.model_tags().map(|t| (t.kind(), t.argument(0))).collect();
    assert_eq!(limbs, [(EventType::HitLimb, "tag1"), (EventType::HitLimb, "tag2")]);
}

#[test]
fn compiled() {
    let script = ModelScript::from_bytes(&sample("waran.msb")).unwrap();

    assert_eq!(script.skeleton, Some(Skeleton { name: "WAR_BODY".into(), disable_mesh: true }));
    assert_eq!(script.meshes, ["WAR_BODY.ASC", "WAR_FIRE_BODY.ASC"]);
    let source = script.source.as_ref().unwrap();
    assert_eq!(source.path, "\\_WORK\\DATA\\ANIMS\\WARAN.MDS");
    assert_eq!((source.date.year, source.date.month, source.date.day), (2002, 8, 28));
    assert_eq!(script.name, "WARAN");

    assert_eq!(script.statements.len(), 62 + 38 + 17 + 1);
    // The script starts with an alias, as written.
    let first = expect!(&script.statements[0], Statement::Alias(a) => a);
    assert_eq!(
        (first.name.as_str(), first.layer, first.next.as_str()),
        ("S_FISTRUN", 1, "S_FISTRUN")
    );
    assert_eq!((first.blend_in, first.blend_out), (0.0, 0.0));
    assert_eq!(first.flags, AnimationFlags::MOVE | AnimationFlags::IDLE);
    assert_eq!((first.alias.as_str(), first.direction), ("S_FISTWALK", Direction::Forward));

    let animations: Vec<_> = script.animations().collect();
    assert_eq!(animations.len(), 62);
    let a = animations[0];
    assert_eq!(a.name, "T_FISTRUN_2_FISTRUNL");
    assert_eq!(a.layer, 1);
    assert_eq!(a.next, "S_FISTRUNL");
    assert_eq!((a.blend_in, a.blend_out), (0.0, 0.0));
    assert_eq!(a.flags, AnimationFlags::MOVE);
    assert_eq!(a.model, "WARAN_RUN_KM01.ASC");
    assert_eq!(a.direction, Direction::Forward);
    assert_eq!((a.first_frame, a.last_frame), (1, 8));
    assert_eq!((a.fps, a.speed, a.collision_volume_scale), (25.0, 0.0, 1.0));
    assert_eq!(a.events, [Event::GroundSound(sound(5, "SNEAK", 1000.0, true))]);

    // A sound, then the ground sounds after it, in file order.
    let kinds: Vec<_> = animations[1]
        .events
        .iter()
        .map(|e| expect!(e, Event::Sound(s) | Event::GroundSound(s) => s.frame))
        .collect();
    assert_eq!(kinds, [10, 9, 15]);

    assert_eq!(script.aliases().count(), 38);
    let b = script.blends().next().unwrap();
    assert_eq!((b.name.as_str(), b.next.as_str()), ("T_FISTRUNR_2_FISTRUN", "S_FISTRUN"));
    assert_eq!((b.blend_in, b.blend_out), (0.0, 0.0));
    assert_eq!(script.blends().count(), 17);

    let c = script.combinations().next().unwrap();
    assert_eq!((c.name.as_str(), c.layer, c.next.as_str()), ("T_LOOK", 9, "T_LOOK"));
    assert_eq!((c.blend_in, c.blend_out), (0.3, 0.3));
    assert_eq!(c.flags, AnimationFlags::MOVE);
    assert_eq!((c.model.as_str(), c.last_frame), ("C_LOOK_", 9));
    assert_eq!(script.disabled_animations().count(), 0);

    let events = &animations[47].events;
    assert_eq!(
        events,
        &[
            tag(0, "DEF_HIT_LIMB", &["BIP01 TONGUE04"], false),
            tag(0, "DEF_OPT_FRAME", &["8"], false),
            tag(0, "DEF_HIT_END", &["15"], false),
            tag(0, "DEF_WINDOW", &["8\t15"], false),
            Event::GroundSound(sound(3, "SNEAK", 1000.0, true)),
            Event::Sound(sound(7, "WAR_ATTACK", 2500.0, false)),
            Event::GroundSound(sound(11, "SNEAK", 1000.0, true)),
        ]
    );
    let window = expect!(&events[3], Event::Tag(t) => t);
    assert_eq!((window.kind(), window.frames()), (EventType::ComboWindow, vec![8, 15]));
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

#[test]
fn compiled_writes_back_byte_for_byte() {
    let data = sample("waran.msb");
    let script = ModelScript::from_bytes(&data).unwrap();
    let written = script.to_msb().unwrap();
    assert_eq!(written.len(), data.len());
    assert!(written == data);
}

#[test]
fn source_compiles_to_what_it_reads() {
    let script = ModelScript::from_bytes(&sample("waran.mds")).unwrap();
    let back = ModelScript::from_bytes(&script.to_msb().unwrap()).unwrap();
    let mut expected = script.clone();
    expected.source = Some(ScriptSource::default());
    // Compiled scripts take their name from the source file, which this one lacks.
    expected.name = String::new();
    // Compiled scripts do not store ATTACH on tags.
    for statement in &mut expected.statements {
        let events = match statement {
            Statement::Animation(a) => &mut a.events,
            Statement::Alias(a) => &mut a.events,
            Statement::Blend(b) => &mut b.events,
            Statement::Combination(c) => &mut c.events,
            _ => continue,
        };
        for event in events {
            if let Event::Tag(tag) = event {
                tag.attached = false;
            }
        }
    }
    assert_eq!(back, expected);
}

#[test]
fn scripts_that_cannot_be_compiled_fail() {
    let mut script = ModelScript::default();
    script.statements.push(Statement::ModelTag(EventTag {
        name: "DEF_HIT_LIMB".into(),
        arguments: vec!["A".into(); 5],
        ..EventTag::default()
    }));
    let error = script.to_msb().unwrap_err().to_string();
    assert!(error.contains("5 arguments"), "{error}");

    let script = ModelScript { meshes: vec!["A\nB".into()], ..ModelScript::default() };
    assert!(script.to_msb().unwrap_err().to_string().contains("line break"));
}

#[test]
fn compiled_scripts_decompile_to_what_they_read() {
    let script = ModelScript::from_bytes(&sample("waran.msb")).unwrap();
    let source = script.to_mds().unwrap();
    let back = ModelScript::from_bytes(&source).unwrap();
    assert_eq!(back, ModelScript { source: None, ..script.clone() });

    // Laid out like the game's scripts.
    let text = String::from_utf8(source).unwrap();
    let start = [
        "Model (\"WARAN\")",
        "{",
        "\tmeshAndTree (\"WAR_BODY\" DONT_USE_MESH)",
        "",
        "\tregisterMesh (\"WAR_BODY.ASC\")",
        "\tregisterMesh (\"WAR_FIRE_BODY.ASC\")",
        "",
        "\taniEnum",
        "\t{",
        "\t\taniAlias (\"S_FISTRUN\" 1 \"S_FISTRUN\" 0.0 0.0 MI \"S_FISTWALK\" F)",
        "\t\tani (\"T_FISTRUN_2_FISTRUNL\" 1 \"S_FISTRUNL\" 0.0 0.0 M. \"WARAN_RUN_KM01.ASC\" F 1 8)",
        "\t\t{",
        "\t\t\t*eventSFXGrnd (5 \"SNEAK\" EMPTY_SLOT)",
        "\t\t}",
        "",
    ]
    .join("\r\n");
    assert!(text.starts_with(&start), "{text}");
    assert!(text.contains("\t\t\t*eventTag (0 \"DEF_WINDOW\" \"8\t15\")\r\n"), "{text}");
    assert!(text.contains("\t\t\t*eventSFX (7 \"WAR_ATTACK\" R:2500)\r\n"), "{text}");
    assert!(text.ends_with("\t}\r\n}\r\n"), "{text}");
}

#[test]
fn source_scripts_write_back_what_they_read() {
    let script = ModelScript::from_bytes(&sample("waran.mds")).unwrap();
    let back = ModelScript::from_bytes(&script.to_mds().unwrap()).unwrap();
    assert_eq!(back, script);

    let mut morph = ModelScript { name: "HEAD".into(), ..ModelScript::default() };
    morph.statements.push(Statement::Animation(Animation {
        name: "T_HURT".into(),
        fps: 10.0,
        speed: 0.5,
        collision_volume_scale: 1.0,
        events: vec![Event::Morph(MorphAnimation {
            frame: 1,
            animation: "R_HURT".into(),
            intensity: 0.75,
            hold_time: 2.0,
            ..MorphAnimation::default()
        })],
        ..Animation::default()
    }));
    let source = morph.to_mds().unwrap();
    let text = String::from_utf8_lossy(&source);
    assert!(text.contains("F 0 0 FPS:10 SPD:0.5)\r\n"), "{text}");
    assert!(text.contains("*eventMMStartAni (1 \"R_HURT\" I:0.75 H:2)\r\n"), "{text}");
    assert_eq!(ModelScript::from_bytes(&source).unwrap(), morph);
}

#[test]
fn scripts_without_a_source_form_fail() {
    let mut script = ModelScript::default();
    script.statements.push(Statement::Unknown(RawChunk { id: 0xF510, data: vec![0; 4] }));
    let error = script.to_mds().unwrap_err().to_string();
    assert!(error.contains("0xf510 has no source form"), "{error}");

    let script = ModelScript { meshes: vec!["A\"B.ASC".into()], ..ModelScript::default() };
    assert!(script.to_mds().unwrap_err().to_string().contains("quote"));

    let mut script = ModelScript::default();
    script
        .statements
        .push(Statement::Blend(AnimationBlend { blend_in: f32::NAN, ..AnimationBlend::default() }));
    assert!(script.to_mds().unwrap_err().to_string().contains("not a number"));
}
