//! The save samples are ZenKit's, with `SAVEDAT.SAV` cut down to a few entries of each list.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use zenkit_rs_archive::Archive;
use zenkit_rs_archive::Format;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion::Gothic1;
use zenkit_rs_core::GameVersion::Gothic2;
use zenkit_rs_save::CutscenePoolItem;
use zenkit_rs_save::MissionEntry;
use zenkit_rs_save::RunBehavior;
use zenkit_rs_save::SaveData;
use zenkit_rs_save::SaveGame;
use zenkit_rs_save::SaveInfo;
use zenkit_rs_save::Statistics;
use zenkit_rs_save::TopicSection;
use zenkit_rs_save::TopicStatus;

fn sample(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn save_info_tells_the_game() {
    let g1 = SaveInfo::from_bytes(&sample("G1/SAVEINFO.SAV")).unwrap();
    assert_eq!(g1.game, Gothic1);
    assert_eq!((g1.title.as_str(), g1.world.as_str()), ("sds", "WORLD"));
    assert_eq!((g1.day, g1.hour, g1.minute), (0, 8, 6));
    assert_eq!(g1.save_date, "24.12.2022 - 21:36");
    assert_eq!((g1.version_major, g1.version_minor, g1.play_time_seconds), (1, 87, 49));
    assert_eq!(g1.version_app_name, "");

    let g2 = SaveInfo::from_bytes(&sample("G2/SAVEINFO.SAV")).unwrap();
    assert_eq!(g2.game, Gothic2);
    assert_eq!((g2.title.as_str(), g2.world.as_str()), ("uwuowo", "NEWWORLD"));
    assert_eq!((g2.day, g2.hour, g2.minute), (0, 12, 28));
    assert_eq!(g2.save_date, "28.12.2022 - 18:26");
    assert_eq!((g2.version_major, g2.version_minor, g2.play_time_seconds), (2, 6, 1277));
    assert_eq!((g2.version_point, g2.version_int), (0, 0));
    assert_eq!(g2.version_app_name, "Gothic II - 2.6 (fix)");
}

#[test]
fn gothic1_save_data() {
    let data = SaveData::from_bytes(&sample("G1/SAVEDAT.SAV"), Gothic1).unwrap();
    assert_eq!((data.day, data.hour, data.minute), (0, 8, 6));
    let infos: Vec<_> = data.infos.iter().map(|i| (i.name.as_str(), i.told)).collect();
    assert_eq!(
        infos[..2],
        [("DIA_EBR_100_GOMEZ_WAIT4SC", false), ("DIA_EBR_106_BARTHOLO_WAIT4SC", false)]
    );
    assert!(data.log.is_empty() && data.missions.is_empty() && data.cutscene_pool.is_empty());
    assert_eq!(data.symbol("printdebugnpc.type"), Some(&[3][..]));
    assert_eq!(data.symbol("PRINTDEBUGNPC.INSTANCE_ID"), Some(&[1][..]));
    assert_eq!(data.guild_attitudes[1][..8], [2, 3, 3, 3, 3, 3, 2, 1]);
}

#[test]
fn gothic2_save_data() {
    let data = SaveData::from_bytes(&sample("G2/SAVEDAT.SAV"), Gothic2).unwrap();
    assert_eq!((data.day, data.hour, data.minute), (0, 12, 28));
    assert_eq!(data.infos[0].name, "PC_CIRCLE_01");

    let topics: Vec<_> =
        data.log.iter().map(|t| (t.description.as_str(), t.entries.len())).collect();
    assert_eq!(topics, [("Das Auge Innos", 1), ("Der'Ring des Wassers'", 2)]);
    assert_eq!(
        (data.log[0].section, data.log[0].status),
        (TopicSection::Missions, TopicStatus::Running)
    );
    assert!(
        data.log[0].entries[0]
            .starts_with("In der Stadt Khorinis lagert eine Gruppe von Paladinen.")
    );
    assert!(data.log[0].entries[0].contains("verfügen"));

    // Gothic 2 stores arrays whole.
    assert_eq!(data.symbol("PLAYER_TALENT_RUNES").unwrap().len(), 100);
    assert_eq!(data.symbol("FoundHouseInfo"), Some(&[0; 6][..]));
    assert_eq!(data.guild_attitudes.len(), 66);
    assert_eq!(data.guild_attitudes[0][8..13], [2, 0, 2, 0, 3]);
    assert_eq!(data.guild_attitudes[65][60..], [3; 6]);
}

#[test]
fn writes_the_save_files_back() {
    let header = |data: &[u8]| Archive::new(data).unwrap().header().clone();
    for (dir, game) in [("G1", Gothic1), ("G2", Gothic2)] {
        let info = sample(&format!("{dir}/SAVEINFO.SAV"));
        let written = SaveInfo::from_bytes(&info).unwrap().to_bytes_with(header(&info)).unwrap();
        assert_same(&written, &info, dir, "SAVEINFO");

        // The cut-down samples keep the whole key table of the original, so only reading them
        // back can tell; ZenKit's complete saves come out byte for byte the same.
        let data = sample(&format!("{dir}/SAVEDAT.SAV"));
        let read = SaveData::from_bytes(&data, game).unwrap();
        let written = read.to_bytes_with(header(&data), game).unwrap();
        assert_eq!(SaveData::from_bytes(&written, game).unwrap(), read, "{dir}");

        let log = sample(&format!("{dir}/LOG.SAV"));
        assert_same(&Statistics::from_bytes(&log).to_bytes().unwrap(), &log, dir, "LOG");
    }
}

#[test]
fn save_data_keeps_its_shape() {
    let mut data = SaveData::from_bytes(&sample("G1/SAVEDAT.SAV"), Gothic1).unwrap();
    // Without a header of its own, it is written the way the games write it.
    let written = data.to_bytes(Gothic1).unwrap();
    let header = Archive::new(&written).unwrap().header().clone();
    assert!(header.save_game && header.format == Format::Binsafe);
    assert_eq!(SaveData::from_bytes(&written, Gothic1).unwrap(), data);

    // Gothic 2 has room for more guilds.
    assert!(data.to_bytes(Gothic2).is_err());
    data.symbols[0].values.push(1);
    let err = data.to_bytes(Gothic1).unwrap_err();
    assert!(err.to_string().contains("Gothic 1 stores one"), "{}", err.report());
}

/// Missions and pooled cutscenes, which no sample has.
#[test]
fn missions_and_cutscene_pool() {
    let data =
        b"ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 1\nEND\nobjects 2\nEND\n\n\
        day=int:2\nhour=int:3\nmin=int:4\nNumOfEntries=int:0\nNumOfEntries=int:1\n\
        name=string:MIS_ORE\nID=int:7\nAv=bool:1\nNumInList=int:1\nVobIDL=int:12\nStatusL=int:2\n\
        startTimeL=float:1.5\nStatusIndex=int:1\nLOGMANAGERTOPICCOUNT=int:1\n\
        TOPICDESCRIPTION=string:Notes\nTOPICSECTION=enum:1\nTOPICSTATUS=enum:2\n\
        LOGTOPICENTRYCOUNT=int:1\nLOGMANAGERENTRYCOUNT=int:1\nENTRYDESCRIPTION=string:Done.\n\
        [% oCCSManager:zCCSManager 36865 0]\npoolCount=int:2\n\
        [% zCCSPoolItem 0 1]\nitemName=string:INTRO.CS\nrunBehaviour=enum:1\n\
        runBehaviourValue=int:1\nnumPlayed=int:1\ndeactivated=int:0\nflags=int:0\n[]\n\
        [% % 0 0]\n[]\n[]\nnumSymbols=int:0\nguildTable=raw:"
            .iter()
            .copied()
            .chain(std::iter::repeat_n(b'0', 2 * 42 * 42))
            .chain(*b"\n")
            .collect::<Vec<_>>();

    let data = SaveData::from_bytes(&data, Gothic1).unwrap();
    assert_eq!((data.day, data.hour, data.minute), (2, 3, 4));
    let mission = &data.missions[0];
    assert_eq!(
        (mission.name.as_str(), mission.id, mission.available, mission.status_index),
        ("MIS_ORE", 7, true, 1)
    );
    assert_eq!(mission.entries, [MissionEntry { vob_id: 12, status: 2, start_time: 1.5 }]);
    assert_eq!(
        (data.log[0].section, data.log[0].status),
        (TopicSection::Notes, TopicStatus::Success)
    );
    assert_eq!(data.log[0].entries, ["Done."]);
    assert_eq!(
        data.cutscene_pool,
        [CutscenePoolItem {
            name: "INTRO.CS".into(),
            run_behavior: RunBehavior::Times,
            run_behavior_value: 1,
            times_played: 1,
            deactivated: 0,
            flags: 0,
        }]
    );
    assert_eq!(data.guild_attitudes, vec![vec![0; 42]; 42]);

    let written = data.to_bytes(Gothic1).unwrap();
    assert_eq!(SaveData::from_bytes(&written, Gothic1).unwrap(), data);
}

/// A Gothic 2 save world holding a single VOB. Unpacked VOBs do not say whether physics is
/// enabled, which makes it so, and saves store a rigid body for those.
fn tiny_world(name: &str) -> String {
    format!(
        "ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 1\nEND\nobjects 3\nEND\n\n\
         [% oCWorld:zCWorld 64513 0]\n[VobTree % 0 0]\nchilds0=int:1\n\
         [% zCVob 52224 1]\npack=int:0\npresetName=string:\nbbox3DWS=rawFloat:0 0 0 0 0 0\n\
         trafoOSToWSRot=raw:0000803f0000000000000000000000000000803f000000000000000000000000000000000000803f\n\
         trafoOSToWSPos=vec3:0 0 0\nvobName=string:{name}\nvisual=string:\nshowVisual=bool:0\n\
         visualCamAlign=enum:0\nvisualAniMode=enum:0\nvisualAniModeStrength=float:0\n\
         vobFarClipZScale=float:1\ncdStatic=bool:0\ncdDyn=bool:0\nstaticVob=bool:0\ndynShadow=enum:0\n\
         zbias=int:0\nisAmbient=bool:0\n[visual % 0 0]\n[]\n[ai % 0 0]\n[]\n\
         sleepMode=int:0\nnextOnTimer=float:0\nvel=vec3:0 0 0\nmode=int:0\ngravOn=bool:1\n\
         gravScale=float:1\nslideDir=vec3:0 0 0\n[]\nchilds1=int:0\n[]\n\
         [EndMarker % 0 0]\n[]\nnpcCount=int:0\nNoOfEntries=int:0\nspawningEnabled=bool:1\n\
         spawnFlags=int:0\n[]\n"
    )
}

/// A save folder of its own, in the temporary directory, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("zenkit-rs-save-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, data: impl AsRef<[u8]>) {
        std::fs::write(self.0.join(name), data).unwrap();
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn opens_a_save_folder() {
    let folder = Folder::new("open");
    // File names are found ignoring case, as on Windows.
    folder.write("SAVEINFO.SAV", sample("G2/SAVEINFO.SAV"));
    folder.write("savedat.sav", sample("G2/SAVEDAT.SAV"));
    folder.write("Log.sav", sample("G2/LOG.SAV"));
    folder.write("SAVEHDR.SAV", sample("G2/SAVEHDR.SAV"));
    folder.write("newworld.sav", tiny_world("IN_NEWWORLD"));
    folder.write("OLDWORLD.SAV", tiny_world("IN_OLDWORLD"));
    folder.write("notes.txt", "not a world");

    let save = SaveGame::open(&folder.0).unwrap();
    assert_eq!(save.game(), Gothic2);
    assert_eq!(save.info.title, "uwuowo");
    assert_eq!(save.data.log.len(), 2);
    assert!(save.thumbnail.is_none());
    assert_eq!(save.world_file.as_deref(), Some("NEWWORLD\\NEWWORLD.ZEN"));
    let stats = save.statistics.as_ref().unwrap();
    assert_eq!(stats.columns.len(), 21);
    assert_eq!(stats.column("experience").last(), Some("490"));

    assert_eq!(save.worlds(), ["newworld", "OLDWORLD"]);
    let name_of = |world: zenkit_rs_world::World| world.vobs[world.roots[0]].name.clone();
    assert_eq!(name_of(save.current_world().unwrap()), "IN_NEWWORLD");
    assert_eq!(name_of(save.world("oldworld\\OLDWORLD.ZEN").unwrap()), "IN_OLDWORLD");
    assert!(matches!(save.world("ADDONWORLD").unwrap_err().root(), Error::NotFound(_)));

    let all: Vec<_> =
        save.all_worlds().into_iter().map(|(n, w)| (n, name_of(w.unwrap()))).collect();
    assert_eq!(
        all,
        [("newworld".into(), "IN_NEWWORLD".into()), ("OLDWORLD".into(), "IN_OLDWORLD".into())]
    );
}

#[test]
fn writes_a_save_folder() {
    let source = Folder::new("write-source");
    source.write("SAVEINFO.SAV", sample("G2/SAVEINFO.SAV"));
    source.write("SAVEDAT.SAV", sample("G2/SAVEDAT.SAV"));
    source.write("LOG.SAV", sample("G2/LOG.SAV"));
    source.write("SAVEHDR.SAV", sample("G2/SAVEHDR.SAV"));
    source.write("NEWWORLD.SAV", tiny_world("IN_NEWWORLD"));
    source.write("OLDWORLD.SAV", tiny_world("IN_OLDWORLD"));
    let mut save = SaveGame::open(&source.0).unwrap();

    // A world left over from another save in the target folder must go.
    let target = Folder::new("write-target");
    target.write("ADDONWORLD.SAV", tiny_world("STALE"));
    target.write("notes.txt", "not a world");

    save.info.title = "renamed".into();
    let mut world = save.current_world().unwrap();
    let root = world.roots[0];
    world.vobs[root].name = "CHANGED".into();
    save.write(&target.0, &[("NEWWORLD\\NEWWORLD.ZEN", &world)]).unwrap();

    let back = SaveGame::open(&target.0).unwrap();
    assert_eq!(back.info, save.info);
    assert_eq!(back.data, save.data);
    assert_eq!(back.statistics, save.statistics);
    assert_eq!(back.world_file, save.world_file);
    assert_eq!(back.worlds(), ["NEWWORLD", "OLDWORLD"]);
    assert_eq!(back.current_world().unwrap(), world);
    let old = back.world("OLDWORLD").unwrap();
    assert_eq!(old.vobs[old.roots[0]].name, "IN_OLDWORLD", "copied as it was");
    assert!(target.0.join("notes.txt").exists());

    // Into its own folder, the worlds it is not given stay.
    save.write(&source.0, &[]).unwrap();
    assert_eq!(SaveGame::open(&source.0).unwrap().worlds(), ["NEWWORLD", "OLDWORLD"]);

    let mut gothic1 = world.clone();
    gothic1.game = Gothic1;
    let err = save.write(&target.0, &[("NEWWORLD", &gothic1)]).unwrap_err();
    assert!(err.report().to_string().contains("is for Gothic1"), "{}", err.report());
}

#[test]
fn a_new_save_has_no_worlds() {
    let info = SaveInfo::from_bytes(&sample("G1/SAVEINFO.SAV")).unwrap();
    let data = SaveData::from_bytes(&sample("G1/SAVEDAT.SAV"), Gothic1).unwrap();
    let save = SaveGame::new(info, data);
    assert!(save.worlds().is_empty() && save.thumbnail.is_none());

    let folder = Folder::new("new");
    save.write(&folder.0, &[]).unwrap();
    let back = SaveGame::open(&folder.0).unwrap();
    assert_eq!((back.info, back.data), (save.info, save.data));
    let mut files: Vec<_> = std::fs::read_dir(&folder.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    assert_eq!(files, ["SAVEDAT.SAV", "SAVEINFO.SAV"]);
}

#[test]
fn a_folder_without_save_data_is_no_save() {
    let folder = Folder::new("missing");
    folder.write("SAVEINFO.SAV", sample("G1/SAVEINFO.SAV"));
    let err = SaveGame::open(&folder.0).unwrap_err();
    assert!(
        matches!(err.root(), Error::NotFound(name) if name == "SAVEDAT.SAV"),
        "{}",
        err.report()
    );

    let err = SaveGame::open(folder.0.join("nowhere")).unwrap_err();
    assert!(matches!(err.root(), Error::Io { .. }), "{}", err.report());
}

/// Collects everything a subscriber writes.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// ZenKit's complete sample saves, if `ZENKIT_SAMPLES` points at a ZenKit checkout's
/// `tests/samples`. The metadata checks are ZenKit's.
#[test]
fn zenkit_sample_saves() {
    let Some(root) = std::env::var_os("ZENKIT_SAMPLES").map(PathBuf::from) else {
        eprintln!("ZENKIT_SAMPLES is not set, skipping");
        return;
    };

    let saves = [
        ("G1/Save", Gothic1, "sds", "WORLD", (0, 8, 6), 49, 1538),
        ("G1/SaveFast", Gothic1, "sds_fast", "WORLD", (0, 8, 6), 60, 1538),
        ("G2/Save", Gothic2, "uwuowo", "NEWWORLD", (0, 12, 28), 1277, 4139),
        ("G2/SaveFast", Gothic2, "inminevalley", "OLDWORLD", (0, 0, 0), 433, 4139),
    ];
    for (dir, game, title, world, time, play_time, infos) in saves {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .finish();
        let (save, worlds) = tracing::subscriber::with_default(subscriber, || {
            let save = SaveGame::open(root.join(dir)).unwrap();
            let worlds = save.all_worlds();
            (save, worlds)
        });

        assert_eq!(save.game(), game, "{dir}");
        assert_eq!((save.info.title.as_str(), save.info.world.as_str()), (title, world), "{dir}");
        assert_eq!((save.data.day, save.data.hour, save.data.minute), time, "{dir}");
        assert_eq!(save.info.play_time_seconds, play_time, "{dir}");
        assert_eq!(save.data.infos.len(), infos, "{dir}");
        let thumbnail = save.thumbnail.as_ref().unwrap();
        assert_eq!((thumbnail.width, thumbnail.height), (256, 256), "{dir}");
        assert!(!worlds.is_empty() && worlds.iter().all(|(_, w)| w.is_ok()), "{dir}");

        let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(!log.contains("unread") && !log.contains("goes on past"), "{dir}: {log}");

        let original = |name: &str| std::fs::read(root.join(dir).join(name)).unwrap();
        let header = |data: &[u8]| Archive::new(data).unwrap().header().clone();
        let info = original("SAVEINFO.SAV");
        assert_same(&save.info.to_bytes_with(header(&info)).unwrap(), &info, dir, "SAVEINFO");
        let data = original("SAVEDAT.SAV");
        let written = save.data.to_bytes_with(header(&data), game).unwrap();
        assert_same(&written, &data, dir, "SAVEDAT");
        if let Some(stats) = &save.statistics {
            assert_same(&stats.to_bytes().unwrap(), &original("LOG.SAV"), dir, "LOG");
        }
        assert_same(&thumbnail.to_bytes(), &original("THUMB.SAV"), dir, "THUMB");

        // The whole save, with every world written anew, opens again as it was.
        let worlds: Vec<_> =
            worlds.into_iter().map(|(name, world)| (name, world.unwrap())).collect();
        let given: Vec<_> = worlds.iter().map(|(name, world)| (name.as_str(), world)).collect();
        let folder = Folder::new(&format!("zenkit-{}", dir.replace('/', "-")));
        save.write(&folder.0, &given).unwrap();
        let back = SaveGame::open(&folder.0).unwrap();
        assert_eq!(
            (&back.info, &back.data, &back.thumbnail),
            (&save.info, &save.data, &save.thumbnail)
        );
        let back_worlds: Vec<_> =
            back.all_worlds().into_iter().map(|(n, w)| (n, w.unwrap())).collect();
        assert!(back_worlds == worlds, "{dir}");
    }
}

/// Checks that a file written back is the original, saying where they first differ if not.
fn assert_same(written: &[u8], original: &[u8], dir: &str, what: &str) {
    if let Some(at) = written.iter().zip(original).position(|(a, b)| a != b) {
        let around = |data: &[u8]| {
            String::from_utf8_lossy(&data[at.saturating_sub(24)..(at + 24).min(data.len())])
                .into_owned()
        };
        panic!(
            "{dir} {what}: first difference at byte {at}:\n  game    {:?}\n  written {:?}",
            around(original),
            around(written)
        );
    }
    assert_eq!(written.len(), original.len(), "{dir} {what}: the lengths differ");
}
