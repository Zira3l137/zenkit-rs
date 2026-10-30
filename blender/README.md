# ZenKit Import/Export for Blender

A bare-bones Blender extension (Blender 4.2 to 5.2) that imports and exports Gothic files
through zenkit-rs, mostly to try the library's readers and writers on real work:

| File                             | Import                                | Export                                |
|----------------------------------|---------------------------------------|---------------------------------------|
| Textures (`.TEX`)                | As an image, packed into the `.blend` | Any image, as DXT1/3/5 or BGRA8       |
| Static meshes (`.MSH`)           | As a mesh, with materials             | The active mesh, polygons as they are |
| Multi-resolution meshes (`.MRM`) | As a mesh, with materials             | The active mesh, triangulated         |

The entries are under File > Import and File > Export, and the image editor's Image menu.

## Building and installing

You need Rust and [maturin](https://www.maturin.rs) (`pip install maturin`). Then:

```sh
python build.py
```

builds the Rust side as a Python module, puts it into the extension and writes
`dist/zenkit_io-<version>-<platform>.zip`. In Blender, install that with Edit > Preferences >
Get Extensions > Install from Disk. The build is for the platform it runs on, so build on
Windows for Windows.

`tests/test_addon.py` checks that it works, importing the repository's samples, exporting them
and reading them back:

```sh
blender --background --python tests/test_addon.py
```

One build serves every supported Blender: the module uses Python's stable ABI from 3.11 on,
which covers Blender 4.2 to 5.0 (Python 3.11) as well as 5.1 and later (Python 3.13).

## How it works

`src/lib.rs` is the Python module `zenkit_py`, made with [PyO3](https://pyo3.rs). Both mesh
formats cross into Python in one shape: polygons and their corners, with large arrays as `bytes`
that NumPy reads directly. `zenkit_io/` is the extension itself, which turns that shape into
Blender meshes and back.

- **Units and axes.** The engine works in centimetres with Y up, Blender in metres with Z up.
  Import and export scale by 0.01 (an option) and swap Y and Z. The swap mirrors the mesh, which
  also turns the engine's clockwise faces into Blender's counter-clockwise ones.
- **Textures.** Importing a mesh looks for each material's compiled texture, `NAME-C.TEX` for
  `NAME.TGA`, next to the mesh and in the folder the import options name, and plugs it into the
  material. Exporting names each material's texture after its first image node.
- **Double-sided faces** are kept: the engine stores them as two triangles on the same corners,
  which Blender would merge if the mesh were validated, so it is not.

## Limits

- Exported `.MRM` files hold the mesh at full detail only. The data the engine reduces detail
  with (vertex updates, edges) is left empty, and whether the game copes with that is untested.
- Exported materials get the engine's defaults for everything Blender has no equivalent for, like
  the material group that decides footstep sounds.
- Models (`.MDL`, `.MDM`), animations and worlds are not covered.
