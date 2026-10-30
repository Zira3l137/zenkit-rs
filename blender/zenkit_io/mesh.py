"""Gothic static meshes (.MSH) and multi-resolution meshes (.MRM) to and from Blender meshes."""

from pathlib import Path

import bpy
import zenkit_py

from . import convert
from . import preferences


def link(context, obj):
    """Adds a new object to the scene and makes it the only one selected."""
    context.collection.objects.link(obj)
    for other in context.selected_objects:
        other.select_set(False)
    obj.select_set(True)
    context.view_layer.objects.active = obj


class ImportMesh(bpy.types.Operator):
    """Import a Gothic static mesh (.msh) or multi-resolution mesh (.mrm)"""

    bl_idname = "import_scene.zenkit_mesh"
    bl_label = "Import Gothic Mesh"
    bl_options = {"REGISTER", "UNDO"}

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.msh;*.mrm", options={"HIDDEN"})
    scale: bpy.props.FloatProperty(
        name="Scale", default=0.01, min=1e-6, description="Metres per engine unit"
    )

    def invoke(self, context, event):
        preferences.browse_from(self, "meshes")
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        path = Path(self.filepath)
        try:
            if path.suffix.lower() == ".mrm":
                data = zenkit_py.read_mrm(str(path))
            else:
                data = zenkit_py.read_msh(str(path))
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}

        mesh = convert.build_mesh(path.stem, data, self.scale, [path.parent])
        link(context, bpy.data.objects.new(path.stem, mesh))
        self.report(
            {"INFO"},
            f"Imported {path.name}: {len(mesh.vertices)} vertices, {len(mesh.polygons)} faces",
        )
        return {"FINISHED"}


class ExportMesh:
    """What both mesh exports share; each says which format it writes."""

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
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
        name = context.active_object.name.upper() + self.extension
        start = preferences.folder("meshes")
        self.filepath = str(start / name) if start else name
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        obj = context.active_object
        data = convert.gather_mesh(
            obj, context.evaluated_depsgraph_get(), self.scale, self.triangles
        )
        try:
            self.write(data, obj)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Exported {obj.name}")
        return {"FINISHED"}


class ExportMsh(ExportMesh, bpy.types.Operator):
    """Export the active mesh as a Gothic static mesh"""

    bl_idname = "export_scene.zenkit_msh"
    bl_label = "Export Gothic Static Mesh"
    extension = ".MSH"
    triangles = False

    filter_glob: bpy.props.StringProperty(default="*.msh", options={"HIDDEN"})

    def write(self, data, obj):
        zenkit_py.write_msh(self.filepath, obj.name.upper() + ".3DS", data, self.game)


class ExportMrm(ExportMesh, bpy.types.Operator):
    """Export the active mesh as a Gothic multi-resolution mesh, triangulated"""

    bl_idname = "export_scene.zenkit_mrm"
    bl_label = "Export Gothic Multi-Resolution Mesh"
    extension = ".MRM"
    triangles = True

    filter_glob: bpy.props.StringProperty(default="*.mrm", options={"HIDDEN"})

    def write(self, data, obj):
        zenkit_py.write_mrm(self.filepath, data, self.game)
