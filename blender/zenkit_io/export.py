"""Exporting a whole model at once: its skeleton, meshes, script and animations, into one folder,
named as the game looks for them.

`plan` works out which files an armature goes out as, which the Export panel shows before
anything is written, and `export_model` writes them:

- the skeleton as a `.MDH`, named as the engine compiles it: after the model when the script
  leaves out the skeleton's own mesh (`DONT_USE_MESH`, as HUMANS), else after the `meshAndTree`
  source, or as a `.MDL` with its meshes when they are in a collection of that name;
- the meshes as one `.MDM` per collection named after one, as importing makes them, and those in
  no such collection into the model's main mesh file;
- the script, with each animation's settings written into it, as `.MSB`, and `.MDS` if asked;
- every animation of the model as `<MODEL>-<NAME>.MAN`.
"""

from dataclasses import dataclass
from dataclasses import field
from pathlib import Path

import bpy
import zenkit_py

from . import animation
from . import properties
from .model import armature_of
from .model import gather_model
from .model import mesh_file
from .model import model_meshes

TEMPLATE = """Model ("{model}")
{{
	meshAndTree ("{model}.ASC" DONT_USE_MESH)
{meshes}
	aniEnum
	{{
	}}
}}
"""

_parsed = {}


def script_info(text):
    """The script dict of a text, or `(None, error)` if it is no script. Kept for as long as the
    text stays the same, as the panels ask on every redraw."""
    source = text.as_string()
    key = (text.name, hash(source))
    if key not in _parsed:
        _parsed.clear()
        try:
            _parsed[key] = (zenkit_py.parse_script(source), None)
        except zenkit_py.ZenKitError as error:
            _parsed[key] = (None, str(error))
    return _parsed[key]


def stem(name):
    """A file name a script gives, without the source's extension: `KRO_BODY.ASC` is
    `KRO_BODY`."""
    return Path(name).stem.upper() if Path(name).suffix else name.upper()


def skeleton_names(info, model):
    """The names the skeleton of a script's model may be compiled under, the engine's first
    (OpenGothic's Resources::implLoadMesh): the model's when the script leaves out the mesh of
    its `meshAndTree` source, the source's otherwise."""
    tree = stem(info["skeleton"]) if info and info["skeleton"] else None
    names = [model] if tree is None or info["disable_mesh"] else [tree]
    return names + [name for name in (tree, model) if name is not None and name not in names]


def main_mesh_file(info, model):
    """Where meshes in no collection of a model file go: the `meshAndTree` source's mesh, or the
    first mesh the script registers, or `<MODEL>_BODY.MDM`."""
    if info and info["skeleton"] and not info["disable_mesh"]:
        return stem(info["skeleton"]) + ".MDM"
    if info and info["meshes"]:
        return stem(info["meshes"][0]) + ".MDM"
    return f"{model}_BODY.MDM"


def new_script(armature):
    """A text with a script for an armature's model: its skeleton apart from its meshes, which
    it registers by their files, and no animations yet, which exporting adds."""
    model = animation.model_name(armature)
    files = {mesh_file(obj) or f"{model}_BODY.MDM" for obj in model_meshes(armature)}
    lines = [f'\tregisterMesh ("{stem(f)}.ASC")' for f in sorted(files)]
    text = bpy.data.texts.new(f"{model}.MDS")
    text.from_string(TEMPLATE.format(model=model, meshes="\n".join(lines)))
    armature.data.zenkit.script = text
    return text


def synced_script(armature):
    """The source of an armature's script with the settings of each of its model's animations
    written into the `ani` of the same name, added where there is none."""
    settings = []
    for action in animation.actions_of(armature):
        anim = action.zenkit
        start, end = animation.frame_range(action)
        settings.append(
            {
                "name": anim.game_name(action),
                "layer": anim.layer,
                "next": anim.next.upper(),
                "blend_in": anim.blend_in,
                "blend_out": anim.blend_out,
                "flags": properties.flags_text(anim.flags),
                "fps": anim.fps,
                "frames": end - start + 1,
            }
        )
    return zenkit_py.update_script(armature.data.zenkit.script.as_string(), settings)


