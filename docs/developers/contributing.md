# Working on zenkit-rs

How the repository is laid out, how to build and test it, the conventions the code keeps, and how
releases are made.

## Layout

| Path                         | Holds                                                               |
|------------------------------|---------------------------------------------------------------------|
| `crates/zenkit-rs`           | The facade: the features, the re-exports, the examples, and tests that span crates |
| `crates/zenkit-rs-core`      | The byte `Reader` and `Writer`, the `Error`, math types, dates, text helpers |
| `crates/zenkit-rs-vfs`       | The virtual file system and `.VDF` archives                         |
| `crates/zenkit-rs-archive`   | ZenGin archives, the object format of worlds, saves and materials   |
| `crates/zenkit-rs-<format>`  | One crate per format, named like its feature                        |
| `blender/`                   | The Blender extension: `src/` the `zenkit_py` module, `zenkit_io/` the extension, outside the workspace |
| `samples/`                   | The files the tests read                                            |
| `docs/`                      | These guides                                                        |

A format crate depends on `zenkit-rs-core` and on the formats it embeds (a world embeds meshes,
which embed materials and textures), never on the facade.

## Building and testing

```sh
cargo test --workspace --all-features
```

runs every crate's tests against `samples/`. **Tests read only files in the repository**, so that
they run anywhere, CI included; the game's files are never committed, and nothing may need them
to pass. Larger checks are optional and run only when their files are there:

- `ZENKIT_SAMPLES` pointing at a ZenKit checkout's `tests/samples` adds ZenKit's complete sample
  saves and worlds, which the tests read and write back.
- In that folder, a copy of Gothic 1's `WORLD.ZEN` named `world.proprietary.zen` and of its German
  `OU.BIN` named `ou.proprietary.bin` add those, as in ZenKit's own tests.

To check a writer against all of the game's files at once, the examples do it from a game folder:
`rewrite_all` writes back every file of a kind (`MAN`, `MRM`, `MSB`…) and groups the differences,
and `rewrite_world` compares a world value by value. Their findings go into fixes and into
[engine-quirks.md](engine-quirks.md); the game's files stay out of the repository.

**The Blender extension** builds on its own:

```sh
cd blender
python build.py                            # dist/zenkit_io-<version>-<platform>.zip
pip install maturin numpy "bpy==4.2.*"     # Blender as a Python module, needs Python 3.11
maturin build --out wheelhouse && pip install wheelhouse/*.whl
python tests/test_addon.py                 # or: blender --background --python tests/test_addon.py
```

`bpy` 5.1 and later need Python 3.13 instead; the module serves both.

## What CI checks

On every push to `main` and every pull request:

- the tests on Linux and Windows;
- `cargo fmt --check`, and clippy and rustdoc with warnings as errors;
- clippy on every feature of every crate on its own (`cargo hack --each-feature`), so no feature
  leans on another being on;
- a build with the oldest Rust supported, 1.88, the workspace's `rust-version`;
- the extension: rustfmt and clippy on `blender/`, then its test in Blender 4.2.

Run the same before sending a change:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo hack clippy --workspace --all-targets --each-feature -- -D warnings
cargo test --workspace --all-features
```

## Conventions

**Follow the game.** Where the engine, ZenKit and the files disagree, the engine and the game's
own files win: readers accept what the game accepts, writers write what the game writes. Check a
writer against the game's files (the rewrite examples) and record what it took in
[engine-quirks.md](engine-quirks.md). Values the engine leaves uninitialised, like padding and
unused flag bits, are not kept: rewritten files read the same, not always byte for byte.

**Use crates.** Where a well-maintained crate does the job (DXT with `texpresso`, zlib with
`flate2`, memory maps with `memmap2`), use it rather than writing one; add it once under
`[workspace.dependencies]`, with a comment on why it is there.

**Work in parallel where it helps**, with rayon, behind the crate's `parallel` feature, so that
targets without threads still build.

**Errors and logging.** Readers fail with `Error::malformed` on data they cannot make sense of,
and add context with `ResultExt::context` where an offset alone would not say where (a file, an
attachment, a VOB). Where they can carry on, they log a `tracing` warning and use a default.
Writers fail with `Error::unwritable` on what the format cannot hold, and warn about what it can
hold but the game refuses. Nothing panics on bad input: every reader has a test that feeds it
truncated data.

**Style.** `cargo fmt`, with imports one per line, grouped std, external, then this crate; stable
rustfmt keeps that layout and nightly enforces it:
`cargo +nightly fmt -- --config imports_granularity=Item,group_imports=StdExternalCrate`.
Comments explain what the code cannot: why a decision was made, what the engine does, what a
field means in the game. They don't narrate the code. Public items have doc comments, and
`missing_debug_implementations`, `unreachable_pub` and the clippy lints in the workspace's
`Cargo.toml` apply to every crate.

**Commits.** One change per commit, in the form `type(scope): what it does`, like
`fix(mesh): read Gothic 1's normal axis from the second flag byte`; the types in use are `feat`,
`fix`, `refactor`, `test`, `docs`, `build` and `ci`, the scope a crate's short name. The body says
why, and what evidence the change rests on. Larger changes come as a series of such commits, each
of which builds and passes on its own.

## Adding a format

1. `crates/zenkit-rs-<name>/` with a `Cargo.toml` that takes `version`, `edition`,
   `rust-version`, `license`, `repository`, `readme`, `keywords` and `categories` from the
   workspace, has a `description`, and ends with `[lints] workspace = true`.
2. A line for it in the root `Cargo.toml`'s `[workspace.dependencies]`, with `version`, so that it
   can be published.
3. In the facade: an optional dependency, a feature of the format's name (in the default list
   unless it is costly), and `pub use zenkit_rs_<name> as <name>;` behind it.
4. The format's type with `from_bytes` and `to_bytes`, and `impl Asset` so that `Vfs::load` reads
   it.
5. Tests against samples in `samples/` (small, and from ZenKit where possible), including writing
   them back and feeding the reader truncated data.
6. A row in the README's feature and writing tables, and its examples in
   [library.md](library.md).

## Releasing

1. Set the new version in the root `Cargo.toml`: `[workspace.package] version`, and every
   `zenkit-rs-*` line of `[workspace.dependencies]`.
2. Set the same version in `blender/Cargo.toml` and `blender/zenkit_io/blender_manifest.toml`.
3. Commit, push, and wait for CI.
4. `cargo publish --workspace --dry-run`, then `cargo publish --workspace`. Cargo publishes the
   crates in dependency order. Brand-new crates are rate limited by crates.io, about five at once
   and then one every ten minutes, so a release that adds one may have to be resumed.
5. Tag it: `git tag v0.2.0 && git push origin v0.2.0`. The Release workflow checks that the tag
   agrees with the three versions, builds the extension and the wheels for Windows and Linux,
   tests the extension, and publishes the GitHub release with notes from the commits.

Every night that `main` changed, the same workflow replaces the `nightly` pre-release; it can be
started by hand from the Actions tab.
