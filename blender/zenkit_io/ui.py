"""The Gothic tab of the 3D viewport's sidebar, where models, their animations and morph meshes
are set up and exported without file browsers.

- Model: the selected armature's model name and script.
- Animations: the model's animations; picking one plays it, and its settings are below the list.
- Export: where and for which game, the files the model will go out as, and the button that
  writes them all.
- Morph Mesh: the selected mesh's morph animations, each a run of shape keys, and its export.
"""

import fnmatch

import bpy
import zenkit_py

from . import animation
from . import export
from . import morph
from .model import armature_checksum
from .model import armature_of


def export_folder(operator, context):
    """The folder exports go to, or `None` after reporting that none is set."""
    folder = context.scene.zenkit.export_folder()
    if folder is None or not folder.is_dir():
        operator.report({"ERROR"}, "Set an existing export folder in the Export panel")
        return None
    return folder


def selected_action(armature):
    """The animation of the model the armature plays, which its list shows selected."""
    anim = armature.animation_data
    action = anim.action if anim is not None else None
    model = animation.model_name(armature)
    return action if action is not None and action.zenkit.model.upper() == model else None


class ZENKIT_UL_animations(bpy.types.UIList):
    """The actions of the armature's model."""

    def draw_item(self, context, layout, data, item, icon, active_data, active_propname, index):
        start, end = animation.frame_range(item)
        row = layout.row(align=True)
        row.label(text=item.zenkit.game_name(item), icon="ACTION")
        row.label(text=f"{end - start + 1} fr")

    def filter_items(self, context, data, propname):
        armature = armature_of(context.active_object)
        model = animation.model_name(armature) if armature else None
        pattern = f"*{self.filter_name.upper()}*" if self.filter_name else "*"
        flags = [
            self.bitflag_filter_item
            if a.zenkit.model.upper() == model
            and fnmatch.fnmatchcase(a.zenkit.game_name(a), pattern)
            else 0
            for a in getattr(data, propname)
        ]
        return flags, []


class ZENKIT_UL_morph(bpy.types.UIList):
    """The morph animations of a mesh."""

    def draw_item(self, context, layout, data, item, icon, active_data, active_propname, index):
        frames = len(morph.animation_keys(context.active_object).get(item.name, []))
        row = layout.row(align=True)
        row.label(text=item.name, icon="SHAPEKEY_DATA")
        row.label(text=f"{frames} fr")


class Sidebar:
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Gothic"


class ZENKIT_PT_model(Sidebar, bpy.types.Panel):
    bl_label = "Model"

    @classmethod
    def poll(cls, context):
        return armature_of(context.active_object) is not None

    def draw(self, context):
        armature = armature_of(context.active_object)
        settings = armature.data.zenkit
        layout = self.layout
        layout.prop(settings, "model", placeholder=armature.name.upper())
        layout.template_ID(settings, "script", new="zenkit.new_script")
        if settings.script is not None:
            _, problem = export.script_info(settings.script)
            if problem:
                layout.label(text=problem, icon="ERROR")
        bones = len(armature.data.bones)
        layout.label(text=f"{bones} bones, checksum {armature_checksum(armature)}")


