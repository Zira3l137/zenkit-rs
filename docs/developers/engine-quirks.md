# ZenGin engine quirks

What the game actually accepts, found while writing zenkit-rs and its Blender extension, by
reading the engine, comparing with ZenKit and OpenGothic, and writing the game's own files back.
Where the engine, ZenKit and the files disagree, the engine and the game's own files win, and the
readers and writers here follow them. Function names in parentheses are the engine's.

Add to it whatever a change to a reader or writer rests on that the code alone does not show.

## Models: skeletons (.MDH), meshes (.MDM), models (.MDL)

- **MDL vs MDM.** An `.MDL` is a standalone model: skeleton + meshes in one file
  (`LoadModelASC`). An `.MDM` is a mesh library worn on an existing skeleton: `Mdl_SetVisualBody`
  / `visual_change` load `ascName + ".mdm"`. Armour in the game ships as both.
- **ASC fallback.** If an `.MDL`/`.MDM` is refused, the engine tries to compile the `.ASC` source
  instead, so the visible error is "ASC not found: NAME.ASC", not the real reason.
- **MDM header version** = `0x04 <high byte of the MRM version> 05 06`:
  Gothic `0x04030506` (MRM `0x305`), Gothic II `0x04090506` (MRM `0x905`), older G2 files
  `0x04080506` (`0x805`). Gothic II refuses a file whose two versions disagree (→ ASC fallback).
  ZenKit writes the Gothic value for both games.
- **Node checksum** = CRC-32 of all node names concatenated, in order, no separators
  (`zCModelPrototype::CalcNodeListChecksum`). The skeleton stores it, and the MDM's soft-skin chunk
  stores the checksum of the skeleton it was made for. If it differs from the wearer's skeleton:
  "LoadMDM: the file's node-hierarchy is incompatible to 'meshAndTree'", and the skins are dropped.
- **Root translation.** Root nodes' rest transform is `T(root_translation) · node.transform`
  (model frame, as KrxImpExp does). When writing, apply `T(-root_translation)` to the roots, for
  `.MDL` and `.MDH` alike.
- **Weights.** A weight's node byte is the index of the hierarchy node itself (engine, OpenGothic
  and KrxImpExp agree). `SoftSkinMesh::nodes` only lists the nodes that have a bounding box (OBB),
  in no particular order. ZenKit documented it wrongly. So at most 256 nodes can carry weights.
- **Skins are drawn from weights.** The engine ignores the stored skin positions and draws
  `Σ weight · world[node] · local_position`. The stored positions only matter to tools.
- **Fatness** pushes vertices along per-position normals (`posNormalList`), not wedge normals.
- **Slots / mesh nodes.** `ZS_*` nodes are slots (attachment points), `ZM_*` nodes carry rigid
  meshes; a `ZS_`/`ZM_` node that has bone children is still a bone. Skinned meshes are named
  `ZM_SKIN_{n}`, attachments `ZM_FRILL_{n}`. Slots never go into an `.MDM`.
- **KrxImpExp rigs** turn bones 90° (X_TO_Y correction + roll) and slots 180°, and add `DUMMY_`
  sample meshes (material `ZS_SLOT`) and sample weapon meshes on slots. Those stand-ins must not
  be exported.
- **Model mesh layout (per the files, not ZenKit's writer):** each attachment's mesh chunk is closed
  by its own end chunk; the attachment list and soft-skin chunk are written only when non-empty;
  the soft-skin chunk covers just checksum and count, with the meshes after it.

## Multi-resolution meshes (.MRM)

- **Vertex updates must not be empty.** `CalcLODRenderArgsSubMesh` with no vertex updates draws
  0 triangles, so the mesh is invisible with no error. One update `{all triangles, all wedges}`
  means "no LOD, draw everything". Game files carry positions + 1 updates.
- The player character never uses LOD.
- ZenKit's writer stores byte sizes where the format has counts, omits vertex updates and writes
  too little padding. The 16 LOD-parameter bytes at the end must be kept.
- Padding after each wedge is uninitialised memory in game files, so byte-exact rewrites differ
  only there.

## Textures (.TEX)

From `zCTex_D3D::XTEX_BuildSurfaces`:

- **DXT5, DXT2, DXT4 are refused**, with or without mipmaps. Use DXT1 or DXT3.
- **Every DXT1/DXT3 mipmap level must be at least 4 px on each side**; a chain down to 1×1 is
  refused. Game textures stop at 8×8 (e.g. `ERZ.TEX`: DXT1 128×128, 5 levels).
- Uncompressed 32-bit (BGRA8 etc.) falls into the 16-bit format list (`pflist[2]`) and is not
  shown; don't offer it.
- A compiled texture is `NAME-C.TEX` in `_work/Data/Textures/_compiled`.
- The average colour is computed before compression, so ours may differ by a step for DXT.

## Static meshes (.MSH) and worlds

- Gothic II reads an alpha-test flag after the materials; ZenKit's writer omits it.
- Lightmaps: both games' own worlds use the shared chunk `B026` (Gothic 1's WORLD.ZEN too). The
  older `B025`, a texture beside every lightmap, is only worth reading; writing it for a shared
  set repeats each texture per lightmap (5 MB → 270 MB for WORLD.ZEN).
