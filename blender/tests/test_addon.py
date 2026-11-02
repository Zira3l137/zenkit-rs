"""Exercises the extension in Blender, from the command line:

    blender --background --python tests/test_addon.py

It uses the installed extension if there is one, and the extension in this folder otherwise.
The library's sample files come from the repository's samples/ folder.
"""

import shutil
import struct
import sys
import tempfile
import types
import zlib
from pathlib import Path

import bpy
import numpy as np
from mathutils import Quaternion

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
package = bpy.types.IMPORT_SCENE_OT_zenkit_mesh.__module__.rsplit(".", 1)[0]
sys.modules[package + ".texture"].save(image, out, "B8G8R8A8", False)
w, h, original = zenkit_py.read_texture(str(samples / "erz.tex"))
w2, h2, written = zenkit_py.read_texture(str(out))
assert (w2, h2) == (w, h) and written == original, "texture changed on its way through Blender"
assert bpy.ops.export_image.zenkit_texture(
    filepath=str(work / "DXT.TEX"), image="erz", format="DXT1"
) == {"FINISHED"}
assert bpy.ops.export_image.zenkit_texture(
    filepath=str(work / "FAST.TEX"), image="erz", format="DXT1", quality="FAST"
) == {"FINISHED"}
assert zenkit_py.read_texture(str(work / "FAST.TEX"))[:2] == (128, 128)
# The game refuses DXT textures with a mipmap under 4 pixels on a side, so the levels stop at 4x4:
# 128, 64, 32, 16, 8, 4.
for format in ("DXT1", "DXT3"):
    path = work / f"MIP_{format}.TEX"
    assert bpy.ops.export_image.zenkit_texture(filepath=str(path), image="erz", format=format) == {
        "FINISHED"
    }
    header = struct.unpack_from("<4sIIIII", path.read_bytes())
    assert header[2:] == ({"DXT1": 10, "DXT3": 12}[format], 128, 128, 6), header
# Imported textures are packed into the .blend as they came.
assert image.packed_file is not None

# A texture's alpha reaches the material's shader when it has any; erz is opaque.
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
# Skinned meshes are named as the engine's tools name them.
(pipe,) = [o for o in bpy.context.scene.objects if o.name.startswith("ZM_SKIN_")]
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
# Meshes go into a collection named after their file, inside the armature's.
assert [c.name for c in body.users_collection] == ["BIPED_BODY.MDM"]
assert bpy.data.collections["BIPED_BODY.MDM"] in skeleton.users_collection[0].children[:]
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
# An armor's model has a skeleton of its own, without the slots, whose checksum the humans who wear
# it do not have. Imported onto their armature, its meshes export with theirs.
armor = {**biped, "nodes": biped_nodes[:5], "soft_skins": [skin]}
zenkit_py.write_model(str(work / "ARMOR.MDL"), armor, "G2")
humans_checksum = zenkit_py.read_model(str(work / "BIPED.MDH"))["checksum"]
assert zenkit_py.read_model(str(work / "ARMOR.MDL"))["checksum"] != humans_checksum
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "BIPED.MDH")) == {"FINISHED"}
humans = bpy.context.active_object
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "ARMOR.MDL"), use_active=True) == {
    "FINISHED"
}
assert [o for o in bpy.data.objects if o.type == "ARMATURE"] == [humans]
(worn,) = [o for o in humans.children if o.name.startswith("ZM_SKIN_")]
assert worn.modifiers[0].object == humans
assert np.allclose([v.co[:] for v in worn.data.vertices], expected, atol=1e-5)
assert bpy.ops.export_scene.zenkit_model(filepath=str(work / "ARMOR.MDM")) == {"FINISHED"}
assert zenkit_py.read_model(str(work / "ARMOR.MDM"))["checksum"] == humans_checksum
# Without the option, or with an armature that lacks bones, it gets an armature of its own.
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "ARMOR.MDL")) == {"FINISHED"}
assert len([o for o in bpy.data.objects if o.type == "ARMATURE"]) == 2
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
body.name = "ZM_SKIN_3"

bpy.ops.mesh.primitive_cube_add(size=0.2)
sword = bpy.context.active_object
sword.parent = rig
sword.parent_type = "BONE"
sword.parent_bone = "BIP01 ARM"
sword.matrix_world = rig.matrix_world @ rig.data.bones["BIP01 ARM"].matrix_local
# Named like the engine's tools name meshes on a bone, it stays a mesh rather than a node.
sword.name = "ZM_FRILL_7"

# A slot of one's own: anything named ZS_ hanging from a bone. A shape standing for it is not
# exported as a mesh.
hand_shape = bpy.data.meshes.new("hand")
hand_shape.from_pydata([(0, 0, 0), (0.1, 0, 0), (0, 0.1, 0)], [], [(0, 1, 2)])
hand = bpy.data.objects.new("ZS_HAND", hand_shape)
bpy.context.collection.objects.link(hand)
hand.parent = rig
hand.parent_type = "BONE"
hand.parent_bone = "BIP01 ARM"
hand.matrix_world = rig.matrix_world @ rig.data.bones["BIP01 ARM"].matrix_local

