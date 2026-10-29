//! Reads a save game: what the load menu shows, the quest log, the player and the worlds.
//!
//! ```text
//! cargo run --release --example save -- "C:\Games\Gothic II\Saves\savegame1" thumbnail.png
//! ```
#![allow(clippy::print_stdout)]

mod common;

use std::path::Path;
use std::process::ExitCode;

use zenkit::Result;
use zenkit::save::SaveGame;
use zenkit::save::TopicSection;
use zenkit::save::TopicStatus;
use zenkit::world::VobKind;

fn main() -> ExitCode {
    let [folder, thumbnail] = common::args("save <save game folder> <thumbnail.png>");
    common::init_logging();
    common::finish(run(Path::new(&folder), Path::new(&thumbnail)))
}

fn run(folder: &Path, thumbnail: &Path) -> Result<()> {
    let save = SaveGame::open(folder)?;
    let info = &save.info;
    println!("{:?}, a {:?} save from {}", info.title, save.game(), info.save_date);
    println!(
        "in {}, day {} at {:02}:{:02}, after {} minutes of play",
        info.world,
        info.day,
        info.hour,
        info.minute,
        info.play_time_seconds / 60
    );
    // The scripts' global variables, by the name the scripts give them.
    if let Some(chapter) = save.data.symbol("KAPITEL") {
        println!("chapter {chapter:?}");
    }

    println!("\nrunning quests:");
    let running = (save.data.log.iter())
        .filter(|t| t.section == TopicSection::Missions && t.status == TopicStatus::Running);
    for topic in running {
        println!("  {}", topic.description);
        if let Some(latest) = topic.entries.last() {
            println!("    {latest}");
        }
    }

    if let Some(stats) = &save.statistics {
        println!("\nprogress, {} rows:", stats.rows.len());
        let level = stats.column("level").collect::<Vec<_>>();
        println!("  level over time: {level:?}");
    }

    if let Some(thumb) = &save.thumbnail {
        common::save_png(thumbnail, thumb.width, thumb.height, &thumb.to_rgba8(0)?)?;
        println!("\nsaved the thumbnail as {}", thumbnail.display());
    }

    // Worlds are only read when asked for: one with `save.world(name)`, the one the player is
    // in with `save.current_world()`, or all of them at once, in parallel.
    println!("\nworlds visited:");
    for (name, world) in save.all_worlds() {
        let world = match world {
            Ok(world) => world,
            Err(error) => {
                println!("  {name:<16} {}", error.report());
                continue;
            }
        };
        let npcs = world.state.as_ref().map_or(0, |s| s.npcs.len());
        println!("  {name:<16} {:>6} objects, {npcs:>4} NPCs", world.vobs.len());

        for (_, vob) in world.vobs.iter() {
            if let VobKind::Npc(npc) = &vob.kind
                && npc.player
            {
                let [hp, hp_max, mana, mana_max, strength, dexterity, ..] = npc.attributes;
                println!("    the player, level {}, {} experience", npc.level, npc.xp);
                println!("    health {hp}/{hp_max}, mana {mana}/{mana_max}");
                println!("    strength {strength}, dexterity {dexterity}");
            }
        }
    }
    Ok(())
}
