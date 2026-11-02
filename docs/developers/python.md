# zenkit_py, from Python

`zenkit_py` is the Blender extension's Python module, built from zenkit-rs with
[PyO3](https://pyo3.rs). It is small, only as big as the extension needs, but it works on its own
too: for scripts that convert, check or batch-edit the game's files without Blender or Rust.

## Installing

Each [release](https://github.com/Zira3l137/zenkit-rs/releases) carries a wheel for Windows and
one for Linux, for Python 3.11 and later (one build serves them all). Install the one for your
system:

```sh
pip install zenkit_py-0.1.0-cp311-abi3-win_amd64.whl
```

On other systems, build it from `blender/` with [maturin](https://www.maturin.rs):
`pip install maturin`, then `maturin build --release` and install the wheel it makes from
`blender/target/wheels`.

## Conventions

- **Files in, files out.** Every `read_*` takes a path and gives a dict; every `write_*` takes a
  path and a dict like the one read, so a file can be read, changed and written back.
- **Engine coordinates and units**: centimetres, Y up. The extension converts to Blender's; this
  module does not.
- **Large arrays are `bytes`** of little-endian numbers, which NumPy reads without a copy:
  `np.frombuffer(mesh["positions"], "<f4").reshape(-1, 3)`. When writing, `bytes` or anything
  with the buffer protocol will do.
- **The game**, where formats differ, is `"G1"` or `"G2"`.
- **Errors** are raised as `zenkit_py.ZenKitError`, with the whole chain of what went wrong.

```python
import numpy as np
import zenkit_py

mesh = zenkit_py.read_mrm("CHESTBIG_OCCRATELARGE.MRM")
positions = np.frombuffer(mesh["positions"], "<f4").reshape(-1, 3)
print(len(positions), "vertices,", [m[1] for m in mesh["materials"]])

mesh["positions"] = (positions * 2).astype("<f4").tobytes()   # twice as big
zenkit_py.write_mrm("CHESTBIG_OCCRATELARGE_BIG.MRM", mesh, "G2")
```

## Functions

| Function                                   | Does                                                         |
|--------------------------------------------|--------------------------------------------------------------|
| `read_texture(path)`                       | `(width, height, rgba)`, the largest mipmap, rows top down   |
| `write_texture(path, width, height, rgba, format, mipmaps, quality="HIGH")` | A texture: `format` `"DXT1"`, `"DXT3"`, `"DXT5"`, `"B8G8R8A8"` or `"R5G6B5"`; `mipmaps` down to 4×4 for DXT, as the game takes them; `quality` `"HIGH"` or `"FAST"` |
| `read_msh(path)`, `read_mrm(path)`         | A mesh dict                                                  |
| `write_msh(path, name, mesh, game)`        | A static mesh                                                |
| `write_mrm(path, mesh, game)`              | A multi-resolution mesh, at full detail                      |
| `read_model(path)`, `write_model(path, model, game)` | A `.MDL`, `.MDH` or `.MDM`, by the extension          |
| `read_morph(path)`, `write_morph(path, morph, game)` | A morph mesh, `.MMB`                                  |
| `read_animation(path)`, `write_animation(path, animation)` | An animation, `.MAN`                            |
| `read_script(path)`                        | A model script, `.MDS` or `.MSB`, as a script dict           |
| `parse_script(text)`                       | The same, from source text                                   |
| `write_script(path, text)`                 | Source text as `.MDS`, or compiled as `.MSB`, by the extension |
| `format_animation(ani)`                    | One `ani (...)` line, from an entry of a script dict's `animations` |
| `Archives(paths)`                          | `.VDF` archives mounted together, newer winning, as the game does |

`Archives` finds files by name, as the game does: `"NAME" in archives`, `archives.names(".MDH")`
(every file ending so), and `read_model(name)`, `read_animation(name)`, `read_texture(name)`,
which give `None` when no archive has the file.

## Dicts

The Rust sources in `blender/src/` describe each dict at their top; in short:

**Mesh**, the same for every format that holds one:

| Key                 | Holds                                                    |
|---------------------|----------------------------------------------------------|
| `positions`         | `f32` x, y, z per vertex                                 |
| `corner_counts`     | `u32` per polygon: how many corners it has               |
| `corner_vertices`   | `u32` per corner: its vertex                             |
| `corner_uvs`        | `f32` u, v per corner                                    |
| `corner_normals`    | `f32` x, y, z per corner                                 |
| `polygon_materials` | `u32` per polygon: its material                          |
| `materials`         | list of `(name, texture)`, like `("WOOD", "OCWOOD.TGA")` |

**Model**: `nodes`, a list of `(name, parent index or -1, transform)` with the transform relative
to the parent as 16 floats row by row; `root_translation`; `checksum` (read only: writing computes
it from the nodes); `soft_skins`, each a mesh with its weights per vertex; `attachments`, each a
node's name and a mesh. A `.MDH` has no meshes and a `.MDM` no nodes.

**Animation**: `name`, `next`, `layer`, `fps`, `checksum` (of the skeleton it was made for),
`node_indices` (which of the skeleton's nodes it moves), `frame_count`, and `transforms`: per frame,
per node it moves, 16 floats relative to the parent, like the model's.

**Morph mesh**: `name`, `mesh` (of triangles), `morph_positions`, and `animations`, each with its
`vertices` and per frame their offsets in `samples`.

**Script**: `name`, `text` (the source, or `None` with `text_error` saying why), `skeleton`,
`disable_mesh`, `meshes`, and `animations`, one entry per `ani`.

## Not in it

The module covers what the extension imports and exports. Worlds, save games, cutscene libraries,
fonts and writing `.VDF` archives are only in the Rust library for now.