# KrxImpExp hangs sample meshes named DUMMY_ on bones to show them; they are not exported.
dummy = bpy.data.objects.new("DUMMY_BIP01 ARM", hand_shape.copy())
bpy.context.collection.objects.link(dummy)
dummy.parent = rig
dummy.parent_type = "BONE"
dummy.parent_bone = "BIP01 ARM"

deselect_all()
rig.select_set(True)
bpy.context.view_layer.objects.active = rig
for ext in ("MDL", "MDH", "MDM"):
    assert bpy.ops.export_scene.zenkit_model(filepath=str(work / f"RIG.{ext}")) == {"FINISHED"}

# The format names what to write when the name has no model extension of its own.
assert bpy.ops.export_scene.zenkit_model(filepath=str(work / "BARE"), format="MDH") == {"FINISHED"}
assert zenkit_py.read_model(str(work / "BARE.MDH"))["nodes"]

# In the file browser, the format and the name's extension follow each other.
check = bpy.types.EXPORT_SCENE_OT_zenkit_model.check
browser = types.SimpleNamespace(filepath="/x/RIG.MDL", format="MDH", shown_format="MDL")
assert check(browser, None) and browser.filepath.endswith("RIG.MDH")
browser.filepath = "/x/RIG.mdm"
assert check(browser, None) and browser.format == "MDM"
browser.filepath = "/x/RIG"
assert check(browser, None) and browser.filepath.endswith("RIG.MDM")
assert not check(browser, None)

model = zenkit_py.read_model(str(work / "RIG.MDL"))
assert [n[0] for n in model["nodes"]] == ["BIP01", "BIP01 ARM", "ZS_HAND"]
assert len(model["soft_skins"]) == 1 and len(model["attachments"]) == 1
assert model["attachments"][0]["node"] == "BIP01 ARM"
# The sword sits at its bone, so its vertices are small offsets from it, in centimetres.
sword_positions = np.frombuffer(model["attachments"][0]["mesh"]["positions"], "<f4")
assert np.abs(sword_positions).max() <= 10.001, sword_positions
assert np.allclose(node_positions(model)[1], (0, 100, 0), atol=1e-3)
# The checksum is the engine's: the CRC-32 of the node names, one after the other, which the game
# compares with the skeleton's when it puts the meshes on.
assert model["checksum"] == zlib.crc32(b"BIP01BIP01 ARMZS_HAND")
assert zenkit_py.read_model(str(work / "RIG.MDM"))["checksum"] == model["checksum"]

# Back in: the whole model, then the meshes alone on the skeleton alone.
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "RIG.MDL")) == {"FINISHED"}
rig = bpy.context.active_object
skins = [o for o in rig.children if o.parent_type != "BONE"]
bones = [o for o in rig.children if o.parent_type == "BONE" and "zenkit_node" not in o]
assert len(skins) == 1 and len(bones) == 1 and bones[0].parent_bone == "BIP01 ARM"
assert (skins[0].name, bones[0].name) == ("ZM_SKIN_0", "ZM_FRILL_0")
assert any(o.get("zenkit_node") == "ZS_HAND" for o in rig.children)
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
sword = bpy.context.scene.objects["ZM_FRILL_0"]
assert np.allclose(sword.matrix_world.to_translation()[:], (0, 0, 1), atol=1e-5)
print("model ok")


# Animations: OpenGothic builds a sample's matrix from the conjugate of the stored quaternion, which
# turns a Biped's pelvis, whose bones run along their node's X axis, to point up the spine.
fistrun = zenkit_py.read_animation(str(samples / "G2" / "HUMANS-S_FISTRUN.MAN"))
assert (fistrun["fps"], fistrun["frame_count"], len(fistrun["node_indices"])) == (10.0, 20, 25)
run = np.frombuffer(fistrun["transforms"], "<f4").reshape(20, 25, 4, 4)
assert np.allclose((run[0, 0] @ run[0, 1])[:3, 0], (0, 1, 0), atol=1e-3)
assert 80 < run[0, 0][1, 3] < 95, "the hips are not at hip height"


def rotation_z(degrees):
    c, s = np.cos(np.radians(degrees)), np.sin(np.radians(degrees))
    m = np.eye(4, dtype="<f4")
    m[:2, :2] = [[c, -s], [s, c]]
    return m


def model_world(nodes, local):
    """Every node's transform in the model, from transforms relative to the parents."""
    world = []
    for (_, parent, _), m in zip(nodes, local, strict=True):
        world.append(world[parent] @ m if parent >= 0 else m)
    return world


