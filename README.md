# zenkit-rs

A pure Rust reader and writer for the asset formats of the ZenGin, the engine behind *Gothic* and
*Gothic II*. It is a port of [ZenKit](https://github.com/GothicKit/ZenKit) and needs no C++
toolchain.

```toml
[dependencies]
zenkit = { package = "zenkit-rs", git = "https://github.com/Zira3l137/zenkit-rs" }
```

## Features

Every format is a crate of its own, pulled in by the cargo feature of the same name and
re-exported as a module of `zenkit`, so `zenkit::mrm` is the `zenkit-rs-mrm` crate.

| Feature        | Default | What it enables                                                          |
|----------------|:-------:|--------------------------------------------------------------------------|
| `vfs`          |   yes   | `.VDF` archives: mounting them with host directories, and writing them   |
| `archive`      |   yes   | ZenGin archives, the object format behind worlds, saves and more         |
| `material`     |   yes   | Materials (`zCMaterial`), as found in meshes, worlds and `.MAT` files    |
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
| `parallel`     |   yes   | Spreads independent work, like reading many files, across all CPU cores  |
| `mmap`         |         | Memory-maps archives and host files instead of reading them into memory  |
| `log`          |         | Emits every message as a `log` record too, for applications using `log`  |

`parallel` uses [rayon](https://docs.rs/rayon). Turn it off for targets without threads, such as
plain `wasm32`; results are the same either way.

`mmap` saves memory and start-up time with large archives, but if another program changes a mapped
file while it is mounted, the data you are reading changes with it. Leave it off unless you control
the files.

## Writing

Writing is being added format by format. So far these can be written:

| Format         | How                                                                       |
|----------------|---------------------------------------------------------------------------|
| `.VDF`         | `Vfs::write_disk`, or `Node::write_disk` for one directory                |
| ZenGin archive | `archive::ArchiveWriter`, in all three encodings                          |
| Materials      | `Material::write`, `write_list` for `.MAT` files                          |
| Textures       | `Texture::to_bytes`, and `Texture::from_rgba8` to make one from pixels    |

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

```sh
cargo run --release --example world -- "C:\Games\Gothic II" NEWWORLD.ZEN
```

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
every feature of every crate on its own, and a build on the minimum Rust version, 1.88.

Code is formatted with `cargo fmt`. Imports are one per line, grouped std, external crates, then
this crate; stable rustfmt keeps that layout but only nightly enforces it:
`cargo +nightly fmt -- --config imports_granularity=Item,group_imports=StdExternalCrate`.

## License

MIT. Portions are derived from ZenKit, © GothicKit Contributors, also MIT. Test samples under
`samples/` are taken from the ZenKit repository; the VOB samples in `samples/*/VOb` had their
key tables cut off to keep them small, those in `samples/*/SaveVOb` are single objects cut out of
its sample saves, and `samples/*/SAVEDAT.SAV` keep only the first few entries of each list.