class ZENKIT_PT_animations(Sidebar, bpy.types.Panel):
    bl_label = "Animations"
    bl_parent_id = "ZENKIT_PT_model"

    def draw(self, context):
        armature = armature_of(context.active_object)
        layout = self.layout
        row = layout.row()
        row.template_list(
            "ZENKIT_UL_animations",
            "",
            bpy.data,
            "actions",
            armature,
            "zenkit_animation_index",
            rows=6,
        )
        column = row.column(align=True)
        column.operator("zenkit.new_animation", icon="ADD", text="")
        column.operator("zenkit.remove_animation", icon="REMOVE", text="")
        column.separator()
        column.operator("import_anim.zenkit_animation", icon="IMPORT", text="")
        column.operator("zenkit.export_animation", icon="EXPORT", text="")

        action = selected_action(armature)
        if action is None:
            layout.label(text="Pick an animation, or add one with +")
            return
        settings = action.zenkit
        column = layout.column()
        column.prop(settings, "name", placeholder=action.name.upper())
        column.prop(settings, "layer")
        column.prop(settings, "next")
        row = column.row(align=True)
        row.prop(settings, "blend_in")
        row.prop(settings, "blend_out")
        column.prop(settings, "flags", expand=True)
        column.prop(settings, "fps")
        column.prop(action, "use_frame_range", text="Manual Frame Range")
        if action.use_frame_range:
            row = column.row(align=True)
            row.prop(action, "frame_start", text="Start")
            row.prop(action, "frame_end", text="End")
        start, end = animation.frame_range(action)
        model = animation.model_name(armature)
        column.label(text=f"Frames {start} to {end}, as {model}-{settings.game_name(action)}.MAN")


class ZENKIT_PT_export(Sidebar, bpy.types.Panel):
    bl_label = "Export"
    bl_parent_id = "ZENKIT_PT_model"

    def draw(self, context):
        armature = armature_of(context.active_object)
        settings = armature.data.zenkit
        scene = context.scene.zenkit
        layout = self.layout

        layout.prop(scene, "folder")
        if not scene.folder:
            folder = scene.export_folder()
            layout.label(text=f"To {folder}" if folder else "No folder set", icon="FILE_FOLDER")
        row = layout.row(align=True)
        row.prop(scene, "game", text="")
        row.prop(scene, "scale")

        files = export.plan(armature)
        box = layout.box()
        box.prop(settings, "export_skeleton", text=f"Skeleton: {files.skeleton}")

        box.prop(settings, "export_meshes", text="Meshes")
        for name, objects in files.meshes.items():
            if name != files.skeleton:
                box.label(text=f"{name}: {', '.join(o.name for o in objects)}", icon="MESH_DATA")
        if not files.meshes:
            box.label(text="No meshes on the armature")

        if files.script:
            row = box.row()
            row.prop(settings, "export_script", text=f"Script: {files.script}")
            row.prop(settings, "export_source", text=".MDS")
            if files.problem:
                box.label(text="The script has an error", icon="ERROR")
        else:
            box.label(text="No script: add one in the Model panel", icon="INFO")

        box.prop(settings, "export_animations", text=f"Animations: {len(files.animations)}")
        if files.unused:
            box.label(text=f"{len(files.unused)} in the script have no action", icon="INFO")

        layout.operator("zenkit.export_model", icon="EXPORT")


class ZENKIT_PT_morph(Sidebar, bpy.types.Panel):
    bl_label = "Morph Mesh"

    @classmethod
    def poll(cls, context):
        return context.active_object is not None and context.active_object.type == "MESH"

    def draw(self, context):
        obj = context.active_object
        settings = obj.data.zenkit_morph
        layout = self.layout
        row = layout.row()
        row.template_list("ZENKIT_UL_morph", "", settings, "animations", settings, "index", rows=4)
        column = row.column(align=True)
        column.operator("zenkit.new_morph_animation", icon="ADD", text="")
        column.operator("zenkit.remove_morph_animation", icon="REMOVE", text="")
        column.separator()
        column.operator("zenkit.add_morph_frame", icon="KEYFRAME", text="")

        selected = morph.active_animation(obj)
        if selected is not None:
            frames = len(morph.animation_keys(obj).get(selected.name, []))
            column = layout.column()
            column.prop(selected, "name")
            column.prop(settings, "frame", text=f"Frame (of {frames})")
            column.prop(selected, "layer")
            column.prop(selected, "fps")
            column.prop(selected, "until_stopped")
            if not selected.until_stopped:
                column.prop(selected, "duration")
            column.prop(selected, "repeat")
            row = column.row(align=True)
            row.prop(selected, "blend_in")
            row.prop(selected, "blend_out")
            layout.label(text="Edit the shown frame in Edit Mode", icon="INFO")

        known = {a.name for a in settings.animations}
        others = [name for name in morph.animation_keys(obj) if name not in known]
        if others:
            layout.label(text=f"Shape keys of no animation: {', '.join(others)}", icon="INFO")
        layout.operator("zenkit.export_morph", icon="EXPORT", text=f"Export {obj.name.upper()}.MMB")


