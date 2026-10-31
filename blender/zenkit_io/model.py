"""Gothic models to and from Blender armatures and meshes.

A skeleton's nodes come in three kinds, told apart by name as the engine's tools do:

- bones, most nodes (BIP01 SPINE), which make up the armature;
- slots (ZS_), the points items are held at, which become small placeholder meshes;
- mesh nodes (ZM_), which become the object of their rigid mesh, or a placeholder without one.

Slots and mesh nodes hang from their bone. A node named like one stays a bone if bones hang from
it, as a bone cannot hang from an object.

A model's meshes come in two kinds. Soft skins, the bodies that bend with the bones, become
meshes with vertex groups and an Armature modifier. Attachments are rigid meshes at a node: the
object of a mesh node, or a mesh parented to a bone.

The checksum that ties a skeleton to its meshes and animations, the root translation and each
node's place in the file are kept, so that what is exported still fits the game's animations.
"""

import math
import zlib
from dataclasses import dataclass
from dataclasses import field
from pathlib import Path

import bpy
import numpy as np
import zenkit_py
from mathutils import Matrix
from mathutils import Quaternion
from mathutils import Vector

from . import convert
from . import preferences
from .mesh import link

CHECKSUM = "zenkit_checksum"
ROOT_TRANSLATION = "zenkit_root_translation"
TURN = "zenkit_turn"
INDEX = "zenkit_index"
NODE = "zenkit_node"
ATTACHMENT = "zenkit_attachment"
PLACEHOLDER = "zenkit_placeholder"

# What the engine's tools name skinned meshes and meshes hanging from a bone, followed by a number.
SKIN = "ZM_SKIN_"
FRILL = "ZM_FRILL_"
MESH_NAMES = (SKIN.rstrip("_"), FRILL.rstrip("_"))

# Blender bones point along their Y axis, while a node may point along any of its axes (the
# bones of 3ds Max's Biped, which Gothic's skeletons come from, along X). A bone starts as its
# node turned so that Y is the node's axis closest to its child, one of these.
TURNS = {
    "Y": Matrix.Identity(4),
    "-Y": Matrix.Rotation(math.pi, 4, "Z"),
    "X": Matrix.Rotation(-math.pi / 2, 4, "Z"),
    "-X": Matrix.Rotation(math.pi / 2, 4, "Z"),
    "Z": Matrix.Rotation(math.pi / 2, 4, "X"),
    "-Z": Matrix.Rotation(-math.pi / 2, 4, "X"),
}

PLACEHOLDER_FACES = [
    (0, 1, 2),
    (0, 2, 3),
    (0, 3, 4),
    (0, 4, 1),
    (5, 2, 1),
    (5, 3, 2),
    (5, 4, 3),
    (5, 1, 4),
]


def node_kinds(nodes):
    """ "bone", "slot" or "mesh" for each node."""
    kinds = [""] * len(nodes)
    holds_bone = [False] * len(nodes)
    for i in reversed(range(len(nodes))):
        name, parent, _ = nodes[i]
        upper = name.upper()
        if upper.startswith("ZS_") and not holds_bone[i]:
            kinds[i] = "slot"
        elif upper.startswith("ZM_") and not holds_bone[i]:
            kinds[i] = "mesh"
        else:
            kinds[i] = "bone"
            if parent >= 0:
                holds_bone[parent] = True
    return kinds


def rest_world(nodes, scale):
    """Every node's transform in the model, in Blender's space."""
    world = []
    for _, parent, rows in nodes:
        local = convert.matrix_to_blender(rows, scale)
        world.append(world[parent] @ local if parent >= 0 else local)
    return world


def node_frame(bone):
    """A bone's node in the armature's space, its turn undone."""
    turn = bone.get(TURN)
    if turn is None:
        return bone.matrix_local.copy()
    return bone.matrix_local @ Quaternion(turn).to_matrix().to_4x4().inverted()


def pick_child(frame, offsets):
    """Of the offsets from a node to its children, the one most along an axis of the node, as
    a bone runs along a node's axis to its main child. `None` if no child stands apart."""
    axes = [frame.to_3x3() @ t.to_3x3() @ Vector((0, 1, 0)) for t in TURNS.values()]
    apart = [o for o in offsets if o.length > 1e-6]
    if not apart:
        return None
    return max(apart, key=lambda o: max(a.dot(o) for a in axes) / o.length)


def bone_turn(frame, direction):
    """The rotation from a node's frame to a bone's frame pointing along `direction`: the node
    turned to the axis closest to it, then tilted onto it as little as possible."""
    rotation = frame.to_3x3()
    y = Vector((0, 1, 0))
    closest = max(TURNS.values(), key=lambda t: (rotation @ t.to_3x3() @ y).dot(direction))
    start = rotation @ closest.to_3x3()
    along = start @ y
    bone = along.rotation_difference(direction).to_matrix() @ start
    return (rotation.inverted() @ bone).to_quaternion()


