//! Puts an animated model together the way the game does, from its model script.
//!
//! ```text
//! cargo run --example character -- "C:\Games\Gothic II" HUMANS.MDS
//! cargo run --example character -- "C:\Games\Gothic II" CHESTBIG_OCCHESTLARGE.ASC
//! ```
//!
//! A model script (`.MDS`, compiled to `.MSB`) names everything an animated model is made of:
//!
//! - the skeleton, `HUMANS.MDH`, named after the script;
//! - the meshes that can be put on it, like `HUM_BODY_NAKED0.ASC`, compiled to `.MDM`;
//! - the animations, each compiled to `HUMANS-<animation>.MAN`.
//!
//! Models that are never animated by a script keep their skeleton and mesh together in one
//! `.MDL` file.
#![allow(clippy::print_stdout)]

mod common;

use std::path::Path;
use std::process::ExitCode;

use zenkit::Result;
use zenkit::animation::ModelAnimation;
use zenkit::model::Model;
use zenkit::model::ModelHierarchy;
use zenkit::model::ModelMesh;
use zenkit::model_script::ModelScript;
use zenkit::vfs::Vfs;

fn main() -> ExitCode {
    let [gothic, name] = common::args("character <gothic folder> <model script or model>");
    common::init_logging();
    common::finish(run(Path::new(&gothic), &name))
}

fn run(gothic: &Path, name: &str) -> Result<()> {
    let vfs = common::mount_game(gothic)?;
    let name = common::compiled_name(name);
    let (stem, extension) = name.rsplit_once('.').unwrap_or((&name, ""));

    if extension.eq_ignore_ascii_case("MSB") {
        from_script(&vfs, stem)
    } else {
        let model: Model = vfs.load(&name)?;
        print_skeleton(&model.hierarchy);
        print_mesh(&model.mesh);
        Ok(())
    }
}

fn from_script(vfs: &Vfs, stem: &str) -> Result<()> {
    let script: ModelScript = vfs.load(&format!("{stem}.MSB"))?;
    println!("{} animations, meshes: {:?}", script.animations.len(), script.meshes);

    let skeleton: ModelHierarchy = vfs.load(&format!("{stem}.MDH"))?;
    print_skeleton(&skeleton);

    if let Some(mesh) = script.meshes.first() {
        let mesh_stem = mesh.rsplit_once('.').map_or(mesh.as_str(), |(stem, _)| stem);
        let mesh: ModelMesh = vfs.load(&format!("{mesh_stem}.MDM"))?;
        print_mesh(&mesh);
    }

    let Some(animation) = script.animation("S_RUNL").or(script.animations.first()) else {
        return Ok(());
    };
    let file = format!("{stem}-{}.MAN", animation.name);
    let frames: ModelAnimation = vfs.load(&file)?;
    println!(
        "\n{}: {} frames at {} fps, then {:?}",
        animation.name, frames.frame_count, frames.fps, animation.next
    );

    // An animation only works on the skeleton it was made for.
    if frames.checksum != skeleton.checksum {
        println!("  made for another skeleton");
        return Ok(());
    }

    // Each frame holds one sample per node the animation moves, in `node_indices` order.
    let root =
        frames.node_indices.iter().position(|&i| skeleton.nodes[i as usize].parent.is_none());
    if let Some(root) = root {
        let last = frames.frame_count.saturating_sub(1) as usize;
        let start = frames.frame(0).map(|f| f[root].position);
        let end = frames.frame(last).map(|f| f[root].position);
        if let (Some(start), Some(end)) = (start, end) {
            println!("  the root moves {:.0} cm", start.distance(end));
        }
    }
    Ok(())
}

fn print_skeleton(skeleton: &ModelHierarchy) {
    println!("\nskeleton, {} nodes:", skeleton.nodes.len());
    for node in &skeleton.nodes {
        let mut depth = 0;
        let mut parent = node.parent;
        while let Some(p) = parent {
            depth += 1;
            parent = skeleton.nodes[p as usize].parent;
        }
        println!("  {:indent$}{}", "", node.name, indent = depth * 2);
    }
}

fn print_mesh(mesh: &ModelMesh) {
    println!("\nmesh:");
    for skin in &mesh.meshes {
        let triangles: usize = skin.mesh.sub_meshes.iter().map(|s| s.triangles.len()).sum();
        println!(
            "  skinned: {} positions, {} triangles, bent by {} nodes",
            skin.mesh.positions.len(),
            triangles,
            skin.nodes.len()
        );
    }
    for attachment in &mesh.attachments {
        println!(
            "  attached to {}: {} positions",
            attachment.node,
            attachment.mesh.positions.len()
        );
    }
}
