"""Exercises the extension in Blender, from the command line:

    blender --background --python tests/test_addon.py

It uses the installed extension if there is one, and the extension in this folder otherwise.
The library's sample files come from the repository's samples/ folder.
"""

import shutil
import struct
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

# A texture's alpha reaches the material's shader when it has any; erz is opaque.
package = bpy.types.IMPORT_SCENE_OT_zenkit_mesh.__module__.rsplit(".", 1)[0]
convert = sys.modules[package + ".convert"]
leaf = np.full((4, 4, 4), 255, np.uint8)
leaf[0, :, 3] = 0
zenkit_py.write_texture(str(work / "LEAF-C.TEX"), 4, 4, leaf.tobytes(), "B8G8R8A8", False)
for name, see_through in (("LEAF", True), ("ERZ", False)):
    mat = convert.material(name, name + ".TGA", [work])
    shader = mat.node_tree.nodes["Principled BSDF"]
    assert shader.inputs["Alpha"].is_linked == see_through, name
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
# The cage's faces are all double-sided, stored twice: each side shades smooth, out of its front.
for polygon in mesh.polygons:
    for corner in polygon.loop_indices:
        assert np.dot(normals[corner], polygon.normal) > 0, polygon.index


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


def node_positions(data):
    """Every node's position in the model, from the transforms relative to the parents."""
    world = []
    for _, parent, rows in data["nodes"]:
        local = np.frombuffer(rows, "<f4").reshape(4, 4).astype(np.float64)
        world.append(world[parent] @ local if parent >= 0 else local)
    return np.array([w[:3, 3] for w in world])


def stored_root(path):
    """The root node's transform and the root translation, as a skeleton file stores them."""
    data = Path(path).read_bytes()
    at = 0
    while struct.unpack_from("<H", data, at)[0] != 0xD100:
        at += 6 + struct.unpack_from("<I", data, at + 2)[0]
    (count,) = struct.unpack_from("<H", data, at + 10)
    at += 12
    for i in range(count):
        at = data.index(b"\n", at) + 3
        if i == 0:
            root = np.frombuffer(data, "<f4", 16, at).reshape(4, 4)
        at += 64
    return root, np.frombuffer(data, "<f4", 3, at + 48)


def deselect_all():
    for obj in bpy.context.selected_objects:
        obj.select_set(False)
    bpy.context.view_layer.objects.active = None


# A skeleton becomes an armature of its bones, with its slots and mesh nodes as objects hanging
# from them, and writes back the same nodes in the same order.
deselect_all()
assert bpy.ops.import_scene.zenkit_model(filepath=str(samples / "hierarchy0.mdh")) == {"FINISHED"}
mill = bpy.context.active_object
assert mill.type == "ARMATURE"
assert [b.name for b in mill.data.bones] == [
    "BIP01 MUEHLE",
    "BIP01 NABE",
    "BIP01 ROLLERL",
    "BIP01 ROLLERR",
]
nodes = {o["zenkit_node"]: o for o in bpy.context.scene.objects if "zenkit_node" in o}
assert sorted(nodes) == ["ZM_MUEHLE01", "ZM_RING", "ZS_POS0"]
assert nodes["ZM_RING"].parent_type == "BONE" and nodes["ZM_RING"].parent_bone == "BIP01 NABE"
assert nodes["ZS_POS0"].parent_bone == "BIP01 MUEHLE"
original = zenkit_py.read_model(str(samples / "hierarchy0.mdh"))
assert bpy.ops.export_scene.zenkit_model(filepath=str(work / "MILL.MDH")) == {"FINISHED"}
written = zenkit_py.read_model(str(work / "MILL.MDH"))
assert written["checksum"] == original["checksum"]
assert [n[:2] for n in written["nodes"]] == [n[:2] for n in original["nodes"]]
for (name, _, ours), (_, _, theirs) in zip(written["nodes"], original["nodes"], strict=True):
    ours, theirs = np.frombuffer(ours, "<f4"), np.frombuffer(theirs, "<f4")
    assert np.allclose(ours, theirs, atol=1e-2), (name, ours, theirs)
