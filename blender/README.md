# ZenKit Import/Export for Blender

A bare-bones Blender extension (Blender 4.2 to 5.2) that imports and exports Gothic files
through zenkit-rs, mostly to try the library's readers and writers on real work:

| File                             | Import                                | Export                                   |
|----------------------------------|---------------------------------------|------------------------------------------|
| Textures (`.TEX`)                | As an image, packed into the `.blend` | Any image, as DXT1 or DXT3               |
| Static meshes (`.MSH`)           | As a mesh, with materials             | The active mesh, polygons as they are    |
| Multi-resolution meshes (`.MRM`) | As a mesh, with materials             | The active mesh, triangulated            |
| Models (`.MDL`, `.MDH`, `.MDM`)  | As an armature with its meshes        | The active armature and the meshes on it |
| Morph meshes (`.MMB`)            | As a mesh with shape keys             | The active mesh and its shape keys       |
| Animations (`.MAN`)              | As actions on the active armature     | The active armature's action             |
| Model scripts (`.MDS`, `.MSB`)   | As an armature, its meshes, actions   | A text, and its animations' actions      |

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
- find the skeleton a `.MDM` belongs to (see below), and everything a model script names;
- choose the game exported files are for, which the game folder tells unless set.

The archives are mounted once and memory-mapped, so only the parts that are looked up are read,
however large they are. On Windows a mapped file cannot be changed, so while Blender holds them,
a mod's `.vdf` cannot be rebuilt in place; changing the archive preferences, or turning Search the
Archives off, lets go of them.

The worlds folder is there for world import and export, which come next.

## Models

A skeleton's nodes come in three kinds, told apart by their names as Gothic's own tools do:

- **Bones**, most nodes (`BIP01 SPINE`), make up an armature.
- **Slots** (`ZS_`), the points weapons and items are held at, become small diamond-shaped
  placeholder meshes hanging from their bone.
- **Mesh nodes** (`ZM_`), the places of rigid parts like a door, become the object of their
  mesh, hanging from their bone, or a placeholder while the mesh is not there.

A model's meshes come in two kinds:

- **Soft skins**, the bodies that bend with the bones: meshes parented to the armature, with an
  Armature modifier and a vertex group per bone that moves them. They are named `ZM_SKIN_0`,
  `ZM_SKIN_1` and so on.
- **Attachments**, rigid meshes: the object of a mesh node, named after it, or a mesh parented to
  a bone, named `ZM_FRILL_0`, `ZM_FRILL_1` and so on.

These are the names the engine's own tools use. When exporting, objects hanging from the
armature are told apart by name where they carry no mark from an import: `ZS_` is a slot, so name
new slots that way, and `ZM_` a mesh node, except `ZM_SKIN` and `ZM_FRILL`, which stay meshes.

A `.MDL` holds a skeleton and its meshes, a `.MDH` only the skeleton and a `.MDM` only the
meshes. Each file's meshes go into a collection named after it, like
`HUM_BODY_NAKED0.MDM`, inside the armature's, and the Export panel writes each collection back to
its file. A `.MDM` brings in just its meshes. They go onto the active armature if that is their
skeleton, which the checksum that ties a skeleton to its meshes and animations tells. Otherwise
they come in on their own, their vertex groups named after the skeleton's bones if the skeleton
is found next to the file, in the models folder or in the game's archives, so that they bind to
it later. To put a body on a skeleton, import the `.MDH`, then the `.MDM` while the armature is
still active.

