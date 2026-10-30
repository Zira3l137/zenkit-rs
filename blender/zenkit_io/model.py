"""Gothic models to and from Blender armatures and meshes.

A skeleton (.MDH) becomes an armature, one bone per node. Its meshes (.MDM) hang from it: soft
skins, the bodies that bend with the bones, as meshes with vertex groups and an Armature
modifier; attachments, rigid meshes like a door or a sword, as meshes parented to their bone. A
.MDL holds both. Exporting goes the other way, from an armature and the meshes parented to it.

The checksum that ties a skeleton to its meshes and animations is kept on the armature, so that
meshes exported later still fit the skeleton the game has.
"""

import math
import zlib
from pathlib import Path

import bpy
import numpy as np
import zenkit_py
from mathutils import Matrix
from mathutils import Vector

from . import convert
from . import preferences
from .mesh import link

CHECKSUM = "zenkit_checksum"
AXIS = "zenkit_axis"

# Blender bones point along their Y axis, while a node may point along any of its axes (the
# bones of 3ds Max's Biped, which Gothic's skeletons come from, along X). A bone is its node
# turned so that Y is the node's axis named here; the turn is kept on the bone to undo it.
TURNS = {
    "Y": Matrix.Identity(4),
    "-Y": Matrix.Rotation(math.pi, 4, "Z"),
    "X": Matrix.Rotation(-math.pi / 2, 4, "Z"),
    "-X": Matrix.Rotation(math.pi / 2, 4, "Z"),
    "Z": Matrix.Rotation(math.pi / 2, 4, "X"),
    "-Z": Matrix.Rotation(-math.pi / 2, 4, "X"),
}


def node_frame(bone):
    """A bone's node in the armature's space, its turn undone."""
    return bone.matrix_local @ TURNS[bone.get(AXIS, "Y")].inverted()


def bone_axis(frame, offsets):
    """The axis of a node that best points at one of its children, and the length along it
    that reaches that child. `None` if no child stands apart from the node."""
    best = None
    for offset in offsets:
        if offset.length < 1e-6:
            continue
        for name, turn in TURNS.items():
            axis = (frame.to_3x3() @ turn.to_3x3() @ Vector((0, 1, 0))).normalized()
            along = axis.dot(offset)
            fit = along / offset.length
            if best is None or fit > best[0]:
                best = (fit, name, along)
    return (best[1], best[2]) if best is not None and best[2] > 0 else None


def build_armature(context, name, data, scale):
    """An armature object of the model's nodes, linked to the scene and left in object mode."""
    nodes = data["nodes"]
    world = []
    for _, parent, rows in nodes:
        local = convert.matrix_to_blender(rows, scale)
        world.append(world[parent] @ local if parent >= 0 else local)

    shapes = []
    for i, (_, parent, _) in enumerate(nodes):
        head = world[i].to_translation()
        children = [world[j].to_translation() - head for j, (_, p, _) in enumerate(nodes) if p == i]
        shape = bone_axis(world[i], children)
        if shape is None:
            # A bone with nowhere to point to follows its parent's axis, at half its length.
            axis, length = shapes[parent] if parent >= 0 else ("Y", 0.0)
            shape = (axis, length / 2)
        shapes.append((shape[0], max(shape[1], 5 * scale)))

    armature = bpy.data.armatures.new(name)
    armature[CHECKSUM] = str(data["checksum"])
    obj = bpy.data.objects.new(name, armature)
    link(context, obj)

    bpy.ops.object.mode_set(mode="EDIT")
    bones = []
    for (node_name, parent, _), (axis, length), frame in zip(nodes, shapes, world, strict=True):
        bone = armature.edit_bones.new(node_name)
        bone.head = (0.0, 0.0, 0.0)
        bone.tail = (0.0, length, 0.0)
        bone.matrix = frame @ TURNS[axis]
        if parent >= 0:
            bone.parent = bones[parent]
        bones.append(bone)
    bpy.ops.object.mode_set(mode="OBJECT")
    for bone, (axis, _) in zip(armature.bones, shapes, strict=True):
        bone[AXIS] = axis
    return obj