# Three frames on the biped skeleton: at rest, the spine bent and the hips moved, then the head
# turned and the helmet's slot pushed off the head, which only the slot's object can show.
rest = [np.frombuffer(rows, "<f4").reshape(4, 4) for _, _, rows in biped_nodes]
moving = []
for frame in range(3):
    local = [m.copy() for m in rest]
    if frame >= 1:
        local[1] = local[1] @ rotation_z(30)
        local[0][2, 3] += 10
    if frame == 2:
        local[2] = local[2] @ rotation_z(-20)
        local[5][0, 3] += 5
    moving.append(local)
moving = np.array(moving, "<f4")
biped_checksum = zenkit_py.read_model(str(work / "BIPED.MDH"))["checksum"]
test_anim = {
    "name": "S_TEST",
    "next": "S_TEST",
    "layer": 1,
    "fps": 25.0,
    "checksum": biped_checksum,
    "node_indices": list(range(len(biped_nodes))),
    "transforms": moving.tobytes(),
    "bbox": ((-50, 0, -50), (50, 200, 50)),
}
zenkit_py.write_animation(str(work / "BIPED-S_TEST.MAN"), test_anim)
stored = zenkit_py.read_animation(str(work / "BIPED-S_TEST.MAN"))
# Positions are packed into 16 bits over the range they span, here 0.002 apart.
assert np.allclose(
    np.frombuffer(stored["transforms"], "<f4").reshape(moving.shape), moving, atol=2e-3
)

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "BIPED.MDH")) == {"FINISHED"}
biped_rig = bpy.context.active_object
assert bpy.ops.import_anim.zenkit_animation(filepath=str(work / "BIPED-S_TEST.MAN")) == {"FINISHED"}
action = biped_rig.animation_data.action
assert action.zenkit.name == "S_TEST" and tuple(action.frame_range) == (0, 2)
assert action.zenkit.model == "BIPED" and action.zenkit.fps == 25.0
assert bpy.context.scene.render.fps == 25
helmet = next(o for o in biped_rig.children_recursive if o.get("zenkit_node") == "ZS_HELMET")
# Before Blender 4.4 an action animates one object, the armature, so the slot stays on the head.
shown = moving.copy()
if not hasattr(bpy.types, "ActionSlot"):
    shown[:, 5] = rest[5]
to_blender = np.array([[-1, 0, 0], [0, 0, -1], [0, 1, 0]]) * 0.01
for frame in range(3):
    bpy.context.scene.frame_set(frame)
    expected = [m[:3, 3] @ to_blender.T for m in model_world(biped_nodes, shown[frame])]
    for (name, _, _), place in zip(biped_nodes, expected, strict=True):
        if name == "ZS_HELMET":
            got = (biped_rig.matrix_world.inverted() @ helmet.matrix_world).to_translation()
        else:
            got = biped_rig.pose.bones[name].head
        assert np.allclose(got[:], place, atol=1e-4), (frame, name, got[:], place)

assert bpy.ops.export_anim.zenkit_animation(filepath=str(work / "OUT.MAN")) == {"FINISHED"}
out = zenkit_py.read_animation(str(work / "OUT.MAN"))
assert (out["name"], out["next"], out["layer"], out["fps"]) == ("S_TEST", "S_TEST", 1, 25.0)
assert out["checksum"] == biped_checksum and out["node_indices"] == list(range(6))
assert np.allclose(np.frombuffer(out["transforms"], "<f4").reshape(moving.shape), shown, atol=3e-3)
# The box holds every node in every frame, in the engine's space.
top = max(m[1, 3] for local in shown for m in model_world(biped_nodes, local))
assert np.allclose(out["bbox"][1][1], top, atol=0.01), out["bbox"]

# A pose made in Blender comes out as the bone's turn relative to its parent.
bones = biped_rig.pose.bones
spine = bones["BIP01 SPINE"]
biped_rig.animation_data.action = None
for bone in bones:
    bone.location, bone.rotation_quaternion = (0, 0, 0), (1, 0, 0, 0)
spine.rotation_quaternion = Quaternion((0, 1, 0), 0.5)
own = bpy.data.actions.new("T_BEND")
biped_rig.animation_data.action = own
if hasattr(own, "slots"):
    biped_rig.animation_data.action_slot = own.slots.new("OBJECT", biped_rig.name)
for frame in (0, 4):
    spine.keyframe_insert("rotation_quaternion", frame=frame)
assert bpy.ops.export_anim.zenkit_animation(filepath=str(work / "BEND.MAN")) == {"FINISHED"}
bend = zenkit_py.read_animation(str(work / "BEND.MAN"))
assert bend["name"] == "T_BEND" and bend["frame_count"] == 5
bent = np.frombuffer(bend["transforms"], "<f4").reshape(5, 6, 4, 4)
assert np.allclose(bent[:, 0], moving[0, 0], atol=3e-3), "the hips moved"
assert np.allclose(bent[:, 1, :3, 3], rest[1][:3, 3], atol=3e-3), "the spine came off the hips"
# Turned about the bone's Y axis, the spine's own axis, its node turns about its X axis.
assert np.allclose(bent[0, 1, :3, 0], rest[1][:3, 0], atol=1e-3)
assert not np.allclose(bent[0, 1, :3, 1], rest[1][:3, 1], atol=1e-2)

