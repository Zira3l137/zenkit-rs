"""Gothic animations (.MAN) to and from Blender actions on armatures.

An animation gives, frame by frame, the transform of some of a skeleton's nodes relative to their
parent, naming each node by its index in the skeleton. The armature it goes onto must have been
imported from that skeleton, so that its bones and node objects know their index; the checksum
the animation was made for tells whether it is the right one.

Each frame of the file becomes one frame of the action, from frame 0 on. Bones get location and
rotation keys; slots and mesh nodes, which hang from bones as objects, are keyed only when the
animation moves them relative to their bone, which needs the slotted actions of Blender 4.4 or
later, as one action then animates several objects. The engine plays the frames at the
animation's own frame rate, kept with the action, as are its layer and the animation that follows
it, as custom properties to edit.

Exporting goes the other way: the action is played frame by frame, and every node's transform
relative to its parent is stored, whatever drives it (keys, constraints or drivers).
"""

import json
from pathlib import Path

import bpy
import numpy as np
import zenkit_py
from mathutils import Matrix
from mathutils import Quaternion

from . import convert
from . import preferences
from .model import TURN
from .model import armature_checksum
from .model import armature_of
from .model import node_frame
from .model import scene_nodes

NAME = "zenkit_name"
LAYER = "zenkit_layer"
NEXT = "zenkit_next"
FPS = "zenkit_fps"
EVENTS = "zenkit_events"
CHECKSUM = "zenkit_checksum"
# On armatures: the model a script imported them for, which their animations' files start with.
MODEL = "zenkit_model"

LINEAR = bpy.types.Keyframe.bl_rna.properties["interpolation"].enum_items["LINEAR"].value


def is_slotted(action):
    return hasattr(action, "slots")


def channels(action, target):
    """Where `action` keeps the F-curves animating `target`, the action itself before Blender
    4.4, a channelbag of the target's slot from then on. Assigns the action to the target."""
    assign(action, target)
    if not is_slotted(action):
        return action
    layer = action.layers[0] if action.layers else action.layers.new("Layer")
    strip = layer.strips[0] if layer.strips else layer.strips.new(type="KEYFRAME")
    return strip.channelbag(target.animation_data.action_slot, ensure=True)


def slot_for(action, target):
    """The slot of `action` that animates `target`: the one named after it, or for an armature
    the first one for objects. `None` if there is none."""
    slots = [s for s in action.slots if s.target_id_type == target.id_type]
    named = next((s for s in slots if s.name_display == target.name), None)
    if named is not None or target.type != "ARMATURE":
        return named
    return slots[0] if slots else None


def assign(action, target, make_slot=True):
    """Plays `action` on `target`, with a slot for it made if `make_slot` and it has none.
    Gives whether the action animates the target."""
    anim = target.animation_data or target.animation_data_create()
    anim.action = action
    if not is_slotted(action):
        return True
    if anim.action_slot is None:
        slot = slot_for(action, target)
        if slot is None and make_slot:
            slot = action.slots.new(target.id_type, target.name)
        if slot is not None:
            anim.action_slot = slot
    return anim.action_slot is not None


def play(action, armature):
    """Plays `action` on an armature and on the node objects it has keys for; node objects
    playing another of the extension's animations stop."""
    assign(action, armature)
    for _, _, _, item in scene_nodes(armature):
        if not isinstance(item, bpy.types.Object):
            continue
        current = item.animation_data.action if item.animation_data else None
        if is_slotted(action) and slot_for(action, item) is not None:
            assign(action, item, make_slot=False)
        elif current is not None and NAME in current:
            item.animation_data.action = None


def turn(bone):
    """The rotation from a bone's node to the bone, as a matrix."""
    value = bone.get(TURN)
    return Quaternion(value).to_matrix().to_4x4() if value is not None else Matrix.Identity(4)


def numpy(matrix):
    return np.array(matrix, dtype=np.float64)