def node_names(armature):
    """The bones of an armature in an order the engine accepts: every parent before its
    children."""
    order = []

    def visit(bone):
        order.append(bone)
        for child in bone.children:
            visit(child)

    for bone in armature.data.bones:
        if bone.parent is None:
            visit(bone)
    return order


def find_skeleton(context, checksum, folder):
    """The nodes of the skeleton with this checksum, for meshes that come without one.

    The active armature is tried first, then the skeletons next to the meshes and in the models
    folder, then the game's archives. Gives `(armature object, None)` for an armature in the
    scene, `(None, model dict)` for a skeleton file, or `(None, None)`.
    """
    active = context.active_object
    if active is not None and active.type == "ARMATURE":
        if active.data.get(CHECKSUM) == str(checksum):
            return active, None

    folders = [folder, preferences.folder("anims")]
    for path in (p for f in folders if f for p in sorted(f.iterdir())):
        if path.suffix.lower() in (".mdh", ".mdl"):
            try:
                data = zenkit_py.read_model(str(path))
            except zenkit_py.ZenKitError:
                continue
            if data["checksum"] == checksum and data["nodes"]:
                data["name"] = path.stem
                return None, data

    archives = preferences.archives()
    for name in archives.names(".MDH") if archives is not None else []:
        data = archives.read_model(name)
        if data is not None and data["checksum"] == checksum:
            data["name"] = Path(name).stem
            return None, data
    return None, None


def import_model(context, path, scale):
    data = zenkit_py.read_model(str(path))
    armature = None
    if data["nodes"]:
        armature = build_armature(context, path.stem, data, scale)
    elif data["soft_skins"] or data["attachments"]:
        armature, skeleton = find_skeleton(context, data["checksum"], path.parent)
        if skeleton is not None:
            armature = build_armature(context, skeleton["name"], skeleton, scale)

    folders = [path.parent]
    names = [bone.name for bone in armature.data.bones] if armature else []
    for i, skin in enumerate(data["soft_skins"]):
        mesh = convert.build_mesh(f"{path.stem}.{i}", skin["mesh"], scale, folders)
        obj = bpy.data.objects.new(mesh.name, mesh)
        link(context, obj)
        add_weights(obj, skin, names)
        if armature is not None:
            obj.parent = armature
            obj.modifiers.new("Armature", "ARMATURE").object = armature

    for attachment in data["attachments"]:
        mesh = convert.build_mesh(attachment["node"], attachment["mesh"], scale, folders)
        obj = bpy.data.objects.new(attachment["node"], mesh)
        link(context, obj)
        bone = armature.data.bones.get(attachment["node"]) if armature else None
        if bone is not None:
            obj.parent = armature
            obj.parent_type = "BONE"
            obj.parent_bone = bone.name
            obj.matrix_world = armature.matrix_world @ node_frame(bone)

    if armature is not None:
        for other in context.selected_objects:
            other.select_set(False)
        armature.select_set(True)
        context.view_layer.objects.active = armature
    return armature, data


def add_weights(obj, skin, names):
    """Vertex groups from a soft skin's weights, named after the bones, or after the node
    indices when there is no skeleton."""
    counts = convert.array(skin, "weight_counts", "<u4")
    nodes = convert.array(skin, "weight_nodes", "<u4")
    values = convert.array(skin, "weight_values", "<f4")
    groups = {}
    start = 0
    for vertex, count in enumerate(counts):
        for node, value in zip(
            nodes[start : start + count], values[start : start + count], strict=True
        ):
            name = names[node] if node < len(names) else f"NODE {node}"
            if name not in groups:
                groups[name] = obj.vertex_groups.new(name=name)
            groups[name].add([vertex], float(value), "REPLACE")
        start += count


