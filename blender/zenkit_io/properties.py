"""What the extension keeps for the game besides geometry and motion, as settings Blender draws
and saves with the .blend:

- per scene, where and for which game exports go;
- per armature, the model it is: its name, which its files are named after, and its script;
- per action, the animation it is: its name in the game, layer, blending, flags and frame rate;
- per mesh, the morph animations its shape keys make up.
"""

from pathlib import Path

import bpy

from . import preferences

FLAGS = [
    ("M", "Move", "The animation moves the model through the world"),
    ("R", "Rotate", "The animation turns the model"),
    ("E", "Queue", "Plays once the animation on its layer ends, instead of replacing it"),
    ("F", "Fly", "The model stays off the ground while it plays"),
    ("I", "Idle", "An idle animation"),
]


def flags_text(flags):
    """The script's spelling of a set of flags: their letters, or a dot for none."""
    return "".join(letter for letter, _, _ in FLAGS if letter in flags) or "."


def flags_of(text):
    return {c for c in text.upper() if c in {letter for letter, _, _ in FLAGS}}


class SceneSettings(bpy.types.PropertyGroup):
    folder: bpy.props.StringProperty(
        name="Folder",
        subtype="DIR_PATH",
        description="Where exports go; the models folder of the preferences if empty",
    )
    game: bpy.props.EnumProperty(
        name="Game",
        items=[
            ("AUTO", "From Preferences", "The game the extension's preferences name"),
            ("G1", "Gothic", ""),
            ("G2", "Gothic II", ""),
        ],
        default="AUTO",
    )
    scale: bpy.props.FloatProperty(
        name="Scale", default=0.01, min=1e-6, description="Metres per engine unit"
    )

    def export_folder(self):
        """The folder exports go to, or `None` if none is set."""
        if self.folder:
            return Path(bpy.path.abspath(self.folder))
        return preferences.folder("anims")

    def game_version(self):
        return preferences.game_version() if self.game == "AUTO" else self.game


class ModelSettings(bpy.types.PropertyGroup):
    model: bpy.props.StringProperty(
        name="Model",
        description="The model's name, which its script and animations are named after: "
        "HUMANS.MSB, HUMANS-S_RUN.MAN",
    )
    script: bpy.props.PointerProperty(
        type=bpy.types.Text,
        name="Script",
        description="The text holding the model's script, as source",
    )
    export_skeleton: bpy.props.BoolProperty(name="Skeleton", default=True)
    export_meshes: bpy.props.BoolProperty(name="Meshes", default=True)
    export_script: bpy.props.BoolProperty(name="Script", default=True)
    export_source: bpy.props.BoolProperty(
        name="Also as Source",
        description="Write the script as source (.MDS) next to the compiled one (.MSB)",
        default=False,
    )
    export_animations: bpy.props.BoolProperty(name="Animations", default=True)


class AnimationSettings(bpy.types.PropertyGroup):
    model: bpy.props.StringProperty(name="Model", description="The model the animation belongs to")
    name: bpy.props.StringProperty(
        name="Name", description="The animation's name in the game, like S_RUN or T_JUMP"
    )
    layer: bpy.props.IntProperty(
        name="Layer",
        description="Animations on different layers play at once, the higher on top",
        default=1,
        min=1,
        max=1000,
    )
    next: bpy.props.StringProperty(
        name="Next", description="The animation played when this one ends; empty for none"
    )
    blend_in: bpy.props.FloatProperty(
        name="Blend In",
        description="How long the animation takes to blend in, in seconds",
        precision=2,
    )
    blend_out: bpy.props.FloatProperty(
        name="Blend Out",
        description="How long the animation takes to blend out, in seconds",
        precision=2,
    )
    flags: bpy.props.EnumProperty(name="Flags", items=FLAGS, options={"ENUM_FLAG"})
    fps: bpy.props.FloatProperty(
        name="Frame Rate",
        description="The frames per second the game plays the animation at",
        default=25.0,
        min=1.0,
        max=1000.0,
    )
    checksum: bpy.props.StringProperty(options={"HIDDEN"})
    events: bpy.props.StringProperty(options={"HIDDEN"})

    def game_name(self, action):
        return (self.name or action.name).upper()