# The game's own run, on a stand-in skeleton whose rest pose is its first frame, goes out as it came
# in: real rotations, through bones turned from their nodes.
indices = fistrun["node_indices"]
stand_in = [
    (
        f"BIP01 N{i}" if i else "BIP01",
        i - 1,
        (run[0, indices.index(i)] if i in indices else np.eye(4, dtype="<f4")).tobytes(),
    )
    for i in range(max(indices) + 1)
]
stand_in_model = {"nodes": stand_in, "root_translation": None, "soft_skins": [], "attachments": []}
zenkit_py.write_model(str(work / "RUNNER.MDH"), stand_in_model, "G2")
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "RUNNER.MDH")) == {"FINISHED"}
fistrun_path = samples / "G2" / "HUMANS-S_FISTRUN.MAN"
assert bpy.ops.import_anim.zenkit_animation(filepath=str(fistrun_path)) == {"FINISHED"}
assert bpy.ops.export_anim.zenkit_animation(filepath=str(work / "RUNNER.MAN")) == {"FINISHED"}
runner = zenkit_py.read_animation(str(work / "RUNNER.MAN"))
assert runner["fps"] == 10.0 and runner["frame_count"] == 20
again = np.frombuffer(runner["transforms"], "<f4").reshape(20, len(stand_in), 4, 4)[:, indices]
assert np.allclose(again[..., :3, :3], run[..., :3, :3], atol=1e-3)
assert np.allclose(again[..., :3, 3], run[..., :3, 3], atol=1e-3)
print("animation ok")


class Layout:
    """Stands in for a panel's layout, checking every call against Blender's: that layouts have
    the function, that it takes those keywords, and that the properties, lists and operators it
    names exist. Panels are never drawn in the background, so this is how they are tried."""

    def __getattr__(self, name):
        function = bpy.types.UILayout.bl_rna.functions.get(name)
        assert function is not None, f"layouts have no {name}"
        keywords = set(function.parameters.keys())

        def call(*args, **kwargs):
            assert set(kwargs) <= keywords, (name, set(kwargs) - keywords)
            if name in ("prop", "template_ID"):
                assert args[1] in args[0].bl_rna.properties, (name, args[1])
            if name == "template_list":
                assert args[3] in args[2].bl_rna.properties, args[3]
                assert args[5] in args[4].bl_rna.properties, args[5]
            if name == "operator" or "new" in kwargs:
                module, operator = (args[0] if name == "operator" else kwargs["new"]).split(".")
                getattr(getattr(bpy.ops, module), operator).get_rna_type()
            return Layout()

        return call


ui = sys.modules[package + ".ui"]


def draw_panels():
    """Draws every panel of the Gothic tab that shows for the active object."""
    drawn = []
    for panel in (c for c in ui.CLASSES if issubclass(c, bpy.types.Panel)):
        parent = getattr(ui, getattr(panel, "bl_parent_id", ""), None)
        shown = getattr(parent or panel, "poll", lambda context: True)(bpy.context)
        if shown:
            panel.draw(types.SimpleNamespace(layout=Layout()), bpy.context)
            drawn.append(panel.bl_label)
    return drawn


# Morph meshes: a shape key per frame, back out the same.
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_mesh(filepath=str(samples / "morph0.mmb")) == {"FINISHED"}
bow = bpy.context.active_object
keys = [k.name for k in bow.data.shape_keys.key_blocks]
assert (
    keys[:4] == ["Basis", "S_NORMAL", "S_SHOOT 0", "S_SHOOT 1"] and len(keys) == 1 + 1 + 10 + 1 + 29
)
# The keys start at 0 but for the frame of the first animation its action shows, so the mesh
# never shows several frames at once.
reference = bow.data.shape_keys.reference_key
assert [k.name for k in bow.data.shape_keys.key_blocks if k.value and k != reference] == [
    "S_NORMAL"
]
# Each animation has an action to watch it with: S_SHOOT shows its frame 3 at frame 3.
bow.data.shape_keys.animation_data.action = bpy.data.actions["ITRWSMALLBOW S_SHOOT"]
bpy.context.view_layer.objects.active = bow
bpy.context.scene.frame_set(3)
shooting = [k.name for k in bow.data.shape_keys.key_blocks if k.value > 0.5 and k != reference]
assert shooting == ["S_SHOOT 3"], shooting
bow.data.shape_keys.animation_data.action = None
shoot = bow.data.zenkit_morph.animations["S_SHOOT"]
assert (shoot.layer, shoot.fps, shoot.until_stopped, shoot.repeat) == (1, 25.0, False, False)
assert np.isclose(shoot.duration, 0.4)
assert bpy.ops.export_scene.zenkit_mmb(filepath=str(work / "BOW.MMB")) == {"FINISHED"}
original, written = (
    zenkit_py.read_morph(str(samples / "morph0.mmb")),
    zenkit_py.read_morph(str(work / "BOW.MMB")),
)
assert written["name"] == "ITRWSMALLBOW" and written["sources"] == original["sources"]
for theirs, ours in zip(original["animations"], written["animations"], strict=True):
    for key in ("name", "layer", "blend_in", "blend_out", "duration", "speed", "flags", "vertices"):
        assert theirs[key] == ours[key], (theirs["name"], key)
    assert np.allclose(
        np.frombuffer(theirs["samples"], "<f4"), np.frombuffer(ours["samples"], "<f4"), atol=1e-4
    )

