"""Gothic morph meshes (.MMB) to and from Blender meshes with shape keys.

A morph mesh is a multi-resolution mesh whose vertices animations move, like a face that talks or
a flag in the wind. Each frame of an animation moves some vertices away from where the mesh has
them, and becomes a shape key: named after the animation when it has one frame, followed by the
frame's number when it has more (`S_SHOOT 0`, `S_SHOOT 1`…).

What the engine needs besides the shapes is kept per animation as a custom property of the mesh,
to edit there: its layer, blend times, speed, duration and flags, and which vertices it moves,
even those it leaves where they are. Shape keys of an animation the property does not know are
exported with settings like the game's files have; see `DEFAULTS`.
"""

import re

import bpy
import numpy as np
import zenkit_py

from . import convert
from . import preferences

ANIMATIONS = "zenkit_morph_animations"
SOURCES = "zenkit_sources"

# Single-frame animations hold a shape, multi-frame ones play once at 25 frames per second, as most
# of the game's do. A negative duration plays on until stopped.
DEFAULTS = {"layer": 1, "blend_in": 0.01, "blend_out": -0.01, "speed": 0.025}
FRAME_NAME = re.compile(r"^(.*\S) (\d+)$")


def key_name(name, frame, frames):
    return name if frames == 1 else f"{name} {frame}"


def build_morph(name, data, scale, folders):
    """A new object, not yet in the scene, of a morph dict's mesh, with a shape key per
    animation frame."""
    mesh = convert.build_mesh(name, data["mesh"], scale, folders)
    obj = bpy.data.objects.new(name, mesh)

    base = convert.to_blender(convert.array(data, "morph_positions", "<f4", 3)) * scale
    obj.shape_key_add(name="Basis", from_mix=False)
    settings = {}
    for anim in data["animations"]:
        vertices = convert.array(anim, "vertices", "<u4").astype(np.int64)
        frames = anim["frame_count"]
        offsets = convert.array(anim, "samples", "<f4", 3).reshape(frames, len(vertices), 3)
        offsets = convert.to_blender(offsets) * scale
        for frame in range(frames):
            key = obj.shape_key_add(name=key_name(anim["name"], frame, frames), from_mix=False)
            # New shape keys start at 1, which would show every frame at once.
            key.value = 0.0
            co = base.copy()
            co[vertices] += offsets[frame]
            key.data.foreach_set("co", co.astype(np.float32).ravel())
        settings[anim["name"]] = {
            "layer": anim["layer"],
            "blend_in": anim["blend_in"],
            "blend_out": anim["blend_out"],
            "speed": anim["speed"],
            "duration": anim["duration"],
            "flags": anim["flags"],
            "vertices": vertices.tolist(),
        }
    mesh[ANIMATIONS] = settings
    mesh[SOURCES] = list(data["sources"])
    return obj


def animation_keys(obj):
    """The shape keys of each animation, in frame order, by animation name: first the animations
    the mesh knows, then others in the order of their keys."""
    keys = obj.data.shape_keys
    groups = {name: [] for name in obj.data.get(ANIMATIONS, {})}
    if keys is None:
        return {}
    for key in keys.key_blocks:
        if key == keys.reference_key:
            continue
        match = FRAME_NAME.match(key.name)
        name, frame = (match[1], int(match[2])) if match else (key.name, 0)
        if name not in groups and key.name in groups:
            name, frame = key.name, 0
        groups.setdefault(name, []).append((frame, key))
    return {
        name: [k for _, k in sorted(found, key=lambda f: f[0])]
        for name, found in groups.items()
        if found
    }


def gather_morph(obj, depsgraph, scale):
    """A morph dict of a mesh object and its shape keys."""
    keys = obj.data.shape_keys
    # The mesh is taken in its basis shape, whatever the keys' values are.
    shown = (obj.show_only_shape_key, obj.active_shape_key_index)
    obj.show_only_shape_key, obj.active_shape_key_index = True, 0
    try:
        mesh = convert.gather_mesh(obj, depsgraph, scale, True)
    finally:
        obj.show_only_shape_key, obj.active_shape_key_index = shown

    count = len(mesh["positions"]) // 12
    animations = []
    groups = animation_keys(obj)
    if groups and count != len(obj.data.vertices):
        raise ValueError("its modifiers change its vertices, which its shape keys are for")

    def shape(key):
        co = np.empty(count * 3, np.float32)
        key.data.foreach_get("co", co)
        return co.reshape(-1, 3).astype(np.float64)

    if groups:
        base = shape(keys.reference_key)
        rotation = np.array(obj.matrix_world.to_3x3())
    known = obj.data.get(ANIMATIONS, {})
    for name, frames in groups.items():
        offsets = np.array([(shape(k) - base) @ rotation.T for k in frames])
        offsets = convert.to_engine(offsets.reshape(-1, 3)).reshape(len(frames), count, 3) / scale
        settings = known.get(name, {})
        listed = [v for v in settings.get("vertices", []) if v < count]
        moved = np.nonzero(np.abs(offsets).max(axis=(0, 2)) > 1e-4)[0]
        vertices = np.union1d(np.array(listed, np.int64), moved).astype(np.int64)
        duration = settings.get("duration", -1000.0 if len(frames) == 1 else 40.0 * len(frames))
        flags = settings.get("flags", 2 if len(frames) == 1 else 0)
        animations.append(
            {
                **{key: settings.get(key, value) for key, value in DEFAULTS.items()},
                "name": name,
                "duration": float(duration),
                "flags": int(flags),
                "vertices": vertices.astype("<u4").tobytes(),
                "samples": offsets[:, vertices].astype("<f4").tobytes(),
            }
        )
    return {
        "name": obj.name.upper(),
        "mesh": mesh,
        "animations": animations,
        "sources": list(obj.data.get(SOURCES, [])),
    }


class ExportMmb(bpy.types.Operator):
    """Export the active mesh and its shape keys as a Gothic morph mesh, triangulated"""

    bl_idname = "export_scene.zenkit_mmb"
    bl_label = "Export Gothic Morph Mesh"

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.mmb", options={"HIDDEN"})
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
        return context.active_object is not None and context.active_object.type == "MESH"

    def invoke(self, context, event):
        self.game = preferences.game_version()
        name = context.active_object.name.upper() + ".MMB"
        start = preferences.folder("anims")
        self.filepath = str(start / name) if start else name
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        obj = context.active_object
        try:
            data = gather_morph(obj, context.evaluated_depsgraph_get(), self.scale)
            zenkit_py.write_morph(self.filepath, data, self.game)
        except (ValueError, zenkit_py.ZenKitError) as error:
            self.report({"ERROR"}, f"Cannot export {obj.name}: {error}")
            return {"CANCELLED"}
        self.report({"INFO"}, f"Exported {obj.name} with {len(data['animations'])} animations")
        return {"FINISHED"}
