"""Gothic model scripts (.MDS source, .MSB compiled) to and from Blender.

A model script names a model's skeleton, the meshes it can wear and its animations, each of which
the engine loads from a file named after the model and the animation: HUMANS-S_RUN.MAN.
Importing one brings in all of that, found next to the script, in the models folder or in the
game's archives: an armature of the skeleton, the meshes on it and an action per animation.

The script itself becomes a text in Blender's Text Editor, as source, to edit there. Exporting
writes that text as a script, compiled or source, and each of its animations that has an action
of the same name as a .MAN file next to it, with the layer, frame rate and next animation the
script gives it. The engine reads the script's frame numbers only when it compiles animations
from their source files, so the action's frames are what the file holds.
"""

import fnmatch
from pathlib import Path

import bpy
import zenkit_py

from . import animation
from . import preferences
from .model import add_meshes
from .model import armature_checksum
from .model import armature_of
from .model import build_skeleton

SCRIPT = "zenkit_script"


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


def stem(name):
    """A file name the script gives, without the source's extension, as compiled files are
    named: `KRO_BODY.ASC` is `KRO_BODY`."""
    return Path(name).stem.upper() if Path(name).suffix else name.upper()


def import_script(context, path, scale, meshes="FIRST", pattern="*"):
    """Imports a model script with its skeleton, meshes and the animations whose names match
    `pattern`. Gives the armature, the actions and the names of the animations not found."""
    info = zenkit_py.read_script(str(path))
    folders = folders_for(path)
    model = info["name"].upper()
    tree = stem(info["skeleton"]) if info["skeleton"] else None
    # Named as the engine compiles them (OpenGothic's Resources::implLoadMesh): a skeleton whose
    # own mesh is left out under the model's name, HUMANS.MDH for meshAndTree("HUM_BODY_NAKED0.ASC"
    # DONT_USE_MESH); one with it under the source's, its mesh in a .MDM of that name or with it
    # in a .MDL. The other name is tried after.
    names = [model] if info["disable_mesh"] or tree is None else [tree]
    names += [name for name in (tree, model) if name is not None and name not in names]
    tried = [name + kind for name in names for kind in (".MDH", ".MDL")]
    skeleton = next(
        (data for name in tried if (data := load_model(name, folders)) and data["nodes"]), None
    )
    if skeleton is None:
        raise LookupError(f"no skeleton was found; looked for {', '.join(tried)}")

    rig = build_skeleton(context, info["name"], skeleton, scale)
    armature = rig.armature
    armature.data[animation.MODEL] = info["name"]
    worn = [] if info["disable_mesh"] else [skeleton]
    if not info["disable_mesh"] and not skeleton["soft_skins"] and not skeleton["attachments"]:
        worn.append(load_model(tree + ".MDM", folders))
    registered = {"FIRST": info["meshes"][:1], "ALL": info["meshes"], "NONE": []}[meshes]
    if not info["disable_mesh"]:
        registered = [name for name in registered if stem(name) != tree]
    worn += [load_model(stem(name) + ".MDM", folders) for name in registered]
    for data in worn:
        if data is not None:
            add_meshes(context, path.parent, data, rig, scale)

    actions, missing = [], []
    for ani in info["animations"]:
        if not fnmatch.fnmatchcase(ani["name"].upper(), pattern.upper()):
            continue
        data = load_animation(f"{info['name']}-{ani['name']}.MAN".upper(), folders)
        if data is None:
            missing.append(ani["name"])
            continue
        action, _ = animation.build_action(armature, data, scale, ani["name"])
        actions.append(action)

    if info["text"] is not None:
        text = bpy.data.texts.new(f"{info['name']}.MDS")
        text.from_string(info["text"].replace("\r\n", "\n"))
        armature.data[SCRIPT] = text.name
    return armature, actions, missing, info["text_error"]


def find_action(name, checksum):
    """The action of the animation `name`, one made for the skeleton of this checksum if there
    are several."""
    found = [a for a in bpy.data.actions if a.get(animation.NAME, a.name).upper() == name.upper()]
    return next((a for a in found if a.get(animation.CHECKSUM) == str(checksum)), None) or (
        found[0] if found else None
    )


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


class ExportScript(bpy.types.Operator):
    """Export a model script from Blender's Text Editor as a Gothic model script (.msb, .mds),
    with the active armature's actions as its animations"""

    bl_idname = "export_scene.zenkit_script"
    bl_label = "Export Gothic Model Script"

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.msb;*.mds", options={"HIDDEN"})
    text: bpy.props.StringProperty(
        name="Script",
        description="The text holding the script; the one imported with the armature if empty",
    )
    animations: bpy.props.BoolProperty(
        name="Export Animations",
        description="Write each animation of the script that has an action as a .man file "
        "next to the script",
        default=True,
    )
    scale: bpy.props.FloatProperty(
        name="Scale", default=0.01, min=1e-6, description="Metres per engine unit"
    )

    @classmethod
    def poll(cls, context):
        return armature_of(context.active_object) is not None

    def draw(self, context):
        self.layout.prop_search(self, "text", bpy.data, "texts")
        self.layout.prop(self, "animations")
        self.layout.prop(self, "scale")

    def invoke(self, context, event):
        armature = armature_of(context.active_object)
        self.text = armature.data.get(SCRIPT, "")
        name = animation.model_name(armature) + ".MSB"
        start = preferences.folder("anims")
        self.filepath = str(start / name) if start else name
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        armature = armature_of(context.active_object)
        text = bpy.data.texts.get(self.text or armature.data.get(SCRIPT, ""))
        if text is None:
            self.report({"ERROR"}, "Pick the text holding the script")
            return {"CANCELLED"}
        source = text.as_string()
        path = Path(self.filepath)
        try:
            info = zenkit_py.parse_script(source)
            zenkit_py.write_script(str(path), source)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, f"Cannot export {text.name}: {error}")
            return {"CANCELLED"}
        armature.data[SCRIPT] = text.name

        written, missing = 0, []
        checksum = armature_checksum(armature)
        for ani in info["animations"] if self.animations else []:
            action = find_action(ani["name"], checksum)
            if action is None:
                missing.append(ani["name"])
                continue
            data = animation.gather_animation(context, armature, action, self.scale)
            data.update(name=ani["name"], next=ani["next"], layer=ani["layer"], fps=ani["fps"])
            out = path.with_name(f"{info['name']}-{ani['name']}.MAN".upper())
            try:
                zenkit_py.write_animation(str(out), data)
            except zenkit_py.ZenKitError as error:
                self.report({"ERROR"}, f"Cannot export {action.name}: {error}")
                return {"CANCELLED"}
            written += 1

        if missing:
            listed = ", ".join(missing[:5]) + (" and more" if len(missing) > 5 else "")
            self.report({"WARNING"}, f"{len(missing)} animations have no action: {listed}")
        self.report({"INFO"}, f"Exported {path.name} and {written} animations")
        return {"FINISHED"}
