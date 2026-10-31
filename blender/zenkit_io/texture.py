"""Gothic textures (.TEX) to and from Blender images."""

import struct
import tempfile
from pathlib import Path

import bpy
import numpy as np
import zenkit_py

from . import preferences


# Set on images made from textures: whether any pixel is not fully opaque.
ALPHA = "zenkit_alpha"


def tga(width, height, rgba):
    """RGBA8 pixels, rows top to bottom, as an uncompressed 32-bit TGA file."""
    # Image type 2 is uncompressed true color; descriptor 0x28 is 8 alpha bits, rows top first.
    header = struct.pack("<BBBHHBHHHHBB", 0, 0, 2, 0, 0, 0, 0, 0, width, height, 32, 0x28)
    pixels = np.frombuffer(rgba, np.uint8).reshape(-1, 4)[:, [2, 1, 0, 3]]
    return header + pixels.tobytes()


def image(name, width, height, rgba):
    """A new Blender image of RGBA8 pixels, rows top to bottom, packed into the .blend.

    Packing an image Blender made itself encodes it as PNG, which takes most of the time an
    import of a large texture does. So the pixels are written as an uncompressed TGA instead,
    which takes none, and Blender packs that file as it is: a little larger, many times faster.
    """
    with tempfile.TemporaryDirectory() as folder:
        path = Path(folder) / "texture.tga"
        path.write_bytes(tga(width, height, rgba))
        result = bpy.data.images.load(str(path))
        result.pack()
    result.name = name
    result.filepath_raw = name + ".tga"
    alpha = np.frombuffer(rgba, np.uint8)[3::4]
    result[ALPHA] = bool((alpha < 255).any())
    return result


def load(path):
    """Reads a texture file into a new image named after the file."""
    return image(Path(path).stem, *zenkit_py.read_texture(str(path)))


def compiled_name(name):
    """The file the engine compiles a texture to: `STONE-C.TEX` for `STONE.TGA`."""
    return Path(name).stem.upper() + "-C.TEX"


def find(name, folders=()):
    """The image of the texture a material names, like `STONE.TGA`.

    An image already in the .blend is reused. Otherwise the compiled texture is looked for in
    `folders`, then in the textures folder of the preferences, then in the game's archives.
    `None` if it is nowhere.
    """
    compiled = compiled_name(name)
    stem = Path(compiled).stem
    existing = bpy.data.images.get(stem)
    if existing is not None:
        return existing

    textures = preferences.folder("textures")
    for folder in [*folders, *([textures] if textures else [])]:
        path = preferences.resolve(folder, (compiled,))
        if path is not None and path.is_file():
            try:
                return load(path)
            except zenkit_py.ZenKitError:
                return None

    archives = preferences.archives()
    found = archives.read_texture(compiled) if archives is not None else None
    return image(stem, *found) if found is not None else None


def save(source, path, format, mipmaps, quality="HIGH"):
    width, height = source.size
    pixels = np.empty(width * height * 4, np.float32)
    source.pixels.foreach_get(pixels)
    pixels = pixels.reshape(height, width, 4)[::-1]
    rgba = (np.clip(pixels, 0.0, 1.0) * 255.0 + 0.5).astype(np.uint8)
    zenkit_py.write_texture(str(path), width, height, rgba.tobytes(), format, mipmaps, quality)


class ImportTexture(bpy.types.Operator):
    """Import a Gothic texture as an image"""

    bl_idname = "import_image.zenkit_texture"
    bl_label = "Import Gothic Texture"
    bl_options = {"REGISTER", "UNDO"}

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.tex", options={"HIDDEN"})

    def invoke(self, context, event):
        preferences.browse_from(self, "textures")
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        try:
            result = load(self.filepath)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Imported {result.name}, {result.size[0]}x{result.size[1]}")
        return {"FINISHED"}


class ExportTexture(bpy.types.Operator):
    """Export an image as a Gothic texture"""

    bl_idname = "export_image.zenkit_texture"
    bl_label = "Export Gothic Texture"

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.tex", options={"HIDDEN"})
    image: bpy.props.StringProperty(name="Image")
    format: bpy.props.EnumProperty(
        name="Format",
        # The two the game's renderer shows: it refuses DXT5 and draws uncompressed 32-bit
        # textures with nothing to convert them into its 16-bit fallback.
        items=[
            ("DXT1", "DXT1", "Compressed; transparency only fully on or off"),
            ("DXT3", "DXT3", "Compressed, with transparency in 16 steps"),
        ],
        default="DXT1",
    )
    mipmaps: bpy.props.BoolProperty(name="Mipmaps", default=True)
    quality: bpy.props.EnumProperty(
        name="Quality",
        description="How hard DXT compression tries to keep the picture as it was",
        items=[
            ("HIGH", "High", "Best picture; slow on large, detailed images"),
            ("FAST", "Fast", "Many times faster, with somewhat blotchier detail"),
        ],
        default="HIGH",
    )

    def invoke(self, context, event):
        space = context.space_data
        active = getattr(space, "image", None) if space else None
        if active is None and bpy.data.images:
            active = bpy.data.images[0]
        if active is not None:
            self.image = active.name
            name = compiled_name(active.name.removesuffix("-C"))
            start = preferences.folder("textures")
            self.filepath = str(start / name) if start else name
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def draw(self, context):
        self.layout.prop_search(self, "image", bpy.data, "images")
        self.layout.prop(self, "format")
        if self.format.startswith("DXT"):
            self.layout.prop(self, "quality")
        self.layout.prop(self, "mipmaps")

    def execute(self, context):
        source = bpy.data.images.get(self.image)
        if source is None:
            self.report({"ERROR"}, "Choose an image to export")
            return {"CANCELLED"}
        try:
            save(source, self.filepath, self.format, self.mipmaps, self.quality)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        width, height = source.size
        if width & (width - 1) or height & (height - 1):
            self.report(
                {"WARNING"},
                f"Exported {source.name}, but {width}x{height} are not powers of two, as the "
                "game's own textures are; its renderer may not show it",
            )
            return {"FINISHED"}
        self.report({"INFO"}, f"Exported {source.name}")
        return {"FINISHED"}
