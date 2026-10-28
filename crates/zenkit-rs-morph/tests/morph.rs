// The literals are copied from ZenKit's tests so the two stay easy to compare.
#![allow(clippy::excessive_precision)]

use std::path::Path;

use zenkit_rs_core::math::Vec3;
use zenkit_rs_morph::MorphMesh;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)).unwrap()
}

#[test]
fn bow() {
    let mesh = MorphMesh::from_bytes(&sample("morph0.mmb")).unwrap();

    assert_eq!(mesh.name, "ITRWSMALLBOW");
    assert_eq!(mesh.morph_positions.len(), 28);
    assert_eq!(mesh.morph_positions[0], Vec3::new(-40.9461403, -1.73638999, -26.7512894));
    assert_eq!(mesh.morph_positions[1], Vec3::new(-29.6147194, -1.97599006, -9.19756984));

    assert_eq!(mesh.animations.len(), 4);
    let anim = &mesh.animations[1];
    assert_eq!(anim.name, "S_SHOOT");
    assert_eq!(anim.layer, 1);
    assert_eq!(anim.blend_in, 0.0100000007);
    assert_eq!(anim.blend_out, -0.0100000007);
    assert_eq!(anim.duration, 400.0);
    assert_eq!(anim.speed, 0.0250000004);
    assert_eq!(anim.flags, 0);
    assert_eq!(anim.vertices, [25, 26, 27]);
    assert_eq!(anim.frame_count, 10);
    assert_eq!(anim.samples.len(), 30);
    assert_eq!(anim.samples[0], Vec3::new(0.519770026, 0.0, 1.27206039));
    assert_eq!(anim.samples[9], Vec3::ZERO);
    assert_eq!(anim.samples[19], Vec3::new(-8.51126003, 0.0, -20.8299408));
    assert_eq!(anim.frame(6).unwrap()[1], anim.samples[19]);

    assert_eq!(mesh.sources.len(), 4);
    let d = mesh.sources[1].date;
    assert_eq!((d.year, d.month, d.day, d.hour, d.minute, d.second), (2000, 5, 8, 9, 13, 58));
    assert_eq!(mesh.sources[1].file_name, "ITRWSMALLBOWSHOOT.ASC");
}

#[test]
fn truncated_files_fail_cleanly() {
    let data = sample("morph0.mmb");
    for len in 0..data.len() {
        let _ = MorphMesh::from_bytes(&data[..len]);
    }
}