# A vertex moved in a shape key joins its animation; new keys make new animations.
bow.data.shape_keys.key_blocks["S_READY"].data[3].co.z += 0.1
new = bow.shape_key_add(name="T_WOBBLE 0", from_mix=False)
new.data[0].co.x += 0.05
bow.shape_key_add(name="T_WOBBLE 1", from_mix=False).data[0].co.x -= 0.05
# The keys' values do not change the mesh that is written.
bow.data.shape_keys.key_blocks["S_READY"].value = 1.0
assert bpy.ops.export_scene.zenkit_mmb(filepath=str(work / "BOW2.MMB")) == {"FINISHED"}
edited = zenkit_py.read_morph(str(work / "BOW2.MMB"))
assert edited["mesh"]["positions"] == written["mesh"]["positions"]
ready = next(a for a in edited["animations"] if a["name"] == "S_READY")
assert list(np.frombuffer(ready["vertices"], "<u4")) == [3, 25, 26, 27]
assert np.allclose(np.frombuffer(ready["samples"], "<f4").reshape(-1, 3)[0], (0, 10, 0), atol=1e-3)
wobble = edited["animations"][-1]
assert (wobble["name"], wobble["frame_count"], wobble["flags"]) == ("T_WOBBLE", 2, 0)
assert list(np.frombuffer(wobble["vertices"], "<u4")) == [0]

# The Morph Mesh panel: an animation of two frames made there, the selected frame shown and edited,
# and exported into the export folder.
morph_out = work / "panel_morph"
morph_out.mkdir()
bpy.context.scene.zenkit.folder = str(morph_out)
assert bpy.ops.zenkit.new_morph_animation(name="t_blink") == {"FINISHED"}
assert bpy.ops.zenkit.add_morph_frame() == {"FINISHED"}
blink_keys = [k.name for k in bow.data.shape_keys.key_blocks if k.name.startswith("T_BLINK")]
assert blink_keys == ["T_BLINK 0", "T_BLINK 1"] and bow.data.zenkit_morph.frame == 1
assert bow.active_shape_key.name == "T_BLINK 1" and bow.active_shape_key.value == 1.0
others = [k for k in bow.data.shape_keys.key_blocks if k not in (bow.active_shape_key, reference)]
assert all(k.value == 0.0 for k in others)
bow.active_shape_key.data[5].co.z += 0.2
assert "Morph Mesh" in draw_panels()
assert bpy.ops.zenkit.export_morph() == {"FINISHED"}
panel_morph = zenkit_py.read_morph(str(morph_out / "ITRWSMALLBOW.MMB"))
blink = next(a for a in panel_morph["animations"] if a["name"] == "T_BLINK")
assert blink["frame_count"] == 2 and list(np.frombuffer(blink["vertices"], "<u4")) == [5]
assert np.isclose(blink["speed"], 0.025) and blink["duration"] == -1000.0
# The wobble's keys had no settings; exporting gave them some, which the panel now shows.
assert "T_WOBBLE" in bow.data.zenkit_morph.animations
# The new animation has an action of its own, which the panel plays: frames 0 and 1.
assert bow.data.shape_keys.animation_data.action.name == "ITRWSMALLBOW T_BLINK"
assert (bpy.context.scene.frame_start, bpy.context.scene.frame_end) == (0, 1)

# Selecting an animation in the list plays its action over its frames, at the rate the game
# shows them: S_SHOOT's 10 frames spread over 0.4 s are 25 a second.
morph_settings = bow.data.zenkit_morph
morph_settings.index = morph_settings.animations.find("S_SHOOT")
assert bow.data.shape_keys.animation_data.action.name == "ITRWSMALLBOW S_SHOOT"
assert (bpy.context.scene.frame_start, bpy.context.scene.frame_end) == (0, 9)
scene_fps = bpy.context.scene.render.fps / bpy.context.scene.render.fps_base
assert np.isclose(scene_fps, 25.0)
# Renaming an animation renames its shape keys and action.
morph_settings.animations["T_BLINK"].name = "T_WINK"
wink_keys = [k.name for k in bow.data.shape_keys.key_blocks if k.name.startswith("T_W")]
assert wink_keys[-2:] == ["T_WINK 0", "T_WINK 1"]
assert "ITRWSMALLBOW T_WINK" in bpy.data.actions
print("morph ok")

