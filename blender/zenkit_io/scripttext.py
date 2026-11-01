"""Keeping a model's script text and its animations' settings the same, both ways.

The script text is where an animation's script settings live, once it has an `ani` there: its
layer, next animation, blending, flags and frame rate. The Animations panel shows them through
the action's settings, as a view of that line:

- changing one in the panel rewrites that animation's `ani` line in the text, and only that line,
  so the rest of the text stays as written, comments and layout included;
- changing the text changes the panel: a timer looks at the scripts twice a second and copies
  what each `ani` says into its action's settings.

An animation without an `ani` keeps its settings on its action until it is added to the script,
with the Add button or by exporting, and a model without a script keeps them on its actions,
where exporting a `.MAN` takes its name, layer, next animation and frame rate from.

An action is bound to the `ani` of its name in the game; renaming it in the panel renames the
`ani` too.
"""

import re
from contextlib import contextmanager

import bpy
import zenkit_py

from . import animation
from . import properties

ANI = re.compile(r'^(?P<indent>[ \t]*)ani\s*\(\s*"(?P<name>[^"]*)"', re.IGNORECASE | re.MULTILINE)
ANI_ENUM = re.compile(r"\baniEnum\b", re.IGNORECASE)

# Set while settings are copied from a text, so that their update callbacks write nothing back.
_applying = False
# The texts as last looked at, by name, to tell when one changed.
_seen = {}
_parsed = {}


@contextmanager
def quiet():
    """Changes to animation settings made inside write nothing into scripts."""
    global _applying
    before, _applying = _applying, True
    try:
        yield
    finally:
        _applying = before


def parse(text):
    """The script dict of a text, or `(None, error)` if it is no script; parsed again only when
    the text changed, as panels ask on every redraw."""
    source = text.as_string()
    key = (text.name, hash(source))
    if key not in _parsed:
        if len(_parsed) > 16:
            _parsed.clear()
        try:
            _parsed[key] = (zenkit_py.parse_script(source), None)
        except zenkit_py.ZenKitError as error:
            _parsed[key] = (None, str(error))
    return _parsed[key]


def bound_name(action):
    """The name of the `ani` an action is bound to."""
    return (action.zenkit.script_name or action.zenkit.game_name(action)).upper()


def find(source, name):
    """`(start, end)` of the `ani (…)` statement of this name in a script's source, from its
    keyword to its closing parenthesis, or the end of its line without one. `None` if it has
    none."""
    for match in ANI.finditer(source):
        if match["name"].upper() != name.upper():
            continue
        line_end = source.find("\n", match.end())
        line_end = len(source) if line_end < 0 else line_end
        close = source.find(")", match.end(), line_end)
        return match.end("indent"), close + 1 if close >= 0 else line_end
    return None


def line_of(text, action):
    """The line of a text where an action's `ani` is, from 0, or `None`."""
    source = text.as_string()
    span = find(source, bound_name(action))
    return source.count("\n", 0, span[0]) if span else None


def ani_line(action, ani):
    """The `ani` statement of an action's settings, keeping what only the script says (source
    file, direction, frames, speed) from `ani`, its script dict, if it has one."""
    settings = action.zenkit
    start, end = animation.frame_range(action)
    old = ani or {}
    return zenkit_py.format_animation(
        {
            "name": settings.game_name(action),
            "layer": settings.layer,
            "next": settings.next.upper(),
            "blend_in": settings.blend_in,
            "blend_out": settings.blend_out,
            "flags": properties.flags_text(settings.flags),
            "flags_text": old.get("flags", ""),
            "model": old.get("model", f"{settings.game_name(action)}.ASC"),
            "direction": old.get("direction", "F"),
            "first_frame": old.get("first_frame", 0),
            "last_frame": old.get("last_frame", end - start),
            "fps": settings.fps,
            "speed": old.get("speed", 0.0),
            "collision_volume_scale": old.get("collision_volume_scale", 1.0),
        }
    )


