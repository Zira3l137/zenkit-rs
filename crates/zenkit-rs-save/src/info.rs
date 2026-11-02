use zenkit_rs_archive::Archive;
use zenkit_rs_archive::ArchiveWriter;
use zenkit_rs_archive::Format;
use zenkit_rs_archive::Header;
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

impl SaveInfo {
    /// Writes the file as the games do: an ASCII archive, with Gothic 2's three fields only for
    /// Gothic 2.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.to_bytes_with(Header::new(Format::Ascii))
    }

    /// [`to_bytes`](Self::to_bytes) with this archive header, like the one of the file it was read
    /// from, which also carries when it was saved and by whom.
    pub fn to_bytes_with(&self, header: Header) -> Result<Vec<u8>> {
        let mut w = ArchiveWriter::with_header(header);
        w.begin_object("%", "oCSavegameInfo", 0);
        w.name("Title", &self.title);
        w.name("WorldName", &self.world);
        w.int("TimeDay", self.day);
        w.int("TimeHour", self.hour);
        w.int("TimeMin", self.minute);
        w.name("SaveDate", &self.save_date);
        w.int("VersionMajor", self.version_major);
        w.int("VersionMinor", self.version_minor);
        w.int("PlayTimeSeconds", self.play_time_seconds);
        if self.game == GameVersion::Gothic2 {
            w.int("VersionPoint", self.version_point);
            w.int("VersionInt", self.version_int);
            w.name("VersionAppName", &self.version_app_name);
        }
        w.end_object();
        w.finish()
    }
}

impl Asset for SaveInfo {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        SaveInfo::from_bytes(data)
    }
}