def placeholder(scale):
    """The small diamond that stands for slots, and for mesh nodes without a mesh."""
    a = 4 * scale
    mesh = bpy.data.meshes.new("ZenKit Node")
    vertices = [(0, 0, 1.5 * a), (a, 0, 0), (0, a, 0), (-a, 0, 0), (0, -a, 0), (0, 0, -a)]
    mesh.from_pydata(vertices, [], PLACEHOLDER_FACES)
    mesh[PLACEHOLDER] = True
    return mesh


@dataclass
class Rig:
    """What meshes are bound to: the armature, if there is one, the names of the skeleton's
    nodes in its order, their transforms in the armature's space, and the objects of its slots
    and mesh nodes by name."""

    armature: bpy.types.Object | None = None
    names: list = field(default_factory=list)
    world: list = field(default_factory=list)
    objects: dict = field(default_factory=dict)


def build_skeleton(context, name, data, scale):
    """A Rig of a new armature for the model's nodes, with an object for each slot and mesh
    node, linked to the scene and left in object mode."""
    nodes = data["nodes"]
    kinds = node_kinds(nodes)
    world = rest_world(nodes, scale)

    shapes = {}
    for i, (_, parent, _) in enumerate(nodes):
        if kinds[i] != "bone":
            continue
        head = world[i].to_translation()
        children = [
            world[j].to_translation() - head
            for j, (_, p, _) in enumerate(nodes)
            if p == i and kinds[j] == "bone"
        ]
        child = pick_child(world[i], children)
        if child is not None:
            direction, length = child.normalized(), child.length
        elif parent >= 0:
            # A bone with nowhere to point to carries on from its parent, at half its length.
            direction, length = shapes[parent][0], shapes[parent][1] / 2
        else:
            direction, length = world[i].to_3x3() @ Vector((0, 1, 0)), 0.0
        shapes[i] = (direction, max(length, 5 * scale), bone_turn(world[i], direction))

    armature = bpy.data.armatures.new(name)
    armature[CHECKSUM] = str(data["checksum"])
    armature[ROOT_TRANSLATION] = list(data["root_translation"])
    obj = bpy.data.objects.new(name, armature)
    obj.show_in_front = True
    link(context, obj)

    bpy.ops.object.mode_set(mode="EDIT")
    edit_bones = {}
    for i, (_, length, turn) in shapes.items():
        node_name, parent, _ = nodes[i]
        bone = armature.edit_bones.new(node_name)
        bone.head = (0.0, 0.0, 0.0)
        bone.tail = (0.0, length, 0.0)
        bone.matrix = world[i] @ turn.to_matrix().to_4x4()
        if parent >= 0:
            bone.parent = edit_bones[parent]
        edit_bones[i] = bone
    # Blender renames bones that share a name, so they are found again by the name they got.
    bone_names = {i: bone.name for i, bone in edit_bones.items()}
    bpy.ops.object.mode_set(mode="OBJECT")
    for i, (_, _, turn) in shapes.items():
        bone = armature.bones[bone_names[i]]
        bone[TURN] = list(turn)
        bone[INDEX] = i

    rig = Rig(obj, [n[0] for n in nodes], world)
    marker = None
    for i, (node_name, parent, _) in enumerate(nodes):
        if kinds[i] == "bone":
            continue
        marker = marker or placeholder(scale)
        node_obj = bpy.data.objects.new(node_name, marker)
        context.collection.objects.link(node_obj)
        node_obj[NODE] = node_name
        node_obj[INDEX] = i
        node_obj.parent = obj
        if parent >= 0 and kinds[parent] == "bone":
            node_obj.parent_type = "BONE"
            node_obj.parent_bone = bone_names[parent]
        elif parent >= 0:
            node_obj.parent = rig.objects[nodes[parent][0]]
        node_obj.matrix_world = obj.matrix_world @ world[i]
        rig.objects[node_name] = node_obj
    return rig


def is_node_object(obj):
    """Whether an object hanging from an armature is a slot or mesh node rather than a mesh.

    Imported ones are marked. Others go by name, as in the engine's own tools: `ZS_` is a slot,
    `ZM_` a mesh node, except for `ZM_SKIN` and `ZM_FRILL`, the names of skinned meshes and of
    meshes hanging from a bone.
    """
    if NODE in obj:
        return True
    if ATTACHMENT in obj:
        return False
    name = obj.name.upper()
    if name.startswith(MESH_NAMES):
        return False
    skinned = any(m.type == "ARMATURE" for m in getattr(obj, "modifiers", []))
    return name.startswith(("ZS_", "ZM_")) and not skinned


