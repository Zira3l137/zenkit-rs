# zenkit-rs

A pure Rust reader for the asset formats of the ZenGin, the engine behind *Gothic* and *Gothic II*.
It is a port of [ZenKit](https://github.com/GothicKit/ZenKit) and needs no C++ toolchain.

```toml
[dependencies]
zenkit = { package = "zenkit-rs", git = "https://github.com/Zira3l137/zenkit-rs" }
```

## Features

Every format sits behind its own cargo feature.

| Feature    | Default | What it enables                                                          |
|------------|:-------:|--------------------------------------------------------------------------|
| `vfs`      |   yes   | Reading `.VDF` archives and mounting them together with host directories |
| `archive`  |   yes   | Reading ZenGin archives, the object format behind worlds, saves and more |
| `material` |   yes   | Materials (`zCMaterial`), as found in meshes, worlds and `.MAT` files     |
| `texture`  |   yes   | Textures (`.TEX`), including decoding every pixel format to RGBA8        |
| `mesh`     |   yes   | Static meshes (`.MSH`), the format of world geometry                      |
| `mrm`      |   yes   | Multi-resolution meshes (`.MRM`), the format of props, items and armour  |
| `model`    |   yes   | Skeletons (`.MDH`), skinned meshes (`.MDM`) and models (`.MDL`)          |
| `morph`    |   yes   | Morph meshes (`.MMB`), animated by moving vertices: faces, bows          |
| `animation`|   yes   | Skeletal animations (`.MAN`)                                             |
| `parallel` |   yes   | Spreads independent work, like reading many files, across all CPU cores  |
| `mmap`     |         | Memory-maps archives and host files instead of reading them into memory  |

`parallel` uses [rayon](https://docs.rs/rayon). Turn it off for targets without threads, such as
plain `wasm32`; results are the same either way.

`mmap` saves memory and start-up time with large archives, but if another program changes a mapped
file while it is mounted, the data you are reading changes with it. Leave it off unless you control
the files.

## Testing

`cargo test` runs against the small samples in `crates/zenkit/tests/samples`. ZenKit ships much
larger ones, including complete world saves; point `ZENKIT_SAMPLES` at a ZenKit checkout's
`tests/samples` directory to run against those too:

```sh
ZENKIT_SAMPLES=../ZenKit/tests/samples cargo test
```

## License

MIT. Portions are derived from ZenKit, © GothicKit Contributors, also MIT. Test samples under
`crates/zenkit/tests/samples` are taken from the ZenKit repository.
