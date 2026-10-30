"""Gothic textures (.TEX) to and from Blender images."""

from pathlib import Path

import bpy
import numpy as np
import zenkit_py


def load(path):
    """Reads a texture into a new Blender image named after the file, packed into the .blend."""
    width, height, rgba = zenkit_py.read_texture(str(path))
    image = bpy.data.images.new(Path(path).stem, width, height, alpha=True)
    # Blender stores rows bottom to top, as floats.
    pixels = np.frombuffer(rgba, np.uint8).reshape(height, width, 4)[::-1]
    image.pixels.foreach_set((pixels.astype(np.float32) / 255.0).ravel())
    image.pack()
    return image


def save(image, path, format, mipmaps):
    width, height = image.size
    pixels = np.empty(width * height * 4, np.float32)
    image.pixels.foreach_get(pixels)
    pixels = pixels.reshape(height, width, 4)[::-1]
    rgba = (np.clip(pixels, 0.0, 1.0) * 255.0 + 0.5).astype(np.uint8)
    zenkit_py.write_texture(str(path), width, height, rgba.tobytes(), format, mipmaps)


class ImportTexture(bpy.types.Operator):
    """Import a Gothic texture as an image"""

    bl_idname = "import_image.zenkit_texture"
    bl_label = "Import Gothic Texture"
    bl_options = {"REGISTER", "UNDO"}

    filepath: bpy.props.StringProperty(subtype="FILE_PATH")
    filter_glob: bpy.props.StringProperty(default="*.tex", options={"HIDDEN"})

    def invoke(self, context, event):
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def execute(self, context):
        try:
            image = load(self.filepath)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Imported {image.name}, {image.size[0]}x{image.size[1]}")
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
        items=[
            ("DXT1", "DXT1", "Compressed, without transparency"),
            ("DXT3", "DXT3", "Compressed, with sharp transparency"),
            ("DXT5", "DXT5", "Compressed, with smooth transparency"),
            ("B8G8R8A8", "BGRA8", "Uncompressed"),
        ],
        default="DXT1",
    )
    mipmaps: bpy.props.BoolProperty(name="Mipmaps", default=True)

    def invoke(self, context, event):
        space = context.space_data
        active = getattr(space, "image", None) if space else None
        if active is None and bpy.data.images:
            active = bpy.data.images[0]
        if active is not None:
            self.image = active.name
            self.filepath = active.name.rsplit(".", 1)[0].upper() + "-C.TEX"
        context.window_manager.fileselect_add(self)
        return {"RUNNING_MODAL"}

    def draw(self, context):
        self.layout.prop_search(self, "image", bpy.data, "images")
        self.layout.prop(self, "format")
        self.layout.prop(self, "mipmaps")

    def execute(self, context):
        image = bpy.data.images.get(self.image)
        if image is None:
            self.report({"ERROR"}, "Choose an image to export")
            return {"CANCELLED"}
        try:
            save(image, self.filepath, self.format, self.mipmaps)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Exported {image.name}")
        return {"FINISHED"}
