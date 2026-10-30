"""Gothic static meshes (.MSH) and multi-resolution meshes (.MRM) to and from Blender meshes.

The engine works in centimetres with Y up in a left-handed space; Blender in metres (by the
default scale) with Z up in a right-handed one. Swapping Y and Z converts between the two, and
since it mirrors the mesh, it also turns the engine's clockwise front faces into Blender's
counter-clockwise ones, so no face needs flipping.
"""

from pathlib import Path

import bpy
import numpy as np
import zenkit_py

from . import texture

SWAP_YZ = [0, 2, 1]


def array(data, key, dtype, width=1):
    values = np.frombuffer(data[key], dtype)
    return values.reshape(-1, width) if width > 1 else values


def find_texture(name, folders):
    """The compiled file of a texture a material names, like `STONE-C.TEX` for `STONE.TGA`."""
    compiled = Path(name).stem.upper() + "-C.TEX"
    for folder in folders:
        for candidate in (folder / compiled, folder / compiled.lower()):
            if candidate.is_file():
                return candidate
    return None


def material(name, texture_name, folders):
    """The Blender material of that name, made with the texture plugged in if it is new."""
    existing = bpy.data.materials.get(name)
    if existing is not None:
        return existing

    mat = bpy.data.materials.new(name)
    path = find_texture(texture_name, folders) if texture_name else None
    if path is None:
        return mat
    try:
        image = texture.load(path)
    except zenkit_py.ZenKitError:
        return mat

    # Materials always use nodes from Blender 5.0 on, where the switch is deprecated.
    if not getattr(mat, "use_nodes", True):
        mat.use_nodes = True
    nodes = mat.node_tree.nodes
    shader = nodes.get("Principled BSDF")
    node = nodes.new("ShaderNodeTexImage")
    node.image = image
    node.location = (-300, 300)
    if shader is not None:
        mat.node_tree.links.new(node.outputs["Color"], shader.inputs["Base Color"])
    return mat


def build(name, data, scale, folders):
    """Turns a mesh dict from zenkit_py into a Blender mesh."""
    positions = array(data, "positions", "<f4", 3)[:, SWAP_YZ] * scale
    counts = array(data, "corner_counts", "<u4")
    vertices = array(data, "corner_vertices", "<u4")
    uvs = array(data, "corner_uvs", "<f4", 2).copy()
    uvs[:, 1] = 1.0 - uvs[:, 1]
    normals = array(data, "corner_normals", "<f4", 3)[:, SWAP_YZ]
    materials = array(data, "polygon_materials", "<u4")

    mesh = bpy.data.meshes.new(name)
    mesh.vertices.add(len(positions))
    mesh.vertices.foreach_set("co", positions.astype(np.float32).ravel())
    mesh.loops.add(len(vertices))
    mesh.loops.foreach_set("vertex_index", vertices.astype(np.int32))
    mesh.polygons.add(len(counts))
    starts = np.concatenate(([0], np.cumsum(counts)[:-1])).astype(np.int32)
    mesh.polygons.foreach_set("loop_start", starts)
    mesh.polygons.foreach_set("material_index", materials.astype(np.int32))
    mesh.polygons.foreach_set("use_smooth", np.ones(len(counts), bool))
    mesh.uv_layers.new(name="UVMap").data.foreach_set("uv", uvs.astype(np.float32).ravel())

    for mat_name, texture_name in data["materials"]:
        mesh.materials.append(material(mat_name, texture_name, folders))

    # Not validated: that would merge the two sides of double-sided faces, which the engine
    # stores as two triangles on the same corners. zenkit_py has checked the indices instead.
    mesh.update()
    mesh.normals_split_custom_set(normals.tolist())
    return mesh


