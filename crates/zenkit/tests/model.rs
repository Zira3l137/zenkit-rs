#![cfg(feature = "model")]
// The literals are copied from ZenKit's tests so the two stay easy to compare.
#![allow(clippy::excessive_precision)]

use std::path::Path;

use zenkit::math::Mat4;
use zenkit::math::Vec3;
use zenkit::model::ModelHierarchy;
use zenkit::model::ModelMesh;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples").join(name)).unwrap()
}

#[test]
fn hierarchy() {
    let h = ModelHierarchy::from_bytes(&sample("hierarchy0.mdh")).unwrap();

    assert_eq!(h.nodes.len(), 7);
    assert_eq!(h.nodes[0].name, "BIP01 MUEHLE");
    assert_eq!(h.nodes[0].parent, None);
    assert_eq!(
        h.nodes[0].transform,
        Mat4::from_cols_array(&[
            -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0
        ])
    );

    assert_eq!(h.nodes[1].name, "BIP01 NABE");
    assert_eq!(h.nodes[1].parent, Some(0));
    assert_eq!(
        h.nodes[1].transform,
        Mat4::from_cols_array(&[
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            -0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            -394.040466,
            1.0
        ])
    );
    assert_eq!(h.nodes[1].transform.w_axis.z, -394.040466);

    assert_eq!(h.bbox.min, Vec3::new(-497.17572, -0.575592041, -105.896698));
    assert_eq!(h.bbox.max, Vec3::new(515.717346, 364.943878, 893.536743));
    assert_eq!(h.collision_bbox.min, Vec3::new(-248.58786, -0.402914435, -52.948349));
    assert_eq!(h.collision_bbox.max, Vec3::new(257.858673, 291.955109, 446.768372));
    assert_eq!(h.root_translation, Vec3::new(0.0, 0.0, -394.040466));

    assert_eq!(h.node("bip01 nabe").map(|(i, _)| i), Some(1));
}

#[test]
fn mesh_with_attachment() {
    let mesh = ModelMesh::from_bytes(&sample("secretdoor.mdm")).unwrap();
    assert_eq!(mesh.attachments.len(), 1);
    assert_eq!(mesh.attachments[0].node, "BIP01 DOOR");
    assert_eq!(mesh.attachment("bip01 door").unwrap().positions.len(), 8);
    assert!(mesh.meshes.is_empty());
}

#[test]
fn mesh_with_soft_skin() {
    let mesh = ModelMesh::from_bytes(&sample("smoke_waterpipe.mdm")).unwrap();
    assert_eq!(mesh.meshes.len(), 1);

    let sk = &mesh.meshes[0];
    assert_eq!(sk.mesh.positions.len(), 115);
    assert_eq!(sk.mesh.normals.len(), 115);
    assert_eq!(sk.mesh.sub_meshes.len(), 1);

    assert_eq!(sk.weights.len(), 115);
    let w = |i: usize| {
        (
            sk.weights[i].len(),
            sk.weights[i][0].weight,
            sk.weights[i][0].position,
            sk.weights[i][0].node,
        )
    };
    assert_eq!(w(0), (1, 1.0, Vec3::new(-5.49776077, 35.086731, -2.64756012), 0));
    assert_eq!(w(62), (1, 1.0, Vec3::new(0.260997772, 18.0412712, -23.9048882), 4));
    assert_eq!(w(114), (1, 1.0, Vec3::new(1.05304337, 71.0284958, 1.32049942), 0));

    assert!(sk.wedge_normals.is_empty());
    assert_eq!(sk.nodes.len(), 6);
    assert_eq!(sk.nodes[..3], [0, 5, 3]);

    assert_eq!(sk.bboxes.len(), 6);
    let b = &sk.bboxes[0];
    assert_eq!(b.center, Vec3::new(0.612892151, 41.7827187, 0.705307007));
    assert_eq!(b.half_width, Vec3::new(15.2073612, 33.4261742, 14.8513918));
    assert_eq!(b.axes[0], Vec3::new(0.777145922, 0.0, -0.629320442));
    assert_eq!(b.axes[1], Vec3::new(0.0, 1.0, 0.0));
    assert_eq!(b.axes[2], Vec3::new(0.629320442, 0.0, 0.777145922));
    assert!(b.children.is_empty());
}

#[test]
fn truncated_files_fail_cleanly() {
    for name in ["hierarchy0.mdh", "secretdoor.mdm", "smoke_waterpipe.mdm"] {
        let data = sample(name);
        for len in 0..data.len() {
            let _ = ModelHierarchy::from_bytes(&data[..len]);
            let _ = ModelMesh::from_bytes(&data[..len]);
        }
    }
}