def add_keys(bag, path, group, values, frames):
    """F-curves along `path`, one per column of `values`, keyed at `frames` with linear
    interpolation, as the engine blends between frames."""
    group = bag.groups.get(group) or bag.groups.new(group)
    for index in range(values.shape[1]):
        curve = bag.fcurves.new(path, index=index)
        curve.group = group
        curve.keyframe_points.add(len(frames))
        co = np.column_stack((frames, values[:, index])).astype(np.float32)
        curve.keyframe_points.foreach_set("co", co.ravel())
        curve.keyframe_points.foreach_set("interpolation", np.full(len(frames), LINEAR, np.int32))
        curve.update()


def quaternions(r):
    """The (w, x, y, z) quaternions of rotation matrices, an (n, 3, 3) array."""
    d0, d1, d2 = r[:, 0, 0], r[:, 1, 1], r[:, 2, 2]
    w = np.sqrt(np.maximum(0.0, 1.0 + d0 + d1 + d2)) / 2
    x = np.sqrt(np.maximum(0.0, 1.0 + d0 - d1 - d2)) / 2
    y = np.sqrt(np.maximum(0.0, 1.0 - d0 + d1 - d2)) / 2
    z = np.sqrt(np.maximum(0.0, 1.0 - d0 - d1 + d2)) / 2
    x = np.copysign(x, r[:, 2, 1] - r[:, 1, 2])
    y = np.copysign(y, r[:, 0, 2] - r[:, 2, 0])
    z = np.copysign(z, r[:, 1, 0] - r[:, 0, 1])
    q = np.column_stack((w, x, y, z))
    return q / np.linalg.norm(q, axis=1, keepdims=True)


def location_rotation(bases):
    """Locations and quaternions of 4x4 matrices, each quaternion turned to the same side as the
    one before it, so that blending between keys takes the short way."""
    locations = bases[:, :3, 3]
    rotations = quaternions(bases[:, :3, :3])
    for f in range(1, len(rotations)):
        if np.dot(rotations[f], rotations[f - 1]) < 0:
            rotations[f] = -rotations[f]
    return locations, rotations


def bone_space(bone):
    """`(before, after)` turning a node's transform relative to its parent into the bone's pose
    relative to its rest: `before @ local @ after`."""
    own = node_frame(bone)
    rest = node_frame(bone.parent).inverted() @ own if bone.parent else own
    t = turn(bone)
    return numpy(t.inverted() @ rest.inverted()), numpy(t)


def object_space(armature, obj):
    """`(before, after)` turning a node's transform relative to its parent into a node object's
    own transform, relative to what it hangs from: the tail of a bone, or another object."""
    offset = Matrix.Identity(4)
    if obj.parent == armature and obj.parent_type == "BONE":
        bone = armature.data.bones[obj.parent_bone]
        offset = turn(bone) @ Matrix.Translation((0, bone.length, 0))
    return numpy(obj.matrix_parent_inverse.inverted() @ offset.inverted()), np.eye(4)


