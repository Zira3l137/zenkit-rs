// The literals are copied from ZenKit's tests so the two stay easy to compare.
#![allow(clippy::excessive_precision)]

use std::path::Path;

use zenkit_rs_core::GameVersion;
use zenkit_rs_core::math::Vec2;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_mrm::LodParams;
use zenkit_rs_mrm::MultiResolutionMesh;
use zenkit_rs_mrm::Plane;
use zenkit_rs_mrm::SubMesh;
use zenkit_rs_mrm::VertexUpdate;
use zenkit_rs_mrm::Wedge;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)).unwrap()
}

#[test]
fn mesh0() {
    let mesh = MultiResolutionMesh::from_bytes(&sample("mesh0.mrm")).unwrap();

    assert_eq!(mesh.positions.len(), 8);
    assert_eq!(mesh.positions[0], Vec3::new(200.0, 398.503_906, 200.0));
    assert_eq!(mesh.positions[1], Vec3::new(-200.0, 398.503_906, 200.0));
    assert_eq!(mesh.positions[7], Vec3::new(-200.0, 0.0, -200.0));
    assert!(mesh.normals.is_empty());
    assert!(mesh.alpha_test);

    assert_eq!(mesh.bbox.min, Vec3::new(-200.0, 0.0, -200.0));
    assert_eq!(mesh.bbox.max, Vec3::new(200.0, 398.503_906, 200.0));

    assert_eq!(mesh.sub_meshes.len(), 1);
    let sub = &mesh.sub_meshes[0];
    assert_eq!(sub.material.name, "EVT_TPL_GITTERKAEFIG_01");
    assert_eq!(sub.material.texture, "OCODFLGATELI.TGA");

    assert!(sub.colors.is_empty());
    assert!(sub.triangle_edges.is_empty());
    assert!(sub.edge_scores.is_empty());
    assert!(sub.edges.is_empty());

    assert_eq!(sub.triangles.len(), 16);
    assert_eq!(sub.triangles[0], [26, 19, 12]);
    assert_eq!(sub.triangles[1], [8, 13, 18]);
    assert_eq!(sub.triangles[14], [2, 6, 29]);
    assert_eq!(sub.triangles[15], [28, 20, 3]);

    let wedge = |normal: Vec3, texture: Vec2, index| Wedge { normal, texture, index };
    assert_eq!(sub.wedges.len(), 32);
    assert_eq!(sub.wedges[0], wedge(-Vec3::Z, Vec2::new(-1.500_000_5, -1.492_519_4), 4));
    assert_eq!(sub.wedges[1], wedge(-Vec3::X, Vec2::new(2.499_999_5, -1.492_519_4), 4));
    assert_eq!(sub.wedges[31], wedge(-Vec3::Z, Vec2::new(-1.500_000_5, 2.492_519_4), 7));

    assert_eq!(sub.triangle_plane_indices.len(), 16);
    assert_eq!(sub.triangle_plane_indices[..2], [0, 1]);
    assert_eq!(sub.triangle_plane_indices[14..], [7, 5]);

    let plane = |distance, normal| Plane { distance, normal };
    assert_eq!(sub.triangle_planes.len(), 8);
    assert_eq!(sub.triangle_planes[0], plane(200.0, Vec3::X));
    assert_eq!(sub.triangle_planes[1], plane(200.0, Vec3::Z));
    assert_eq!(sub.triangle_planes[6], plane(200.0, -Vec3::Z));
    assert_eq!(sub.triangle_planes[7], plane(-200.0, Vec3::Z));

    assert_eq!(sub.wedge_map.len(), 32);
    assert_eq!(sub.wedge_map[..2], [65535, 65535]);
    assert_eq!(sub.wedge_map[29..], [2, 1, 0]);
}

