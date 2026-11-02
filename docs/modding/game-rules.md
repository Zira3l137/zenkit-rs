# What the game accepts

The game is strict in ways it does not tell you about: a file it refuses usually just does not
show, or the log names some other file. The extension keeps these rules when it exports, but
knowing them explains what you see, and matters when files come from elsewhere. The technical
version, with the engine's own function names, is [engine-quirks.md](../developers/engine-quirks.md).

## Names

- The game finds files by name alone, wherever they are in its archives and folders, ignoring
  case. Two files of the same name replace each other: the one in the newer archive wins.
- Assets name their **source** files, and the game loads the **compiled** ones:

  | Named as     | Loaded as                    | From                             |
  |--------------|------------------------------|----------------------------------|
  | `STONE.TGA`  | `STONE-C.TEX`                | `_work/Data/Textures/_compiled`  |
  | `CHEST.3DS`  | `CHEST.MRM`                  | `_work/Data/Meshes/_compiled`    |
  | `ARMOR.ASC`  | `ARMOR.MDM` or `ARMOR.MDL`   | `_work/Data/Anims/_compiled`     |
  | `HUMANS.MDS` | `HUMANS.MSB`                 | `_work/Data/Anims/_compiled`     |
  | `HEAD.MMS`   | `HEAD.MMB`                   | `_work/Data/Anims/_compiled`     |

- A model's animations are files named after the model and the animation: `HUMANS-S_RUN.MAN`. The
  model's name is its script's file name, `HUMANS.MDS`, whatever the script's `Model` line says.

## Textures

- **DXT1 or DXT3 only.** DXT5, and uncompressed 32-bit textures, are refused or not shown, with or
  without transparency. DXT1 for opaque images, DXT3 for ones with transparency.
- **Mipmaps stop at 4 × 4 pixels.** A texture whose smaller versions go down to 1 × 1, as most
  tools make them, is refused. The extension stops at 4 × 4 by itself.
- **Sides in powers of two**, like the game's own (128, 256, 512). Others may show wrongly.

## Meshes

- Props, items and weapons are multi-resolution meshes (`.MRM`); world geometry is a static mesh
  (`.MSH`) inside the world.
- A `.MRM` without any detail level is drawn as nothing, with no error. The extension writes one
  level, the full mesh, so its meshes always show; the game's own have many, for distance.
- Faces are seen from one side. A face seen from both sides is two faces back to back, which is
  how the game's files have them and how the extension imports them.

## Skeletons, meshes and checksums

- A **skeleton** (`.MDH`) is the list of a model's bones and other nodes. **Model meshes**
  (`.MDM`) are the meshes that bend with it, like a body or an armour. A **model** (`.MDL`) is
  both in one file.
- Every skeleton has a **checksum** computed from its nodes' names, in order. Model meshes and
  animations store the checksum of the skeleton they were made for, and the game refuses them on
  any other skeleton: an armour made for another skeleton is invisible, and the log says "the
  file's node-hierarchy is incompatible to 'meshAndTree'".
- So **armour and bodies for people are exported from the `HUMANS` armature** (import
  `HUMANS.MDH` first). An armour's own `.MDL` has a skeleton without the humans' slots, and a
  different checksum.
- Renaming, adding or removing a node changes the checksum. Exporting writes a skeleton's nodes in
  the order they were imported, so it keeps its checksum, and with it every mesh and animation
  made for it.
- Node names tell the game what a node is: `ZS_…` are **slots**, where weapons and items are held,
  and never go into a `.MDM`; `ZM_…` carry **rigid parts**. Bodies are `ZM_SKIN_0`, `ZM_SKIN_1`…
- A model's meshes are drawn from their bones' weights, wherever the meshes lay when saved, so a
  body fits whichever skeleton with its checksum wears it.

## When a file is refused

If the game cannot use a compiled file, it falls back to compiling the source instead, which a mod
does not ship. The log then says it cannot find the source, like "ASC not found: ARMOR_NEW.ASC",
not why the compiled file was refused. So that message usually means: the `.MDM` or `.MDL` is
there, but the game did not take it. See [troubleshooting](troubleshooting.md).

## Archives

- A mod's `.VDF` holds its files under the game's paths, names in upper case, and is dated after
  its newest file, so it wins over the game's older archives.
- Union, the community's engine extension, also reads compressed `.VDF` archives. The plain game
  does not: only ship a compressed one to players who use Union.

## Fonts

A font (`.FNT`) has 256 characters, one for each byte of the Western European code page
(Windows-1252), whether its texture draws them all or not. The game expects exactly that many.
