"""Gothic model scripts (.MDS source, .MSB compiled) to and from Blender.

A model script names a model's skeleton, the meshes it can wear and its animations, each of which
the engine loads from a file named after the model and the animation: HUMANS-S_RUN.MAN. The engine
names the model after the script's file; the name in its `Model` line usually agrees, but not
always (Gothic 1's BABE.MDS says BAB), so both are tried. A skeleton found under neither is looked
for by the checksum its meshes and animations carry.
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
from .model import find_skeleton


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


def model_names(info, path):
    """The names a script's model may go by, the engine's first: the script file's, which the
    engine names the model and its animation files after, then the one its `Model` line gives."""
    return list(dict.fromkeys((Path(path).stem.upper(), info["name"].upper())))


def script_checksums(info, names, folders):
    """The checksums of the skeleton a script's meshes and animations were made for: those of the
    meshes it names, and of the first of its animations found under each of `names`."""
    meshes = info["meshes"] + ([info["skeleton"]] if info["skeleton"] else [])
    found = [load_model(stem(name) + ".MDM", folders) for name in dict.fromkeys(meshes)]
    for model in names:
        files = (f"{model}-{ani['name']}.MAN".upper() for ani in info["animations"])
        tried = (load_animation(name, folders) for name in files)
        found.append(next((data for data in tried if data is not None), None))
    return {data["checksum"] for data in found if data is not None and data["checksum"]}


def find_script_skeleton(info, names, folders, path):
    """The file name and model dict of a script's skeleton, and the model name it was found under,
    if it was found by name.

    The engine's names for it are tried first. If none of them has it, as when the script's
    `Model` line names another model than its file, every skeleton is searched for the checksum
    the script's meshes and animations carry, and the first that has it is taken.
    """
    tried = {}
    for model in names:
        for name in skeleton_names(info, model):
            for kind in (".MDH", ".MDL"):
                tried.setdefault(name + kind, model)
    for name, model in tried.items():
        data = load_model(name, folders)
        if data is not None and data["nodes"]:
            return name, data, model

    checksums = script_checksums(info, names, folders)
    found = find_skeleton(checksums, Path(path).parent) if checksums else None
    if found is None:
        looked = f"looked for {', '.join(tried)}"
        if checksums:
            listed = ", ".join(str(c) for c in sorted(checksums))
            looked += f", and for any skeleton with the checksum {listed}"
        raise LookupError(f"no skeleton was found; {looked}")
    return found[0], found[1], None


def load_animations(info, prefixes, folders, pattern):
    """The animations whose names match `pattern`, each with its data and the model name its file
    was found under, trying `prefixes` in turn, and the names of those not found."""
    loaded, missing = [], []
    prefixes = list(prefixes)
    for ani in info["animations"]:
        if not fnmatch.fnmatchcase(ani["name"].upper(), pattern.upper()):
            continue
        for prefix in prefixes:
            data = load_animation(f"{prefix}-{ani['name']}.MAN".upper(), folders)
            if data is not None:
                loaded.append((ani, data, prefix))
                # The rest are most likely named alike.
                prefixes.insert(0, prefixes.pop(prefixes.index(prefix)))
                break
        else:
            missing.append(ani["name"])
    return loaded, missing


def import_script(context, path, scale, meshes="FIRST", pattern="*"):
    """Imports a model script with its skeleton, meshes and the animations whose names match
    `pattern`. Gives the armature, the actions and the names of the animations not found."""
    info = zenkit_py.read_script(str(path))
    folders = folders_for(path)
    names = model_names(info, path)
    tree = stem(info["skeleton"]) if info["skeleton"] else None
    skeleton_file, skeleton, named = find_script_skeleton(info, names, folders, path)

    prefixes = (names + [stem(skeleton_file)]) if named is None else ([named] + names)
    loaded, missing = load_animations(info, dict.fromkeys(prefixes), folders, pattern)
    # The model is named as its files are: its animations', or its skeleton's.
    model = loaded[0][2] if loaded else (named or names[0])

    collection = file_collection(model, context.collection)
    rig = build_skeleton(context, model, skeleton, scale, collection)
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

    actions = []
    for ani, data, _ in loaded:
        action, _ = animation.build_action(armature, data, scale, ani["name"])
        settings = action.zenkit
        settings.layer, settings.next, settings.fps = max(ani["layer"], 1), ani["next"], ani["fps"]
        settings.blend_in, settings.blend_out = ani["blend_in"], ani["blend_out"]
        settings.flags = properties.flags_of(ani["flags"])
        settings.script_name = ani["name"].upper()
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