def build_action(armature, data, scale, name=None):
    """A new action of an animation dict on an armature. Gives it, and the names of the nodes the
    animation moves that could not be keyed: node objects, before Blender 4.4."""
    nodes = scene_nodes(armature)
    indices = data["node_indices"]
    frames = data["frame_count"]
    transforms = np.frombuffer(data["transforms"], "<f4").reshape(frames, len(indices), 4, 4)
    local = convert.AXES4 @ transforms.astype(np.float64) @ convert.AXES4.T
    local[..., :3, 3] *= scale

    action = bpy.data.actions.new(name or data["name"])
    action[NAME] = data["name"]
    action[LAYER] = data["layer"]
    action[NEXT] = data["next"]
    action[FPS] = data["fps"]
    # As text: Blender's integer properties are signed 32-bit, too small for a checksum.
    action[CHECKSUM] = str(data["checksum"])
    if data["events"]:
        action[EVENTS] = json.dumps(data["events"])
    action.use_fake_user = True
    if frames:
        action.use_frame_range = True
        action.frame_start, action.frame_end = 0, max(frames - 1, 0)

    keyed_frames = np.arange(frames, dtype=np.float64)
    unkeyed = []
    for column, index in enumerate(indices):
        if index >= len(nodes):
            continue
        _, _, _, item = nodes[index]
        if isinstance(item, bpy.types.Bone):
            before, after = bone_space(item)
            locations, rotations = location_rotation(before @ local[:, column] @ after)
            bag = channels(action, armature)
            pose_bone = armature.pose.bones[item.name]
            pose_bone.rotation_mode = "QUATERNION"
            path = f'pose.bones["{bpy.utils.escape_identifier(item.name)}"]'
            add_keys(bag, path + ".location", item.name, locations, keyed_frames)
            add_keys(bag, path + ".rotation_quaternion", item.name, rotations, keyed_frames)
            continue

        before, after = object_space(armature, item)
        bases = before @ local[:, column] @ after
        if np.allclose(bases, numpy(item.matrix_basis), atol=1e-5):
            continue
        if not is_slotted(action):
            unkeyed.append(item.name)
            continue
        locations, rotations = location_rotation(bases)
        item.rotation_mode = "QUATERNION"
        bag = channels(action, item)
        add_keys(bag, "location", item.name, locations, keyed_frames)
        add_keys(bag, "rotation_quaternion", item.name, rotations, keyed_frames)
    return action, unkeyed


def frame_range(action):
    start, end = action.frame_range
    return round(start), max(round(start), round(end))


def gather_animation(context, armature, action, scale):
    """An animation dict of `action` played on `armature`, for every node of its skeleton."""
    nodes = scene_nodes(armature)
    scene = context.scene
    current = scene.frame_current
    before = {}
    for _, _, _, item in nodes:
        if isinstance(item, bpy.types.Object) and item.animation_data:
            before[item.name] = item.animation_data.action
    previous = armature.animation_data.action if armature.animation_data else None
    play(action, armature)

    turns = [turn(item) if isinstance(item, bpy.types.Bone) else None for *_, item in nodes]
    to_armature = armature.matrix_world.inverted()
    rows = []
    positions = []
    start, end = frame_range(action)
    try:
        for frame in range(start, end + 1):
            scene.frame_set(frame)
            frames = []
            for (_, _, _, item), t in zip(nodes, turns, strict=True):
                if t is not None:
                    frames.append(armature.pose.bones[item.name].matrix @ t.inverted())
                else:
                    frames.append(to_armature @ item.matrix_world)
            for i, (_, parent, _, _) in enumerate(nodes):
                own = frames[parent].inverted() @ frames[i] if parent >= 0 else frames[i]
                rows.append(convert.matrix_to_engine(own, scale))
                positions.append(frames[i].to_translation()[:])
    finally:
        if previous is not None:
            play(previous, armature)
        for _, _, _, item in nodes:
            if isinstance(item, bpy.types.Object) and item.name in before:
                item.animation_data.action = before[item.name]
        scene.frame_set(current)

    engine = convert.to_engine(np.array(positions)) / scale
    fps = action.get(FPS) or scene.render.fps / scene.render.fps_base
    events = json.loads(action[EVENTS]) if EVENTS in action else []
    return {
        "name": action.get(NAME, action.name),
        "next": action.get(NEXT, ""),
        "layer": action.get(LAYER, 1),
        "fps": float(fps),
        "checksum": armature_checksum(armature),
        "node_indices": list(range(len(nodes))),
        "transforms": b"".join(rows),
        "bbox": (tuple(engine.min(axis=0)), tuple(engine.max(axis=0))),
        "events": events,
    }


def model_name(armature):
    """What the files of an armature's animations start with: the model a script named, or the
    armature's name."""
    return armature.data.get(MODEL, armature.name).upper()


