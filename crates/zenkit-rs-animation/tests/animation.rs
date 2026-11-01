// The literals are copied from ZenKit's tests so the two stay easy to compare.
#![allow(clippy::excessive_precision)]

use std::path::Path;

use zenkit_rs_animation::AnimationEvent;
use zenkit_rs_animation::AnimationEventType;
use zenkit_rs_animation::AnimationSample;
use zenkit_rs_animation::ModelAnimation;
use zenkit_rs_core::math::Quat;
use zenkit_rs_core::math::Vec3;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)).unwrap()
}

const NODE_INDICES: [u32; 25] =
    [0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 15, 16, 17, 18, 19, 26, 27, 28, 29, 30, 31, 32, 33];

/// ZenKit's `Quat` takes `w` first.
fn quat(w: f32, x: f32, y: f32, z: f32) -> Quat {
    Quat::from_xyzw(x, y, z, w)
}

fn fist_run(game: &str) -> ModelAnimation {
    ModelAnimation::from_bytes(&sample(&format!("{game}/HUMANS-S_FISTRUN.MAN"))).unwrap()
}

fn check(anim: &ModelAnimation) {
    assert_eq!(anim.bbox.max, Vec3::new(46.33139419555664, 67.0935287475586, 49.88602828979492));
    assert_eq!(anim.checksum, 3325331650);
    assert!(anim.events.is_empty());
    assert_eq!(anim.fps, 10.0);
    assert_eq!(anim.fps_source, 25.0);
    assert_eq!(anim.frame_count, 20);
    assert_eq!(anim.layer, 1);
    assert_eq!(anim.name, "S_FISTRUN");
    assert_eq!(anim.next, "S_FISTRUN");
    assert_eq!(anim.node_count, 25);
    assert_eq!(anim.node_indices, NODE_INDICES);
    assert_eq!(anim.sample_position_min, -9.01021957397461);
    assert_eq!(anim.sample_position_scale, 0.001491763861849904);

    assert_eq!(anim.samples.len(), 25 * 20);
    assert_eq!(
        anim.samples[0],
        AnimationSample {
            position: Vec3::new(12.635274887084961, 88.75251770019531, -1.093428611755371),
            rotation: quat(0.7771535515785217, 0.0, 0.6293110251426697, 0.0),
        }
    );
    let last = AnimationSample {
        position: Vec3::new(12.626323699951172, -0.00145721435546875, 22.643518447875977),
        rotation: quat(0.7071319222450256, 0.0, 0.70708167552948, 0.0),
    };
    assert_eq!(anim.samples[249], last);
    assert_eq!(anim.samples[499], last);
    assert_eq!(anim.frame(19).unwrap()[24], last);
    assert!(anim.frame(20).is_none());

    assert_eq!(anim.source_path, "\\_WORK\\DATA\\ANIMS\\HUM_AMB_FISTRUN_M01.ASC");
    assert_eq!(
        anim.source_script,
        "\t\t\tANI\t\t\t(\"S_FISTRUN\"\t\t\t\t1\t\"S_FISTRUN\"\t\t0.0 0.1 MI\t\"HUM_AMB_FISTRUN_M01.ASC\"\tF   1\t50\tFPS:10)"
    );
}

#[test]
fn gothic1() {
    let anim = fist_run("G1");
    check(&anim);
    assert_eq!(
        anim.bbox.min,
        Vec3::new(-51.09061050415039, -94.02226257324219, -31.280731201171875)
    );
}

#[test]
fn gothic2() {
    let anim = fist_run("G2");
    check(&anim);
    assert_eq!(
        anim.bbox.min,
        Vec3::new(-51.090614318847656, -94.02226257324219, -31.280733108520508)
    );
}

#[test]
fn truncated_files_fail_cleanly() {
    let data = sample("G2/HUMANS-S_FISTRUN.MAN");
    for len in 0..data.len() {
        let _ = ModelAnimation::from_bytes(&data[..len]);
    }
}

#[test]
fn both_games_write_back_byte_for_byte() {
    for game in ["G1", "G2"] {
        let mut data = sample(&format!("{game}/HUMANS-S_FISTRUN.MAN"));
        let anim = ModelAnimation::from_bytes(&data).unwrap();
        // The source date's two bytes of padding, which are as uninitialised as the rest of the
        // date and written as zeros. The date starts after the marker, the header chunk and the
        // source chunk's header.
        let padding = 6 + 6 + 74 + 6 + 14;
        data[padding..padding + 2].fill(0);
        assert!(anim.to_bytes().unwrap() == data, "{game}");
    }
}

#[test]
fn animations_made_in_code_read_back() {
    let rotations = [
        Quat::IDENTITY,
        Quat::from_rotation_y(1.0),
        Quat::from_rotation_x(-2.5),
        // A negative `w`, stored as the same rotation with a positive one.
        -Quat::from_rotation_z(0.5),
    ];
    let mut anim = ModelAnimation {
        name: "S_TEST".into(),
        next: "S_TEST".into(),
        frame_count: 2,
        node_count: 2,
        fps: 25.0,
        node_indices: vec![0, 3],
        samples: (0..4)
            .map(|i| AnimationSample {
                position: Vec3::new(i as f32 * 10.0, -5.0, 2.5),
                rotation: rotations[i],
            })
            .collect(),
        ..ModelAnimation::default()
    };
    anim.fit_sample_positions();
    assert_eq!((anim.sample_position_min, anim.sample_position_scale), (-5.0, 35.0 / 65535.0));

    let back = ModelAnimation::from_bytes(&anim.to_bytes().unwrap()).unwrap();
    for (got, want) in back.samples.iter().zip(&anim.samples) {
        assert!((got.position - want.position).abs().max_element() < 1e-3, "{got:?} {want:?}");
        assert!(got.rotation.dot(want.rotation).abs() > 0.99999, "{got:?} {want:?}");
    }

    anim.samples[0].position.x = 100.0;
    let error = anim.to_bytes().unwrap_err().to_string();
    assert!(error.contains("sample 0 has position"), "{error}");
}

#[test]
fn events_read_back() {
    let mut anim = fist_run("G2");
    anim.events = vec![AnimationEvent {
        kind: AnimationEventType::SoundEffect,
        frame: 7,
        tag: "DEF_STEP".into(),
        content: ["STEP_SAND".into(), "A".into(), "B".into(), "C".into()],
        values: [1.0, 2.0, 3.0, 4.0],
        probability: 0.5,
    }];
    let back = ModelAnimation::from_bytes(&anim.to_bytes().unwrap()).unwrap();
    assert_eq!(back.events, anim.events);
}
