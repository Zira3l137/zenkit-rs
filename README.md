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
| `parallel` |   yes   | Spreads independent work, like reading many files, across all CPU cores  |
| `mmap`     |         | Memory-maps archives and host files instead of reading them into memory  |

`parallel` uses [rayon](https://docs.rs/rayon). Turn it off for targets without threads, such as
plain `wasm32`; results are the same either way.

`mmap` saves memory and start-up time with large archives, but if another program changes a mapped
file while it is mounted, the data you are reading changes with it. Leave it off unless you control
the files.

## License

MIT. Portions are derived from ZenKit, © GothicKit Contributors, also MIT. Test samples under
`crates/zenkit/tests/samples` are taken from the ZenKit repository.
