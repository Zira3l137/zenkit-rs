use std::path::Path;

use zenkit_rs_core::GameVersion;
use zenkit_rs_core::math::Mat4;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_model::HierarchyNode;
use zenkit_rs_model::Model;
use zenkit_rs_model::ModelHierarchy;
use zenkit_rs_model::ModelMesh;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)).unwrap()
}

fn u16_at(data: &[u8], at: usize) -> usize {
    u16::from_le_bytes(data[at..at + 2].try_into().unwrap()) as usize
}

fn u32_at(data: &[u8], at: usize) -> usize {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize
}

/// Zeroes what the engine leaves uninitialised and writing turns into zeros: the two bytes of
/// padding after the time in source dates, and after every wedge of every mesh.
fn clear_padding(mut data: Vec<u8>) -> Vec<u8> {
    let mut at = 0;
    while at < data.len() {
        let (id, len) = (u16_at(&data, at) as u16, u32_at(&data, at + 2));
        let body = at + 6;
        match id {
            0xD010 | 0xD110 => data[body + 14..body + 16].fill(0),
            0xB100 => {
                let content = body + 6;
                let table = content + u32_at(&data, body + 2);
                for sub in 0..data[table] as usize {
                    let wedges = table + 1 + 16 + sub * 80 + 8;
                    let (offset, count) = (u32_at(&data, wedges), u32_at(&data, wedges + 4));
                    for wedge in 0..count {
                        let padding = content + offset + wedge * 24 + 22;
                        data[padding..padding + 2].fill(0);
                    }
                }
            }
            _ => {}
        }
        // The soft skin chunk only covers its header; the meshes follow as chunks of their own.
        at = body + len;
    }
    data
}

#[test]
fn samples_write_back_byte_for_byte() {
    let hierarchy = sample("hierarchy0.mdh");
    let written = ModelHierarchy::from_bytes(&hierarchy).unwrap().to_bytes().unwrap();
    assert!(written == clear_padding(hierarchy), "hierarchy0.mdh");

    for name in ["secretdoor.mdm", "smoke_waterpipe.mdm"] {
        let data = sample(name);
        let written = ModelMesh::from_bytes(&data).unwrap().to_bytes(GameVersion::Gothic1).unwrap();
        assert!(written == clear_padding(data), "{name}");
    }
}

#[test]
fn model_mesh_keeps_its_source() {
    let mesh = ModelMesh::from_bytes(&sample("secretdoor.mdm")).unwrap();
    assert_eq!(mesh.source_path, "\\_WORK\\DATA\\ANIMS\\SECRETDOOR_STONE.ASC");
}

#[test]
fn models_round_trip() {
    let mut hierarchy = ModelHierarchy::from_bytes(&sample("hierarchy0.mdh")).unwrap();
    let mut mesh = ModelMesh::from_bytes(&sample("smoke_waterpipe.mdm")).unwrap();
    mesh.attachments = ModelMesh::from_bytes(&sample("secretdoor.mdm")).unwrap().attachments;
    hierarchy.checksum = mesh.checksum;
    hierarchy.nodes.push(HierarchyNode {
        name: "BIP01 DOOR".into(),
        parent: Some(0),
        transform: Mat4::from_translation(Vec3::X),
    });
    let model = Model { hierarchy, mesh };

    for game in [GameVersion::Gothic1, GameVersion::Gothic2] {
        let back = Model::from_bytes(&model.to_bytes(game).unwrap()).unwrap();
        // Meshes remember which game they were read as, which is the one they were written for.
        let mut expected = model.clone();
        for mesh in expected.mesh.meshes.iter_mut().map(|m| &mut m.mesh) {
            mesh.game = Some(game);
        }
        for attachment in &mut expected.mesh.attachments {
            attachment.mesh.game = Some(game);
        }
        assert_eq!(back, expected, "{game:?}");
    }
}

#[test]
fn mesh_header_names_the_version_of_its_meshes() {
    // The game refuses a file whose header and meshes disagree, and compiles the model from its
    // source instead, which a mod does not ship.
    let mut mesh = ModelMesh::from_bytes(&sample("smoke_waterpipe.mdm")).unwrap();
    mesh.attachments = ModelMesh::from_bytes(&sample("secretdoor.mdm")).unwrap().attachments;
    for (game, header, meshes) in
        [(GameVersion::Gothic1, 0x0403_0506, 0x305), (GameVersion::Gothic2, 0x0409_0506, 0x905)]
    {
        let data = mesh.to_bytes(game).unwrap();
        let (mut at, mut seen) = (0, 0);
        while at < data.len() {
            let (id, size) = (u16_at(&data, at), u32_at(&data, at + 2));
            match id {
                0xD000 => assert_eq!(u32_at(&data, at + 6), header, "{game:?}"),
                0xB100 => {
                    assert_eq!(u16_at(&data, at + 6), meshes, "{game:?}");
                    seen += 1;
                }
                _ => {}
            }
            at += 6 + size;
        }
        assert_eq!(seen, 2, "{game:?}: a soft skin and an attachment");
    }
}

#[test]
fn refuses_what_does_not_fit() {
    let error = |result: zenkit_rs_core::Result<Vec<u8>>| result.unwrap_err().to_string();

    let mut hierarchy = ModelHierarchy::from_bytes(&sample("hierarchy0.mdh")).unwrap();
    hierarchy.nodes[1].parent = Some(40000);
    assert!(error(hierarchy.to_bytes()).contains("past 32767"));
    hierarchy.nodes[1].parent = Some(0);
    hierarchy.nodes[1].name = "TWO\nLINES".into();
    assert!(error(hierarchy.to_bytes()).contains("line break"));

    let mut mesh = ModelMesh::from_bytes(&sample("smoke_waterpipe.mdm")).unwrap();
    mesh.meshes[0].weights.pop();
    let message = zenkit_rs_core::Error::report(&mesh.to_bytes(GameVersion::Gothic2).unwrap_err())
        .to_string();
    assert!(message.contains("every position needs one"), "{message}");
}
