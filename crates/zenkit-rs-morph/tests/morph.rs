// The literals are copied from ZenKit's tests so the two stay easy to compare.
#![allow(clippy::excessive_precision)]

use std::path::Path;

use zenkit_rs_core::GameVersion;
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

#[test]
fn bow_writes_back_byte_for_byte() {
    let mut data = sample("morph0.mmb");
    let mesh = MorphMesh::from_bytes(&data).unwrap();
    // The two bytes of padding after each wedge are left uninitialised by the engine and written
    // as zeros. The mesh's arrays start 41 bytes in, after the marker and header chunks, the mesh
    // chunk's header, the version and the arrays' length; the wedges follow the 28 positions,
    // 28 normals and 44 triangles.
    let wedges = 41 + 28 * 12 + 28 * 12 + 44 * 6;
    for wedge in 0..mesh.mesh.sub_meshes[0].wedges.len() {
        let padding = wedges + wedge * 24 + 22;
        data[padding..padding + 2].fill(0);
    }
    let written = mesh.to_bytes(GameVersion::Gothic1).unwrap();
    assert_eq!(written.len(), data.len());
    assert!(written == data);
}

#[test]
fn morph_meshes_that_cannot_be_stored_fail() {
    let mut mesh = MorphMesh::from_bytes(&sample("morph0.mmb")).unwrap();
    mesh.animations[1].samples.pop();
    let error = mesh.to_bytes(GameVersion::Gothic2).unwrap_err().to_string();
    assert!(error.contains("\"S_SHOOT\" has 29 samples"), "{error}");

    let mut mesh = MorphMesh::from_bytes(&sample("morph0.mmb")).unwrap();
    mesh.morph_positions.pop();
    let error = mesh.to_bytes(GameVersion::Gothic2).unwrap_err().to_string();
    assert!(error.contains("27 morph positions for 28"), "{error}");
}
