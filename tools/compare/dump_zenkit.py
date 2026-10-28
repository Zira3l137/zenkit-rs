"""Dumps every asset of one kind from a Gothic installation as JSON lines using ZenKit's Python
bindings (`pip install zenkit`), in the same shape as `zk-dump`.

    python dump_zenkit.py <mrm|msh|tex> <gothic directory> <output.jsonl>
"""

import hashlib
import json
import sys
from ctypes import c_size_t
from pathlib import Path

import zenkit
from zenkit._core import DLL, Date
from zenkit.core.vector import Vec3f

# Formats ZenKit can convert to RGBA. Asking for any other one throws a C++ exception through
# ctypes, which aborts the interpreter.
RGBA_FORMATS = {
    zenkit.TextureFormat.B8G8R8A8,
    zenkit.TextureFormat.R8G8B8A8,
    zenkit.TextureFormat.A8B8G8R8,
    zenkit.TextureFormat.A8R8G8B8,
    zenkit.TextureFormat.B8G8R8,
    zenkit.TextureFormat.R8G8B8,
    zenkit.TextureFormat.A4R4G4B4,
    zenkit.TextureFormat.R5G6B5,
    zenkit.TextureFormat.DXT1,
    zenkit.TextureFormat.DXT3,
    zenkit.TextureFormat.DXT5,
}


def latin1(text):
    """ZenKit decodes names as Windows-1252, zenkit-rs as Latin-1. Map back to the same bytes."""
    return text.encode("windows-1252", errors="replace").decode("latin-1")


def safe(fn):
    try:
        return fn()
    except Exception as e:
        return {"__error__": f"{type(e).__name__}: {e}"}


def v2(v):
    return [v.x, v.y]


def v3(v):
    return [v.x, v.y, v.z]


def aabb(b):
    return {"min": v3(b.min), "max": v3(b.max)}


def obb(b):
    # OrientedBoundingBox.axis forgets to pass the axis index, so call the C API directly.
    DLL.ZkOrientedBoundingBox_getAxis.restype = Vec3f
    axes = [v3(DLL.ZkOrientedBoundingBox_getAxis(b._handle, c_size_t(i))) for i in range(3)]
    return {
        "center": v3(b.center),
        "axes": axes,
        "half_width": v3(b.half_width),
        "children": [obb(c) for c in b.children],
    }


def material(m):
    c = m.color
    return {
        "name": latin1(m.name),
        "group": safe(lambda: int(m.group)),
        "color": [c.r, c.g, c.b, c.a],
        "smooth_angle": m.smooth_angle,
        "texture": latin1(m.texture),
        "texture_scale": v2(m.texture_scale),
        "texture_anim_fps": m.texture_animation_fps,
        "texture_anim_map_mode": safe(lambda: int(m.texture_animation_mapping)),
        "texture_anim_map_dir": v2(m.texture_animation_mapping_direction),
        "disable_collision": m.disable_collision,
        "disable_lightmap": m.disable_lightmap,
        "dont_collapse": m.dont_collapse,
        "detail_object": latin1(m.detail_object),
        "detail_object_scale": m.detail_object_scale,
        "force_occluder": m.force_occluder,
        "environment_mapping": m.environment_mapping,
        "environment_mapping_strength": m.environment_mapping_strength,
        "wave_mode": safe(lambda: int(m.wave_mode)),
        "wave_speed": safe(lambda: int(m.wave_speed)),
        "wave_max_amplitude": m.wave_amplitude,
        "wave_grid_size": m.wave_grid_size,
        "ignore_sun": m.ignore_sun,
        "alpha_func": safe(lambda: int(m.alpha_function)),
        "default_mapping": v2(m.default_mapping),
    }


def fields(**getters):
    """Evaluates each getter on its own, so a broken binding only costs that one field."""
    return {name: safe(getter) for name, getter in getters.items()}


def sub_mesh(s):
    return fields(
        material=lambda: material(s.material),
        triangles=lambda: [list(t.wedges) for t in s.triangles],
        wedges=lambda: [{"normal": v3(w.normal), "texture": v2(w.texture), "index": w.index} for w in s.wedges],
        colors=lambda: list(s.colors),
        triangle_plane_indices=lambda: list(s.triangle_plane_indices),
        triangle_planes=lambda: [{"distance": p.distance, "normal": v3(p.normal)} for p in s.triangle_planes],
        triangle_edges=lambda: [list(e.edges) for e in s.triangle_edges],
        # MeshEdge.edges reads a third element from its two-element array.
        edges=lambda: [[e._edges[0], e._edges[1]] for e in s.edges],
        edge_scores=lambda: list(s.edge_scores),
        wedge_map=lambda: list(s.wedge_map),
    )


