"""Import and export of Gothic textures, meshes, models, animations and model scripts, through the
zenkit-rs library."""

import bpy

from . import animation
from . import export
from . import mesh
from . import model
from . import morph
from . import preferences
from . import properties
from . import script
from . import scripttext
from . import texture
from . import ui

CLASSES = (
    preferences.Preferences,
    texture.ImportTexture,
    texture.ExportTexture,
    mesh.ImportMesh,
    mesh.ExportMsh,
    mesh.ExportMrm,
    morph.ExportMmb,
    model.ImportModel,
    model.ExportModel,
    animation.ImportAnimation,
    animation.ExportAnimation,
    script.ImportScript,
    export.ExportScript,
    *ui.CLASSES,
)


def menu_import(self, context):
    self.layout.operator(texture.ImportTexture.bl_idname, text="Gothic Texture (.tex)")
    self.layout.operator(mesh.ImportMesh.bl_idname, text="Gothic Mesh (.msh, .mrm, .mmb)")
    self.layout.operator(model.ImportModel.bl_idname, text="Gothic Model (.mdl, .mdh, .mdm)")
    self.layout.operator(animation.ImportAnimation.bl_idname, text="Gothic Animation (.man)")
    self.layout.operator(script.ImportScript.bl_idname, text="Gothic Model Script (.mds, .msb)")


def menu_export(self, context):
    self.layout.operator(texture.ExportTexture.bl_idname, text="Gothic Texture (.tex)")
    self.layout.operator(mesh.ExportMsh.bl_idname, text="Gothic Static Mesh (.msh)")
    self.layout.operator(mesh.ExportMrm.bl_idname, text="Gothic Multi-Resolution Mesh (.mrm)")
    self.layout.operator(morph.ExportMmb.bl_idname, text="Gothic Morph Mesh (.mmb)")
    self.layout.operator(model.ExportModel.bl_idname, text="Gothic Model (.mdl, .mdh, .mdm)")
    self.layout.operator(animation.ExportAnimation.bl_idname, text="Gothic Animation (.man)")
    self.layout.operator(export.ExportScript.bl_idname, text="Gothic Model Script (.msb, .mds)")


def menu_image(self, context):
    self.layout.operator(texture.ExportTexture.bl_idname, text="Export as Gothic Texture")


def register():
    properties.register()
    scripttext.register()
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
    scripttext.unregister()
    properties.unregister()
