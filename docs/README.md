# Documentation

## For modders

Making mods for Gothic and Gothic II with the Blender extension.

- [Getting started](modding/getting-started.md): installing the extension, pointing it at the game,
  a first round trip, and shipping a mod.
- [Recipes](modding/recipes.md): retexturing, props, armour, animations, creatures and faces, step
  by step.
- [What the game accepts](modding/game-rules.md): the rules exported files have to keep, and why a
  refused file just does not show.
- [Troubleshooting](modding/troubleshooting.md): what goes wrong, by what you see.
- The extension's [README](../blender/README.md): every menu, panel and setting.

## For developers

Using zenkit-rs in programs, and working on it.

- [Using the library](developers/library.md): reading, writing and mounting the game's files from
  Rust, with examples. The API reference is on [docs.rs](https://docs.rs/zenkit-rs).
- [zenkit_py, from Python](developers/python.md): the extension's module as a wheel, for scripts.
- [Working on zenkit-rs](developers/contributing.md): layout, building, testing, conventions, and
  releasing.
- [Engine quirks](developers/engine-quirks.md): what the engine really does, where it, ZenKit and the
  files disagree.