# Model scripts: the skeleton, its registered mesh and the animations a script names, found next
# to it, and the script itself as a text. Earlier tests' animations of the biped go first.
for stale in list(bpy.data.actions):
    bpy.data.actions.remove(stale)
script_source = """Model ("BIPED")
{
	meshAndTree ("HUM_BODY_NAKED0.ASC" DONT_USE_MESH)
	registerMesh ("BIPED_BODY.ASC")

	aniEnum
	{
		ani ("S_TEST" 1 "S_TEST" 0.1 0.1 M. "BIPED_TEST.ASC" F 1 3 FPS:10)
		{
			*eventSFX (1 "STEP" R:500)
		}
		ani ("S_GONE" 2 "" 0.0 0.0 M. "BIPED_GONE.ASC" F 1 3)
	}
}
"""
zenkit_py.write_script(str(work / "BIPED.MSB"), script_source)
info = zenkit_py.read_script(str(work / "BIPED.MSB"))
assert (info["name"], info["disable_mesh"]) == ("BIPED", True)
assert info["skeleton"].upper().startswith("HUM_BODY_NAKED0")
assert [a["name"] for a in info["animations"]] == ["S_TEST", "S_GONE"]
assert info["animations"][0]["fps"] == 10.0 and "*eventSFX" in info["text"]

# As in HUMANS.MDS, the skeleton's source is a body whose mesh is left out, so the compiled
# skeleton is named after the model, BIPED.MDH, while a mesh of the source's name lies beside it.
shutil.copy(work / "BIPED_BODY.MDM", work / "HUM_BODY_NAKED0.MDM")
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_script(filepath=str(work / "BIPED.MSB")) == {"FINISHED"}
model_rig = bpy.context.active_object
assert model_rig.type == "ARMATURE" and model_rig.name == "BIPED"
assert [o.name for o in model_rig.children if o.name.startswith("ZM_SKIN_")] == ["ZM_SKIN_0"]
assert model_rig.data.zenkit.model == "BIPED"
text = model_rig.data.zenkit.script
assert zenkit_py.parse_script(text.as_string())["animations"] == info["animations"]
played = model_rig.animation_data.action
assert played.zenkit.name == "S_TEST" and played.zenkit.checksum == str(biped_checksum)
# The script's settings for the animation are the action's.
assert (played.zenkit.fps, played.zenkit.flags, played.zenkit.next) == (10.0, {"M"}, "S_TEST")
assert np.isclose(played.zenkit.blend_in, 0.1)
# Everything is in a collection named after the model, the body in one named after its file.
model_collection = bpy.data.collections["BIPED"]
assert model_rig.users_collection[0] == model_collection
(skin,) = [o for o in model_rig.children if o.name.startswith("ZM_SKIN_")]
assert skin.users_collection[0].name == "BIPED_BODY.MDM"
assert skin.users_collection[0] in model_collection.children[:]

# Exported, the script's animations take their layer, frame rate and next animation from it.
(work / "out").mkdir()
assert bpy.ops.export_scene.zenkit_script(filepath=str(work / "out" / "BIPED.MSB")) == {"FINISHED"}
assert sorted(p.name for p in (work / "out").iterdir()) == ["BIPED-S_TEST.MAN", "BIPED.MSB"]
assert zenkit_py.read_script(str(work / "out" / "BIPED.MSB"))["animations"] == info["animations"]
again = zenkit_py.read_animation(str(work / "out" / "BIPED-S_TEST.MAN"))
assert (again["fps"], again["layer"], again["next"]) == (10.0, 1, "S_TEST")
assert np.allclose(
    np.frombuffer(again["transforms"], "<f4").reshape(moving.shape), shown, atol=3e-3
)
assert bpy.ops.export_scene.zenkit_script(
    filepath=str(work / "out" / "BIPED.MDS"), animations=False
) == {"FINISHED"}
assert zenkit_py.read_script(str(work / "out" / "BIPED.MDS"))["animations"] == info["animations"]