def morph_renamed(settings, context):
    from . import morph

    if settings.keys_name and settings.keys_name != settings.name:
        morph.rename(settings.id_data, settings, settings.keys_name)
    settings.keys_name = settings.name


class MorphAnimation(bpy.types.PropertyGroup):
    name: bpy.props.StringProperty(
        name="Name",
        description="The animation's name, which its shape keys are named after",
        update=morph_renamed,
    )
    # The name its shape keys have, to rename them when the animation is.
    keys_name: bpy.props.StringProperty(options={"HIDDEN"})
    layer: bpy.props.IntProperty(
        name="Layer",
        description="Animations on different layers play at once",
        default=1,
        min=0,
        max=1000,
    )
    fps: bpy.props.FloatProperty(
        name="Frame Rate", description="Frames per second", default=25.0, min=0.001, max=1000.0
    )
    until_stopped: bpy.props.BoolProperty(
        name="Until Stopped",
        description="Plays until the game stops it, rather than for a duration",
        default=True,
    )
    duration: bpy.props.FloatProperty(
        name="Duration", description="How long it plays", default=1.0, min=0.0, unit="TIME_ABSOLUTE"
    )
    repeat: bpy.props.BoolProperty(
        name="Repeat",
        description="Plays the frames at the frame rate over and over, rather than once spread "
        "over the duration",
        default=False,
    )
    blend_in: bpy.props.FloatProperty(name="Blend In", default=0.01, precision=3)
    blend_out: bpy.props.FloatProperty(name="Blend Out", default=-0.01, precision=3)
    # The flags beside Repeat, which the files carry and nothing is known to read.
    other_flags: bpy.props.IntProperty(options={"HIDDEN"})
    # The action playing the animation on the shape keys, to watch it.
    action: bpy.props.PointerProperty(type=bpy.types.Action, options={"HIDDEN"})


def show_morph_animation(settings, context):
    from . import morph

    morph.show(context, context.object)


def show_morph_frame(settings, context):
    from . import morph

    morph.show_frame(context, context.object)


class MorphSettings(bpy.types.PropertyGroup):
    animations: bpy.props.CollectionProperty(type=MorphAnimation)
    index: bpy.props.IntProperty(update=show_morph_animation)
    frame: bpy.props.IntProperty(
        name="Frame",
        description="The frame of the selected animation the mesh shows",
        min=0,
        update=show_morph_frame,
    )


def get_animation_index(obj):
    anim = obj.animation_data
    if anim is None or anim.action is None:
        return -1
    return bpy.data.actions.find(anim.action.name)


def set_animation_index(obj, index):
    from . import animation

    if 0 <= index < len(bpy.data.actions):
        animation.show(bpy.context, bpy.data.actions[index], obj)


CLASSES = (
    SceneSettings,
    ModelSettings,
    AnimationSettings,
    MorphAnimation,
    MorphSettings,
)


def register():
    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.Scene.zenkit = bpy.props.PointerProperty(type=SceneSettings)
    bpy.types.Armature.zenkit = bpy.props.PointerProperty(type=ModelSettings)
    bpy.types.Action.zenkit = bpy.props.PointerProperty(type=AnimationSettings)
    bpy.types.Mesh.zenkit_morph = bpy.props.PointerProperty(type=MorphSettings)
    # The row of the model's animation list that is selected: the action the armature plays, so
    # picking one plays it.
    bpy.types.Object.zenkit_animation_index = bpy.props.IntProperty(
        get=get_animation_index, set=set_animation_index
    )


def unregister():
    del bpy.types.Object.zenkit_animation_index
    del bpy.types.Mesh.zenkit_morph
    del bpy.types.Action.zenkit
    del bpy.types.Armature.zenkit
    del bpy.types.Scene.zenkit
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