def free_name(prefix):
    """The first of `prefix` followed by 0, 1, 2… that no object has yet."""
    index = 0
    while f"{prefix}{index}" in bpy.data.objects:
        index += 1
    return f"{prefix}{index}"


def node_objects(armature):
    """The slots and mesh nodes hanging from an armature, and from each other."""
    found = []

    def visit(parent):
        for child in parent.children:
            if is_node_object(child):
                found.append(child)
                visit(child)

    visit(armature)
    return found


def scene_nodes(armature):
    """An armature's nodes as (name, parent index or -1, frame in the armature's space, bone or
    object), in the order the file had them, new ones after, every parent before its children.
    """
    entries = []
    for bone in armature.data.bones:
        parent = bone.parent.name if bone.parent else None
        entries.append((bone.get(INDEX), bone.name, parent, node_frame(bone), bone))
    to_armature = armature.matrix_world.inverted()
    for obj in node_objects(armature):
        if obj.parent == armature and obj.parent_type == "BONE":
            parent = obj.parent_bone
        elif obj.parent != armature:
            parent = obj.parent.get(NODE, obj.parent.name)
        else:
            parent = None
        frame = to_armature @ obj.matrix_world
        entries.append((obj.get(INDEX), obj.get(NODE, obj.name), parent, frame, obj))

    entries.sort(key=lambda e: (e[0] is None, e[0] or 0))
    by_name = {e[1]: e for e in entries}
    order = {}
    nodes = []

    def emit(entry):
        _, name, parent, frame, item = entry
        if name in order:
            return
        if parent in by_name:
            emit(by_name[parent])
        order[name] = len(nodes)
        nodes.append((name, order.get(parent, -1), frame, item))

    for entry in entries:
        emit(entry)
    return nodes


def rig_of_armature(armature):
    nodes = scene_nodes(armature)
    objects = {name: item for name, _, _, item in nodes if isinstance(item, bpy.types.Object)}
    return Rig(armature, [n[0] for n in nodes], [n[2] for n in nodes], objects)


def find_skeleton(checksum, folder):
    """The model dict of the skeleton with this checksum, looked for next to the meshes, in the
    models folder and in the game's archives, or `None`."""
    folders = [folder, preferences.folder("anims")]
    for path in (p for f in folders if f for p in sorted(f.iterdir())):
        if path.suffix.lower() in (".mdh", ".mdl"):
            try:
                data = zenkit_py.read_model(str(path))
            except zenkit_py.ZenKitError:
                continue
            if data["checksum"] == checksum and data["nodes"]:
                return data

    archives = preferences.archives()
    for name in archives.names(".MDH") if archives is not None else []:
        data = archives.read_model(name)
        if data is not None and data["checksum"] == checksum:
            return data
    return None


def meshes_rig(context, data, folder, scale):
    """The Rig for meshes that come without their skeleton: the active armature if it is their
    skeleton, or else just the names and places of the skeleton's nodes, if it can be found."""
    active = context.active_object
    if active is not None and active.type == "ARMATURE":
        if active.data.get(CHECKSUM) == str(data["checksum"]):
            return rig_of_armature(active)
    skeleton = find_skeleton(data["checksum"], folder)
    if skeleton is None:
        return Rig()
    return Rig(names=[n[0] for n in skeleton["nodes"]], world=rest_world(skeleton["nodes"], scale))


def import_model(context, path, scale):
    """Imports a model file. Gives the model dict and the Rig its meshes were bound to."""
    data = zenkit_py.read_model(str(path))
    if data["nodes"]:
        rig = build_skeleton(context, path.stem, data, scale)
    else:
        rig = meshes_rig(context, data, path.parent, scale)
    add_meshes(context, path, data, rig, scale)

    if rig.armature is not None:
        for other in context.selected_objects:
            other.select_set(False)
        rig.armature.select_set(True)
        context.view_layer.objects.active = rig.armature
    return data, rig


