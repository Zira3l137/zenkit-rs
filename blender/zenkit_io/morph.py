"""Gothic morph meshes (.MMB) to and from Blender meshes with shape keys.

A morph mesh is a multi-resolution mesh whose vertices animations move, like a face that talks or
a flag in the wind. Each frame of an animation moves some vertices away from where the mesh has
them, and becomes a shape key: named after the animation when it has one frame, followed by the
frame's number when it has more (`S_SHOOT 0`, `S_SHOOT 1`…). The keys start at 0, so the mesh
shows its rest shape; `preview` shows one frame.

What the engine needs besides the shapes is kept per animation in the mesh's settings
(`Mesh.zenkit_morph`): its layer, frame rate, how long it plays and blending. Which vertices it
moves is kept too, even those it leaves where they are, as the game's files list some. Shape keys
of an animation the settings do not know get settings of their own on export; see
`default_settings`.
"""

import re

import bpy
import numpy as np
import zenkit_py

from . import convert
from . import preferences

VERTICES = "zenkit_morph_vertices"
SOURCES = "zenkit_sources"
FRAME_NAME = re.compile(r"^(.*\S) (\d+)$")
# The engine's flag for playing the frames at the frame rate (OpenGothic's ProtoMesh::mkAnimation).
REPEAT = 0x2


def key_name(name, frame, frames):
    return name if frames == 1 else f"{name} {frame}"


def default_settings(settings, frames):
    """Settings like the game's files have: a single frame holds its shape until stopped, more
    play once, a second for every 25 frames."""
    settings.layer = 1
    settings.fps = 25.0
    settings.until_stopped = frames == 1
    settings.repeat = frames == 1
    settings.duration = frames / 25.0
    settings.blend_in, settings.blend_out = 0.01, -0.01


def build_morph(name, data, scale, folders):
    """A new object, not yet in the scene, of a morph dict's mesh, with a shape key per
    animation frame."""
    mesh = convert.build_mesh(name, data["mesh"], scale, folders)
    obj = bpy.data.objects.new(name, mesh)

    base = convert.to_blender(convert.array(data, "morph_positions", "<f4", 3)) * scale
    obj.shape_key_add(name="Basis", from_mix=False)
    vertices_of = {}
    for anim in data["animations"]:
        vertices = convert.array(anim, "vertices", "<u4").astype(np.int64)
        frames = anim["frame_count"]
        offsets = convert.array(anim, "samples", "<f4", 3).reshape(frames, len(vertices), 3)
        offsets = convert.to_blender(offsets) * scale
        for frame in range(frames):
            key = obj.shape_key_add(name=key_name(anim["name"], frame, frames), from_mix=False)
            key.value = 0.0
            co = base.copy()
            co[vertices] += offsets[frame]
            key.data.foreach_set("co", co.astype(np.float32).ravel())

        settings = mesh.zenkit_morph.animations.add()
        settings.name = anim["name"]
        settings.layer = anim["layer"]
        settings.fps = anim["speed"] * 1000.0
        settings.until_stopped = anim["duration"] <= 0
        settings.duration = anim["duration"] / 1000.0 if anim["duration"] > 0 else 1.0
        settings.repeat = bool(anim["flags"] & REPEAT)
        settings.other_flags = anim["flags"] & ~REPEAT
        settings.blend_in, settings.blend_out = anim["blend_in"], anim["blend_out"]
        vertices_of[anim["name"]] = vertices.tolist()
    mesh[VERTICES] = vertices_of
    mesh[SOURCES] = list(data["sources"])
    return obj


def animation_keys(obj):
    """The shape keys of each animation, in frame order, by animation name: first the animations
    the mesh's settings know, then others in the order of their keys."""
    keys = obj.data.shape_keys
    groups = {a.name: [] for a in obj.data.zenkit_morph.animations}
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
        name: [k for _, k in sorted(found, key=lambda f: f[0])] for name, found in groups.items()
    }


