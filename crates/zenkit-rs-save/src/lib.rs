//! Save games: a folder per save slot, like `saves/savegame1`, holding
//!
//! - `SAVEINFO.SAV`: what the load menu shows, see [`SaveInfo`];
//! - `SAVEDAT.SAV`: the state of the scripts, the quest log and the guilds, see [`SaveData`];
//! - `THUMB.SAV`: a screenshot, as a texture;
//! - `LOG.SAV`: the player's progress over time, see [`Statistics`];
//! - `SAVEHDR.SAV`: the `.ZEN` file of the world the player was in;
//! - a `.SAV` file for every world the player visited, read with
//!   [`zenkit_rs_world::World`].
//!
//! ```no_run
//! # fn main() -> zenkit_rs_core::Result<()> {
//! use zenkit_rs_save::SaveGame;
//!
//! let save = SaveGame::open("Saves/savegame1")?;
//! println!("{} in {}, {} seconds in", save.info.title, save.info.world, save.info.play_time_seconds);
//! let world = save.current_world()?;
//! # Ok(()) }
//! ```

mod data;
mod info;
mod stats;

use std::path::Path;
use std::path::PathBuf;

use zenkit_rs_archive::Archive;
use zenkit_rs_archive::ObjectHeader;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::ResultExt;
use zenkit_rs_core::text;
use zenkit_rs_texture::Texture;
use zenkit_rs_world::World;

pub use data::CutscenePoolItem;
pub use data::GUILDS;
pub use data::InfoState;
pub use data::LogTopic;
pub use data::Mission;
pub use data::MissionEntry;
pub use data::RunBehavior;
pub use data::SaveData;
pub use data::SymbolState;
pub use data::TopicSection;
pub use data::TopicStatus;
pub use info::SaveInfo;
pub use stats::Statistics;

const FORMAT: &str = "save game";

/// The files every save has besides its worlds.
const OWN_FILES: [&str; 5] = ["SAVEINFO", "SAVEDAT", "SAVEHDR", "THUMB", "LOG"];

/// A save game folder. Worlds are only read when asked for, since they are most of its size.
#[derive(Debug, Clone)]
pub struct SaveGame {
    pub info: SaveInfo,
    pub data: SaveData,
    pub thumbnail: Option<Texture>,
    pub statistics: Option<Statistics>,
    /// The `.ZEN` file of the world the player was in, like `NEWWORLD\NEWWORLD.ZEN`. The save
    /// only holds that world's state; its mesh comes from this file.
    pub world_file: Option<String>,
    path: PathBuf,
    files: Vec<PathBuf>,
}

impl SaveGame {
    /// Reads a save game folder, telling the game it is for from `SAVEINFO.SAV`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let context = || format!("cannot open save game {}", path.display());
        Self::open_inner(path).context(context)
    }

    fn open_inner(path: &Path) -> Result<Self> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(path).map_err(Error::io(path))? {
            files.push(entry.map_err(Error::io(path))?.path());
        }
        files.sort();
        let read = |name: &str| read(&files, name);

        let data = read("SAVEINFO.SAV")?.ok_or_else(|| Error::NotFound("SAVEINFO.SAV".into()))?;
        let info = SaveInfo::from_bytes(&data).context(|| "SAVEINFO.SAV")?;

        let data = read("SAVEDAT.SAV")?.ok_or_else(|| Error::NotFound("SAVEDAT.SAV".into()))?;
        let data = SaveData::from_bytes(&data, info.game).context(|| "SAVEDAT.SAV")?;

        let thumbnail = match read("THUMB.SAV")? {
            Some(thumb) => Some(Texture::from_bytes(&thumb).context(|| "THUMB.SAV")?),
            None => None,
        };
        let statistics = read("LOG.SAV")?.map(|log| Statistics::from_bytes(&log));
        let world_file = read("SAVEHDR.SAV")?.map(|header| text::latin1(header.trim_ascii()));

        Ok(Self { info, data, thumbnail, statistics, world_file, path: path.to_owned(), files })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn game(&self) -> GameVersion {
        self.info.game
    }

    /// The names of the worlds the save holds, like `NEWWORLD`, sorted.
    pub fn worlds(&self) -> Vec<String> {
        let mut worlds: Vec<String> = (self.files.iter())
            .filter(|path| path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sav")))
            .filter_map(|path| path.file_stem()?.to_str())
            .filter(|stem| !OWN_FILES.iter().any(|own| stem.eq_ignore_ascii_case(own)))
            .map(str::to_owned)
            .collect();
        worlds.sort_by_key(|name| name.to_ascii_uppercase());
        worlds
    }

    /// Reads the state of a world the player visited. The name may be given as in
    /// [`SaveInfo::world`], or as a world file like `NEWWORLD\NEWWORLD.ZEN`, in any case.
    pub fn world(&self, name: &str) -> Result<World> {
        let file = name.rsplit(['\\', '/']).next().unwrap_or(name);
        let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
        let file = format!("{stem}.SAV");
        let data = read(&self.files, &file)?.ok_or_else(|| Error::NotFound(file.clone()))?;
        let _span = tracing::warn_span!("load", file = file.as_str()).entered();
        World::from_bytes_as(&data, self.game()).context(|| format!("cannot read world {file}"))
    }

    /// Reads the state of the world the player was in.
    pub fn current_world(&self) -> Result<World> {
        self.world(&self.info.world)
    }

    /// Reads every world the save holds, in parallel with the `parallel` feature, in the order
    /// of [`worlds`](Self::worlds). One failure does not stop the others.
    pub fn all_worlds(&self) -> Vec<(String, Result<World>)> {
        let names = self.worlds();

        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            // A subscriber set with `with_default` only applies to the thread that set it.
            let dispatch = tracing::dispatcher::get_default(tracing::Dispatch::clone);
            let span = tracing::Span::current();
            names
                .into_par_iter()
                .map(|name| {
                    let world = tracing::dispatcher::with_default(&dispatch, || {
                        span.in_scope(|| self.world(&name))
                    });
                    (name, world)
                })
                .collect()
        }

        #[cfg(not(feature = "parallel"))]
        names.into_iter().map(|name| (name.clone(), self.world(&name))).collect()
    }
}

/// Reads a file of the save, found ignoring case, as the engine does on Windows.
fn read(files: &[PathBuf], name: &str) -> Result<Option<Vec<u8>>> {
    let matches = |path: &&PathBuf| {
        path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(name))
    };
    let Some(path) = files.iter().find(matches) else {
        return Ok(None);
    };
    std::fs::read(path).map(Some).map_err(Error::io(path))
}

/// Closes an object, skipping whatever was left unread in it.
fn end(archive: &mut Archive, object: &ObjectHeader) -> Result<()> {
    if !archive.end_object()? {
        tracing::warn!("object {:?} has unread fields", text::latin1(object.class));
        archive.skip_object()?;
    }
    Ok(())
}
