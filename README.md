# zenkit-rs

[![CI](https://github.com/Zira3l137/zenkit-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/Zira3l137/zenkit-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/zenkit-rs.svg)](https://crates.io/crates/zenkit-rs)
[![docs.rs](https://img.shields.io/docsrs/zenkit-rs)](https://docs.rs/zenkit-rs)

A pure Rust reader and writer for the asset formats of the ZenGin, the engine behind *Gothic* and
*Gothic II*. It is a port of [ZenKit](https://github.com/GothicKit/ZenKit) and needs no C++
toolchain. It also comes as a [Blender extension](#blender) for modders.

```toml
[dependencies]
zenkit = { package = "zenkit-rs", version = "0.1" }
```

or `cargo add zenkit-rs --rename zenkit`. The package is called `zenkit-rs` because `zenkit` on
crates.io is an unrelated project; renaming it lets code say `zenkit::`, as the examples here do.
It needs Rust 1.88 or later. The API is documented on [docs.rs](https://docs.rs/zenkit-rs).

## Features

Every format is a crate of its own, pulled in by the cargo feature of the same name and
re-exported as a module of `zenkit`, so `zenkit::mrm` is the `zenkit-rs-mrm` crate.

| Feature        | Default | What it enables                                                          |
|----------------|:-------:|--------------------------------------------------------------------------|
| `vfs`          |   yes   | `.VDF` archives, Union's compressed ones too: mounting and writing them  |
| `archive`      |   yes   | ZenGin archives, the object format behind worlds, saves and more         |
| `material`     |   yes   | Materials (`zCMaterial`), which compiled meshes and worlds embed         |
| `texture`      |   yes   | Textures (`.TEX`), converting every pixel format to and from RGBA8       |
| `mesh`         |   yes   | Static meshes (`.MSH`), the format of world geometry                     |
| `mrm`          |   yes   | Multi-resolution meshes (`.MRM`), the format of props, items and armour  |
| `model`        |   yes   | Skeletons (`.MDH`), skinned meshes (`.MDM`) and models (`.MDL`)          |
| `morph`        |   yes   | Morph meshes (`.MMB`), animated by moving vertices: faces, bows          |
| `animation`    |   yes   | Skeletal animations (`.MAN`)                                             |
| `model-script` |   yes   | Model scripts, both source (`.MDS`) and compiled (`.MSB`)                |
| `world`        |   yes   | Worlds (`.ZEN`, or saved in a save game): mesh, BSP, way-net, VOBs       |
| `cutscene`     |   yes   | Cutscene libraries (`OU.BIN`, `OU.CSL`): every line of dialogue          |
| `save`         |   yes   | Save games: metadata, script state, quest log, thumbnail and worlds      |
| `font`         |   yes   | Fonts (`.FNT`): where each character is in the font's texture            |
| `parallel`     |   yes   | Spreads independent work, like reading many files, across all CPU cores  |
| `mmap`         |         | Memory-maps archives and host files instead of reading them into memory  |
| `log`          |         | Emits every message as a `log` record too, for applications using `log`  |

`parallel` uses [rayon](https://docs.rs/rayon). Turn it off for targets without threads, such as
plain `wasm32`; results are the same either way.

`mmap` saves memory and start-up time with large archives, but if another program changes a mapped
file while it is mounted, the data you are reading changes with it. Leave it off unless you control
the files.

## Writing

Every format can be written too:

| Format                 | How                                                                    |
|------------------------|------------------------------------------------------------------------|
| `.VDF`                 | `Vfs::write_disk`, or `Node::write_disk` for one directory             |
| ZenGin archive         | `archive::ArchiveWriter`, in all three encodings                       |
| Materials              | `Material::write`, `write_list` for the material lists of meshes       |
| Textures               | `Texture::to_bytes`, and `Texture::from_rgba8` to make one from pixels |
| `.MSH`                 | `Mesh::to_bytes`, or `Mesh::write` for the mesh inside a world         |
| `.MRM`                 | `MultiResolutionMesh::to_bytes`                                        |
| `.MDH`, `.MDM`, `.MDL` | `ModelHierarchy`, `ModelMesh` and `Model::to_bytes`                    |
| `.MMB`                 | `MorphMesh::to_bytes`                                                  |
| `.MAN`                 | `ModelAnimation::to_bytes`, after `fit_sample_positions` for new ones  |
| `.MDS`, `.MSB`         | `ModelScript::to_mds` and `ModelScript::to_msb`                        |
| Cutscene libraries     | `CutsceneLibrary::to_bytes`, as `OU.BIN` or `OU.CSL`                   |
| `.ZEN` worlds          | `World::to_bytes`, or `Vobs::write` for a single VOB                   |
| Save games             | `SaveGame::write` for the folder, or each file's own `to_bytes`        |
| `.FNT`                 | `Font::to_bytes`                                                       |

Writers follow the engine where it and ZenKit differ, so that the game reads what they write, and
where a sample of the game's own files exists, the tests check that rewriting it gives the same
bytes.

## Errors

Every format returns the same `zenkit::Error`. Its message describes only what went wrong; where
it went wrong is in its causes. `Error::report` prints the whole chain:

```rust
match vfs.load::<ModelMesh>("FIREPLACE_HIGH2.MDM") {
    Ok(mesh) => { /* ... */ }
    Err(e) => eprintln!("{}", e.report()),
    // cannot load "FIREPLACE_HIGH2.MDM": attachment "ZM_FIREPLACE": the data ends early:
    // 12 bytes needed at offset 11345, 5 left
}
```

Offsets always count from the start of the file. `Error::root` gives the error under all the
context, to match on what kind of failure it was.

## Logging

The crate reports what it skips or works around, such as unknown chunks or values it replaces
with a default, through [tracing](https://docs.rs/tracing). Nothing is printed unless the
application installs a subscriber, for example with `tracing-subscriber`:

```rust
tracing_subscriber::fmt().with_env_filter("zenkit=warn").init();
```

Every warning raised while loading a file through a `Vfs` sits in a span naming that file. An
application that logs through `log` instead turns on the `log` feature.

## Examples

`crates/zenkit-rs/examples` holds small programs that read files from a Gothic installation, or make
new ones. Each prints how to call it when run without arguments:

| Example        | Shows                                                            |
|----------------|------------------------------------------------------------------|
| `browse`       | Mounting the game's archives and searching them                  |
| `texture`      | Decoding a texture and saving it as a PNG                        |
| `mesh`         | What a static mesh or a morph mesh (a head) is made of           |
| `character`    | Putting an animated model together from its model script         |
| `world`        | A world's mesh, objects and way net                              |
| `world_obj`    | Saving a world's mesh as a Wavefront `.obj` for Blender          |
| `dialogue`     | Looking up lines of dialogue in the cutscene library (`OU.BIN`)  |
| `save`         | A save game: the quest log, the player and the worlds            |
| `pack`         | Packing a folder into a `.VDF` archive, the way mods are shipped |
| `make_texture` | Turning a PNG into a game texture with mipmaps                   |
| `font`         | Writing a line of text with one of the game's fonts, as a PNG    |

```sh
cargo run --release --example world -- "C:\Games\Gothic II" NEWWORLD.ZEN
```

## Not supported

- Daedalus, the games' script language: neither compiled scripts (`.DAT`) nor running them.
- The source formats the games compile their assets from: `.ASC`, `.3DS`, `.MMS` and the like.
  ZenKit does not read them either; the compiled formats above are what the games load.
- Values the engine stores without ever setting them, like padding and stray flag bits. Files
  written back read the same as the originals, but are not always the same byte for byte.

## Blender

`blender/` holds an extension for Blender 4.2 to 5.2 that imports and exports textures, meshes,
models, morph meshes, animations and model scripts through these crates. Download it from the
[releases](https://github.com/Zira3l137/zenkit-rs/releases): each version has a zip for Windows and one for Linux, and the
[nightly](https://github.com/Zira3l137/zenkit-rs/releases/tag/nightly) pre-release follows `main`. Install the zip in Blender with
Edit > Preferences > Get Extensions > Install from Disk. Its [README](blender/README.md) says how
to use it, and how to build it for other systems.

The releases also carry the extension's Python module, `zenkit_py`, as a wheel for Python 3.11 and
later, to use the library from Python outside Blender.

## Testing

`cargo test` runs every crate's tests against the small samples in `samples/`. ZenKit ships much
larger ones, including complete world saves; point `ZENKIT_SAMPLES` at a ZenKit checkout's
`tests/samples` directory to run against those too:

```sh
ZENKIT_SAMPLES=../ZenKit/tests/samples cargo test
```

Like ZenKit's own tests, the world tests also read Gothic 1's `WORLD.ZEN` when a copy of it sits in
that directory as `world.proprietary.zen`, and the cutscene tests read the German Gothic 1 `OU.BIN`
as `ou.proprietary.bin`. Both are part of the game, so they cannot ship here.

## Layout

| Crate                    | Contents                                                          |
|--------------------------|-------------------------------------------------------------------|
| `zenkit-rs`              | The facade: features, re-exports, and tests that span crates      |
| `zenkit-rs-core`         | The byte reader, errors, math types, dates and text helpers       |
| `zenkit-rs-vfs`          | The virtual file system                                           |
| `zenkit-rs-archive`      | ZenGin archives                                                   |
| `zenkit-rs-<format>`     | One crate per format, named like its feature                      |

CI runs the tests on Linux and Windows, clippy and rustdoc with warnings as errors, clippy on
every feature of every crate on its own, a build on the minimum Rust version, 1.88, and the Blender
extension's test in Blender 4.2. Pushing a tag like `v0.2.0` publishes that version's release, once
the tag agrees with the versions in `Cargo.toml`, `blender/Cargo.toml` and the extension's
manifest; every night `main` changed, the nightly pre-release is built anew.

Code is formatted with `cargo fmt`. Imports are one per line, grouped std, external crates, then
this crate; stable rustfmt keeps that layout but only nightly enforces it:
`cargo +nightly fmt -- --config imports_granularity=Item,group_imports=StdExternalCrate`.

## License

MIT. Portions are derived from ZenKit, © GothicKit Contributors, also MIT. Test samples under
`samples/` are taken from the ZenKit repository; the VOB samples in `samples/*/VOb` had their
key tables cut off to keep them small, those in `samples/*/SaveVOb` are single objects cut out of
its sample saves, and `samples/*/SAVEDAT.SAV` keep only the first few entries of each list.