# The root translation moves the root, and is stored apart from it again.
root, root_translation = stored_root(work / "MILL.MDH")
original_root, original_translation = stored_root(samples / "hierarchy0.mdh")
assert np.allclose(root, original_root, atol=1e-4)
assert np.allclose(root_translation, original_translation, atol=1e-2)
assert np.allclose(mill.data.bones[0].head_local[:], (0, 3.9404, 0), atol=1e-4)

# A mesh node's mesh is its object, at the node, and goes back out as the node's attachment.
door = zenkit_py.read_model(str(samples / "secretdoor.mdm"))["attachments"][0]["mesh"]
mill_model = {**original, "attachments": [{"node": "ZM_RING", "mesh": door}]}
zenkit_py.write_model(str(work / "MILL.MDL"), mill_model, "G2")
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "MILL.MDL")) == {"FINISHED"}
ring = next(o for o in bpy.context.scene.objects if o.get("zenkit_node") == "ZM_RING")
assert not ring.data.get("zenkit_placeholder") and len(ring.data.vertices) > 6
assert ring.parent_bone == "BIP01 NABE"
assert bpy.ops.export_scene.zenkit_model(filepath=str(work / "MILL2.MDL")) == {"FINISHED"}
mill_again = zenkit_py.read_model(str(work / "MILL2.MDL"))
assert [a["node"] for a in mill_again["attachments"]] == ["ZM_RING"]
assert [n[0] for n in mill_again["nodes"]] == [n[0] for n in original["nodes"]]
print("skeleton ok")

# A soft skin without its skeleton still comes in, its groups named after the nodes.
deselect_all()
assert bpy.ops.import_scene.zenkit_model(filepath=str(samples / "smoke_waterpipe.mdm")) == {
    "FINISHED"
}
pipe = [o for o in bpy.context.scene.objects if o.name.startswith("smoke_waterpipe")][0]
assert len(pipe.data.vertices) == 115 and pipe.vertex_groups.get("NODE 0") is not None
# A weight names its node by the node's index in the skeleton: vertex 62's is node 4.
assert pipe.vertex_groups["NODE 4"].weight(62) == 1.0
print("lone soft skin ok")


# A skeleton the way Gothic's are: bones along their node's X axis, a slot on the head, and the
# root's position, the hips, mostly in the root translation. Its mesh is skinned around the hips.
def node(x, y, z, turn=((1, 0, 0), (0, 1, 0), (0, 0, 1))):
    rows = [(*turn[i], (x, y, z)[i]) for i in range(3)] + [(0, 0, 0, 1)]
    return np.array(rows, "<f4").tobytes()


biped_nodes = [
    ("BIP01", -1, node(0, 90, 5, ((0, -1, 0), (1, 0, 0), (0, 0, 1)))),
    ("BIP01 SPINE", 0, node(20, 0, 0)),
    ("BIP01 HEAD", 1, node(30, 0, 0)),
    ("BIP01 L ARM", 1, node(25, 0, -15)),
    ("BIP01 L HAND", 3, node(10, 0, 8)),
    ("ZS_HELMET", 2, node(10, 0, 0)),
]
weighted_names = {"BIP01", "BIP01 SPINE", "BIP01 HEAD", "BIP01 L ARM"}
bone_names = weighted_names | {"BIP01 L HAND"}
skin = zenkit_py.read_model(str(samples / "smoke_waterpipe.mdm"))["soft_skins"][0]
vertices = len(skin["mesh"]["positions"]) // 12
skin["weight_counts"] = np.ones(vertices, "<u4").tobytes()
skin["weight_nodes"] = (np.arange(vertices) % 4).astype("<u4").tobytes()
skin["weight_values"] = np.ones(vertices, "<f4").tobytes()
biped = {
    "nodes": biped_nodes,
    "root_translation": (0, 85, 0),
    "checksum": 1234,
    "soft_skins": [],
    "attachments": [],
}
zenkit_py.write_model(str(work / "BIPED.MDH"), biped, "G2")
zenkit_py.write_model(str(work / "BIPED_BODY.MDM"), {**biped, "soft_skins": [skin]}, "G2")
root, root_translation = stored_root(work / "BIPED.MDH")
assert np.allclose(root[:3, 3], (0, 5, 5)) and np.allclose(root_translation, (0, 85, 0))
# Engine (x, y, z) is Blender (-x, -z, y): what faces the engine's +Z faces Blender's front, -Y.
expected = np.frombuffer(skin["mesh"]["positions"], "<f4").reshape(-1, 3)[:, [0, 2, 1]]
expected = expected * (-0.01, -0.01, 0.01)