@dataclass
class Plan:
    """The files an armature goes out as."""

    skeleton: str
    meshes: dict = field(default_factory=dict)
    script: str | None = None
    animations: list = field(default_factory=list)
    # The script's animations that no action plays, which keep only what the script says.
    unused: list = field(default_factory=list)
    problem: str | None = None


def plan(armature):
    """The files an armature goes out as, by what its scene and script say."""
    settings = armature.data.zenkit
    model = animation.model_name(armature)
    info, problem = script_info(settings.script) if settings.script else (None, None)

    meshes = {}
    default = main_mesh_file(info, model)
    for obj in model_meshes(armature):
        meshes.setdefault(mesh_file(obj) or default, []).append(obj)
    skeleton = skeleton_names(info, model)[0]
    skeleton = skeleton + ".MDL" if skeleton + ".MDL" in meshes else skeleton + ".MDH"

    actions = animation.actions_of(armature)
    files = [(f"{model}-{a.zenkit.game_name(a)}.MAN", a) for a in actions]
    names = {a.zenkit.game_name(a) for a in actions}
    unused = (
        [a["name"] for a in info["animations"] if a["name"].upper() not in names] if info else []
    )
    return Plan(
        skeleton=skeleton,
        meshes=dict(sorted(meshes.items())),
        script=f"{model}.MSB" if settings.script else None,
        animations=files,
        unused=unused,
        problem=problem,
    )


def export_model(context, armature, folder, game, scale):
    """Writes the files of `plan` that the armature's settings tick into `folder`. Gives the
    names written."""
    settings = armature.data.zenkit
    files = plan(armature)
    depsgraph = context.evaluated_depsgraph_get()
    written = []

    def write_model(name, meshes):
        data = gather_model(armature, depsgraph, scale, set(meshes))
        zenkit_py.write_model(str(folder / name), data, game)
        written.append(name)

    if settings.export_skeleton:
        write_model(files.skeleton, files.meshes.get(files.skeleton, []))
    if settings.export_meshes:
        for name, objects in files.meshes.items():
            if name != files.skeleton:
                write_model(name, objects)
    if settings.export_script and files.script:
        if files.problem:
            raise ValueError(f"the script has an error: {files.problem}")
        source = synced_script(armature)
        settings.script.from_string(source)
        zenkit_py.write_script(str(folder / files.script), source)
        written.append(files.script)
        if settings.export_source:
            name = Path(files.script).with_suffix(".MDS").name
            zenkit_py.write_script(str(folder / name), source)
            written.append(name)
    if settings.export_animations:
        for name, action in files.animations:
            data = animation.gather_animation(context, armature, action, scale)
            zenkit_py.write_animation(str(folder / name), data)
            written.append(name)
    return written


class ExportScript(bpy.types.Operator):
    """Export the active armature's script as a Gothic model script (.msb, .mds), with each
    animation's settings written into it, and its animations as .man files next to it"""

    bl_idname = "export_scene.zenkit_script"
    bl_label = "Export Gothic Model Script"

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.msb;*.mds", options={"HIDDEN"})
    animations: bpy.props.BoolProperty(
        name="Export Animations",
        description="Write the model's animations as .man files next to the script",
        default=True,
    )
    scale: bpy.props.FloatProperty(
        name="Scale", default=0.01, min=1e-6, description="Metres per engine unit"
    )

    @classmethod
    def poll(cls, context):
        armature = armature_of(context.active_object)
        return armature is not None and armature.data.zenkit.script is not None

    def invoke(self, context, event):
        armature = armature_of(context.active_object)
        name = animation.model_name(armature) + ".MSB"
        start = context.scene.zenkit.export_folder()
        self.filepath = str(start / name) if start else name
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        armature = armature_of(context.active_object)
        path = Path(self.filepath)
        try:
            source = synced_script(armature)
            zenkit_py.write_script(str(path), source)
            armature.data.zenkit.script.from_string(source)
            files = plan(armature).animations if self.animations else []
            for name, action in files:
                data = animation.gather_animation(context, armature, action, self.scale)
                zenkit_py.write_animation(str(path.with_name(name)), data)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Exported {path.name} and {len(files)} animations")
        return {"FINISHED"}
