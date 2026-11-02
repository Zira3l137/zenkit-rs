# Recipes

The common jobs, step by step. Each assumes the extension is installed and its **Game Folder** is
set ([getting started](getting-started.md)). File names in the game are not case-sensitive;
`ARMOR_NEW.MDM` and `armor_new.mdm` are the same file.

## Retexture something

A material names its texture as the source image, like `OCWOOD.TGA`; the game loads the compiled
`OCWOOD-C.TEX` from `_work/Data/Textures/_compiled` or its archives.

1. Make or edit the image in Blender's image editor, or open one there. Its sides should be powers
   of two (64, 128, 256, 512…), as the game's are.
2. **Image > Export as Gothic Texture**, and save it as `<NAME>-C.TEX`, like `OCWOOD-C.TEX` to
   replace that texture, in `_work/Data/Textures/_compiled`.
3. Format: **DXT1** for images without transparency, **DXT3** for images with it. The game shows
   no others reliably ([why](game-rules.md#textures)).

To give a mesh a new texture rather than replace one, name the material's image after it before
exporting the mesh: the export names each material's texture after the first image in it.

## Change a prop

Chests, tables, weapons, items: anything placed in the world or held is a multi-resolution mesh,
`.MRM`.

1. **File > Import > Gothic Mesh**, and pick the `.MRM`.
2. Edit it. Keep its materials, or give it new ones with images (see above).
3. **File > Export > Gothic Multi-Resolution Mesh (.mrm)** into `_work/Data/Meshes/_compiled`,
   under the name the game asks for: a world or item names `CHEST.3DS`, which the game loads as
   `CHEST.MRM`.

Exported meshes are triangulated and have no reduced-detail versions; the game draws them at full
detail at every distance.

## Make armour

Armour is worn by humans, so its meshes have to fit the humans' skeleton, `HUMANS.MDH`. The game
checks that with a checksum and refuses the meshes on any other skeleton, without an error on
screen ([why](game-rules.md#skeletons-meshes-and-checksums)).

1. Import the humans' skeleton: **File > Import > Gothic Model**, `HUMANS.MDH`. Keep its armature
   selected.
2. Start from an armour of the game: **File > Import > Gothic Model**, its `.MDL` (like
   `ARMOR_MIL_L.MDL`) with **Use Active Armature** ticked. Its meshes go onto the `HUMANS` armature.
   Or import a body (`HUM_BODY_NAKED0.MDM`) the same way, or skin your own mesh to the armature
   with an Armature modifier and vertex groups named after the bones.
3. Model and weight it. Meshes that bend with the body are named `ZM_SKIN_0`, `ZM_SKIN_1`…; rigid
   parts hang from a bone.
4. Put the meshes in a collection named after the file, like `ARMOR_NEW.MDM`, in the armature's
   collection, and export it from the **Gothic** tab's **Export** panel, or with **File > Export >
   Gothic Model** and Format **Meshes (.mdm)**, into `_work/Data/Anims/_compiled`.

The game puts armour on with the item's `visual_change`, which names the source file:
`visual_change = "ARMOR_NEW.ASC";` loads `ARMOR_NEW.MDM`.

## Change or add an animation

Animations belong to a model, `HUMANS` for people, and are listed in its model script.

1. **File > Import > Gothic Model Script**, `HUMANS.MSB` (or `.MDS`). The script names over a
   thousand animations, so give **Animations** a pattern, like `S_RUN*` or `T_DANCE*`; `*` stands
   for anything.
2. The armature comes in with an action per animation, listed in the **Gothic** tab's
   **Animations** panel. Clicking one plays it.
3. Edit the action as any other. For a new one, **+** starts one from the current pose; give it a
   name (`T_MYDANCE`) and settings: layer, the animation that follows, blending, flags, frame rate.
4. In the **Export** panel, tick the animations and the script, and export into
   `_work/Data/Anims/_compiled`. Each animation goes out as `<MODEL>-<NAME>.MAN`, like
   `HUMANS-T_MYDANCE.MAN`, and the script as `HUMANS.MSB`, with an `ani` line for every animation.

An animation's settings live in the script once it has a line for it: the panel's fields edit
that line, and the script text (in the Text Editor) stays as you wrote the rest of it. The
extension's README explains [where the settings live](../../blender/README.md#the-gothic-tab).

The game's scripts start animations by name: `AI_PlayAni(self, "T_MYDANCE");`.

## Make a creature

A creature is a skeleton of its own, its meshes and animations, tied together by a model script.

1. Build an armature. Bones are named freely; slots, the points weapons and items are held at,
   are named `ZS_…` (`ZS_RIGHTHAND`), and parts that do not bend, `ZM_…`. Gothic's own skeletons
   start from a root called `BIP01`.
2. Skin the body to it: an Armature modifier and vertex groups named after the bones.
3. In the **Gothic** tab's **Model** panel, give the model a name, like `MYBEAST`, and click
   **New** to make its script.
4. Add animations as above: at least the ones the game's AI calls for that kind of creature,
   which the game's own creature scripts show.
5. Export everything from the **Export** panel: `MYBEAST.MDH` (the skeleton), the meshes' `.MDM`,
   `MYBEAST.MSB` and the `.MAN` files.

The creature's script instance sets it up with `Mdl_SetVisual(self, "MYBEAST.MDS");` and
`Mdl_SetVisualBody(...)`, naming the body's source file.

## Animate a face or a flag

Heads, and anything whose vertices move rather than bend, are morph meshes, `.MMB`.

1. **File > Import > Gothic Mesh**, the `.MMB`, like a `HUM_HEAD_*.MMB`. Each frame of each
   animation becomes a shape key, and each animation an action that plays them.
2. The **Gothic** tab's **Morph Mesh** panel lists the animations. Selecting one plays it; the
   **Frame** slider picks a frame and makes its shape key the one Edit Mode edits.
3. **+** adds an animation, the key button a frame (a copy of the last). Its settings say how the
   game plays it: once over its duration, repeating, or held until stopped.
4. Export from the panel, or **File > Export > Gothic Morph Mesh (.mmb)**, into
   `_work/Data/Anims/_compiled`.

## Batch jobs

For work over many files, like converting every texture of a mod or checking every armour's
checksum, the extension's Python module runs outside Blender too: [python.md](../developers/python.md).
