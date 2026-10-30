"""Exercises the extension in Blender, from the command line:

    blender --background --python tests/test_addon.py

It uses the installed extension if there is one, and the extension in this folder otherwise.
The library's sample files come from the repository's samples/ folder.
"""

import shutil
import sys
import tempfile
from pathlib import Path

import bpy
import numpy as np

HERE = Path(__file__).resolve().parent
samples = HERE.parent.parent / "samples"

if not hasattr(bpy.types, "IMPORT_SCENE_OT_zenkit_mesh"):
    sys.path.insert(0, str(HERE.parent))
    import zenkit_io

    zenkit_io.register()

import zenkit_py  # noqa: E402

print("Blender", bpy.app.version_string)
work = Path(tempfile.mkdtemp())

# Textures: import, export uncompressed, compare with what the library decodes.
assert bpy.ops.import_image.zenkit_texture(filepath=str(samples / "erz.tex")) == {"FINISHED"}
image = bpy.data.images["erz"]
assert tuple(image.size) == (128, 128), image.size
out = work / "ERZ-C.TEX"
assert bpy.ops.export_image.zenkit_texture(
    filepath=str(out), image="erz", format="B8G8R8A8", mipmaps=False
) == {"FINISHED"}
w, h, original = zenkit_py.read_texture(str(samples / "erz.tex"))
w2, h2, written = zenkit_py.read_texture(str(out))
assert (w2, h2) == (w, h) and written == original, "texture changed on its way through Blender"
assert bpy.ops.export_image.zenkit_texture(
    filepath=str(work / "DXT.TEX"), image="erz", format="DXT1"
) == {"FINISHED"}
print("texture ok")

# A mesh whose texture sits next to it, under the name its material asks for.
shutil.copy(samples / "mesh0.mrm", work / "CAGE.MRM")
shutil.copy(samples / "erz.tex", work / "OCODFLGATELI-C.TEX")
assert bpy.ops.import_scene.zenkit_mesh(filepath=str(work / "CAGE.MRM")) == {"FINISHED"}
obj = bpy.context.active_object
mesh = obj.data
assert (len(mesh.vertices), len(mesh.polygons)) == (8, 16), (len(mesh.vertices), len(mesh.polygons))
mat = mesh.materials[0]
assert mat.name == "EVT_TPL_GITTERKAEFIG_01"
tex = [n for n in mat.node_tree.nodes if n.type == "TEX_IMAGE"]
assert tex and tex[0].image is not None, "the texture was not found and plugged in"
# The engine's Y up became Blender's Z up: the cage is 4 m tall.
dims = tuple(round(d, 3) for d in obj.dimensions)
assert dims == (4.0, 4.0, 3.985), dims

# Faces must point the way the engine's normals say, or the winding came out mirrored.
normals = np.empty(len(mesh.loops) * 3, np.float32)
mesh.corner_normals.foreach_get("vector", normals)
normals = normals.reshape(-1, 3)
agree = sum(np.dot(p.normal, normals[list(p.loop_indices)].mean(axis=0)) > 0 for p in mesh.polygons)
print(f"faces agreeing with their normals: {agree}/{len(mesh.polygons)}")


def geometry(data):
    pos = np.frombuffer(data["positions"], "<f4").reshape(-1, 3)
    verts = np.frombuffer(data["corner_vertices"], "<u4").reshape(-1, 3)
    return sorted(tuple(map(tuple, np.round(pos[t], 2))) for t in verts)


original = zenkit_py.read_mrm(str(samples / "mesh0.mrm"))
for op, ext, read in [
    (bpy.ops.export_scene.zenkit_mrm, "MRM", zenkit_py.read_mrm),
    (bpy.ops.export_scene.zenkit_msh, "MSH", zenkit_py.read_msh),
]:
    for game in ("G1", "G2"):
        path = work / f"OUT_{game}.{ext}"
        assert op(filepath=str(path), game=game) == {"FINISHED"}
        data = read(str(path))
        if ext == "MSH":
            counts = np.frombuffer(data["corner_counts"], "<u4")
            assert (counts == 3).all()
        assert geometry(data) == geometry(original), f"{ext} {game} geometry changed"
        assert data["materials"][0][0] == "EVT_TPL_GITTERKAEFIG_01"
        assert data["materials"][0][1] == "OCODFLGATELI.TGA", data["materials"]
    assert bpy.ops.import_scene.zenkit_mesh(filepath=str(work / f"OUT_G2.{ext}")) == {"FINISHED"}
    assert len(bpy.context.active_object.data.polygons) == 16
print("meshes ok")

# Quads stay quads in a static mesh, and are split for a multi-resolution one.
bpy.ops.mesh.primitive_cube_add(size=2)
cube = bpy.context.active_object
assert bpy.ops.export_scene.zenkit_msh(filepath=str(work / "CUBE.MSH")) == {"FINISHED"}
assert set(np.frombuffer(zenkit_py.read_msh(str(work / "CUBE.MSH"))["corner_counts"], "<u4")) == {4}
assert bpy.ops.export_scene.zenkit_mrm(filepath=str(work / "CUBE.MRM")) == {"FINISHED"}
assert len(np.frombuffer(zenkit_py.read_mrm(str(work / "CUBE.MRM"))["corner_counts"], "<u4")) == 12
print("cube ok")

print("all ok")