class ZENKIT_OT_new_script(bpy.types.Operator):
    """Make a script for the model: its skeleton, the meshes on it by file, and its animations
    when exported"""

    bl_idname = "zenkit.new_script"
    bl_label = "New Script"
    bl_options = {"REGISTER", "UNDO"}

    @classmethod
    def poll(cls, context):
        return armature_of(context.active_object) is not None

    def execute(self, context):
        armature = armature_of(context.active_object)
        animation.adopt(armature)
        export.new_script(armature)
        return {"FINISHED"}


class ZENKIT_OT_new_animation(bpy.types.Operator):
    """Add an animation to the model, starting from the current pose"""

    bl_idname = "zenkit.new_animation"
    bl_label = "New Animation"
    bl_options = {"REGISTER", "UNDO"}

    name: bpy.props.StringProperty(name="Name", default="S_NEW")

    @classmethod
    def poll(cls, context):
        return armature_of(context.active_object) is not None

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self)

    def execute(self, context):
        armature = armature_of(context.active_object)
        animation.adopt(armature)
        action = bpy.data.actions.new(self.name.upper())
        action.zenkit.model = animation.model_name(armature)
        action.zenkit.name = self.name.upper()
        action.use_fake_user = True
        animation.play(action, armature)
        for bone in armature.pose.bones:
            bone.rotation_mode = "QUATERNION"
            bone.keyframe_insert("location", frame=0, group=bone.name)
            bone.keyframe_insert("rotation_quaternion", frame=0, group=bone.name)
        animation.show(context, action, armature)
        return {"FINISHED"}


class ZENKIT_OT_remove_animation(bpy.types.Operator):
    """Delete the selected animation"""

    bl_idname = "zenkit.remove_animation"
    bl_label = "Delete Animation"
    bl_options = {"REGISTER", "UNDO"}

    @classmethod
    def poll(cls, context):
        armature = armature_of(context.active_object)
        return armature is not None and selected_action(armature) is not None

    def invoke(self, context, event):
        return context.window_manager.invoke_confirm(self, event)

    def execute(self, context):
        armature = armature_of(context.active_object)
        bpy.data.actions.remove(selected_action(armature))
        return {"FINISHED"}


class ZENKIT_OT_export_animation(bpy.types.Operator):
    """Export the selected animation as a .MAN into the export folder"""

    bl_idname = "zenkit.export_animation"
    bl_label = "Export Animation"

    @classmethod
    def poll(cls, context):
        armature = armature_of(context.active_object)
        return armature is not None and selected_action(armature) is not None

    def execute(self, context):
        folder = export_folder(self, context)
        if folder is None:
            return {"CANCELLED"}
        armature = armature_of(context.active_object)
        action = selected_action(armature)
        scale = context.scene.zenkit.scale
        name = f"{animation.model_name(armature)}-{action.zenkit.game_name(action)}.MAN"
        try:
            data = animation.gather_animation(context, armature, action, scale)
            zenkit_py.write_animation(str(folder / name), data)
        except zenkit_py.ZenKitError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Exported {name}")
        return {"FINISHED"}


class ZENKIT_OT_export_model(bpy.types.Operator):
    """Export the model's ticked files into the export folder"""

    bl_idname = "zenkit.export_model"
    bl_label = "Export Model"

    @classmethod
    def poll(cls, context):
        return armature_of(context.active_object) is not None

    def execute(self, context):
        folder = export_folder(self, context)
        if folder is None:
            return {"CANCELLED"}
        armature = armature_of(context.active_object)
        scene = context.scene.zenkit
        animation.adopt(armature)
        try:
            written = export.export_model(
                context, armature, folder, scene.game_version(), scene.scale
            )
        except (ValueError, zenkit_py.ZenKitError) as error:
            self.report({"ERROR"}, f"Cannot export {armature.name}: {error}")
            return {"CANCELLED"}
        if not written:
            self.report({"WARNING"}, "Nothing is ticked to export")
            return {"CANCELLED"}
        self.report({"INFO"}, f"Exported {len(written)} files to {folder}")
        return {"FINISHED"}


