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


def mrm(m):
    sub_meshes = []
    for s in m.submeshes:
        sub_meshes.append(
            {
                "material": material(s.material),
                "triangles": [list(t.wedges) for t in s.triangles],
                "wedges": [{"normal": v3(w.normal), "texture": v2(w.texture), "index": w.index} for w in s.wedges],
                "colors": list(s.colors),
                "triangle_plane_indices": list(s.triangle_plane_indices),
                "triangle_planes": [{"distance": p.distance, "normal": v3(p.normal)} for p in s.triangle_planes],
                "triangle_edges": [list(e.edges) for e in s.triangle_edges],
                "edges": [list(e.edges)[:2] for e in s.edges],
                "edge_scores": list(s.edge_scores),
                "wedge_map": list(s.wedge_map),
            }
        )

    return {
        "positions": [v3(p) for p in m.positions],
        "normals": [v3(n) for n in m.normals],
        "alpha_test": bool(m.alpha_test),
        "bbox": aabb(m.bbox),
        "obb": obb(m.oriented_bbox),
        "sub_meshes": sub_meshes,
    }


def mesh(m):
    DLL.ZkMesh_getSourceDate.restype = Date
    d = DLL.ZkMesh_getSourceDate(m.handle)

    polygons = []
    for p in m.polygons:
        polygons.append(
            {
                "material": p.material_index,
                "lightmap": p.light_map_index,
                "vertices": p.position_indices,
                "features": p.feature_indices,
                "is_portal": bool(p.is_portal),
                "is_occluder": bool(p.is_occluder),
                "is_sector": bool(p.is_sector),
                "should_relight": bool(p.should_relight),
                "is_outdoor": bool(p.is_outdoor),
                "is_ghost_occluder": bool(p.is_ghost_occluder),
                "is_dynamically_lit": bool(p.is_dynamically_lit),
                "is_lod": bool(p.is_lod),
                "normal_axis": int(p.normal_axis),
                "sector_index": int(p.sector_index),
            }
        )

    return {
        "name": latin1(m.name),
        "date": [d.year, d.month, d.day, d.hour, d.minute, d.second],
        "bbox": aabb(m.bounding_box),
        "obb": obb(m.oriented_bounding_box),
        "materials": [material(x) for x in m.materials],
        "vertices": [v3(v) for v in m.positions],
        "features": [{"texture": v2(f.texture), "light": f.light, "normal": v3(f.normal)} for f in m.features],
        "lightmaps": [
            {"texture": texture(l.image), "origin": v3(l.origin), "normals": [v3(n) for n in l.normal]}
            for l in m.light_maps
        ],
        "polygons": polygons,
    }


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