The **checksum** is the CRC-32 of the skeleton's node names, one after the other. The engine
computes it, and so does exporting, from the nodes it writes. A `.MDM` stores the checksum of the
skeleton it was made for, and the game refuses its skinned meshes on any other ("the file's
node-hierarchy is incompatible to 'meshAndTree'"), so a `.MDM` has to be exported from the armature
of the skeleton that will wear it. An armor's `.MDL` has a skeleton of its own, without the slots
and so with another checksum; to make the `.MDM` worn by humans from it, import `HUMANS.MDH`, then
the armor's `.MDL` with **Use Active Armature** ticked, which puts its meshes on the `HUMANS`
armature when that has all of the model's bones, and export the `.MDM` from there.

The export's Format option says what to write: the whole model (`.MDL`), the skeleton alone
(`.MDH`) or the meshes alone (`.MDM`), so one armature can go out as a `.MDL` or as a `.MDH` and
its `.MDM`. The file name's extension follows the format, and typing one of the three into the name
picks the format. Bones, slots and mesh nodes go out in the order the file had them,
since animations refer to nodes by their place, and new ones after.

The armature keeps the **root translation**: an offset files keep apart from
the root, which the engine moves the root by in the rest pose and which animations replace with
their own. It usually holds the height of the hips, so the skeleton stands where its meshes do.

Blender bones point along their Y axis, but a node can point along any of its axes; the bones of
Gothic's skeletons, which come from 3ds Max's Biped, point along X. Each bone runs from its node
to the head of its main child bone, the one most in line with an axis of the node, turned from
the node as little as that takes. The turn is kept on the bone (the `zenkit_turn` property), so
the node is exported exactly as it came in. Bones made in Blender are nodes as they are.
Armatures are drawn in front of their meshes.

## The Gothic tab

Everything needed to edit and export models, animations and morph meshes is in the **Gothic** tab
of the 3D viewport's sidebar (N), for the selected object; the File menu's entries do the same
one file at a time.

**Model**, when an armature or anything on it is selected: the model's name, which its files are
named after (`HUMANS` makes `HUMANS.MSB` and `HUMANS-S_RUN.MAN`), and its script, a text in the
Text Editor. Importing fills both in; **New** makes a script for a model that has none, with its
skeleton and the meshes on it registered by their files.

**Animations** lists the model's animations, each an action. Clicking one plays it on the armature
and sets the scene's frames and frame rate to it; below the list are its settings:

| Setting            | What it does in the game                                               |
|--------------------|------------------------------------------------------------------------|
| Name               | The name scripts and the game call it by, like `S_RUN`                 |
| Layer              | Animations on different layers play at once, the higher on top         |
| Next               | The animation that follows when this one ends                          |
| Blend In/Out       | Seconds it takes to blend in and out                                   |
| Flags              | Move, Rotate, Queue, Fly, Idle, as the script's letters `M R E F I`    |
| Frame Rate         | Frames per second the game plays it at                                 |
| Manual Frame Range | Which frames of the action go out; all its keys if off                 |

**+** adds an animation that starts from the current pose, **-** deletes the selected one, and the
other two buttons import `.MAN` files and export the selected animation.

**Export** says where files go (the models folder of the preferences unless a folder is given)
and for which game, lists the files the model will go out as, and writes the ticked ones:

- the **skeleton**, `.MDH`, named as the engine looks for it (see Model scripts), or a `.MDL` when
  the meshes are in a collection of that name;
- the **meshes**, one `.MDM` per collection named after one, as importing makes them; meshes in no
  such collection go into the model's main mesh file;
- the **script**, `.MSB`, and `.MDS` source if ticked, with every animation's settings written into
  its `ani` and new animations added, while the rest of it, events included, stays as written;
- the **animations**, one `.MAN` each.

**Morph Mesh**, when a mesh is selected: its morph animations, each a run of shape keys. Selecting
one, and a frame of it with the Frame slider, shows that frame and makes its shape key the active
one, so Edit Mode edits it. **+** adds an animation of one frame, the key button adds a frame (a
copy of the last), and **-** deletes one with its shape keys. Its settings:

| Setting       | What it does in the game                                                        |
|---------------|---------------------------------------------------------------------------------|
| Layer         | Animations on different layers play at once                                     |
| Frame Rate    | Frames per second                                                               |
| Until Stopped | Plays until the game stops it; otherwise for the Duration                       |
| Repeat        | Plays the frames at the frame rate over and over, rather than once spread over the duration |
| Blend In/Out  | As the game's files have them                                                   |

The button below writes the mesh as `<NAME>.MMB` into the export folder.

## Animations

An animation stores, frame by frame, the transform of some of a skeleton's nodes relative to
their parent, naming each node by its place in the skeleton. It goes onto the active armature,
which must have been imported from the skeleton it was made for, so that its bones know their
place: importing warns when the animation's checksum is not the armature's. Several `.MAN` files
can be picked at once.

Each animation becomes an action of the armature's model, one key per frame of the file from
frame 0 on, with linear interpolation as the engine blends between frames. Bones get location and
rotation keys. Slots and mesh nodes are keyed only when the animation moves them relative to their
bone, which needs Blender 4.4 or later, whose actions can animate several objects at once; older
versions warn and leave them where they hang. Its name in the game, layer, next animation and
frame rate become its settings in the Animations panel.

Exporting plays the action over its frame range and stores every node of the skeleton in every
frame, whatever moves it: keys, constraints or drivers.

## Morph meshes

A morph mesh is a mesh whose vertices animations move, like a talking face or a flag in the wind.
Each frame of an animation becomes a shape key, named after the animation when it has one frame
(`S_NORMAL`) or followed by the frame's number when it has more (`S_SHOOT 0`, `S_SHOOT 1`…). The
keys start at 0. Each animation also gets an action, `<MESH> <ANIMATION>`, that plays it on the
keys, one scene frame per animation frame, to watch it; the first is assigned, and the others are
picked in the Shape Key Editor. Exporting reads the keys, not these actions. Exporting does the reverse, whatever the keys'
values are. Each animation's settings are in the Morph Mesh panel; which vertices it moves is kept
too, so that ones it lists but leaves where they are stay in it. Vertices moved in Blender join
their animation, and shape keys named like an animation the panel does not list export as a new
one, with settings like the game's: a single frame holds its shape until stopped, more frames play
once at 25 frames a second.

## Model scripts

A model script names a model's skeleton, the meshes it can wear and its animations; the engine
loads each animation from a file named after the model and it, like `HUMANS-S_RUN.MAN`.
Importing a script brings in all of that, looked for next to the script, in the models folder and
in the game's archives, into a collection named after the model: an armature of the skeleton, the
first mesh the script registers on it (Meshes picks all or none instead), and an action per
animation, with the settings the script gives it. The Animations field picks which by name, with
`*` for anything (`S_*RUN*`), since the humans' script names over a thousand.

The skeleton is looked for under the name the engine compiles it with. When the script leaves out
the mesh of its `meshAndTree` source (`DONT_USE_MESH`), that's the model's name: `HUMANS.MDS` has
`meshAndTree ("HUM_BODY_NAKED0.ASC" DONT_USE_MESH)`, and its skeleton is `HUMANS.MDH`, while
`HUM_BODY_NAKED0.MDM` is only a body. Otherwise it's the source's name, with its mesh in a `.MDM`
of the same name, or both in a `.MDL`.

The script itself becomes a text in the Text Editor, as source, to edit there; a compiled `.MSB`
is turned into source on the way. Aliases, blends, events and the like are edited there; what the
Animations panel shows for an animation is written into its `ani` on export, so set those in the
panel. The frame numbers in an `ani` say which frames of its source file to take, which only
matters when the engine compiles animations from those; the action's frames are what the `.MAN`
holds. Events stay in the script, where the game reads them.

## How it works

`src/` is the Python module `zenkit_py`, made with [PyO3](https://pyo3.rs). Every mesh crosses
into Python in one shape, whether it comes from a `.MSH`, a `.MRM` or a model: polygons and their
corners, with large arrays as `bytes` that NumPy reads directly. `zenkit_io/` is the extension
itself, which turns that shape into Blender meshes and back.

- **Units and axes.** The engine works in centimetres with Y up, characters facing +Z; Blender in
  metres with Z up, its front view looking at -Y. Import and export scale by 0.01 (an option) and
  turn engine (x, y, z) into Blender (-x, -z, y), so characters face Blender's front. That mirrors
  the mesh, which also turns the engine's clockwise faces into Blender's counter-clockwise ones.
- **Textures.** Importing a mesh or model plugs each material's compiled texture, `NAME-C.TEX` for
  `NAME.TGA`, into the material, found as the preferences say, its alpha too if it has any.
  Exporting names each material's texture after its first image node. Imported textures are
  packed into the `.blend` as uncompressed TGA, which is quick to write, where Blender would
  spend most of the import compressing them to PNG. Textures export as DXT1 or DXT3, the two
  formats the game's renderer shows (it refuses DXT5, and has nothing to show uncompressed
  32-bit ones with), with mipmaps down to 4×4, below which it refuses DXT textures. Their sides
  should be powers of two, as the game's are; exporting warns when they are not. Exporting
  takes a quality: High searches each 4×4 block for its best two colors, which is slow on large
  detailed images; Fast is many times quicker at the cost of somewhat blotchier detail.
- **Animations.** Each frame stores a node's rotation as a quaternion and its position. The engine
  builds the node's matrix from the quaternion's conjugate, as OpenGothic does, so the extension
  does too, and the result is the same kind of transform as the skeleton's. A root's position in
  an animation is its whole position, root translation included, so it replaces the rest pose's.
  The positions are packed into 16 bits over the range they span, so exported ones are within a
  few hundredths of a millimetre of Blender's for a body-sized range.
- **Skinning.** Soft skins store, for every vertex, each bone's weight and where the vertex lies
  relative to that bone. The engine draws a skin from those alone, ignoring the positions the mesh
  stores, so a body fits whatever skeleton it is drawn on. Import does the same whenever the
  skeleton is there, and export works the relative positions out from the armature's rest pose.
- **Double-sided faces** are kept: the engine stores them as two triangles on the same corners,
  the second wound the other way, which Blender would merge if the mesh were validated, so it is
  not. Their normals in the files were smoothed over both sides at once, so the two cancel out
  and point any way along the face; each side is smoothed again on its own instead, fronts with
  fronts and backs with backs, so it shades out of its own front.

## Limits

- Exported `.MRM` files hold the mesh at full detail only. The data the engine reduces detail
  with (vertex updates, edges) is left empty, and whether the game copes with that is untested.
- Exported materials get the engine's defaults for everything Blender has no equivalent for, like
  the material group that decides footstep sounds.
- Worlds are not covered yet.
- Animations keep no events of their own: the game's `.MAN` files have none, as the script holds
  them. Exported animations go out for every node of the skeleton, where the game's leave out
  nodes nothing moves.
- Morph meshes' reduced detail is left out, as for `.MRM`.
- The engine's nodes have no length: bones reach the child they point to, and bones without one
  are half as long as their parent's.