def show(context, action, armature, match_fps):
    """Plays an imported action, with the scene's frames set to it."""
    play(action, armature)
    start, end = frame_range(action)
    context.scene.frame_start, context.scene.frame_end = start, end
    context.scene.frame_set(start)
    fps = action.get(FPS)
    if match_fps and fps and float(fps).is_integer():
        context.scene.render.fps, context.scene.render.fps_base = int(fps), 1.0


class ImportAnimation(bpy.types.Operator):
    """Import Gothic animations (.man) onto the active armature, which must have been imported
    from the skeleton they were made for"""

    bl_idname = "import_anim.zenkit_animation"
    bl_label = "Import Gothic Animation"
    bl_options = {"REGISTER", "UNDO"}

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    directory: bpy.props.StringProperty(subtype="DIR_PATH", options={"HIDDEN", "SKIP_SAVE"})
    files: bpy.props.CollectionProperty(type=bpy.types.OperatorFileListElement)
    filter_glob: bpy.props.StringProperty(default="*.man", options={"HIDDEN"})
    scale: bpy.props.FloatProperty(
        name="Scale", default=0.01, min=1e-6, description="Metres per engine unit"
    )
    match_fps: bpy.props.BoolProperty(
        name="Match Frame Rate",
        description="Set the scene's frame rate to the animation's, so it plays as in the game",
        default=True,
    )

    @classmethod
    def poll(cls, context):
        return armature_of(context.active_object) is not None

    def invoke(self, context, event):
        preferences.browse_from(self, "anims")
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def paths(self):
        if self.files and self.files[0].name:
            return [Path(self.directory) / f.name for f in self.files]
        return [Path(self.filepath)]

    def execute(self, context):
        if context.object is not None and context.object.mode == "EDIT":
            bpy.ops.object.mode_set(mode="OBJECT")
        armature = armature_of(context.active_object)
        checksum = armature_checksum(armature)
        last = None
        foreign, unkeyed = [], set()
        for path in self.paths():
            try:
                data = zenkit_py.read_animation(str(path))
            except zenkit_py.ZenKitError as error:
                self.report({"ERROR"}, str(error))
                return {"CANCELLED"}
            if data["checksum"] != checksum:
                foreign.append(path.name)
            last, missed = build_action(armature, data, self.scale)
            unkeyed.update(missed)

        show(context, last, armature, self.match_fps)
        if foreign:
            self.report(
                {"WARNING"},
                f"{', '.join(foreign)} made for another skeleton than {armature.name}'s; "
                "the bones may not be the ones meant",
            )
        if unkeyed:
            self.report(
                {"WARNING"},
                f"Moving {', '.join(sorted(unkeyed))} needs Blender 4.4 or later; they stay put",
            )
        if not foreign and not unkeyed:
            self.report({"INFO"}, f"Imported {last.name}")
        return {"FINISHED"}


class ExportAnimation(bpy.types.Operator):
    """Export the active armature's action as a Gothic animation (.man)"""

    bl_idname = "export_anim.zenkit_animation"
    bl_label = "Export Gothic Animation"

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.man", options={"HIDDEN"})
    scale: bpy.props.FloatProperty(
        name="Scale", default=0.01, min=1e-6, description="Metres per engine unit"
    )

    @classmethod
    def poll(cls, context):
        armature = armature_of(context.active_object)
        anim = armature.animation_data if armature is not None else None
        return anim is not None and anim.action is not None

    def invoke(self, context, event):
        armature = armature_of(context.active_object)
        action = armature.animation_data.action
        name = f"{model_name(armature)}-{action.get(NAME, action.name).upper()}.MAN"
        start = preferences.folder("anims")
        self.filepath = str(start / name) if start else name
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        armature = armature_of(context.active_object)
        action = armature.animation_data.action
        data = gather_animation(context, armature, action, self.scale)
        try:
            zenkit_py.write_animation(self.filepath, data)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Exported {action.name} as {Path(self.filepath).name}")
        return {"FINISHED"}