def active_animation(obj):
    """The settings of the animation selected in the mesh's list, or `None`."""
    morph = obj.data.zenkit_morph
    return morph.animations[morph.index] if 0 <= morph.index < len(morph.animations) else None


def preview(obj):
    """Shows the selected frame of the selected animation, and makes its shape key the active
    one, the one edit mode edits; every other key goes to 0."""
    keys = obj.data.shape_keys if obj is not None and obj.type == "MESH" else None
    selected = active_animation(obj) if keys is not None else None
    if selected is None:
        return
    frames = animation_keys(obj).get(selected.name, [])
    shown = frames[min(obj.data.zenkit_morph.frame, len(frames) - 1)] if frames else None
    for key in keys.key_blocks:
        if key != keys.reference_key:
            key.value = 1.0 if key == shown else 0.0
    if shown is not None:
        obj.active_shape_key_index = keys.key_blocks.find(shown.name)


def new_animation(obj, name):
    """Adds a single-frame animation, with a shape key in the mesh's rest shape, and selects it."""
    if obj.data.shape_keys is None:
        obj.shape_key_add(name="Basis", from_mix=False)
    obj.shape_key_add(name=name, from_mix=False).value = 0.0
    settings = obj.data.zenkit_morph.animations.add()
    settings.name = name
    default_settings(settings, 1)
    obj.data.zenkit_morph.index = len(obj.data.zenkit_morph.animations) - 1


def add_frame(obj):
    """Adds a frame to the selected animation, a copy of its last one, and shows it."""
    selected = active_animation(obj)
    frames = animation_keys(obj).get(selected.name, [])
    if len(frames) == 1 and frames[0].name == selected.name:
        frames[0].name = key_name(selected.name, 0, 2)
    key = obj.shape_key_add(name=key_name(selected.name, len(frames), 2), from_mix=False)
    key.value = 0.0
    if frames:
        co = np.empty(len(obj.data.vertices) * 3, np.float32)
        frames[-1].data.foreach_get("co", co)
        key.data.foreach_set("co", co)
    obj.data.zenkit_morph.frame = len(frames)


def remove_animation(obj):
    """Removes the selected animation and its shape keys."""
    morph = obj.data.zenkit_morph
    selected = active_animation(obj)
    for key in animation_keys(obj).get(selected.name, []):
        obj.shape_key_remove(key)
    morph.animations.remove(morph.index)
    morph.index = min(morph.index, len(morph.animations) - 1)


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
    groups = {name: frames for name, frames in animation_keys(obj).items() if frames}
    if groups and count != len(obj.data.vertices):
        raise ValueError("its modifiers change its vertices, which its shape keys are for")

    def shape(key):
        co = np.empty(count * 3, np.float32)
        key.data.foreach_get("co", co)
        return co.reshape(-1, 3).astype(np.float64)

    if groups:
        base = shape(keys.reference_key)
        rotation = np.array(obj.matrix_world.to_3x3())
    known = {a.name: a for a in obj.data.zenkit_morph.animations}
    listed_vertices = obj.data.get(VERTICES, {})
    for name, frames in groups.items():
        offsets = np.array([(shape(k) - base) @ rotation.T for k in frames])
        offsets = convert.to_engine(offsets.reshape(-1, 3)).reshape(len(frames), count, 3) / scale
        listed = [v for v in listed_vertices.get(name, []) if v < count]
        moved = np.nonzero(np.abs(offsets).max(axis=(0, 2)) > 1e-4)[0]
        vertices = np.union1d(np.array(listed, np.int64), moved).astype(np.int64)

        settings = known.get(name)
        if settings is None:
            settings = obj.data.zenkit_morph.animations.add()
            settings.name = name
            default_settings(settings, len(frames))
        animations.append(
            {
                "name": name,
                "layer": settings.layer,
                "blend_in": settings.blend_in,
                "blend_out": settings.blend_out,
                "speed": settings.fps / 1000.0,
                "duration": -1000.0 if settings.until_stopped else settings.duration * 1000.0,
                "flags": settings.other_flags | (REPEAT if settings.repeat else 0),
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
