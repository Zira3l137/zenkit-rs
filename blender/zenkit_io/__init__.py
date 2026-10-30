"""Import and export of Gothic textures and meshes, through the zenkit-rs library."""

import bpy

from . import mesh
from . import texture

CLASSES = (
    texture.ImportTexture,
    texture.ExportTexture,
    mesh.ImportMesh,
    mesh.ExportMsh,
    mesh.ExportMrm,
)


def menu_import(self, context):
    self.layout.operator(texture.ImportTexture.bl_idname, text="Gothic Texture (.tex)")
    self.layout.operator(mesh.ImportMesh.bl_idname, text="Gothic Mesh (.msh, .mrm)")


def menu_export(self, context):
    self.layout.operator(texture.ExportTexture.bl_idname, text="Gothic Texture (.tex)")
    self.layout.operator(mesh.ExportMsh.bl_idname, text="Gothic Static Mesh (.msh)")
    self.layout.operator(mesh.ExportMrm.bl_idname, text="Gothic Multi-Resolution Mesh (.mrm)")


def menu_image(self, context):
    self.layout.operator(texture.ExportTexture.bl_idname, text="Export as Gothic Texture")


def register():
    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.TOPBAR_MT_file_import.append(menu_import)
    bpy.types.TOPBAR_MT_file_export.append(menu_export)
    bpy.types.IMAGE_MT_image.append(menu_image)


def unregister():
    bpy.types.IMAGE_MT_image.remove(menu_image)
    bpy.types.TOPBAR_MT_file_export.remove(menu_export)
    bpy.types.TOPBAR_MT_file_import.remove(menu_import)
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
