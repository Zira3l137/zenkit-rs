//! Loading assets through a Vfs, which ties the file system to the format crates.

#![cfg(all(feature = "vfs", feature = "texture", feature = "model-script"))]

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use zenkit::Error;
use zenkit::model_script::ModelScript;
use zenkit::texture::Texture;
use zenkit::vfs::Node;
use zenkit::vfs::Overwrite;
use zenkit::vfs::Vfs;

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)
}

#[test]
fn load_all_parses_in_order_and_reports_each_failure() {
    let tex = std::fs::read(sample("erz.tex")).unwrap();
    let mut vfs = Vfs::new();
    for (name, data) in [("A.TEX", tex.clone()), ("BROKEN.TEX", b"ZTEX".to_vec()), ("C.TEX", tex)] {
        vfs.mount(Node::file(name, data, UNIX_EPOCH), "/", Overwrite::All).unwrap();
    }

    let results = vfs.load_all::<Texture, _>(&["c.tex", "broken.tex", "missing.tex", "a.tex"]);
    assert_eq!(results.len(), 4);
    assert_eq!(results[0].as_ref().unwrap().width, 128);
    let broken = results[1].as_ref().unwrap_err();
    assert!(matches!(broken.root(), Error::UnexpectedEof { .. }));
    assert!(broken.report().to_string().starts_with("cannot load \"broken.tex\": the data ends"));
    assert!(matches!(results[2], Err(Error::NotFound(_))));
    assert!(results[3].is_ok());
}

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
fn warnings_name_the_file_being_loaded() {
    let script = br#"Model("M") { aniEnum { ani("A" 1 "A" 0 0 M. "A.ASC" F 1 2 {
        *eventTag(0 "DEF_SOMETHING_NEW")
    } } }"#;
    let mut vfs = Vfs::new();
    vfs.mount(Node::file("HUMANS.MDS", script.to_vec(), UNIX_EPOCH), "/", Overwrite::All).unwrap();

    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .finish();
    tracing::subscriber::with_default(subscriber, || vfs.load::<ModelScript>("humans.mds"))
        .unwrap();

    let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains(r#"load{file="humans.mds"}"#), "{log}");
    assert!(log.contains(r#"unknown event type "DEF_SOMETHING_NEW""#), "{log}");
}
