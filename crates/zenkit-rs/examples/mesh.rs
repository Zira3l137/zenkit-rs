//! Prints what a static mesh (`.MRM`) or a morph mesh (`.MMB`, like a head) is made of.
//!
//! ```text
//! cargo run --example mesh -- "C:\Games\Gothic II" ITMW_1H_AXE_01.3DS
//! cargo run --example mesh -- "C:\Games\Gothic II" HUM_HEAD_PONY.MMS
//! ```
#![allow(clippy::print_stdout)]

mod common;

use std::path::Path;
use std::process::ExitCode;

use zenkit::Result;
use zenkit::morph::MorphMesh;
use zenkit::mrm::MultiResolutionMesh;

fn main() -> ExitCode {
    let [gothic, name] = common::args("mesh <gothic folder> <mesh>");
    common::init_logging();
    common::finish(run(Path::new(&gothic), &name))
}

fn run(gothic: &Path, name: &str) -> Result<()> {
    let vfs = common::mount_game(gothic)?;
    let name = common::compiled_name(name);

    if name.to_ascii_uppercase().ends_with(".MMB") {
        let morph: MorphMesh = vfs.load(&name)?;
        print_mesh(&morph.mesh);
        println!("\n{} animations:", morph.animations.len());
        for anim in &morph.animations {
            println!(
                "  {:<24} {:>3} frames, moves {} vertices",
                anim.name,
                anim.frame_count,
                anim.vertices.len()
            );
        }
    } else {
        let mesh: MultiResolutionMesh = vfs.load(&name)?;
        print_mesh(&mesh);
    }
    Ok(())
}

fn print_mesh(mesh: &MultiResolutionMesh) {
    let size = mesh.bbox.max - mesh.bbox.min;
    println!(
        "{} positions, {:.0} x {:.0} x {:.0} cm",
        mesh.positions.len(),
        size.x,
        size.y,
        size.z
    );

    for sub in &mesh.sub_meshes {
        println!(
            "  {:<24} texture {:<28} {:>5} triangles",
            sub.material.name,
            sub.material.texture,
            sub.triangles.len()
        );
    }

    // Triangles index their sub-mesh's wedges: corners with their own normal and UV, which in
    // turn index the positions all sub-meshes share.
    if let Some(sub) = mesh.sub_meshes.first()
        && let Some(triangle) = sub.triangles.first()
    {
        let corners = triangle.map(|w| mesh.positions[sub.wedges[w as usize].index as usize]);
        println!("first triangle: {corners:?}");
    }
}