# The mesh alone comes in alone, its groups named after the skeleton found next to it.
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "BIPED_BODY.MDM")) == {"FINISHED"}
assert [o.type for o in bpy.context.scene.objects] == ["MESH"]
lone = bpy.context.scene.objects[0]
assert {g.name for g in lone.vertex_groups} == weighted_names

# The skeleton stands where the mesh is, and the mesh goes onto it.
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "BIPED.MDH")) == {"FINISHED"}
skeleton = bpy.context.active_object
bones = skeleton.data.bones
assert {b.name for b in bones} == bone_names
assert skeleton.show_in_front
assert np.allclose(bones["BIP01"].head_local[:], (0, -0.05, 0.9), atol=1e-5)
assert np.allclose(bones["BIP01 HEAD"].head_local[:], (0, -0.05, 1.4), atol=1e-5)
# Bones reach their child, along a node axis (the spine) or not (the arm to the hand).
for bone, child in (("BIP01 SPINE", "BIP01 HEAD"), ("BIP01 L ARM", "BIP01 L HAND")):
    assert np.allclose(bones[bone].tail_local[:], bones[child].head_local[:], atol=1e-5), bone
helmet = next(o for o in bpy.context.scene.objects if o.get("zenkit_node") == "ZS_HELMET")
assert helmet.parent_type == "BONE" and helmet.parent_bone == "BIP01 HEAD"
assert np.allclose(helmet.matrix_world.to_translation()[:], (0, -0.05, 1.5), atol=1e-5)

assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "BIPED_BODY.MDM")) == {"FINISHED"}
assert len([o for o in bpy.context.scene.objects if o.type == "ARMATURE"]) == 1
(body,) = [o for o in skeleton.children if o.type == "MESH" and "zenkit_node" not in o]
assert body.modifiers[0].object == skeleton
assert np.allclose([v.co[:] for v in body.data.vertices], expected, atol=1e-5)

# On a skeleton with a longer spine, the body follows its bones, as the engine draws it: what
# hangs from the head moves up with it, what hangs from the hips stays.
tall_nodes = [*biped_nodes]
tall_nodes[1] = ("BIP01 SPINE", 0, node(25, 0, 0))
tall = {**biped, "nodes": tall_nodes}
zenkit_py.write_model(str(work / "TALL.MDH"), tall, "G2")
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "TALL.MDH")) == {"FINISHED"}
tall_skeleton = bpy.context.active_object
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "BIPED_BODY.MDM")) == {"FINISHED"}
(tall_body,) = [o for o in tall_skeleton.children if o.type == "MESH" and "zenkit_node" not in o]
moved = np.array([v.co[:] for v in tall_body.data.vertices]) - expected
on_head = np.arange(vertices) % 4 == 2
# Moving the vertices leaves the file's normals as they were.
wanted = convert.to_blender(np.frombuffer(skin["mesh"]["corner_normals"], "<f4").reshape(-1, 3))
normals = np.empty(len(tall_body.data.loops) * 3, np.float32)
tall_body.data.corner_normals.foreach_get("vector", normals)
assert np.allclose(normals.reshape(-1, 3), wanted, atol=1e-3)
assert np.allclose(moved[on_head], (0, 0, 0.05), atol=1e-5)
assert np.allclose(moved[np.arange(vertices) % 4 == 0], 0, atol=1e-5)
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "BIPED.MDH")) == {"FINISHED"}
skeleton = bpy.context.active_object

