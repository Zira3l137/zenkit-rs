//! Looks around a world: its mesh, the objects placed in it and the paths NPCs walk.
//!
//! ```text
//! cargo run --release --example world -- "C:\Games\Gothic II" NEWWORLD.ZEN
//! ```
#![allow(clippy::print_stdout)]

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use zenkit::Result;
use zenkit::world::VobId;
use zenkit::world::VobKind;
use zenkit::world::World;

fn main() -> ExitCode {
    let [gothic, name] = common::args("world <gothic folder> <world>");
    common::init_logging();
    common::finish(run(Path::new(&gothic), &name))
}

fn run(gothic: &Path, name: &str) -> Result<()> {
    let vfs = common::mount_game(gothic)?;
    let world: World = vfs.load(name)?;

    println!("{name}, made for {:?}", world.game);
    println!(
        "mesh: {} vertices, {} polygons, {} materials, {} triangles drawn",
        world.mesh.vertices.len(),
        world.mesh.polygons.len(),
        world.mesh.materials.len(),
        world.triangles().count()
    );

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, vob) in world.vobs.iter() {
        *counts.entry(category(&vob.kind)).or_default() += 1;
    }
    println!("\n{} objects:", world.vobs.len());
    for (category, count) in &counts {
        println!("  {count:>6} {category}");
    }

    println!("\nthe top of the object tree, {} roots:", world.roots.len());
    for &root in world.roots.iter().take(10) {
        print_tree(&world, root, 0);
    }

    println!("\nchests:");
    let chests = world.vobs.iter().filter_map(|(_, vob)| match &vob.kind {
        VobKind::Container(chest) if !chest.contents.is_empty() => Some((vob, chest)),
        _ => None,
    });
    for (vob, chest) in chests.take(10) {
        let lock =
            if chest.locked { format!("locked, key {:?}", chest.key) } else { "open".into() };
        println!("  at {:.0} {lock:<30} {}", vob.position, chest.contents);
    }

    println!("\nways to other worlds:");
    for vob in world.walk().map(|(_, vob)| vob) {
        if let VobKind::TriggerChangeLevel(change) = &vob.kind {
            println!(
                "  at {:.0}: to {} at {:?}",
                vob.position, change.level_name, change.start_vob
            );
        }
    }

    print_way_net(&world);
    Ok(())
}

/// Sorts a VOB into a rough category. Doors, chests and beds are all mobs, and the `mob`,
/// `trigger` and `sound` accessors find the shared part of each family.
fn category(kind: &VobKind) -> &'static str {
    match kind {
        VobKind::Vob => "plain",
        VobKind::Item(_) => "items",
        VobKind::Light(_) => "lights",
        VobKind::Door(_) => "doors",
        VobKind::Container(_) => "chests",
        VobKind::Npc(_) => "NPCs",
        kind if kind.mob().is_some() => "other mobs",
        kind if kind.trigger().is_some() => "triggers",
        kind if kind.sound().is_some() => "sounds",
        _ => "other",
    }
}

/// VOBs live in one flat list, `world.vobs`, and refer to each other by `VobId`.
fn print_tree(world: &World, id: VobId, depth: usize) {
    let vob = &world.vobs[id];
    let visual = vob.visual.as_ref().map_or("", |v| v.name.as_str());
    println!(
        "  {:indent$}{} {:?}, visual {visual:?}, {} children",
        "",
        category(&vob.kind),
        vob.name,
        vob.children.len(),
        indent = depth * 2
    );
    if depth < 1 {
        for &child in vob.children.iter().take(5) {
            print_tree(world, child, depth + 1);
        }
    }
}

fn print_way_net(world: &World) {
    let net = &world.way_net;
    println!("\nway net: {} points, {} ways", net.points.len(), net.ways.len());

    let start = world.vobs.iter().find(|(_, vob)| matches!(vob.kind, VobKind::StartPoint));
    let Some((_, start)) = start else {
        return;
    };
    let nearest = (net.points.iter().enumerate())
        .min_by(|(_, a), (_, b)| {
            let (a, b) = (a.position.distance(start.position), b.position.distance(start.position));
            a.total_cmp(&b)
        })
        .map(|(i, _)| i);
    let Some(nearest) = nearest else {
        return;
    };

    // Ways are pairs of indices into `points`, walkable both ways.
    let neighbours: Vec<&str> = (net.ways.iter())
        .filter_map(|&(a, b)| match (a == nearest, b == nearest) {
            (true, _) => Some(b),
            (_, true) => Some(a),
            _ => None,
        })
        .map(|i| net.points[i].name.as_str())
        .collect();
    println!("the player starts near {}, which leads to {neighbours:?}", net.points[nearest].name);
}
