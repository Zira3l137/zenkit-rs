"""Dumps every asset of one kind from a Gothic installation as JSON lines using ZenKit's Python
bindings (`pip install zenkit`), in the same shape as `zk-dump`.

    python dump_zenkit.py <mrm|msh|tex|mdh|mdm|mdl|mmb|man> <gothic directory> <output.jsonl>
"""

import hashlib
import json
import sys
import threading
import traceback
from ctypes import byref, c_float, c_size_t, c_ulong, c_void_p, string_at
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


# zenkit-rs rejects deeper box trees. ZenKit builds them from misread data, sometimes hundreds of
# levels deep with thousands of children each, which takes hours to walk through ctypes.
OBB_MAX_DEPTH = 64


def obb(b, depth=0):
    if depth > OBB_MAX_DEPTH:
        raise ValueError(f"box tree is deeper than {OBB_MAX_DEPTH} levels, so ZenKit misread it")

    # OrientedBoundingBox.axis forgets to pass the axis index, so call the C API directly.
    DLL.ZkOrientedBoundingBox_getAxis.restype = Vec3f
    axes = [v3(DLL.ZkOrientedBoundingBox_getAxis(b._handle, c_size_t(i))) for i in range(3)]
    return {
        "center": v3(b.center),
        "axes": axes,
        "half_width": v3(b.half_width),
        "children": [obb(c, depth + 1) for c in b.children],
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


def mipmap_raw(t, level):
    # Texture.mipmap_raw declares the result as c_char_p, so ctypes copies the pixels only up to
    # the first zero byte, and string_at then reads the full size past the end of that copy. That
    # hashes garbage and sometimes crashes. The C function returns a plain pointer; ZkSize is a C
    # `unsigned long`.
    DLL.ZkTexture_getMipmapRaw.restype = c_void_p
    size = c_ulong(0)
    pointer = DLL.ZkTexture_getMipmapRaw(t._handle, c_ulong(level), byref(size))
    return string_at(pointer, size.value) if pointer else b""


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
                "raw": hashlib.sha256(mipmap_raw(t, level)).hexdigest(),
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


def floats(struct, count):
    """The struct's floats in memory order, whatever its Python field names claim."""
    return list((c_float * count).from_buffer_copy(struct))


def raw_date(fn, handle):
    # The bindings convert dates with datetime(), which fails on the nonsense dates some
    # files store. Read the C struct instead.
    fn.restype = Date
    d = fn(handle)
    return [d.year, d.month, d.day, d.hour, d.minute, d.second]


NOT_EXPOSED = {"__error__": "not exposed by ZenKit's C API"}


def hierarchy(h):
    return fields(
        nodes=lambda: [
            {"name": latin1(n.name), "parent": n.parent, "transform": floats(n.transform, 16)} for n in h.nodes
        ],
        bbox=lambda: aabb(h.bbox),
        collision_bbox=lambda: aabb(h.collision_bbox),
        root_translation=lambda: v3(h.root_translation),
        checksum=lambda: h.checksum,
        source_date=lambda: raw_date(DLL.ZkModelHierarchy_getSourceDate, h._handle),
        source_path=lambda: latin1(h.source_path),
    )


def soft_skin(s):
    return fields(
        mesh=lambda: mrm(s.mesh),
        weights=lambda: [
            [{"weight": w.weight, "position": v3(w.position), "node": w.index} for w in ws] for ws in s.weights
        ],
        wedge_normals=lambda: [{"normal": v3(n.normal), "index": n.index} for n in s.wedge_normals],
        nodes=lambda: list(s.nodes),
        bboxes=lambda: [obb(b) for b in s.bboxes],
    )


def model_mesh(m):
    return fields(
        checksum=lambda: m.checksum,
        meshes=lambda: [soft_skin(s) for s in m.meshes],
        attachments=lambda: {latin1(name): mrm(a) for name, a in m.attachments.items()},
    )


def model(m):
    return fields(hierarchy=lambda: hierarchy(m.hierarchy), mesh=lambda: model_mesh(m.mesh))


def morph_animation(a):
    return fields(
        name=lambda: latin1(a.name),
        layer=lambda: a.layer,
        blend_in=lambda: a.blend_in,
        blend_out=lambda: a.blend_out,
        # MorphAnimation.duration wraps the stored milliseconds in timedelta(seconds=...).
        duration=lambda: DLL.ZkMorphAnimation_getDuration(a._handle),
        speed=lambda: a.speed,
        flags=lambda: a.flags,
        frame_count=lambda: a.frame_count,
        vertices=lambda: list(a.vertices),
        samples=lambda: [v3(v) for v in a.samples],
    )


def morph(m):
    return fields(
        name=lambda: latin1(m.name),
        mesh=lambda: mrm(m.mesh),
        morph_positions=lambda: [v3(v) for v in m.morph_positions],
        animations=lambda: [morph_animation(a) for a in m.animations],
        sources=lambda: [
            {"date": raw_date(DLL.ZkMorphSource_getFileDate, s._handle), "file_name": latin1(s.file)}
            for s in m.sources
        ],
    )


def animation(a):
    return fields(
        name=lambda: latin1(a.name),
        next=lambda: latin1(a.next),
        layer=lambda: a.layer,
        frame_count=lambda: a.frame_count,
        node_count=lambda: a.node_count,
        fps=lambda: a.fps,
        fps_source=lambda: a.fps_source,
        sample_position_min=lambda: NOT_EXPOSED,
        sample_position_scale=lambda: NOT_EXPOSED,
        bbox=lambda: aabb(a.bbox),
        checksum=lambda: a.checksum,
        source_path=lambda: latin1(a.source_path),
        source_date=lambda: raw_date(DLL.ZkModelAnimation_getSourceDate, a._handle),
        source_script=lambda: latin1(a.source_script),
        # The Python Quat lists its fields as w, x, y, z; the C struct is x, y, z, w.
        samples=lambda: [{"position": v3(s.position), "rotation": floats(s.rotation, 4)} for s in a.samples],
        events=lambda: NOT_EXPOSED,
        node_indices=lambda: list(a.node_indices),
    )


KINDS = {
    "mrm": (zenkit.MultiResolutionMesh, mrm),
    "msh": (zenkit.Mesh, mesh),
    "tex": (zenkit.Texture, texture),
    "mdh": (zenkit.ModelHierarchy, hierarchy),
    "mdm": (zenkit.ModelMesh, model_mesh),
    "mdl": (zenkit.Model, model),
    "mmb": (zenkit.MorphMesh, morph),
    "man": (zenkit.ModelAnimation, animation),
}


def collect(node, prefix, extension, out):
    for child in node.children:
        path = prefix + latin1(child.name)
        if child.is_dir():
            collect(child, path + "/", extension, out)
        elif path.lower().endswith(extension):
            out.append((path, child))


SLOW_FILE_SECONDS = 60


def report_slow(path):
    """Says which file is taking so long and where the main thread is stuck: the last frame is a
    ZenKit call if the C++ side hangs, or a loop here if a getter returned a nonsense count."""
    frame = sys._current_frames().get(threading.main_thread().ident)
    print(f"\n{path} has been running for over {SLOW_FILE_SECONDS} s, currently at:", file=sys.stderr)
    traceback.print_stack(frame, file=sys.stderr)
    sys.stderr.flush()


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
            watchdog = threading.Timer(SLOW_FILE_SECONDS, report_slow, (path,))
            watchdog.daemon = True
            watchdog.start()

            asset = cls.load(node)
            if asset._handle.value is None:
                entry = {"path": path, "error": "ZenKit failed to load the file"}
            else:
                entry = {"path": path, "result": safe(lambda: dump(asset))}

            watchdog.cancel()
            out.write(json.dumps(entry) + "\n")
            if (i + 1) % 250 == 0:
                print(f"  {i + 1}/{len(files)}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    sys.exit(main())
