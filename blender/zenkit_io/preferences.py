"""Where the game's files are: picked once in the extension's preferences.

Either the game folder is given and every other folder is worked out from it, or "Custom
folders" is ticked and each one is given on its own. Lookups of compiled textures search the
textures folder first and then, if allowed, the game's .VDF archives, where a plain install keeps
everything.
"""

from pathlib import Path

import bpy
import zenkit_py

# Where each folder lies in a game folder, and what it is for.
FOLDERS = {
    "textures": (("_work", "Data", "Textures", "_compiled"), "Compiled textures (.TEX)"),
    "meshes": (("_work", "Data", "Meshes", "_compiled"), "Compiled meshes (.MSH, .MRM)"),
    "anims": (("_work", "Data", "Anims", "_compiled"), "Compiled models (.MDL, .MDH, .MDM)"),
    "worlds": (("_work", "Data", "Worlds"), "Worlds (.ZEN)"),
    "archives": (("Data",), "The game's archives (.VDF)"),
}


def resolve(base, parts):
    """`base` joined with `parts`, matching each part in any case, as the game does on Windows.
    `None` if a part does not exist."""
    current = Path(base)
    for part in parts:
        if not current.is_dir():
            return None
        match = next((p for p in current.iterdir() if p.name.lower() == part.lower()), None)
        if match is None:
            return None
        current = match
    return current


def detect_game(folder):
    """Which game is installed in `folder`, by the executable it ships."""
    system = resolve(folder, ("System",))
    if system is not None:
        if resolve(system, ("Gothic2.exe",)) is not None:
            return "G2"
        if resolve(system, ("GothicMod.exe",)) is not None:
            return "G1"
    return None


class Preferences(bpy.types.AddonPreferences):
    bl_idname = __package__

    game_folder: bpy.props.StringProperty(
        name="Game Folder", subtype="DIR_PATH", description="Where Gothic or Gothic II is installed"
    )
    game: bpy.props.EnumProperty(
        name="Game",
        description="The game files are exported for",
        items=[
            ("AUTO", "From the Game Folder", "Gothic II unless the game folder holds Gothic"),
            ("G1", "Gothic", ""),
            ("G2", "Gothic II", ""),
        ],
        default="AUTO",
    )
    use_archives: bpy.props.BoolProperty(
        name="Search the Archives",
        description="Look for textures in the game's .VDF archives when they are not loose files",
        default=True,
    )
    use_custom: bpy.props.BoolProperty(
        name="Custom Folders", description="Pick every folder instead of working them out"
    )
    textures: bpy.props.StringProperty(name="Textures", subtype="DIR_PATH")
    meshes: bpy.props.StringProperty(name="Meshes", subtype="DIR_PATH")
    anims: bpy.props.StringProperty(name="Models", subtype="DIR_PATH")
    worlds: bpy.props.StringProperty(name="Worlds", subtype="DIR_PATH")
    archives: bpy.props.StringProperty(name="Archives", subtype="DIR_PATH")

    def draw(self, context):
        layout = self.layout
        layout.prop(self, "game_folder")
        layout.prop(self, "game")
        layout.prop(self, "use_archives")
        layout.prop(self, "use_custom")

        box = layout.box()
        for key, (_, description) in FOLDERS.items():
            if self.use_custom:
                box.prop(self, key)
                continue
            folder = self.folder(key)
            row = box.row()
            row.label(text=self.bl_rna.properties[key].name)
            row.label(
                text=str(folder) if folder else "Not found", icon="CHECKMARK" if folder else "ERROR"
            )
            row.label(text=description)

    def folder(self, key):
        """The folder for `key`, one of FOLDERS, or `None` if it is not set or does not exist."""
        if self.use_custom:
            path = Path(bpy.path.abspath(getattr(self, key))) if getattr(self, key) else None
            return path if path is not None and path.is_dir() else None
        if not self.game_folder:
            return None
        return resolve(bpy.path.abspath(self.game_folder), FOLDERS[key][0])

    def game_version(self):
        if self.game != "AUTO":
            return self.game
        return detect_game(bpy.path.abspath(self.game_folder)) if self.game_folder else None


class Unset:
    """What the preferences say while the extension is not enabled through Blender's add-on
    system, as when a script registers it: no folders."""

    use_archives = False

    def folder(self, key):
        return None

    def game_version(self):
        return None


def get():
    addon = bpy.context.preferences.addons.get(__package__)
    return addon.preferences if addon is not None else Unset()


def folder(key):
    return get().folder(key)


def game_version():
    return get().game_version() or "G2"


_archives = {}


def archives():
    """The game's archives, mounted once per folder, or `None` if they are not to be searched."""
    prefs = get()
    data = prefs.folder("archives") if prefs.use_archives else None
    if data is None:
        return None
    key = str(data)
    if key not in _archives:
        paths = sorted(str(p) for p in data.iterdir() if p.suffix.lower() == ".vdf")
        try:
            _archives[key] = zenkit_py.Archives(paths)
        except zenkit_py.ZenKitError:
            _archives[key] = None
    return _archives[key]


def browse_from(operator, key):
    """Opens an operator's file browser in the folder for `key`, unless it has a path already."""
    start = folder(key)
    if not operator.filepath and start is not None:
        operator.filepath = str(start) + "/"