- **Gothic 1 polygon normal axis** = low two bits of the second flag byte (0 X, 1 Y, 2 Z: the
  normal's largest component, in all 138215 polygons of WORLD.ZEN). Bit 7 of the first byte is
  never set. ZenKit splits the axis across both bytes, losing every Z.
- With both, WORLD.ZEN's mesh and BSP tree rewrite byte for byte but for the date's padding.
- XZEN worlds use 32-bit vertex indices even for Gothic 1.
- A world's mesh + BSP block is written raw into the archive stream, past the encoder: same bytes
  in ASCII, binary and binsafe, and no length field.
- Game detection: the BSP version; uncompiled worlds have none, so the first VOB's class version
  tells (zCVob differs between games). oCNpc is 52224 in Gothic 1, 29697 in Gothic II.
- **World layout** (from a Gothic II save's world): `oCWorld` holds the sections `MeshAndBsp`
  (compiled worlds only), `VobTree`, `WayNet`, `EndMarker`, each an object of class `%`.
- **VOB tree child counts** are named through the whole tree: `childs0` for the roots, then
  `childs1`, `childs2`… one per VOB, in the order the VOBs are written.
- **Way-net:** `numWaypoints` free points as `waypoint{i}`, then `numWays` ways as
  `wayl{i}`/`wayr{i}`; a point is written whole the first time, as a `§` reference after that.
- **Shared visuals:** only `zCProgMeshProto` objects are shared between VOBs (by reference).
  Models, particle effects, morph meshes and decals are one object per VOB.
- **Class versions differ between worlds and saves** for some classes, e.g. Gothic II
  `zCProgMeshProto` 64513 in worlds, 53505 in saves; `zCCSCamera` 33793 / 12289;
  `zCCamTrj_KeyFrame` 31489 / 9218; `zCDecal` 64513 / 53505. In Gothic II worlds `zCMesh` and
  `zCDecal` are 64513 like `zCProgMeshProto`; `zCModel`, `zCParticleFX`, `zCMorphMesh` are 0.
  Gothic II saves also give `zCPFXControler`, `zCEarthquake`, `zCTriggerWorldStart` 20483,
  `zCTriggerList` 45570, `zCTriggerUntouch` 33026 (ZenKit's world values for these are guesses),
  `oCAIHuman` 36865 and `oCNpcTalent` 64513 (both 0 in Gothic 1). Gothic 1's world and save
  versions agree.
- **Mob `locked` and `takeable`** are one-bit fields in the engine, so true is stored as -1
  (every bit set): `0xFFFFFFFF` in BIN_SAFE, `0xFF` in binary, as in NEWWORLD.ZEN and the save
  samples (38 takeable mobs in Gothic 1's).
- Packed Gothic II VOBs keep wind mode, depth bias and ambient at bits 8–9, 10–14 and 15, one bit
  higher than ZenKit reads them. Bit 7 is set on most VOBs; meaning unknown, nothing reads it.
- Packed Gothic 1 VOBs set bit 5 (event manager) on most VOBs in worlds, though only save games
  store an event manager, and bit 7 on some. Nothing reads either in a world. In saves bit 7 is set
  per object, not by class (Gothic 1: 618 of 8263 `zCVob`s, 150 of 249 containers; Gothic II:
  1104 VOBs), so it likely means something; still unknown, and the writers leave it clear.
- Light `colorAniList`: a grey as one number, other colours as `(r g b)`, each followed by a
  space: `"(0 20 0) (64 128 128) 255 "`.
- A trigger's `flags` byte: bit 0 start enabled, bit 1 enabled now (equal to bit 0 in every
  world sample), bit 2 send untrigger. `filterFlags` uses bits 0–5; bits 6–7 hold leftover
  memory in some Gothic 1 triggers (0x79, 0xD2).
- Gothic II NPCs' packed items split at every `;` (ZenKit keeps only the first part); a list with
  no `;` at all occurs too (the hero of the Gothic II sample).

## Save games

- **Files:** `SAVEINFO.SAV` ASCII archive, `saveGame 0`; `SAVEDAT.SAV` BIN_SAFE, `saveGame 1`;
  worlds BIN_SAFE in normal saves, BINARY in quick saves (`saveGame 1`); `LOG.SAV` tab-separated
  text, every line ending in a tab and LF (no CR); `SAVEHDR.SAV` the world file + LF
  (`NEWWORLD\NEWWORLD.ZEN`); `THUMB.SAV` a 256×256 texture.
- **Guild attitudes:** a raw table of 42×42 bytes in Gothic 1, **66×66** in Gothic II (ZenKit
  reads 42×42 for both).
- **Script variables** in `SAVEDAT.SAV`: `symName{i}`, then Gothic 1 `symValue{i}`, Gothic II
  `symName{i}cnt` and `symValue{i}_{j}` for each element (ZenKit writes `_0` for all).
- Missions keep their `NumInList` entries (`VobIDL`, `StatusL`, `startTimeL`); ZenKit writes 0.
- **Save worlds** have no `MeshAndBsp`. Sections: `VobTree`, `CutscenePlayer`, `SkyCtrl`,
  `WayNet`, `EndMarker`; then, inside `oCWorld` after the last section, `npcCount`, `npc{i}`,
  `NoOfEntries` spawns (`npc`, `spawnPos`, `timer`), `spawningEnabled`, and in Gothic II
  `spawnFlags` (an int; ZenKit writes a bool).
- The sky controller's last two Gothic II keys really end in whitespace: `"isRaining\t\t "`,
  `"rainCtr\t\t "`.
- **NPCs:** talents the NPC lacks are null objects at their place in the list (the Gothic II hero:
  22 entries, 4 set); hit chances are `hc1`–`hc4` (ZenKit `hc0`–`hc3`); `spells` is 4 raw bytes,
  not always zero (the hero has 1); an item's short key is `shortKey{i}` after `item{i}` when the
  item's flags have `0x200`.
- A VOB's AI object is named `ai`, its event manager is unnamed (`[% zCEventManager 0 n]`) with
  the cutscene in `emCutscene`; then `sleepMode`, `nextOnTimer`, and the rigid body when physics
  is on.
- **Cutscenes are shared:** the cutscene player's `playContext{i}` is a reference to the context a
  VOB's event manager holds, not a copy.
- Values the games write but nothing reads, which no writer can reproduce from what is read:
  packed VOB bit 7, trigger filter bit 6, camera alignment 3, a mover's lerp and speed enums and
  some bools (`rewind` 16, trigger bools `x`) holding uninitialised memory. With those aside, all
  five of ZenKit's sample save worlds rewrite to the same length and values.

## Morph meshes (.MMB), animations (.MAN), model scripts (.MDS/.MSB)

- **MMB layout:** `E000` empty marker, `E020` header (version 3, name), `B100` MRM, `B1FF` morph
  positions (one per MRM position, so after `B100`), `E010` sources, `E030` animations.
- **MAN layout:** `A000` empty marker, `A020` header (version 12 in both games), `A010` source,
  `A090` samples. No sample with events (`A030`); its position and empty-string handling unknown.
- **MAN positions** are quantised over the full u16 range: min = smallest coordinate (raw 0),
  scale = (max − min) / 65535 (raw 65535). Rotations store x, y, z; w is taken as positive.
- Source dates in MAN/MDH/MMB are uninitialised memory, padding included.
- **MSB layout:** `F000` (u32 1 + source directory), `F100` (date + source path), `F200` model,
  `F300` meshAndTree, `F400` registerMesh…, `F500` aniEnum, statements in source order, each
  followed by `F5A0` (event count) … events … `F5AF` only when it has events; `F5FF`, `F2FF`,
  `FFFF`. Events are in source order (sound and ground sound interleave).
- MSB alias direction is the source token as written: `F`, `R`, empty (left out), even a stray
  `"` (3 of Gothic II's scripts); the engine reads anything not starting with R as forward.
- MSB flag text is the source token as written: `M`, `M.`, `M:`, `.`, `..`, `MF`, `MI` all occur
  in Gothic II's scripts; most use one flag + dot, none = `.`.
- MSB sound/ground sound's last field and the particle effect's `attached` are **floats**
  (1.0 = set). Every ground sound in waran.msb has 1.0. Tag events store 4 argument lines, unused
  ones empty.
- Verified on Gothic II: all 3977 MAN and 61 MMB files rewrite byte-exact but for uninitialised
  padding (wedges, source dates); MSB root chunk directory = source path's directory.
- MSB `meshAndTree` name has no extension (`WAR_BODY`), `registerMesh` keeps it.
- `*eventMMStartAni` stores `I:` intensity and `H:` hold time as two floats after the node.
- **MAN rotations:** the engine builds a sample's matrix from the stored quaternion's conjugate in
  glam/glm terms (OpenGothic `mkMatrix` writes the transposed rotation). With that, a Biped
  pelvis from HUMANS-S_FISTRUN points up the spine (+Y); a sample's matrix is then the same kind
  of transform as the skeleton's nodes. A root's sample position is the whole position, root
  translation included.
- **MAN frame rate** is the script's `FPS:` (S_FISTRUN: 10), `fps_source` the source's (25), and
  the frames are resampled: source frames 1–50 became 20. MAN events are absent in all of
  Gothic II's files; the MSB holds them.
- MAN `node_indices` may skip nodes (S_FISTRUN: 25 of 26 indices, 7 missing); the alligator's
  animation lists all 39.
- **Compiled scripts store no model name**: readers take it from the source path in `F100`, and
  the engine names a model after its file. A script compiled from text needs a source path,
  like `\_WORK\DATA\ANIMS\HUMANS.MDS`, to keep it.
- **Compiled skeleton names** (OpenGothic `Resources::implLoadMesh`): with `DONT_USE_MESH` the
  skeleton is `<MODEL>.MDH`, whatever `meshAndTree` names: HUMANS has
  `meshAndTree("HUM_BODY_NAKED0.ASC" DONT_USE_MESH)` and `HUMANS.MDH`; `HUM_BODY_NAKED0.MDM` is a
  body. Without it, `<source>.MDH` + `<source>.MDM`, or `<source>.MDL`. Animations are
  `<MODEL>-<ANI>.MAN`.
- MMB morph positions equal the mesh's positions in both samples. Animations can list vertices
  they leave in place (the bow's S_NORMAL: all 28, zero offsets). Single-frame state animations
  have duration −1000 and flags 2 in the samples; multi-frame ones a positive duration (ms).
- **MMB animation timing** (OpenGothic `ProtoMesh::mkAnimation`): duration in ms, ≤ 0 = plays until
  stopped; speed in frames per ms (0.025 = 25 fps); flag `0x2` (or duration ≤ 0) plays the frames
  at that speed, otherwise they are spread over the duration. Layer: one animation per layer at a
  time, at most 4 at once.

## Archives, materials, VDF, cutscenes

- ASCII archives: bytes and words are plain ints, 3×3 matrices raw hex, floats should be written
  shortest-exact (ZenKit's `%f` loses small values).
- A binary archive's header ends "END\n" plus at most one blank line; the first data byte can be
  `0x0A` (low byte of an object size), so only one blank line may be skipped.
- Objects of class `%` (sections, groups) take no object index and are stored with index 0:
  `[VobTree % 0 0]`, and the first VOB after it is object 1.
- **Binary archives store a section's class empty** (`VobTree\0\0`), while a null object keeps `%`
  (`carryVob\0%\0`). The text encodings print both as `%`.
- The BIN_SAFE key table is written by walking the engine's hash table: bucket by bucket
  (hash % 0x61), each bucket listing the newest key first.
- Materials: texture scale and animation direction are text printed like `%.8g` with three-digit
  exponents; the alpha function is an enum, not a bool. `.MAT` is not a game format — materials
  are embedded in meshes and worlds.
- VDF: names upper case; catalog lists directories first, each directory's entries after all its
  siblings; empty directories can't be represented; timestamp = newest file.
- **Union's compressed VDFs:** volume flags `0xA0` (normal `0x50`). The catalog is unchanged and
  gives each file's uncompressed size; the header's archive size is the sum of those. Each file is
  a ZippedStream: `length | block size | block count`, then per block `length | compressed length |
  block size` and its own zlib data. Sounds (`.WAV`, `.OGG`) are stored uncompressed, so a reader
  must check for a valid stream. Union's tool picks block size = length / 4 (4–5 blocks per file,
  zlib level 9); ZenKit writes 8 KiB blocks. Only games running Union read them.
- Cutscene/message lookups ignore case (ZenKit's is case-sensitive). The format allows several
  sub-blocks per block, but the games never write it.
- File names everywhere are matched ignoring case, as on Windows.