# The Model, Animations and Export panels: settings changed there, a new animation, and the whole
# model written at once, named as the game looks for it.
deselect_all()
model_rig.select_set(True)
bpy.context.view_layer.objects.active = model_rig
# The script holds an animation's settings once it has its ani: the panel shows that line and
# changing a setting there rewrites it, and only it.
text = model_rig.data.zenkit.script
text.from_string(text.as_string().replace("aniEnum", "// kept as written\n\taniEnum"))
scripttext = sys.modules[package + ".scripttext"]
assert scripttext.line_of(text, played) is not None
played.zenkit.layer = 3
played.zenkit.flags = {"M", "I"}
assert 'ani ("S_TEST" 3 "S_TEST" 0.1 0.1 MI ' in text.as_string()
assert "// kept as written" in text.as_string() and "*eventSFX" in text.as_string()
# Changing the text changes the panel, as the timer finds.
text.from_string(text.as_string().replace('3 "S_TEST" 0.1', '3 "S_GONE" 0.1'))
scripttext.watch()
assert played.zenkit.next == "S_GONE"
played.zenkit.next = "S_TEST"
# Renaming the animation renames its ani.
played.zenkit.name = "S_TESTED"
assert 'ani ("S_TESTED"' in text.as_string() and 'ani ("S_TEST" ' not in text.as_string()
played.zenkit.name = "S_TEST"
assert bpy.ops.zenkit.new_animation(name="t_wave") == {"FINISHED"}
wave = model_rig.animation_data.action
# A new animation goes into the script at once.
assert 'ani ("T_WAVE"' in text.as_string()
assert (wave.zenkit.name, wave.zenkit.model) == ("T_WAVE", "BIPED")
# The list's selection is the action the armature plays: picking a row plays it.
model_rig.zenkit_animation_index = bpy.data.actions.find(played.name)
assert model_rig.animation_data.action == played and bpy.context.scene.render.fps == 10
listed, _ = ui.ZENKIT_UL_animations.filter_items(
    types.SimpleNamespace(filter_name="", bitflag_filter_item=1), bpy.context, bpy.data, "actions"
)
assert sorted(a.zenkit.name for a, f in zip(bpy.data.actions, listed) if f) == ["S_TEST", "T_WAVE"]

export = sys.modules[package + ".export"]
files = export.plan(model_rig)
assert (files.skeleton, list(files.meshes), files.script) == (
    "BIPED.MDH",
    ["BIPED_BODY.MDM"],
    "BIPED.MSB",
)
assert [n for n, _ in files.animations] == ["BIPED-S_TEST.MAN", "BIPED-T_WAVE.MAN"]
assert files.unused == ["S_GONE"]
assert draw_panels() == ["Model", "Animations", "Export"]

panel_out = work / "panel"
panel_out.mkdir()
bpy.context.scene.zenkit.folder = str(panel_out)
assert bpy.ops.zenkit.export_model() == {"FINISHED"}
assert sorted(p.name for p in panel_out.iterdir()) == [
    "BIPED-S_TEST.MAN",
    "BIPED-T_WAVE.MAN",
    "BIPED.MDH",
    "BIPED.MSB",
    "BIPED_BODY.MDM",
]
for name in ("BIPED.MDH", "BIPED_BODY.MDM"):
    assert zenkit_py.read_model(str(panel_out / name))["checksum"] == biped_checksum
# The script got the animations' settings and the new animation, and kept the rest.
exported = zenkit_py.read_script(str(panel_out / "BIPED.MSB"))
by_name = {a["name"]: a for a in exported["animations"]}
assert list(by_name) == ["S_TEST", "S_GONE", "T_WAVE"]
assert (by_name["S_TEST"]["layer"], by_name["S_TEST"]["flags"]) == (3, "MI")
assert by_name["S_GONE"] == info["animations"][1]
assert "*eventSFX" in exported["text"] and "T_WAVE" in model_rig.data.zenkit.script.as_string()
test_man = zenkit_py.read_animation(str(panel_out / "BIPED-S_TEST.MAN"))
assert (test_man["layer"], test_man["fps"]) == (3, 10.0)

# A model without a script gets one from the Model panel, its meshes registered by file.
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete()
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "BIPED.MDH")) == {"FINISHED"}
assert bpy.ops.import_scene.zenkit_model(filepath=str(work / "BIPED_BODY.MDM")) == {"FINISHED"}
bare = bpy.context.view_layer.objects.active
assert bare.type == "ARMATURE" and bare.data.zenkit.script is None
assert export.plan(bare).script is None
assert bpy.ops.zenkit.new_script() == {"FINISHED"}
made = zenkit_py.parse_script(bare.data.zenkit.script.as_string())
assert (made["name"], made["meshes"], made["disable_mesh"]) == ("BIPED", ["BIPED_BODY.ASC"], True)
assert export.plan(bare).skeleton == "BIPED.MDH"
print("panels ok")