class ZENKIT_OT_new_morph_animation(bpy.types.Operator):
    """Add a morph animation of one frame, a shape key in the mesh's rest shape"""

    bl_idname = "zenkit.new_morph_animation"
    bl_label = "New Morph Animation"
    bl_options = {"REGISTER", "UNDO"}

    name: bpy.props.StringProperty(name="Name", default="S_NEW")

    @classmethod
    def poll(cls, context):
        return context.active_object is not None and context.active_object.type == "MESH"

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self)

    def execute(self, context):
        obj = context.active_object
        if self.name.upper() in {a.name for a in obj.data.zenkit_morph.animations}:
            self.report({"ERROR"}, f"There is an animation named {self.name.upper()} already")
            return {"CANCELLED"}
        morph.new_animation(obj, self.name.upper())
        return {"FINISHED"}


class MorphOperator:
    @classmethod
    def poll(cls, context):
        obj = context.active_object
        return obj is not None and obj.type == "MESH" and morph.active_animation(obj) is not None


class ZENKIT_OT_add_morph_frame(MorphOperator, bpy.types.Operator):
    """Add a frame to the selected morph animation, a copy of its last one"""

    bl_idname = "zenkit.add_morph_frame"
    bl_label = "Add Frame"
    bl_options = {"REGISTER", "UNDO"}

    def execute(self, context):
        morph.add_frame(context.active_object)
        return {"FINISHED"}


class ZENKIT_OT_remove_morph_animation(MorphOperator, bpy.types.Operator):
    """Delete the selected morph animation and its shape keys"""

    bl_idname = "zenkit.remove_morph_animation"
    bl_label = "Delete Morph Animation"
    bl_options = {"REGISTER", "UNDO"}

    def invoke(self, context, event):
        return context.window_manager.invoke_confirm(self, event)

    def execute(self, context):
        morph.remove_animation(context.active_object)
        return {"FINISHED"}


class ZENKIT_OT_export_morph(bpy.types.Operator):
    """Export the mesh and its morph animations as a .MMB into the export folder"""

    bl_idname = "zenkit.export_morph"
    bl_label = "Export Morph Mesh"

    @classmethod
    def poll(cls, context):
        return context.active_object is not None and context.active_object.type == "MESH"

    def execute(self, context):
        folder = export_folder(self, context)
        if folder is None:
            return {"CANCELLED"}
        obj = context.active_object
        scene = context.scene.zenkit
        name = obj.name.upper() + ".MMB"
        try:
            data = morph.gather_morph(obj, context.evaluated_depsgraph_get(), scene.scale)
            zenkit_py.write_morph(str(folder / name), data, scene.game_version())
        except (ValueError, zenkit_py.ZenKitError) as error:
            self.report({"ERROR"}, f"Cannot export {obj.name}: {error}")
            return {"CANCELLED"}
        self.report({"INFO"}, f"Exported {name}")
        return {"FINISHED"}


CLASSES = (
    ZENKIT_UL_animations,
    ZENKIT_UL_morph,
    ZENKIT_PT_model,
    ZENKIT_PT_animations,
    ZENKIT_PT_export,
    ZENKIT_PT_morph,
    ZENKIT_OT_new_script,
    ZENKIT_OT_new_animation,
    ZENKIT_OT_remove_animation,
    ZENKIT_OT_export_animation,
    ZENKIT_OT_export_model,
    ZENKIT_OT_new_morph_animation,
    ZENKIT_OT_add_morph_frame,
    ZENKIT_OT_remove_morph_animation,
    ZENKIT_OT_export_morph,
)