#[test]
fn truncated_meshes_fail_cleanly() {
    let data = sample("mesh0.mrm");
    // Only the trailing end chunk is optional; cutting into the mesh chunk must fail.
    let mesh_chunk_end = data.len() - 6;
    for len in 0..mesh_chunk_end {
        assert!(MultiResolutionMesh::from_bytes(&data[..len]).is_err(), "{len}");
    }
    assert!(MultiResolutionMesh::from_bytes(&data[..mesh_chunk_end]).is_ok());
}

#[test]
fn mesh0_keeps_its_level_of_detail() {
    let mesh = MultiResolutionMesh::from_bytes(&sample("mesh0.mrm")).unwrap();
    let sub = &mesh.sub_meshes[0];
    assert_eq!(sub.vertex_updates.len(), mesh.positions.len() + 1);
    assert_eq!(sub.vertex_updates[0], VertexUpdate { triangles: 0, wedges: 16 });
    let all = VertexUpdate { triangles: 16, wedges: 32 };
    assert_eq!(sub.vertex_updates.last(), Some(&all));
    assert_eq!((sub.triangles.len(), sub.wedges.len()), (16, 32));
    assert_eq!(mesh.lod, LodParams::default());
}

#[test]
fn mesh0_writes_back_byte_for_byte() {
    let mut data = sample("mesh0.mrm");
    let mesh = MultiResolutionMesh::from_bytes(&data).unwrap();
    // The engine leaves the two bytes of padding after each wedge uninitialised; they are
    // written as zeros. The wedges start 192 bytes into the content, which starts at 12.
    for wedge in 0..32 {
        let padding = 12 + 192 + wedge * 24 + 22;
        data[padding..padding + 2].fill(0);
    }
    assert!(mesh.to_bytes(GameVersion::Gothic1).unwrap() == data);
}

#[test]
fn gothic_2_meshes_store_the_alpha_test_flag() {
    let mut mesh = MultiResolutionMesh::from_bytes(&sample("mesh0.mrm")).unwrap();
    mesh.alpha_test = false;
    mesh.lod.morph_percentage = 0.03;
    mesh.normals = mesh.positions.iter().map(|p| p.normalize()).collect();

    let back =
        MultiResolutionMesh::from_bytes(&mesh.to_bytes(GameVersion::Gothic2).unwrap()).unwrap();
    assert_eq!(back.game, Some(GameVersion::Gothic2));
    assert_eq!(MultiResolutionMesh { game: mesh.game, ..back }, mesh);
}

#[test]
fn too_many_sub_meshes_cannot_be_written() {
    let mesh = MultiResolutionMesh {
        sub_meshes: vec![SubMesh::default(); 256],
        ..MultiResolutionMesh::default()
    };
    let error = mesh.to_bytes(GameVersion::Gothic2).unwrap_err();
    assert!(error.to_string().contains("256 sub-meshes"), "{error}");
}

#[test]
fn sub_meshes_without_vertex_updates_are_written_in_full_detail() {
    let mut mesh = MultiResolutionMesh::from_bytes(&sample("mesh0.mrm")).unwrap();
    mesh.sub_meshes[0].vertex_updates.clear();

    let back =
        MultiResolutionMesh::from_bytes(&mesh.to_bytes(GameVersion::Gothic1).unwrap()).unwrap();
    let all = VertexUpdate { triangles: 16, wedges: 32 };
    assert_eq!(mesh.sub_meshes[0].full_detail(), Some(all));
    assert_eq!(back.sub_meshes[0].vertex_updates, [all]);
}

#[test]
fn vertex_updates_beyond_their_sub_mesh_cannot_be_written() {
    let mut mesh = MultiResolutionMesh::from_bytes(&sample("mesh0.mrm")).unwrap();
    mesh.sub_meshes[0].vertex_updates.push(VertexUpdate { triangles: 17, wedges: 32 });
    let error = mesh.to_bytes(GameVersion::Gothic2).unwrap_err();
    assert!(error.to_string().contains("draws 17 triangles"), "{error}");
}
