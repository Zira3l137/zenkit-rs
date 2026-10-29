use zenkit_rs_archive::Archive;
use zenkit_rs_core::Asset;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;

use crate::end;

/// What the load menu shows about a save game (`SAVEINFO.SAV`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveInfo {
    /// Told by which fields the file has: Gothic 2 stores three more.
    pub game: GameVersion,
    pub title: String,
    /// The world the player was in, like `NEWWORLD`.
    pub world: String,
    /// The time of day in the game.
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    /// When the game was saved, as the game shows it, like `28.12.2022 - 18:26`.
    pub save_date: String,
    pub version_major: i32,
    pub version_minor: i32,
    pub play_time_seconds: i32,
    /// Gothic 2 only, like the fields after it.
    pub version_point: i32,
    pub version_int: i32,
    /// Like `Gothic II - 2.6 (fix)`.
    pub version_app_name: String,
}

impl SaveInfo {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut archive = Archive::new(data)?;
        let object = archive.expect_object(b"oCSavegameInfo")?;
        let mut info = Self {
            game: GameVersion::Gothic1,
            title: archive.name()?,
            world: archive.name()?,
            day: archive.int()?,
            hour: archive.int()?,
            minute: archive.int()?,
            save_date: archive.name()?,
            version_major: archive.int()?,
            version_minor: archive.int()?,
            play_time_seconds: archive.int()?,
            version_point: 0,
            version_int: 0,
            version_app_name: String::new(),
        };
        if !archive.end_object()? {
            info.game = GameVersion::Gothic2;
            info.version_point = archive.int()?;
            info.version_int = archive.int()?;
            info.version_app_name = archive.name()?;
            end(&mut archive, &object)?;
        }
        Ok(info)
    }
}

impl Asset for SaveInfo {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        SaveInfo::from_bytes(data)
    }
}
