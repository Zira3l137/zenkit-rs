# Troubleshooting

What goes wrong most often, by what you see. The game writes a log, `zSpy` or the game's own
`.log`, worth reading alongside: it names files it could not use.

## In the game

| What you see | Likely cause | What to do |
|--------------|--------------|------------|
| The new or changed file makes no difference | The game loads another copy: one in an archive dated after yours, or the archive over the loose file in `_work` | Pack the mod into a `.VDF` dated after the game's ([shipping a mod](getting-started.md#shipping-a-mod)), and check no other mod archive holds the same file name |
| A prop or item is invisible | Its texture was refused, or the mesh has no detail level | Re-export the textures as DXT1 or DXT3 from the extension; re-export the mesh with it, which always writes one level |
| An armour or body is invisible, or the person shows naked | The meshes were made for another skeleton; the log says "node-hierarchy is incompatible to 'meshAndTree'" | Import `HUMANS.MDH`, put the meshes on its armature (**Use Active Armature**), export the `.MDM` from there ([make armour](recipes.md#make-armour)) |
| The log says "ASC not found: NAME.ASC" | The game refused the compiled `.MDM` or `.MDL`, or it is not where the game looks, under that name | Check the file is `NAME.MDM` in `_work/Data/Anims/_compiled` or the mod archive; then the line above |
| A texture is missing or black | Its `-C.TEX` is missing, misnamed, DXT5, or has mipmaps down to 1 × 1 | Export it from the extension as `<NAME>-C.TEX`, DXT1 or DXT3 |
| An animation does not play | No `.MAN` named `<MODEL>-<ANIMATION>.MAN`, or the model script lacks its `ani` line | Export the animation and the script together from the **Export** panel |
| An animation twists the body | It was made for another skeleton (another checksum) | Make it on an armature imported from the model's own skeleton |
| Things are 100 times too big or small | The scale was changed on import or export | Use the same **Scale** both ways; the default, 0.01, makes the game's centimetres Blender's metres |

## In Blender

| Message or symptom | Cause | What to do |
|--------------------|-------|------------|
| "No skeleton with checksum … was found" on importing a `.MDM` | Its skeleton is not next to it, in the models folder or the archives | Import the skeleton (`.MDH`) first and keep its armature selected, or set **Game Folder** and **Search the Archives** in the preferences |
| "…imported on its own; make its skeleton's armature active to bind it" | A `.MDM` was imported without its armature selected | Select the skeleton's armature and import it again |
| "Cannot import NAME.MDS: no skeleton was found; looked for …" | None of the files the message lists exist, and no skeleton has the checksum of the script's meshes or animations | Set the game folder, or put the script's skeleton next to it |
| "N animations were not found: …" | The script lists animations whose `.MAN` files are not there | Normal for a script whose animations are not all compiled; the rest are imported |
| "… made for another skeleton than …'s; the bones may not be the ones meant" | The animation's checksum is not the armature's | Import it onto the armature of the skeleton it was made for |
| "Moving ZS_… needs Blender 4.4 or later; they stay put" | Older Blender cannot animate objects hanging from bones in one action | Use Blender 4.4 or later for animations that move slots |
| "Exported NAME.MDM, but no skeleton found has its checksum …" | The armature it came from is not one of the game's skeletons | Export meshes from an armature imported from the skeleton that will wear them |
| A mod's `.vdf` cannot be rebuilt while Blender is open (Windows) | Blender holds the archives it searches open | Turn **Search the Archives** off, or close Blender, while packing |
| Materials come in plain, without their texture | The texture was not found | Set **Game Folder**, turn **Search the Archives** on, or put the `-C.TEX` next to the mesh, and import again. A material without an image exports without a texture, so don't export such a mesh as it is |

## Still stuck

Open an [issue](https://github.com/Zira3l137/zenkit-rs/issues) with the file, or the steps that
make it, what you expected, and what happened, including the game's log or Blender's message.