assert bpy.ops.export_scene.zenkit_model(filepath=str(work / "BIPED2.MDH")) == {"FINISHED"}
again = zenkit_py.read_model(str(work / "BIPED2.MDH"))
assert [n[:2] for n in again["nodes"]] == [n[:2] for n in biped_nodes]
for (_, _, ours), (_, _, theirs) in zip(again["nodes"], biped_nodes, strict=True):
    assert np.allclose(np.frombuffer(ours, "<f4"), np.frombuffer(theirs, "<f4"), atol=1e-3)
assert np.allclose(stored_root(work / "BIPED2.MDH")[0], root, atol=1e-5)
print("game skeleton ok")

# A model made in Blender: two bones, a skinned body and a rigid attachment.
deselect_all()
bpy.ops.object.armature_add(location=(0, 0, 0))
rig = bpy.context.active_object
rig.name = "RIG"
bpy.ops.object.mode_set(mode="EDIT")
root = rig.data.edit_bones[0]
root.name = "BIP01"
root.head, root.tail = (0, 0, 0), (0, 0, 1)
arm = rig.data.edit_bones.new("BIP01 ARM")
arm.head, arm.tail, arm.parent = (0, 0, 1), (1, 0, 1), root
bpy.ops.object.mode_set(mode="OBJECT")

bpy.ops.mesh.primitive_cube_add(size=1, location=(0.5, 0, 1))
body = bpy.context.active_object
body.parent = rig
body.matrix_world.translation = (0.5, 0, 1)
body.vertex_groups.new(name="BIP01").add(list(range(4)), 1.0, "REPLACE")
body.vertex_groups.new(name="BIP01 ARM").add(list(range(4, 8)), 1.0, "REPLACE")
body.modifiers.new("Armature", "ARMATURE").object = rig

bpy.ops.mesh.primitive_cube_add(size=0.2)
sword = bpy.context.active_object
sword.parent = rig
sword.parent_type = "BONE"
sword.parent_bone = "BIP01 ARM"
sword.matrix_world = rig.matrix_world @ rig.data.bones["BIP01 ARM"].matrix_local

deselect_all()
rig.select_set(True)
bpy.context.view_layer.objects.active = rig
for ext in ("MDL", "MDH", "MDM"):
    assert bpy.ops.export_scene.zenkit_model(filepath=str(work / f"RIG.{ext}")) == {"FINISHED"}

model = zenkit_py.read_model(str(work / "RIG.MDL"))
assert [n[0] for n in model["nodes"]] == ["BIP01", "BIP01 ARM"]
assert len(model["soft_skins"]) == 1 and len(model["attachments"]) == 1
assert model["attachments"][0]["node"] == "BIP01 ARM"
# The sword sits at its bone, so its vertices are small offsets from it, in centimetres.
sword_positions = np.frombuffer(model["attachments"][0]["mesh"]["positions"], "<f4")
assert np.abs(sword_positions).max() <= 10.001, sword_positions
assert np.allclose(node_positions(model)[1], (0, 100, 0), atol=1e-3)
assert zenkit_py.read_model(str(work / "RIG.MDM"))["checksum"] == model["checksum"]