def gather_model(armature, depsgraph, scale):
    """A model dict of an armature and the meshes parented to it."""
    bones = node_names(armature)
    index = {bone.name: i for i, bone in enumerate(bones)}
    nodes = []
    for bone in bones:
        parent = bone.parent
        local = node_frame(parent).inverted() @ node_frame(bone) if parent else node_frame(bone)
        nodes.append(
            (
                bone.name,
                index[parent.name] if parent else -1,
                convert.matrix_to_engine(local, scale),
            )
        )

    checksum = armature.data.get(CHECKSUM)
    if checksum is None:
        checksum = zlib.crc32("\n".join(b.name for b in bones).encode("latin-1", "replace"))
    soft_skins = []
    attachments = []
    for obj in armature.children:
        if obj.type != "MESH":
            continue
        if obj.parent_type == "BONE" and obj.parent_bone in index:
            space = armature.matrix_world @ node_frame(armature.data.bones[obj.parent_bone])
            mesh = convert.gather_mesh(obj, depsgraph, scale, True, space)
            attachments.append({"node": obj.parent_bone, "mesh": mesh})
        else:
            mesh, weights = convert.gather_mesh(
                obj, depsgraph, scale, True, armature.matrix_world, index
            )
            soft_skins.append(
                {
                    "mesh": mesh,
                    "weight_counts": np.array([len(w) for w in weights], "<u4").tobytes(),
                    "weight_nodes": np.array([n for w in weights for n, _ in w], "<u4").tobytes(),
                    "weight_values": np.array([v for w in weights for _, v in w], "<f4").tobytes(),
                }
            )

    return {
        "nodes": nodes,
        "checksum": int(checksum),
        "soft_skins": soft_skins,
        "attachments": attachments,
    }


def armature_of(obj):
    """The armature an object belongs to: itself, its parent, or its Armature modifier's."""
    if obj is None:
        return None
    if obj.type == "ARMATURE":
        return obj
    if obj.parent is not None and obj.parent.type == "ARMATURE":
        return obj.parent
    for modifier in getattr(obj, "modifiers", []):
        if modifier.type == "ARMATURE" and modifier.object is not None:
            return modifier.object
    return None


class ImportModel(bpy.types.Operator):
    """Import a Gothic model (.mdl), skeleton (.mdh) or model mesh (.mdm)"""

    bl_idname = "import_scene.zenkit_model"
    bl_label = "Import Gothic Model"
    bl_options = {"REGISTER", "UNDO"}

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.mdl;*.mdh;*.mdm", options={"HIDDEN"})
    scale: bpy.props.FloatProperty(
        name="Scale", default=0.01, min=1e-6, description="Metres per engine unit"
    )

    def invoke(self, context, event):
        preferences.browse_from(self, "anims")
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        if context.object is not None and context.object.mode != "OBJECT":
            bpy.ops.object.mode_set(mode="OBJECT")
        path = Path(self.filepath)
        try:
            armature, data = import_model(context, path, self.scale)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        if armature is None and not data["nodes"]:
            self.report({"WARNING"}, f"No skeleton with checksum {data['checksum']} was found")
        else:
            self.report({"INFO"}, f"Imported {path.name}")
        return {"FINISHED"}


class ExportModel(bpy.types.Operator):
    """Export the active armature and its meshes as a Gothic model (.mdl), skeleton (.mdh) or
    model mesh (.mdm), by the file's extension"""

    bl_idname = "export_scene.zenkit_model"
    bl_label = "Export Gothic Model"

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.mdl;*.mdh;*.mdm", options={"HIDDEN"})
    scale: bpy.props.FloatProperty(
        name="Scale", default=0.01, min=1e-6, description="Metres per engine unit"
    )
    game: bpy.props.EnumProperty(
        name="Game",
        items=[("G1", "Gothic", ""), ("G2", "Gothic II", "")],
        default="G2",
    )

    @classmethod
    def poll(cls, context):
        return armature_of(context.active_object) is not None

    def invoke(self, context, event):
        self.game = preferences.game_version()
        name = armature_of(context.active_object).name.upper() + ".MDL"
        start = preferences.folder("anims")
        self.filepath = str(start / name) if start else name
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        armature = armature_of(context.active_object)
        data = gather_model(armature, context.evaluated_depsgraph_get(), self.scale)
        try:
            zenkit_py.write_model(self.filepath, data, self.game)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Exported {armature.name}")
        return {"FINISHED"}