def add_meshes(context, path, data, rig, scale):
    folders = [path.parent]
    armature = rig.armature
    to_world = armature.matrix_world if armature is not None else Matrix.Identity(4)

    for i, skin in enumerate(data["soft_skins"]):
        positions = skin_positions(skin, rig.world, scale)
        name = free_name(SKIN)
        mesh = convert.build_mesh(name, skin["mesh"], scale, folders, positions)
        obj = bpy.data.objects.new(name, mesh)
        link(context, obj)
        add_weights(obj, skin, rig.names)
        if armature is not None:
            obj.parent = armature
            obj.modifiers.new("Armature", "ARMATURE").object = armature

    for attachment in data["attachments"]:
        node = attachment["node"]
        target = rig.objects.get(node)
        if target is not None and target.type == "MESH" and target.data.get(PLACEHOLDER):
            target.data = convert.build_mesh(node, attachment["mesh"], scale, folders)
            continue

        # A mesh node's mesh is named after it; a mesh on a bone is a frill.
        bone = armature.data.bones.get(node) if armature is not None else None
        is_frill = bone is not None or not node.upper().startswith("ZM_")
        name = free_name(FRILL) if is_frill else node
        obj = bpy.data.objects.new(
            name, convert.build_mesh(name, attachment["mesh"], scale, folders)
        )
        link(context, obj)
        if is_frill:
            obj[ATTACHMENT] = node
        if bone is not None:
            obj.parent = armature
            obj.parent_type = "BONE"
            obj.parent_bone = bone.name
            obj.matrix_world = to_world @ node_frame(bone)
        elif node in rig.names:
            obj.matrix_world = to_world @ rig.world[rig.names.index(node)]


def skin_positions(skin, world, scale):
    """Where the engine draws a soft skin's vertices on the skeleton in `world`, in Blender's
    space, or `None` to keep the positions the mesh stores.

    The engine ignores the positions a mesh stores: it places each vertex by the bones that move
    it, from where the vertex lies relative to each, so a mesh made for a slightly different
    skeleton fits the one it is drawn with. Vertices without weights, and meshes whose weights
    name nodes the skeleton lacks, keep their stored positions.
    """
    counts = convert.array(skin, "weight_counts", "<u4")
    nodes = convert.array(skin, "weight_nodes", "<u4")
    if len(nodes) == 0 or nodes.max() >= len(world):
        return None
    values = convert.array(skin, "weight_values", "<f4").astype(np.float64)
    local = convert.to_blender(convert.array(skin, "weight_positions", "<f4", 3)) * scale

    matrices = np.array([np.array(m) for m in world])[nodes]
    placed = np.einsum("wij,wj->wi", matrices[:, :3, :3], local) + matrices[:, :3, 3]
    vertex = np.repeat(np.arange(len(counts)), counts)
    totals = np.bincount(vertex, values, len(counts))
    sums = np.zeros((len(counts), 3))
    np.add.at(sums, vertex, placed * values[:, None])

    positions = convert.to_blender(convert.array(skin["mesh"], "positions", "<f4", 3)) * scale
    weighted = totals > 0
    positions[weighted] = sums[weighted] / totals[weighted, None]
    return positions


def add_weights(obj, skin, names):
    """Vertex groups from a soft skin's weights, named after the nodes, or after the node
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
    """A model dict of an armature, the slots and mesh nodes hanging from it and its meshes."""
    nodes = scene_nodes(armature)
    index = {name: i for i, (name, _, _, _) in enumerate(nodes)}
    engine_nodes = []
    for name, parent, frame, _ in nodes:
        local = nodes[parent][2].inverted() @ frame if parent >= 0 else frame
        engine_nodes.append((name, parent, convert.matrix_to_engine(local, scale)))

    checksum = armature.data.get(CHECKSUM)
    if checksum is None:
        joined = "\n".join(n[0] for n in nodes)
        checksum = zlib.crc32(joined.encode("latin-1", "replace"))
    root = armature.data.get(ROOT_TRANSLATION)

    attachments = []
    for name, _, _, item in nodes:
        is_mesh = isinstance(item, bpy.types.Object) and item.type == "MESH"
        if is_mesh and not item.data.get(PLACEHOLDER):
            mesh = convert.gather_mesh(item, depsgraph, scale, True, item.matrix_world)
            attachments.append({"node": name, "mesh": mesh})

    soft_skins = []
    for obj in armature.children:
        if obj.type != "MESH" or is_node_object(obj):
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
        "nodes": engine_nodes,
        "root_translation": tuple(root) if root is not None else None,
        "checksum": int(checksum),
        "soft_skins": soft_skins,
        "attachments": attachments,
    }


def armature_of(obj):
    """The armature an object belongs to: itself, the one it hangs from, or its Armature
    modifier's."""
    while obj is not None:
        if obj.type == "ARMATURE":
            return obj
        for modifier in getattr(obj, "modifiers", []):
            if modifier.type == "ARMATURE" and modifier.object is not None:
                return modifier.object
        obj = obj.parent
    return None


class ImportModel(bpy.types.Operator):
    """Import a Gothic model (.mdl), skeleton (.mdh) or model mesh (.mdm). A model mesh goes
    onto the active armature if that is its skeleton"""

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
            data, rig = import_model(context, path, self.scale)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        if rig.armature is None and not rig.names:
            self.report({"WARNING"}, f"No skeleton with checksum {data['checksum']} was found")
        elif rig.armature is None:
            self.report(
                {"INFO"},
                f"Imported {path.name} on its own; make its skeleton's armature active to bind it",
            )
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
