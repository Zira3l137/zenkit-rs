# Getting started

The ZenKit Import/Export extension brings Gothic's and Gothic II's files into Blender and writes
them back in a form the game loads: textures, props, characters and their armour, animations,
model scripts and morph meshes like faces. This page sets it up and takes one file through it.

## What you need

- **Blender 4.2 to 5.2.**
- **Windows or Linux** for the ready-made builds. On other systems the extension builds from
  source; see [its README](../../blender/README.md#building).
- **The game**, Gothic or Gothic II, installed. The extension reads its archives directly; nothing
  needs unpacking first.

## Installing

1. Download `zenkit_io-<version>-windows-x64.zip` or `…-linux-x64.zip` from the
   [releases](https://github.com/Zira3l137/zenkit-rs/releases). The
   [nightly](https://github.com/Zira3l137/zenkit-rs/releases/tag/nightly) has the newest changes,
   untested beyond the extension's own checks.
2. In Blender: **Edit > Preferences > Get Extensions**, the arrow at the top right, **Install from
   Disk**, and pick the zip.
3. Open the extension's preferences (its entry under **Add-ons**) and set **Game Folder** to where
   the game is installed, like `C:\Games\Gothic II`.

From the game folder, the extension works out the rest:

| Folder   | Where                               | Used for                                          |
|----------|-------------------------------------|---------------------------------------------------|
| Textures | `_work/Data/Textures/_compiled`     | Textures that meshes and models name              |
| Meshes   | `_work/Data/Meshes/_compiled`       | Where the mesh import and export dialogs open     |
| Models   | `_work/Data/Anims/_compiled`        | Skeletons, model meshes, animations, model scripts |
| Archives | `Data`                              | The game's `.VDF` archives, searched for what is not loose |

A plain install keeps everything in the archives in `Data/`, and leaves the `_work` folders empty
or missing; that is fine, as long as **Search the Archives** is on. Tick **Custom Folders** to set
each folder yourself instead. **Game** says which game exported files are for; left on **From the
Game Folder**, it tells by which game is installed there.

## A first round trip

Take a chest from the game, change it, and put it back:

1. **File > Import > Gothic Mesh (.msh, .mrm, .mmb)**, and pick a mesh from the meshes folder, or,
   if it is empty, any `.MRM` you have. Its textures are found and put on its materials.
2. Change it: move vertices, add a lid, whatever you like.
3. **File > Export > Gothic Multi-Resolution Mesh (.mrm)**, and save it under the same name in
   `_work/Data/Meshes/_compiled`.

The game uses a file in `_work` in place of the archived one of the same name only where it is
set up to prefer loose files; otherwise, pack your files into a `.VDF` (below). The
[recipes](recipes.md) go through the common jobs one by one: retexturing, new armour, animations,
creatures and faces.

## Where things are

- **File > Import** and **File > Export** have an entry for every format; the image editor's
  **Image** menu exports the image shown as a texture.
- The **Gothic** tab of the 3D viewport's sidebar (press **N**) edits and exports whole models: a
  model's script, its animations and their settings, its files, and morph meshes' animations. It
  shows for the selected object.
- The extension's [README](../../blender/README.md) describes every option, panel and setting.

## Shipping a mod

A mod reaches players as a `.VDF` archive in the game's `Data/` folder, holding its files under
the same paths as the game's (`_WORK/DATA/TEXTURES/_COMPILED/…`). The archive's date decides
which wins when two hold the same file, the newer one, so a fresh mod archive replaces the game's
files with its own. Any VDF packer will do; with Rust installed, this repository's `pack` example
packs a folder:

```sh
cargo run --release --example pack -- MyMod/_work MyMod.vdf
```

If something does not work, [troubleshooting](troubleshooting.md) lists the usual causes, and
[what the game accepts](game-rules.md) the rules exported files have to keep.
