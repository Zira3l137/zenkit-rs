use tracing::warn;
use zenkit_rs_archive::Archive;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::ResultExt;
use zenkit_rs_core::raw_enum;
use zenkit_rs_core::text;

use crate::FORMAT;
use crate::end;

/// How many guilds the attitude table has room for, in both games.
pub const GUILDS: usize = 42;

raw_enum! {
    /// Which part of the quest log a topic is in.
    pub enum TopicSection: u8 = Missions {
        Missions = 0,
        Notes = 1,
    }
}

raw_enum! {
    pub enum TopicStatus: u8 = Free {
        Free = 0,
        Running = 1,
        Success = 2,
        Failure = 3,
        Obsolete = 4,
    }
}

raw_enum! {
    /// How often a pooled cutscene may play.
    pub enum RunBehavior: u8 = Always {
        Always = 0,
        /// At most [`run_behavior_value`](CutscenePoolItem::run_behavior_value) times.
        Times = 1,
        PerHour = 2,
    }
}

/// The state of the scripts and the game around them (`SAVEDAT.SAV`).
#[derive(Debug, Clone, PartialEq)]
pub struct SaveData {
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    /// The dialogue options the player has seen.
    pub infos: Vec<InfoState>,
    pub missions: Vec<Mission>,
    /// The quest log.
    pub log: Vec<LogTopic>,
    /// The cutscenes the game keeps track of, and how often they played.
    pub cutscene_pool: Vec<CutscenePoolItem>,
    /// The values of the scripts' global variables.
    pub symbols: Vec<SymbolState>,
    /// How each guild feels about each other one, by guild number.
    pub guild_attitudes: [[u8; GUILDS]; GUILDS],
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InfoState {
    /// The `C_INFO` script instance, like `DIA_ARTO_PERM`.
    pub name: String,
    pub told: bool,
}

/// A mission, the quest system Gothic 1 was planned with. Both games still store one per
/// `C_MISSION` script instance.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mission {
    pub name: String,
    pub id: i32,
    pub available: bool,
    pub status_index: i32,
    pub entries: Vec<MissionEntry>,
}

/// Where one VOB stands in a [`Mission`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MissionEntry {
    pub vob_id: i32,
    pub status: i32,
    pub start_time: f32,
}

/// A topic of the quest log.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LogTopic {
    pub description: String,
    pub section: TopicSection,
    pub status: TopicStatus,
    /// What was written about it, oldest first.
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CutscenePoolItem {
    pub name: String,
    pub run_behavior: RunBehavior,
    pub run_behavior_value: i32,
    pub times_played: i32,
    pub deactivated: i32,
    pub flags: i32,
}

/// A global script variable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SymbolState {
    pub name: String,
    /// One value in Gothic 1. Gothic 2 stores arrays whole.
    pub values: Vec<i32>,
}

impl SaveData {
    pub fn from_bytes(data: &[u8], game: GameVersion) -> Result<Self> {
        let mut archive = Archive::new(data)?;
        let data = Self::read(&mut archive, game)?;
        if !archive.is_eof() {
            warn!("the save data goes on past offset {}, where reading it ended", archive.offset());
        }
        Ok(data)
    }

    fn read(r: &mut Archive, game: GameVersion) -> Result<Self> {
        let (day, hour, minute) = (r.int()?, r.int()?, r.int()?);

        let count = r.int()?;
        let mut infos = Vec::new();
        for _ in 0..count {
            let told = r.bool()?;
            infos.push(InfoState { name: r.name()?, told });
        }

        let count = r.int()?;
        let mut missions = Vec::new();
        for i in 0..count {
            missions.push(read_mission(r).context(|| format!("mission {i}"))?);
        }

        let count = r.int()?;
        let mut log = Vec::new();
        for i in 0..count {
            log.push(read_topic(r).context(|| format!("log topic {i}"))?);
        }

        let cutscene_pool = read_cutscene_manager(r).context(|| "cutscene manager")?;

        let count = r.int()?;
        let mut symbols = Vec::new();
        for _ in 0..count {
            let name = r.name()?;
            let values = match game {
                GameVersion::Gothic1 => vec![r.int()?],
                GameVersion::Gothic2 => {
                    let count = r.int()?;
                    (0..count).map(|_| r.int()).collect::<Result<_>>()?
                }
            };
            symbols.push(SymbolState { name, values });
        }

        let raw = r.raw(GUILDS * GUILDS)?;
        let mut guild_attitudes = [[0; GUILDS]; GUILDS];
        for (row, bytes) in guild_attitudes.iter_mut().zip(raw.chunks_exact(GUILDS)) {
            row.copy_from_slice(bytes);
        }

        Ok(Self {
            day,
            hour,
            minute,
            infos,
            missions,
            log,
            cutscene_pool,
            symbols,
            guild_attitudes,
        })
    }

    /// The value of a global script variable, by name, ignoring case like the scripts.
    pub fn symbol(&self, name: &str) -> Option<&[i32]> {
        let symbol = self.symbols.iter().find(|s| s.name.eq_ignore_ascii_case(name))?;
        Some(&symbol.values)
    }
}

fn read_mission(r: &mut Archive) -> Result<Mission> {
    let mut mission =
        Mission { name: r.name()?, id: r.int()?, available: r.bool()?, ..Mission::default() };
    let count = r.int()?;
    for _ in 0..count {
        mission.entries.push(MissionEntry {
            vob_id: r.int()?,
            status: r.int()?,
            start_time: r.float()?,
        });
    }
    mission.status_index = r.int()?;
    Ok(mission)
}

fn read_topic(r: &mut Archive) -> Result<LogTopic> {
    let mut topic = LogTopic {
        description: r.name()?,
        section: TopicSection::from_raw(r.enumeration()?),
        status: TopicStatus::from_raw(r.enumeration()?),
        entries: Vec::new(),
    };
    let count = r.int()?;
    // The log manager's own count of the entries, which is always the same.
    r.int()?;
    for _ in 0..count {
        topic.entries.push(r.name()?);
    }
    Ok(topic)
}

fn read_cutscene_manager(r: &mut Archive) -> Result<Vec<CutscenePoolItem>> {
    let offset = r.offset();
    let Some(object) = r.begin_object()? else {
        return Err(Error::malformed(
            FORMAT,
            format!("expected the cutscene manager at offset {offset}"),
        ));
    };
    if object.class != b"oCCSManager:zCCSManager" {
        if !object.is_null() {
            warn!("skipping cutscene manager of class {:?}", text::latin1(object.class));
        }
        r.skip_object()?;
        return Ok(Vec::new());
    }

    let mut pool = Vec::new();
    let count = r.int()?;
    for _ in 0..count {
        let offset = r.offset();
        let Some(item) = r.begin_object()? else {
            return Err(Error::malformed(
                FORMAT,
                format!("expected a pool item at offset {offset}"),
            ));
        };
        if item.class != b"zCCSPoolItem" {
            if !item.is_null() {
                warn!("skipping cutscene pool item of class {:?}", text::latin1(item.class));
            }
            r.skip_object()?;
            continue;
        }
        pool.push(CutscenePoolItem {
            name: r.name()?,
            run_behavior: RunBehavior::from_raw(r.enumeration()?),
            run_behavior_value: r.int()?,
            times_played: r.int()?,
            deactivated: r.int()?,
            flags: r.int()?,
        });
        end(r, &item)?;
    }
    end(r, &object)?;
    Ok(pool)
}