def gather(obj, depsgraph, scale, triangles):
    """Turns an object's evaluated mesh, in world space, into a mesh dict for zenkit_py."""
    evaluated = obj.evaluated_get(depsgraph)
    mesh = evaluated.to_mesh()
    try:
        mesh.transform(obj.matrix_world)

        positions = np.empty(len(mesh.vertices) * 3, np.float32)
        mesh.vertices.foreach_get("co", positions)
        positions = positions.reshape(-1, 3)[:, SWAP_YZ] / scale

        loop_vertices = np.empty(len(mesh.loops), np.int32)
        mesh.loops.foreach_get("vertex_index", loop_vertices)
        loop_normals = np.empty(len(mesh.loops) * 3, np.float32)
        mesh.corner_normals.foreach_get("vector", loop_normals)
        loop_normals = loop_normals.reshape(-1, 3)[:, SWAP_YZ]
        loop_uvs = np.zeros((len(mesh.loops), 2), np.float32)
        if mesh.uv_layers.active is not None:
            mesh.uv_layers.active.data.foreach_get("uv", loop_uvs.ravel())
        loop_uvs[:, 1] = 1.0 - loop_uvs[:, 1]

        if triangles:
            mesh.calc_loop_triangles()
            corners = np.empty(len(mesh.loop_triangles) * 3, np.int32)
            mesh.loop_triangles.foreach_get("loops", corners)
            polygon_materials = np.empty(len(mesh.loop_triangles), np.int32)
            mesh.loop_triangles.foreach_get("material_index", polygon_materials)
            counts = np.full(len(mesh.loop_triangles), 3, np.uint32)
        else:
            corners = np.arange(len(mesh.loops), dtype=np.int32)
            polygon_materials = np.empty(len(mesh.polygons), np.int32)
            mesh.polygons.foreach_get("material_index", polygon_materials)
            counts = np.empty(len(mesh.polygons), np.int32)
            mesh.polygons.foreach_get("loop_total", counts)
    finally:
        evaluated.to_mesh_clear()

    materials = []
    for slot in obj.material_slots:
        mat = slot.material
        name = mat.name if mat else "DEFAULT"
        materials.append((name, texture_name(mat) or name.upper() + ".TGA"))
    if not materials:
        materials.append(("DEFAULT", "DEFAULT.TGA"))
    polygon_materials = np.clip(polygon_materials, 0, len(materials) - 1)

    return {
        "positions": positions.astype("<f4").tobytes(),
        "corner_counts": counts.astype("<u4").tobytes(),
        "corner_vertices": loop_vertices[corners].astype("<u4").tobytes(),
        "corner_uvs": loop_uvs[corners].astype("<f4").tobytes(),
        "corner_normals": loop_normals[corners].astype("<f4").tobytes(),
        "polygon_materials": polygon_materials.astype("<u4").tobytes(),
        "materials": materials,
    }


def texture_name(mat):
    """The texture a material's first image node shows, by the name the engine knows it under."""
    if mat is None or mat.node_tree is None:
        return None
    for node in mat.node_tree.nodes:
        if node.type == "TEX_IMAGE" and node.image is not None:
            stem = node.image.name.rsplit(".", 1)[0].upper()
            return stem.removesuffix("-C") + ".TGA"
    return None


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
    texture_folder: bpy.props.StringProperty(
        name="Textures",
        subtype="DIR_PATH",
        description="Where to look for compiled textures besides the mesh's own folder",
    )

    def invoke(self, context, event):
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

        folders = [path.parent] + ([Path(self.texture_folder)] if self.texture_folder else [])
        mesh = build(path.stem, data, self.scale, folders)
        obj = bpy.data.objects.new(path.stem, mesh)
        context.collection.objects.link(obj)
        for other in context.selected_objects:
            other.select_set(False)
        obj.select_set(True)
        context.view_layer.objects.active = obj
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
        self.filepath = context.active_object.name.upper() + self.extension
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        obj = context.active_object
        data = gather(obj, context.evaluated_depsgraph_get(), self.scale, self.triangles)
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
