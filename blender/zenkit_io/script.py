"""Gothic model scripts (.MDS source, .MSB compiled) to and from Blender.

A model script names a model's skeleton, the meshes it can wear and its animations, each of which
the engine loads from a file named after the model and the animation: HUMANS-S_RUN.MAN.
Importing one brings in all of that, found next to the script, in the models folder or in the
game's archives: an armature of the skeleton, the meshes on it and an action per animation.

Everything goes into a collection named after the model, the meshes into one inside it per file
they came from. Each animation's settings in the script (layer, next animation, blending, flags,
frame rate) become its action's, and the script itself a text in Blender's Text Editor, as source,
to edit there; export.py writes the actions' settings back into it.
"""

import fnmatch
from pathlib import Path

import bpy
import zenkit_py

from . import animation
from . import preferences
from . import properties
from .export import skeleton_names
from .export import stem
from .model import add_meshes
from .model import build_skeleton
from .model import file_collection


def folders_for(path):
    """Where the files a script names are looked for first: next to it, then in the models
    folder."""
    anims = preferences.folder("anims")
    return [f for f in dict.fromkeys((path.parent, anims)) if f is not None]


def load(name, folders, read, read_archived):
    """The file `name` read by `read` from the first of `folders` that has it, matching case as
    the game does, or by `read_archived` from the game's archives. `None` if none has it."""
    for folder in folders:
        path = preferences.resolve(folder, (name,))
        if path is not None and path.is_file():
            return read(str(path))
    archives = preferences.archives()
    return read_archived(archives, name) if archives is not None else None


def load_model(name, folders):
    return load(name, folders, zenkit_py.read_model, lambda a, n: a.read_model(n))


def load_animation(name, folders):
    return load(name, folders, zenkit_py.read_animation, lambda a, n: a.read_animation(n))


def import_script(context, path, scale, meshes="FIRST", pattern="*"):
    """Imports a model script with its skeleton, meshes and the animations whose names match
    `pattern`. Gives the armature, the actions and the names of the animations not found."""
    info = zenkit_py.read_script(str(path))
    folders = folders_for(path)
    model = info["name"].upper()
    tree = stem(info["skeleton"]) if info["skeleton"] else None
    tried = [name + kind for name in skeleton_names(info, model) for kind in (".MDH", ".MDL")]
    found = ((name, load_model(name, folders)) for name in tried)
    skeleton_file, skeleton = next(((n, d) for n, d in found if d and d["nodes"]), (None, None))
    if skeleton is None:
        raise LookupError(f"no skeleton was found; looked for {', '.join(tried)}")

    collection = file_collection(model, context.collection)
    rig = build_skeleton(context, info["name"], skeleton, scale, collection)
    armature = rig.armature
    worn = [] if info["disable_mesh"] else [(skeleton_file, skeleton)]
    if not info["disable_mesh"] and not skeleton["soft_skins"] and not skeleton["attachments"]:
        worn.append((tree + ".MDM", load_model(tree + ".MDM", folders)))
    registered = {"FIRST": info["meshes"][:1], "ALL": info["meshes"], "NONE": []}[meshes]
    if not info["disable_mesh"]:
        registered = [name for name in registered if stem(name) != tree]
    worn += [(stem(n) + ".MDM", load_model(stem(n) + ".MDM", folders)) for n in registered]
    for name, data in worn:
        if data is not None:
            add_meshes(context, path.parent, data, rig, scale, file_collection(name, collection))

    actions, missing = [], []
    for ani in info["animations"]:
        if not fnmatch.fnmatchcase(ani["name"].upper(), pattern.upper()):
            continue
        data = load_animation(f"{info['name']}-{ani['name']}.MAN".upper(), folders)
        if data is None:
            missing.append(ani["name"])
            continue
        action, _ = animation.build_action(armature, data, scale, ani["name"])
        settings = action.zenkit
        settings.layer, settings.next, settings.fps = max(ani["layer"], 1), ani["next"], ani["fps"]
        settings.blend_in, settings.blend_out = ani["blend_in"], ani["blend_out"]
        settings.flags = properties.flags_of(ani["flags"])
        actions.append(action)

    if info["text"] is not None:
        text = bpy.data.texts.new(f"{model}.MDS")
        text.from_string(info["text"].replace("\r\n", "\n"))
        armature.data.zenkit.script = text
    return armature, actions, missing, info["text_error"]


class ImportScript(bpy.types.Operator):
    """Import a Gothic model script (.mds, .msb) with its skeleton, meshes and animations"""

    bl_idname = "import_scene.zenkit_script"
    bl_label = "Import Gothic Model Script"
    bl_options = {"REGISTER", "UNDO"}

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.mds;*.msb", options={"HIDDEN"})
    scale: bpy.props.FloatProperty(
        name="Scale", default=0.01, min=1e-6, description="Metres per engine unit"
    )
    meshes: bpy.props.EnumProperty(
        name="Meshes",
        description="Which of the meshes the script registers to put on the skeleton",
        items=[
            ("FIRST", "First", "The first one, the model's usual look"),
            ("ALL", "All", "Every one, on top of each other"),
            ("NONE", "None", "Only the skeleton"),
        ],
        default="FIRST",
    )
    pattern: bpy.props.StringProperty(
        name="Animations",
        description="Which animations to import, by name: * stands for anything, as in S_*RUN*",
        default="*",
    )
    match_fps: bpy.props.BoolProperty(
        name="Match Frame Rate",
        description="Set the scene's frame rate to the first animation's",
        default=True,
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
            armature, actions, missing, text_error = import_script(
                context, path, self.scale, self.meshes, self.pattern
            )
        except (LookupError, zenkit_py.ZenKitError) as error:
            self.report({"ERROR"}, f"Cannot import {path.name}: {error}")
            return {"CANCELLED"}

        for other in context.selected_objects:
            other.select_set(False)
        armature.select_set(True)
        context.view_layer.objects.active = armature
        if actions:
            animation.show(context, actions[0], armature, self.match_fps)
        if missing:
            listed = ", ".join(missing[:5]) + (" and more" if len(missing) > 5 else "")
            self.report({"WARNING"}, f"{len(missing)} animations were not found: {listed}")
        if text_error is not None:
            self.report({"WARNING"}, f"The script cannot be kept as text: {text_error}")
        self.report({"INFO"}, f"Imported {path.name} with {len(actions)} animations")
        return {"FINISHED"}
