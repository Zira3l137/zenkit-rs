use std::path::Path;

use zenkit_rs_archive::Archive;
use zenkit_rs_core::math::Color;
use zenkit_rs_core::math::Vec2;
use zenkit_rs_material::AlphaFunction;
use zenkit_rs_material::AnimationMapping;
use zenkit_rs_material::Material;
use zenkit_rs_material::MaterialGroup;
use zenkit_rs_material::WaveMode;
use zenkit_rs_material::WaveSpeed;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)).unwrap()
}

fn demon_body(game: &str) -> Material {
    let data = sample(&format!("{game}/DEMON_DIE_BODY.MAT"));
    let mut archive = Archive::new(&data).unwrap();
    let mut list = Material::read_list(&mut archive).unwrap();
    assert_eq!(list.len(), 1);

    // The G2 sample was cut from a mesh: the material list is followed by the mesh's alpha-test
    // byte and the start of its vertex chunk (0xB030).
    let rest = &data[archive.position()..];
    match game {
        "G1" => assert!(rest.is_empty()),
        _ => assert!(rest.starts_with(&[0x00, 0x30, 0xB0]), "{rest:02x?}"),
    }
    list.remove(0)
}

fn check_common(m: &Material) {
    assert_eq!(m.name, "BODY");
    assert_eq!(m.group, MaterialGroup::Undefined);
    assert_eq!(m.color, Color::new(115, 91, 77, 255));
    assert_eq!(m.smooth_angle, 60.0);
    assert_eq!(m.texture, "DEM_BODY_V0.TGA");
    assert_eq!(m.texture_scale, Vec2::new(512.0, 512.0));
    assert_eq!(m.texture_anim_fps, 0.0);
    assert_eq!(m.texture_anim_map_mode, AnimationMapping::None);
    assert!(!m.disable_collision);
    assert!(!m.disable_lightmap);
    assert!(!m.dont_collapse);
    assert_eq!(m.detail_object, "");
    assert_eq!(m.default_mapping, Vec2::new(2.34375, 2.34375));
}

#[test]
fn gothic1() {
    let m = demon_body("G1");
    check_common(&m);
    assert_eq!(m.texture_anim_map_dir, Vec2::new(1e-4, 0.0));
    assert_eq!(m.alpha_func, AlphaFunction::Default);
    assert_eq!(m.wave_speed, WaveSpeed::None);
}

#[test]
fn gothic2() {
    let m = demon_body("G2");
    check_common(&m);
    assert_eq!(m.texture_anim_map_dir, Vec2::ZERO);
    assert_eq!(m.alpha_func, AlphaFunction::None);
    assert_eq!(m.detail_object_scale, 1.0);
    assert!(!m.force_occluder);
    assert!(!m.environment_mapping);
    assert_eq!(m.environment_mapping_strength, 1.0);
    assert_eq!(m.wave_mode, WaveMode::None);
    assert_eq!(m.wave_speed, WaveSpeed::Normal);
    assert_eq!(m.wave_max_amplitude, 30.0);
    assert_eq!(m.wave_grid_size, 100.0);
    assert!(!m.ignore_sun);
}

#[test]
fn named_material_from_binary_archive() {
    let data = sample("binary.zen");
    let mut archive = Archive::new(&data).unwrap();
    assert_eq!(archive.string().unwrap(), b"DT_BOOKSHELF_V1_1");

    let m = Material::read(&mut archive).unwrap();
    assert_eq!(m.name, "DT_BOOKSHELF_V1_1");
    assert_eq!(m.texture, "MODIBOOKS01.TGA");
    assert_eq!(m.texture_scale, Vec2::new(256.0, 256.0));
    assert_eq!(m.color, Color::new(0x2A, 0x23, 0x19, 0xFF));
}

#[test]
fn wrong_object_class_is_an_error() {
    let data = sample("G2/SAVEINFO.SAV");
    let mut archive = Archive::new(&data).unwrap();
    let err = Material::read(&mut archive).unwrap_err().to_string();
    assert!(err.contains("oCSavegameInfo"), "{err}");
}