# Back in: the whole model, then the meshes alone on the skeleton alone.
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "RIG.MDL")) == {"FINISHED"}
rig = bpy.context.active_object
skins = [o for o in rig.children if o.parent_type != "BONE"]
bones = [o for o in rig.children if o.parent_type == "BONE"]
assert len(skins) == 1 and len(bones) == 1 and bones[0].parent_bone == "BIP01 ARM"
assert {g.name for g in skins[0].vertex_groups} == {"BIP01", "BIP01 ARM"}
positions = np.array([v.co[:] for v in skins[0].data.vertices])
assert np.allclose(positions.min(axis=0), (0, -0.5, 0.5), atol=1e-4), positions.min(axis=0)
tip = (rig.matrix_world @ rig.data.bones["BIP01 ARM"].matrix_local).to_translation()
assert np.allclose(bones[0].matrix_world.to_translation()[:], tip[:], atol=1e-5)

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "RIG.MDH")) == {"FINISHED"}
skeleton = bpy.context.active_object
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "RIG.MDM")) == {"FINISHED"}
assert all(o.parent == skeleton for o in bpy.context.scene.objects if o.type == "MESH")
assert len([o for o in bpy.context.scene.objects if o.type == "ARMATURE"]) == 1

# Without an armature, the meshes come in alone, the sword where its bone would be.
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "RIG.MDM")) == {"FINISHED"}
assert not [o for o in bpy.context.scene.objects if o.type == "ARMATURE"]
sword = bpy.context.scene.objects["BIP01 ARM"]
assert np.allclose(sword.matrix_world.to_translation()[:], (0, 0, 1), atol=1e-5)
print("model ok")


def write_vdf(path, files):
    """A .VDF of the given name-to-bytes files, all in its root."""
    import struct

    entries, data = b"", b""
    offset = 296 + 80 * len(files)
    for i, (name, contents) in enumerate(files.items()):
        last = 0x40000000 if i == len(files) - 1 else 0
        entries += name.encode().ljust(64, b" ")
        entries += struct.pack("<IIII", offset + len(data), len(contents), last, 0x20)
        data += contents
    header = b"Test".ljust(256, b"\x1a") + b"PSVDSC_V2.00\n\r\n\r"
    header += struct.pack("<IIIIII", len(files), len(files), 0, len(data), 296, 0x50)
    path.write_bytes(header + entries + data)


# The folders of the preferences, worked out from a game folder, and the archives searched for
# textures. Only when the extension is installed, since only then are there preferences; the
# values it finds are put back afterwards.
addon = next((a for a in bpy.context.preferences.addons if a.module.endswith("zenkit_io")), None)
if addon is None:
    print("preferences skipped: the extension is not installed")
else:
    prefs = addon.preferences
    saved = {key: getattr(prefs, key) for key in ("game_folder", "use_custom", "textures", "game")}
    game = work / "Gothic II"
    (game / "System").mkdir(parents=True)
    (game / "System" / "Gothic2.exe").write_bytes(b"")
    meshes = game / "_WORK" / "data" / "Meshes" / "_COMPILED"
    meshes.mkdir(parents=True)
    shutil.copy(samples / "mesh0.mrm", meshes / "CAGE.MRM")
    (game / "Data").mkdir()
    texture = (samples / "erz.tex").read_bytes()
    write_vdf(game / "Data" / "Textures.vdf", {"OCODFLGATELI-C.TEX": texture})
    try:
        prefs.game_folder = str(game)
        prefs.use_custom = False
        prefs.game = "AUTO"
        assert prefs.folder("meshes") == meshes, prefs.folder("meshes")
        assert prefs.folder("textures") is None
        assert prefs.game_version() == "G2"

        for mat in list(bpy.data.materials):
            bpy.data.materials.remove(mat)
        for image in list(bpy.data.images):
            bpy.data.images.remove(image)
        assert bpy.ops.import_scene.zenkit_mesh(filepath=str(meshes / "CAGE.MRM")) == {"FINISHED"}
        mat = bpy.context.active_object.data.materials[0]
        found = [n.image for n in mat.node_tree.nodes if n.type == "TEX_IMAGE"]
        assert found and found[0].name == "OCODFLGATELI-C", "the archive's texture was not used"

        prefs.use_custom = True
        prefs.textures = str(samples)
        assert prefs.folder("textures") == samples
        assert prefs.folder("meshes") is None
    finally:
        for key, value in saved.items():
            setattr(prefs, key, value)
    print("preferences ok")

print("all ok")
