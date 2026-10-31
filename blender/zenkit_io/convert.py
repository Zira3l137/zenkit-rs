"""Converting between the engine's data, as zenkit_py hands it over, and Blender's.

The engine works in centimetres with Y up in a left-handed space, characters facing +Z; Blender
in metres (by the default scale) with Z up in a right-handed one, its front view looking at -Y.
Swapping Y and Z, then turning half way around Z, converts between the two: engine (x, y, z)
becomes Blender (-x, -z, y), so what faces forward in the game faces Blender's front. Since the
swap mirrors the mesh, it also turns the engine's clockwise front faces into Blender's
counter-clockwise ones, so no face needs flipping. Transforms are converted on both sides, which
keeps rotations rotations.
"""

from pathlib import Path

import bpy
import numpy as np
from mathutils import Matrix

from . import texture

AXES = np.array([[-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]])
AXES4 = np.eye(4)
AXES4[:3, :3] = AXES


def to_blender(points):
    """Engine positions or directions, an (n, 3) array, in Blender's axes."""
    return points @ AXES.T


def to_engine(points):
    """Blender positions or directions, an (n, 3) array, in the engine's axes."""
    return points @ AXES


def array(data, key, dtype, width=1):
    values = np.frombuffer(data[key], dtype)
    return values.reshape(-1, width) if width > 1 else values


def matrix_to_blender(rows, scale):
    """A transform of the engine, 16 floats row by row, as a Blender matrix."""
    m = AXES4 @ np.frombuffer(rows, "<f4").reshape(4, 4).astype(np.float64) @ AXES4.T
    m[:3, 3] *= scale
    return Matrix(m.tolist())


def matrix_to_engine(matrix, scale):
    """A Blender matrix as a transform of the engine, 16 floats row by row."""
    m = np.array(matrix, dtype=np.float64)
    m[:3, 3] /= scale
    return (AXES4.T @ m @ AXES4).astype("<f4").tobytes()


def material(name, texture_name, folders):
    """The Blender material of that name, made with its texture plugged in if it is new."""
    existing = bpy.data.materials.get(name)
    if existing is not None:
        return existing

    mat = bpy.data.materials.new(name)
    image = texture.find(texture_name, folders) if texture_name else None
    if image is None:
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
        if image.get(texture.ALPHA):
            mat.node_tree.links.new(node.outputs["Alpha"], shader.inputs["Alpha"])
    return mat


def texture_name(mat):
    """The texture a material's first image node shows, by the name the engine knows it under."""
    if mat is None or mat.node_tree is None:
        return None
    for node in mat.node_tree.nodes:
        if node.type == "TEX_IMAGE" and node.image is not None:
            stem = Path(node.image.name).stem.upper()
            return stem.removesuffix("-C") + ".TGA"
    return None


def build_mesh(name, data, scale, folders, positions=None):
    """Turns a mesh dict from zenkit_py into a Blender mesh, its vertices at `positions`, in
    Blender's space, if given."""
    if positions is None:
        positions = to_blender(array(data, "positions", "<f4", 3)) * scale
    counts = array(data, "corner_counts", "<u4")
    vertices = array(data, "corner_vertices", "<u4")
    uvs = array(data, "corner_uvs", "<f4", 2).copy()
    uvs[:, 1] = 1.0 - uvs[:, 1]
    normals = to_blender(array(data, "corner_normals", "<f4", 3))
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

    for mat_name, tex_name in data["materials"]:
        mesh.materials.append(material(mat_name, tex_name, folders))

    # Not validated: that would merge the two sides of double-sided faces, which the engine
    # stores as two triangles on the same corners. zenkit_py has checked the indices instead.
    # Blender keeps custom normals relative to the faces around them, so they are set last, once
    # the vertices are where they stay.
    mesh.update()
    mesh.normals_split_custom_set(normals.tolist())
    return mesh


def gather_mesh(obj, depsgraph, scale, triangles, space=None, bones=None):
    """Turns an object's evaluated mesh into a mesh dict for zenkit_py.

    The mesh is taken in world space, or relative to `space`, a world matrix, when given: models
    keep their meshes relative to the armature or the bone they hang from. Modifiers are applied,
    except an armature's, which would pose the mesh.

    `bones` maps bone names to node indices; with it, each vertex's weights are gathered from the
    vertex groups of those names as `weights`, a list of (node, weight) lists. Weights are made to
    add up to one, and a vertex in no group is given to node 0, the root.
    """
    hidden = [m for m in obj.modifiers if m.type == "ARMATURE" and m.show_viewport]
    for modifier in hidden:
        modifier.show_viewport = False
    depsgraph.update()
    evaluated = obj.evaluated_get(depsgraph)
    mesh = evaluated.to_mesh()
    try:
        to_space = obj.matrix_world if space is None else space.inverted() @ obj.matrix_world
        mesh.transform(to_space)

        positions = np.empty(len(mesh.vertices) * 3, np.float32)
        mesh.vertices.foreach_get("co", positions)
        positions = to_engine(positions.reshape(-1, 3)) / scale

        loop_vertices = np.empty(len(mesh.loops), np.int32)
        mesh.loops.foreach_get("vertex_index", loop_vertices)
        loop_normals = np.empty(len(mesh.loops) * 3, np.float32)
        mesh.corner_normals.foreach_get("vector", loop_normals)
        loop_normals = to_engine(loop_normals.reshape(-1, 3))
        loop_uvs = np.zeros((len(mesh.loops), 2), np.float32)
        if mesh.uv_layers.active is not None:
            mesh.uv_layers.active.data.foreach_get("uv", loop_uvs.ravel())
        loop_uvs[:, 1] = 1.0 - loop_uvs[:, 1]

        weights = None
        if bones is not None:
            names = {group.index: group.name for group in obj.vertex_groups}
            weights = []
            for vertex in mesh.vertices:
                pairs = [
                    (bones[names[g.group]], g.weight)
                    for g in vertex.groups
                    if names.get(g.group) in bones and g.weight > 0.0
                ]
                total = sum(weight for _, weight in pairs)
                weights.append([(n, w / total) for n, w in pairs] if pairs else [(0, 1.0)])

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
        for modifier in hidden:
            modifier.show_viewport = True

    materials = []
    for slot in obj.material_slots:
        mat = slot.material
        name = mat.name if mat else "DEFAULT"
        materials.append((name, texture_name(mat) or name.upper() + ".TGA"))
    if not materials:
        materials.append(("DEFAULT", "DEFAULT.TGA"))
    polygon_materials = np.clip(polygon_materials, 0, len(materials) - 1)

    data = {
        "positions": positions.astype("<f4").tobytes(),
        "corner_counts": counts.astype("<u4").tobytes(),
        "corner_vertices": loop_vertices[corners].astype("<u4").tobytes(),
        "corner_uvs": loop_uvs[corners].astype("<f4").tobytes(),
        "corner_normals": loop_normals[corners].astype("<f4").tobytes(),
        "polygon_materials": polygon_materials.astype("<u4").tobytes(),
        "materials": materials,
    }
    return data if weights is None else (data, weights)
