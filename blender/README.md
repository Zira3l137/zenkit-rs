# ZenKit Import/Export for Blender

A bare-bones Blender extension (Blender 4.2 to 5.2) that imports and exports Gothic files
through zenkit-rs, mostly to try the library's readers and writers on real work:

| File                             | Import                                | Export                                   |
|----------------------------------|---------------------------------------|------------------------------------------|
| Textures (`.TEX`)                | As an image, packed into the `.blend` | Any image, as DXT1/3/5 or BGRA8          |
| Static meshes (`.MSH`)           | As a mesh, with materials             | The active mesh, polygons as they are    |
| Multi-resolution meshes (`.MRM`) | As a mesh, with materials             | The active mesh, triangulated            |
| Models (`.MDL`, `.MDH`, `.MDM`)  | As an armature with its meshes        | The active armature and the meshes on it |

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

## Preferences

The extension's preferences say where the game's files are. Give the game folder, and the
folders for textures, meshes, models and worlds are worked out from it (`_work/Data/...`, in any
case), or tick Custom Folders and give each one yourself. They are used to:

- open the file browsers in the right folder;
- find the textures a mesh or model's materials name: next to the file, in the textures folder,
  then, with Search the Archives on, in the game's `Data/*.vdf`, where a plain install keeps
  them;
- find the skeleton a `.MDM` belongs to (see below);
- choose the game exported files are for, which the game folder tells unless set.

The worlds folder is there for world import and export, which come next.

## Models

A skeleton becomes an armature, one bone per node of the skeleton. The meshes on it come in two
kinds, kept apart on export the same way:

- **Soft skins**, the bodies that bend with the bones: meshes parented to the armature, with an
  Armature modifier and a vertex group per bone that moves them.
- **Attachments**, rigid meshes like a door or a sword: meshes parented to a bone.

A `.MDL` holds both the skeleton and its meshes; a `.MDH` only the skeleton and a `.MDM` only the
meshes. Since a `.MDM` needs a skeleton, it binds to the active armature if that is the right one,
and otherwise looks for its skeleton next to it, in the models folder and in the game's archives,
by the checksum that ties a skeleton to its meshes and animations. The checksum is kept on the
armature, so meshes exported later still fit the skeleton the game has.

Exporting picks the file by its extension, so one armature can be exported as a whole `.MDL` or
as a `.MDH` and its `.MDM`.

A skeleton made for animation, a `.MDH`, stores its root at the origin and the root's position,
usually the hips, apart from it as the root translation, since animations move the root
themselves. The root bone is put back at that position, where the meshes were skinned, and a
`.MDH` is exported the same way again.

Blender bones point along their Y axis, but a node can point along any of its axes; the bones of
Gothic's skeletons, which come from 3ds Max's Biped, point along X. Each bone is made along the
axis of its node that points to a child, reaching that child, and remembers which axis that was
(the `zenkit_axis` property), so the node is exported exactly as it came in. Bones made in
Blender are nodes as they are.

## How it works

`src/` is the Python module `zenkit_py`, made with [PyO3](https://pyo3.rs). Every mesh crosses
into Python in one shape, whether it comes from a `.MSH`, a `.MRM` or a model: polygons and their
corners, with large arrays as `bytes` that NumPy reads directly. `zenkit_io/` is the extension
itself, which turns that shape into Blender meshes and back.

- **Units and axes.** The engine works in centimetres with Y up, Blender in metres with Z up.
  Import and export scale by 0.01 (an option) and swap Y and Z. The swap mirrors the mesh, which
  also turns the engine's clockwise faces into Blender's counter-clockwise ones.
- **Textures.** Importing a mesh or model plugs each material's compiled texture, `NAME-C.TEX` for
  `NAME.TGA`, into the material, found as the preferences say. Exporting names each material's
  texture after its first image node.
- **Skinning.** Soft skins store, for every vertex, each bone's weight and where the vertex lies
  relative to that bone. Export works those positions out from the armature's rest pose.
- **Double-sided faces** are kept: the engine stores them as two triangles on the same corners,
  which Blender would merge if the mesh were validated, so it is not.

## Limits

- Exported `.MRM` files hold the mesh at full detail only. The data the engine reduces detail
  with (vertex updates, edges) is left empty, and whether the game copes with that is untested.
- Exported materials get the engine's defaults for everything Blender has no equivalent for, like
  the material group that decides footstep sounds.
- Animations (`.MAN`), morph meshes (`.MMB`) and worlds are not covered yet.
- The engine's nodes have no length: bones reach the child they point to, and bones without one
  are half as long as their parent's.