def mrm(m):
    return fields(
        positions=lambda: [v3(p) for p in m.positions],
        normals=lambda: [v3(n) for n in m.normals],
        alpha_test=lambda: bool(m.alpha_test),
        # MultiResolutionMesh.bbox compares the box with 0 and returns that bool.
        bbox=lambda: aabb(DLL.ZkMultiResolutionMesh_getBbox(m._handle)),
        obb=lambda: obb(m.oriented_bbox),
        sub_meshes=lambda: [sub_mesh(s) for s in m.submeshes],
    )


def polygon(p):
    return fields(
        material=lambda: p.material_index,
        lightmap=lambda: p.light_map_index,
        vertices=lambda: p.position_indices,
        features=lambda: p.feature_indices,
        is_portal=lambda: bool(p.is_portal),
        is_occluder=lambda: bool(p.is_occluder),
        is_sector=lambda: bool(p.is_sector),
        should_relight=lambda: bool(p.should_relight),
        is_outdoor=lambda: bool(p.is_outdoor),
        is_ghost_occluder=lambda: bool(p.is_ghost_occluder),
        is_dynamically_lit=lambda: bool(p.is_dynamically_lit),
        is_lod=lambda: bool(p.is_lod),
        # Polygon.normal_axis and .sector_index compare the value with 0 and return that bool.
        normal_axis=lambda: DLL.ZkPolygon_getNormalAxis(p._handle),
        sector_index=lambda: DLL.ZkPolygon_getSectorIndex(p._handle),
    )


def mesh(m):
    def date():
        DLL.ZkMesh_getSourceDate.restype = Date
        d = DLL.ZkMesh_getSourceDate(m.handle)
        return [d.year, d.month, d.day, d.hour, d.minute, d.second]

    def lightmap(l):
        return fields(
            texture=lambda: texture(l.image),
            origin=lambda: v3(l.origin),
            normals=lambda: [v3(n) for n in l.normal],
        )

    return fields(
        name=lambda: latin1(m.name),
        date=date,
        bbox=lambda: aabb(m.bounding_box),
        obb=lambda: obb(m.oriented_bounding_box),
        materials=lambda: [material(x) for x in m.materials],
        vertices=lambda: [v3(v) for v in m.positions],
        features=lambda: [{"texture": v2(f.texture), "light": f.light, "normal": v3(f.normal)} for f in m.features],
        lightmaps=lambda: [lightmap(l) for l in m.light_maps],
        polygons=lambda: [polygon(p) for p in m.polygons],
    )


def texture(t):
    mipmaps = []
    for level in range(t.mipmap_count):
        rgba = None
        if t.format in RGBA_FORMATS and t.width_mipmap(level) > 0 and t.height_mipmap(level) > 0:
            rgba = hashlib.sha256(t.mipmap_rgba(level)).hexdigest()
        mipmaps.append(
            {
                "width": t.width_mipmap(level),
                "height": t.height_mipmap(level),
                "raw": hashlib.sha256(t.mipmap_raw(level)).hexdigest(),
                "rgba": rgba,
            }
        )

    return {
        "format": int(t.format),
        "width": t.width,
        "height": t.height,
        "reference_width": t.width_ref,
        "reference_height": t.height_ref,
        "average_color": t.average_color,
        "mipmaps": mipmaps,
    }


KINDS = {
    "mrm": (zenkit.MultiResolutionMesh, mrm),
    "msh": (zenkit.Mesh, mesh),
    "tex": (zenkit.Texture, texture),
}


def collect(node, prefix, extension, out):
    for child in node.children:
        path = prefix + latin1(child.name)
        if child.is_dir():
            collect(child, path + "/", extension, out)
        elif path.lower().endswith(extension):
            out.append((path, child))


def main():
    if len(sys.argv) != 4 or sys.argv[1] not in KINDS:
        print(__doc__, file=sys.stderr)
        return 1

    kind, game, output = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    cls, dump = KINDS[kind]

    disks = sorted((p for p in (game / "Data").iterdir() if p.suffix.lower() == ".vdf"), key=lambda p: p.name.lower())
    print(f"mounting {len(disks)} archives", file=sys.stderr)

    vfs = zenkit.Vfs()
    for disk in disks:
        vfs.mount_disk(disk, clobber=zenkit.VfsOverwriteBehavior.OLDER)

    files = []
    collect(vfs.root, "", "." + kind, files)
    files.sort(key=lambda f: f[0].lower())
    print(f"dumping {len(files)} .{kind} files, this takes a while", file=sys.stderr)

    with output.open("w", encoding="utf-8") as out:
        for i, (path, node) in enumerate(files):
            asset = cls.load(node)
            if asset._handle.value is None:
                entry = {"path": path, "error": "ZenKit failed to load the file"}
            else:
                entry = {"path": path, "result": safe(lambda: dump(asset))}
            out.write(json.dumps(entry) + "\n")
            if (i + 1) % 250 == 0:
                print(f"  {i + 1}/{len(files)}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    sys.exit(main())