# A script whose Model line names another model than its file, as Gothic 1's BABE.MDS says BAB.
# The engine goes by the file, so BABE.MDH and BABE-S_TEST.MAN are found by name; a skeleton under
# another name still is, by the checksum of the script's mesh, or of its animation if it has none.
renamed_source = script_source.replace('Model ("BIPED")', 'Model ("BAB")')
cases = {
    "by_name": {"BABE.MDH": "BIPED.MDH", "BIPED_BODY.MDM": "BIPED_BODY.MDM"},
    "by_mesh": {"SKELETON.MDH": "BIPED.MDH", "BIPED_BODY.MDM": "BIPED_BODY.MDM"},
    "by_animation": {"SKELETON.MDH": "BIPED.MDH"},
}
for case, files in cases.items():
    folder = work / case
    folder.mkdir()
    (folder / "BABE.MDS").write_text(renamed_source)
    for name, source in files.items():
        shutil.copy(work / source, folder / name)
    animation_file = "BAB-S_TEST.MAN" if case == "by_animation" else "BABE-S_TEST.MAN"
    shutil.copy(work / "BIPED-S_TEST.MAN", folder / animation_file)

    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete()
    assert bpy.ops.import_scene.zenkit_script(filepath=str(folder / "BABE.MDS")) == {"FINISHED"}
    rig = bpy.context.active_object
    expected = "BAB" if case == "by_animation" else "BABE"
    assert rig.type == "ARMATURE" and rig.data.zenkit.model == expected, (case, rig.name)
    assert rig.animation_data.action.zenkit.name == "S_TEST", case
    skins = [o for o in rig.children if o.name.startswith("ZM_SKIN_")]
    assert len(skins) == (0 if case == "by_animation" else 1), case

# With nothing to go by, the error says what was looked for.
lost = work / "lost"
lost.mkdir()
(lost / "BABE.MDS").write_text(renamed_source)
shutil.copy(work / "BIPED-S_TEST.MAN", lost / "BABE-S_TEST.MAN")
try:
    bpy.ops.import_scene.zenkit_script(filepath=str(lost / "BABE.MDS"))
except RuntimeError as error:
    assert "BABE.MDH" in str(error) and f"checksum {biped_checksum}" in str(error), error
else:
    raise AssertionError("a script without its skeleton imported")
print("misnamed script ok")

# The game's own: without the game, the alligator's skeleton is nowhere, and the import says so.
# With the installed extension pointed at Gothic II, it comes from the archives, and one of its
# animations goes out as it came in.
alligator_man = samples / "G2" / "ALLIGATOR-R_AGRESSIVE.MAN"
try:
    bpy.ops.import_scene.zenkit_script(
        filepath=str(samples / "G2" / "ALLIGATOR.MSB"), pattern="R_AGRESSIVE"
    )
except RuntimeError as error:
    assert "ALLIGATOR.MDH" in str(error), error
    print("game script skipped: the alligator's skeleton is not in the game's archives")
else:
    alligator = bpy.context.active_object
    assert alligator.name.startswith("ALLIGATOR")
    assert alligator.animation_data.action.zenkit.name == "R_AGRESSIVE"
    assert bpy.ops.export_anim.zenkit_animation(filepath=str(work / "ALLIGATOR.MAN")) == {
        "FINISHED"
    }
    theirs = zenkit_py.read_animation(str(alligator_man))
    ours = zenkit_py.read_animation(str(work / "ALLIGATOR.MAN"))
    assert ours["checksum"] == theirs["checksum"] and ours["node_indices"] == theirs["node_indices"]
    shape = (theirs["frame_count"], len(theirs["node_indices"]), 4, 4)
    theirs = np.frombuffer(theirs["transforms"], "<f4").reshape(shape)
    ours = np.frombuffer(ours["transforms"], "<f4").reshape(shape)
    assert np.allclose(ours[..., :3, :3], theirs[..., :3, :3], atol=1e-3)
    assert np.allclose(ours[..., :3, 3], theirs[..., :3, 3], atol=1e-2)
    print("game script ok")
print("script ok")


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

        # The archives stay mounted, memory-mapped, until the preferences that pick them change.
        mounted = sys.modules[package + ".preferences"]._mounted
        assert mounted["archives"] is not None
        prefs.use_archives = False
        assert mounted["archives"] is None and mounted["folder"] is None

        prefs.use_custom = True
        prefs.textures = str(samples)
        assert prefs.folder("textures") == samples
        assert prefs.folder("meshes") is None
    finally:
        for key, value in saved.items():
            setattr(prefs, key, value)
    print("preferences ok")

# Packed textures come back with the .blend. Opening it replaces everything, so this goes last.
convert.material("ERZ_SAVED", "ERZ.TGA", [work]).use_fake_user = True
saved_image = next(i for i in bpy.data.images if i.name.startswith("ERZ-C"))
erz_name = saved_image.name
erz_pixels = np.empty(128 * 128 * 4, np.float32)
saved_image.pixels.foreach_get(erz_pixels)
bpy.ops.wm.save_as_mainfile(filepath=str(work / "saved.blend"))
bpy.ops.wm.open_mainfile(filepath=str(work / "saved.blend"))
reopened = np.empty(128 * 128 * 4, np.float32)
bpy.data.images[erz_name].pixels.foreach_get(reopened)
assert np.array_equal(reopened, erz_pixels), "the packed texture changed in the .blend"
print("saved ok")

print("all ok")