def enum_end(source):
    """Where the `aniEnum` block's closing brace is, or `None`. Braces in strings and comments
    do not count."""
    match = ANI_ENUM.search(source)
    if match is None:
        return None
    depth, at, quoted = 0, source.find("{", match.end()), False
    while 0 <= at < len(source):
        c = source[at]
        if c == '"':
            quoted = not quoted
        elif not quoted and source.startswith("//", at):
            at = source.find("\n", at)
            continue
        elif not quoted and c == "{":
            depth += 1
        elif not quoted and c == "}":
            depth -= 1
            if depth == 0:
                return at
        at += 1
    return None


def set_text(text, source):
    """Replaces a text's source, keeping its cursor where it was."""
    line, column = text.current_line_index, text.current_character
    text.from_string(source)
    text.cursor_set(min(line, len(text.lines) - 1), character=column)
    _seen[text.name] = hash(source)


def write_back(text, action):
    """Rewrites the `ani` an action is bound to with its settings. Gives whether it had one."""
    info, _ = parse(text)
    source = text.as_string()
    span = find(source, bound_name(action))
    if info is None or span is None:
        return False
    ani = next((a for a in info["animations"] if a["name"].upper() == bound_name(action)), None)
    set_text(text, source[: span[0]] + ani_line(action, ani) + source[span[1] :])
    action.zenkit.script_name = action.zenkit.game_name(action)
    return True


def add(text, actions):
    """Adds an `ani` for each of `actions` that has none, at the end of the `aniEnum` block, and
    binds them to it. Gives the actions added."""
    source = text.as_string()
    added = [a for a in actions if find(source, bound_name(a)) is None]
    if not added:
        return []
    end = enum_end(source)
    if end is None:
        raise ValueError(f"{text.name} has no aniEnum block to add animations to")
    indents = [m["indent"] for m in ANI.finditer(source)]
    indent = indents[-1] if indents else "\t\t"
    line_start = source.rfind("\n", 0, end) + 1
    lines = "".join(f"{indent}{ani_line(a, None)}\n" for a in added)
    if source[line_start:end].strip():
        lines = "\n" + lines
        line_start = end
    set_text(text, source[:line_start] + lines + source[line_start:])
    for action in added:
        action.zenkit.script_name = action.zenkit.game_name(action)
    return added


def apply(armature, force=False):
    """Copies what an armature's script says of each animation into its action's settings, if
    the text changed since last time, or always if `force`."""
    text = armature.data.zenkit.script
    if text is None:
        return
    source = text.as_string()
    if not force and _seen.get(text.name) == hash(source):
        return
    _seen[text.name] = hash(source)
    info, _ = parse(text)
    if info is None:
        return
    anis = {a["name"].upper(): a for a in info["animations"]}
    with quiet():
        for action in animation.actions_of(armature):
            ani = anis.get(bound_name(action))
            if ani is None:
                continue
            settings = action.zenkit
            values = {
                "layer": max(ani["layer"], 1),
                "next": ani["next"],
                "blend_in": ani["blend_in"],
                "blend_out": ani["blend_out"],
                "flags": properties.flags_of(ani["flags"]),
                "fps": ani["fps"],
            }
            for key, value in values.items():
                current = getattr(settings, key)
                same = abs(current - value) < 1e-6 if isinstance(value, float) else current == value
                if not same:
                    setattr(settings, key, value)


def scripted_armatures(model):
    return [
        o
        for o in bpy.data.objects
        if o.type == "ARMATURE"
        and o.data.zenkit.script is not None
        and animation.model_name(o) == model.upper()
    ]


def changed(settings, context):
    """The update callback of an animation's settings: writes them into its `ani`."""
    if _applying:
        return
    action = settings.id_data
    for armature in scripted_armatures(settings.model):
        write_back(armature.data.zenkit.script, action)


def watch():
    """The timer that follows scripts edited in the Text Editor."""
    for obj in bpy.data.objects:
        if obj.type == "ARMATURE" and obj.data.zenkit.script is not None:
            apply(obj)
    return 0.5


def register():
    if not bpy.app.timers.is_registered(watch):
        bpy.app.timers.register(watch, first_interval=0.5, persistent=True)


def unregister():
    if bpy.app.timers.is_registered(watch):
        bpy.app.timers.unregister(watch)
