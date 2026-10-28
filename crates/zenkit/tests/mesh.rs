#![cfg(feature = "mesh")]

use std::path::Path;

use zenkit::GameVersion;
use zenkit::archive::Archive;
use zenkit::material::Material;
use zenkit::math::Vec2;
use zenkit::math::Vec3;
use zenkit::mesh::Mesh;
use zenkit::mesh::Triangle;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples").join(name)).unwrap()
}

fn chunk(out: &mut Vec<u8>, id: u16, payload: &[u8]) {
    out.extend(id.to_le_bytes());
    out.extend((payload.len() as u32).to_le_bytes());
    out.extend(payload);
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// The G2 material sample was cut from a mesh's material chunk; this is that chunk's payload.
fn material_chunk() -> Vec<u8> {
    let data = sample("G2/DEMON_DIE_BODY.MAT");
    let mut archive = Archive::new(&data).unwrap();
    Material::read_list(&mut archive).unwrap();
    data[..archive.position() + 1].to_vec()
}

fn tiny_texture() -> Vec<u8> {
    let mut out = b"ZTEX".to_vec();
    for v in [0u32, 5, 1, 1, 1, 1, 1, 0xFF00_0000] {
        out.extend(v.to_le_bytes());
    }
    out.extend([10, 20, 30]);
    out
}

/// A quad and a degenerate two-corner polygon, in Gothic 1 or Gothic 2 layout.
fn mesh(version: u16) -> Vec<u8> {
    let g2 = version == 265;
    let mut out = Vec::new();

    let mut header = version.to_le_bytes().to_vec();
    header.extend(2004u32.to_le_bytes());
    for v in [5u16, 17, 12, 30, 45, 0] {
        header.extend(v.to_le_bytes());
    }
    header.extend(b"QUAD.3DS\n");
    chunk(&mut out, 0xB000, &header);

    let mut bbox = floats(&[-1.0, 0.0, -1.0, 1.0, 0.0, 1.0]);
    bbox.extend(floats(&[0.0; 15]));
    bbox.extend(0u16.to_le_bytes());
    chunk(&mut out, 0xB010, &bbox);

    if g2 {
        chunk(&mut out, 0xB020, &material_chunk());
    }

    let mut lightmaps = 1u32.to_le_bytes().to_vec();
    lightmaps.extend(tiny_texture());
    lightmaps.extend(1u32.to_le_bytes());
    lightmaps.extend(floats(&[1.0, 2.0, 3.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
    lightmaps.extend(0u32.to_le_bytes());
    chunk(&mut out, 0xB026, &lightmaps);

    let mut vertices = 4u32.to_le_bytes().to_vec();
    vertices.extend(floats(&[-1.0, 0.0, -1.0, 1.0, 0.0, -1.0, 1.0, 0.0, 1.0, -1.0, 0.0, 1.0]));
    chunk(&mut out, 0xB030, &vertices);

    let mut features = 1u32.to_le_bytes().to_vec();
    features.extend(floats(&[0.5, 0.25]));
    features.extend(0xFF80_8080u32.to_le_bytes());
    features.extend(floats(&[0.0, 1.0, 0.0]));
    chunk(&mut out, 0xB040, &features);

    let mut polygons = 2u32.to_le_bytes().to_vec();
    for (corners, lightmap) in [(&[0u32, 1, 2, 3][..], 0i16), (&[0, 1][..], -1)] {
        polygons.extend(0u16.to_le_bytes());
        polygons.extend(lightmap.to_le_bytes());
        polygons.extend(floats(&[0.0, 0.0, 1.0, 0.0]));
        if g2 {
            polygons.push(0b0010_0100);
        } else {
            polygons.extend([0b1010_0100, 0b0000_0001]);
        }
        polygons.extend(7i16.to_le_bytes());
        polygons.push(corners.len() as u8);
        for &v in corners {
            if g2 {
                polygons.extend(v.to_le_bytes());
            } else {
                polygons.extend((v as u16).to_le_bytes());
            }
            polygons.extend(0u32.to_le_bytes());
        }
    }
    chunk(&mut out, 0xB050, &polygons);

    chunk(&mut out, 0xB060, &[]);
    out
}

#[test]
fn gothic2() {
    let mesh = Mesh::from_bytes(&mesh(265)).unwrap();
    assert_eq!(mesh.name, "QUAD.3DS");
    assert_eq!(mesh.game, Some(GameVersion::Gothic2));
    assert_eq!((mesh.date.year, mesh.date.month, mesh.date.second), (2004, 5, 45));
    assert_eq!(mesh.bbox.max, Vec3::new(1.0, 0.0, 1.0));

    assert_eq!(mesh.materials.len(), 1);
    assert_eq!(mesh.materials[0].texture, "DEM_BODY_V0.TGA");
    assert!(!mesh.alpha_test);

    assert_eq!(mesh.lightmap_textures.len(), 1);
    assert_eq!(mesh.lightmap_textures[0].to_rgba8(0).unwrap(), [10, 20, 30, 255]);
    assert_eq!(mesh.lightmaps[0].origin, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(mesh.lightmaps[0].normals[1], Vec3::Z);

    assert_eq!(mesh.vertices.len(), 4);
    assert_eq!(mesh.features[0].texture, Vec2::new(0.5, 0.25));
    assert_eq!(mesh.features[0].light, 0xFF80_8080);

    let quad = &mesh.polygons[0];
    assert_eq!(quad.lightmap, Some(0));
    assert_eq!(mesh.polygons[1].lightmap, None);
    assert!(quad.flags.is_occluder && quad.flags.is_outdoor);
    assert_eq!(quad.flags.sector_index, 7);
    assert_eq!(mesh.polygon_vertices(quad), [0, 1, 2, 3]);

    let triangles: Vec<Triangle> = mesh.triangles().collect();
    assert_eq!(triangles.len(), 2);
    assert_eq!(triangles[0].vertices, [0, 1, 2]);
    assert_eq!(triangles[1].vertices, [0, 2, 3]);
    assert!(triangles.iter().all(|t| t.polygon == 0));
}

#[test]
fn gothic1() {
    let mesh = Mesh::from_bytes(&mesh(9)).unwrap();
    assert_eq!(mesh.game, Some(GameVersion::Gothic1));
    assert!(mesh.alpha_test);

    let flags = mesh.polygons[0].flags;
    assert!(flags.is_occluder && flags.is_outdoor);
    assert_eq!(flags.normal_axis, 0b11);
    assert_eq!(mesh.polygon_vertices(&mesh.polygons[0]), [0, 1, 2, 3]);
}

#[test]
fn truncated_meshes_fail_cleanly() {
    let data = mesh(265);
    for len in 0..data.len() {
        let _ = Mesh::from_bytes(&data[..len]);
    }
}
