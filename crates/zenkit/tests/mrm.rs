#![cfg(feature = "mrm")]
// The literals are copied from ZenKit's tests so the two stay easy to compare.
#![allow(clippy::excessive_precision)]

use std::path::Path;

use zenkit::math::{Vec2, Vec3};
use zenkit::mrm::{MultiResolutionMesh, Plane, Wedge};

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/samples")
            .join(name),
    )
    .unwrap()
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

    let wedge = |normal: Vec3, texture: Vec2, index| Wedge {
        normal,
        texture,
        index,
    };
    assert_eq!(sub.wedges.len(), 32);
    assert_eq!(
        sub.wedges[0],
        wedge(-Vec3::Z, Vec2::new(-1.500_000_5, -1.492_519_4), 4)
    );
    assert_eq!(
        sub.wedges[1],
        wedge(-Vec3::X, Vec2::new(2.499_999_5, -1.492_519_4), 4)
    );
    assert_eq!(
        sub.wedges[31],
        wedge(-Vec3::Z, Vec2::new(-1.500_000_5, 2.492_519_4), 7)
    );

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
        assert!(
            MultiResolutionMesh::from_bytes(&data[..len]).is_err(),
            "{len}"
        );
    }
    assert!(MultiResolutionMesh::from_bytes(&data[..mesh_chunk_end]).is_ok());
}
