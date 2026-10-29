//! Loading worlds through a Vfs.

#![cfg(all(feature = "vfs", feature = "world"))]

use std::sync::Arc;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use zenkit::vfs::Node;
use zenkit::vfs::Overwrite;
use zenkit::vfs::Vfs;
use zenkit::world::World;

/// An uncompiled Gothic 2 world with one VOB of a class nobody knows, and one level VOB.
const WORLD: &[u8] = b"ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 0\nEND\nobjects 3\nEND\n\n\
[% oCWorld:zCWorld 64513 0]\n[VobTree % 0 0]\nchilds0=int:2\n\
[% zCVobLevelCompo:zCVob 52224 1]\npack=int:0\npresetName=string:\nbbox3DWS=rawFloat:0 0 0 0 0 0\n\
trafoOSToWSRot=raw:0000803f0000000000000000000000000000803f000000000000000000000000000000000000803f\n\
trafoOSToWSPos=vec3:0 0 0\nvobName=string:LEVEL-VOB\nvisual=string:\nshowVisual=bool:0\n\
visualCamAlign=enum:0\nvisualAniMode=enum:0\nvisualAniModeStrength=float:0\nvobFarClipZScale=float:1\n\
cdStatic=bool:0\ncdDyn=bool:0\nstaticVob=bool:0\ndynShadow=enum:0\nzbias=int:0\nisAmbient=bool:0\n\
[visual % 0 0]\n[]\n[ai % 0 0]\n[]\n[]\nchilds1=int:0\n\
[% zCVobMagic:zCVob 52224 2]\npack=int:0\n[]\nchilds2=int:0\n[]\n\
[EndMarker % 0 0]\n[]\n[]\n";

/// Collects everything a subscriber writes, to look at in a test.
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

#[test]
fn worlds_load_by_name_and_their_warnings_name_them() {
    let mut vfs = Vfs::new();
    vfs.mount(Node::file("WORLD.ZEN", WORLD.to_vec(), UNIX_EPOCH), "/", Overwrite::All).unwrap();

    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .finish();
    let world =
        tracing::subscriber::with_default(subscriber, || vfs.load::<World>("world.zen")).unwrap();

    assert_eq!(world.game, zenkit::GameVersion::Gothic2);
    assert_eq!(world.roots.len(), 1);
    assert_eq!(world.vobs[world.roots[0]].name, "LEVEL-VOB");

    let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains(r#"load{file="world.zen"}"#), "{log}");
    assert!(log.contains(r#"unknown class "zCVobMagic:zCVob""#), "{log}");
}
