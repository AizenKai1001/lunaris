"""Write examples/ui_kit3.lunaris.json: one screen with the widgets of UI kit v3 (docs/UI.md), over a dark background with no 3D at all:

    * a scrolling list (`UiScroll` + `UiScrollBar`): twelve rows in a `UiLayout` column that is taller than the viewport, cut at it, with a rounded scroll bar;

    python -I tools/build_ui_kit3.py

Everything is a `Ui` rectangle with a `UiStyle` and the pixel font of the tests (examples/fonts/pixel.ttf, made by tools/make_test_font.py), so that text is in the order the rectangles are in (the
built-in bitmap font is always drawn on top of every rectangle, which a popup must not be). The GPU comparison test draws it with both renderers, and examples/tests/ui_kit3.test.json plays it with the
mouse: the `input` tool's mouse_x, mouse_y, mouse_left and mouse_wheel.
"""
import json
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
FONT = "examples/fonts/pixel.ttf"
SQUARE = 9 / 16  # a width that is as many pixels as a height on a 16:9 picture

entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


def r(v):
    return round(v, 5)


def label(name, text, x, y, size=0.04, color=(0.75, 0.8, 0.95, 1), **extra):
    return add(name, Text={"value": text, "pos": [r(x), r(y)], "size": size, "color": list(color), "shadow": False}, TextFont={"file": FONT}, **extra)


def box(name, x, y, w, h, color, layer=0, button=False, text="", text_size=0.04, text_color=(1, 1, 1, 1), radius=0.012, color2=None, border=0.0, border_color=(1, 1, 1, 0.5), parent=None, back_color=None, visible=True, **extra):
    ui = {"pos": [r(x), r(y)], "size": [r(w), r(h)], "color": list(color), "layer": layer, "button": button}
    if back_color:
        ui["back_color"] = list(back_color)
    if not visible:
        ui["visible"] = False
    if text:
        ui.update({"text": text, "text_size": text_size, "text_color": list(text_color)})
    else:
        ui.update({"text_size": text_size, "text_color": list(text_color)})  # (a tooltip box has its text put in by the engine)
    if border:
        ui.update({"border": border, "border_color": list(border_color)})
    style = {"radius": radius}
    if color2:
        style["color2"] = list(color2)
    comps = {"Ui": ui, "UiStyle": style, "TextFont": {"file": FONT}}
    if parent:
        comps["Parent"] = {"target": parent}
    comps.update(extra)
    return add(name, **comps)


components = []  # the game's own components
systems = []  # and graph systems
resources = {}

# the background, the shape the interface was made for (16:9: so it is laid out the same on any window) and the title
add("Camera", Transform={"pos": [0, 0, 5]}, Camera={"clear": [0.07, 0.08, 0.13, 1]})
add("Interface shape", UiScale={"aspect": 16 / 9})
label("Title", "UI KIT 3", 0.03, 0.012, size=0.05, color=(1, 0.9, 0.5, 1))


# ---------------------------------------------------------------------------------------------------------------- a scrolling list
def scroll_list():
    x, y, w, h = 0.03, 0.14, 0.27, 0.5
    label("Scroll label", "Scroll list", x, y - 0.06)
    components.append({"name": "ListItem", "doc": "A row of the scrolling list: `n` is its number.", "fields": [{"name": "n", "type": "u32", "default": 0}]})
    systems.append({"name": "pick_item", "doc": "A click on a row of the list says which one in the resource list.picked.", "each": ["ListItem", "Ui"],
                    "do": [{"if": {"get": "Ui.clicked"}, "then": [{"res_set": "list.picked", "to": {"get": "ListItem.n"}}]}]})
    resources["list.picked"] = 0
    label("Picked", "picked: {list.picked}", x, y + h + 0.015, size=0.04, color=(0.6, 0.9, 0.6, 1))
    view = box("Items view", x, y, w, h, (0.11, 0.13, 0.2, 1), layer=1, radius=0.015, border=0.003, border_color=(0.4, 0.5, 0.8, 0.8),
               UiScroll={"offset": [0.0, 0.05], "speed": 0.06})
    # the content: a column twice as high as the viewport, laid out by the engine; the viewport shows part of it and the bar says which part
    content_h = h * 2.4
    n = 12
    content = add("Items content", Ui={"pos": [r(x), r(y)], "size": [r(w - 0.03 * SQUARE), r(content_h)], "color": [0, 0, 0, 0], "layer": 2}, UiLayout={"kind": 0, "padding": 0.012, "spacing": 0.012}, Parent={"target": view})
    for i in range(n):
        shade = 0.28 + 0.04 * (i % 2)
        box(f"Item {i + 1}", 0, 0, 0.1, 0.1, (shade, shade + 0.05, shade + 0.22, 1), layer=3, button=True, text=f"Item {i + 1}", text_size=0.04, radius=0.012, color2=(shade * 0.6, shade * 0.7, shade + 0.1, 1), parent=content, ListItem={"n": i + 1})
    track = box("Items track", 0, 0, 0.02, 0.02, (0.05, 0.06, 0.1, 0.9), layer=4, button=True, radius=0.01)
    box("Items bar", 0, 0, 0.02, 0.02, (0.55, 0.65, 0.95, 1), layer=5, button=True, radius=0.01, color2=(0.3, 0.4, 0.75, 1), UiScrollBar={"view": view, "track": track, "thickness": 0.022, "inset": 0.006, "min_length": 0.08})
    return view


# ---------------------------------------------------------------------------------------------------------------- a drop-down
def dropdown():
    x, y, w = 0.36, 0.15, 0.26
    label("Dropdown label", "Drop-down", x, y - 0.06)
    # the list first (the header names it): a column of three buttons that is hidden until the header is clicked, over everything else
    lst = add("Quality list", Ui={"pos": [r(x), r(y + 0.075)], "size": [w, 0.2], "color": [0.1, 0.12, 0.2, 1], "layer": 20, "border": 0.003, "border_color": [0.4, 0.5, 0.8, 0.9]},
              UiStyle={"radius": 0.014, "shadow_color": [0, 0, 0, 0.5], "shadow_offset": [0.004, 0.008]}, UiLayout={"kind": 0, "padding": 0.01, "spacing": 0.008}, UiGroup={"shown": False})
    header = box("Quality header", x, y, w, 0.07, (0.2, 0.28, 0.5, 1), layer=6, button=True, text="Quality", text_size=0.04, radius=0.014, color2=(0.12, 0.17, 0.34, 1), border=0.003,
                 UiDropdown={"list": lst, "selected": 1, "resource": "ui.quality"})
    for i, name in enumerate(["Low", "Medium", "High"]):
        box(f"Quality {name}", 0, 0, 0.1, 0.05, (0.2, 0.3, 0.5, 1), layer=21, button=True, text=name, text_size=0.04, radius=0.01, color2=(0.12, 0.18, 0.34, 1), parent=lst, UiOption={"dropdown": header, "index": i})
    resources["ui.quality"] = 1
    return header


# ---------------------------------------------------------------------------------------------------------------- tabs
def tabs():
    x, y = 0.36, 0.5
    label("Tabs label", "Tabs", x, y - 0.06)
    state = add("Tabs state", UiTabs={"selected": 0, "resource": "ui.tab"})
    contents = [("Stats", ["HP 100   MP 40", "ATK 12   DEF 8"]), ("Items", ["Sword   Shield", "Potion x3"]), ("Map", ["Cave   Forest", "Town   Castle"])]
    for i, (name, lines) in enumerate(contents):
        panel = add(f"{name} panel", Ui={"pos": [r(x), r(y + 0.065)], "size": [0.26, 0.15], "color": [0.12, 0.15, 0.25, 1], "layer": 2, "border": 0.003, "border_color": [0.4, 0.5, 0.8, 0.7]},
                    UiStyle={"radius": 0.012}, UiGroup={"shown": i == 0})
        for k, line in enumerate(lines):
            box(f"{name} line {k + 1}", x + 0.01, y + 0.07 + k * 0.04, 0.24, 0.04, (0, 0, 0, 0), layer=3, text=line, text_size=0.034, text_color=(0.8, 0.85, 1, 1), parent=panel)
        box(f"{name} button", x + 0.16, y + 0.065 + 0.15 - 0.045, 0.09, 0.035, (0.3, 0.5, 0.4, 1), layer=3, button=True, text="More", text_size=0.028, radius=0.008, parent=panel)
        box(f"{name} tab", x + i * 0.0875, y, 0.085, 0.055, (0.16, 0.2, 0.32, 0.92), layer=4, button=True, text=name, text_size=0.035, radius=0.01,
            UiTab={"tabs": state, "index": i, "panel": panel, "on_color": [0.3, 0.5, 0.9, 1], "off_color": [0.16, 0.2, 0.32, 0.92]})
    resources["ui.tab"] = 0


# ---------------------------------------------------------------------------------------------------------------- inventory slots
def inventory():
    x, y = 0.70, 0.15
    label("Inventory label", "Inventory", x, y - 0.06)
    sw, sh = 0.1, 0.178  # a square on a 16:9 picture
    slots = []
    for i, name in enumerate(["A", "B"]):
        slots.append(box(f"Slot {name}", x + i * 0.14, y, sw, sh, (0.12, 0.15, 0.25, 1), layer=1, radius=0.012, border=0.003, border_color=(0.4, 0.5, 0.8, 0.8), UiDrop={"name": name}))
    iw, ih = 0.07, 0.124
    # a sword in slot A: the engine puts it in the middle of the slot (it is told where in the scene too, so that the very first picture is right)
    box("Sword", x + (sw - iw) / 2, y + (sh - ih) / 2, iw, ih, (0.95, 0.65, 0.2, 1), layer=2, button=True, text="Sw", text_size=0.05, radius=0.02, color2=(0.7, 0.35, 0.1, 1), UiDrag={"item": 7, "slot": slots[0]},
        UiTooltip={"text": "Iron sword\n+5 attack", "delay": 0.4})
    systems.append({"name": "remember_drop", "doc": "A drop on a slot is remembered in resources: which item, and how many drops there have been.", "each": ["UiDrop"],
                    "do": [{"if": {"get": "UiDrop.dropped"}, "then": [{"res_set": "inv.last_item", "to": {"get": "UiDrop.item"}}, {"res_add": "inv.moves", "by": 1}]}]})
    resources["inv.last_item"] = 0
    resources["inv.moves"] = 0
    label("Inventory moves", "moves: {inv.moves}", x, y + sh + 0.02, size=0.034, color=(0.6, 0.9, 0.6, 1))
    label("Inventory last item", "last item: {inv.last_item}", x, y + sh + 0.06, size=0.034, color=(0.6, 0.9, 0.6, 1))


# ---------------------------------------------------------------------------------------------------------------- cooldowns
def cooldown():
    x, y = 0.70, 0.55
    label("Cooldown label", "Cooldown", x, y - 0.06)
    w, h = 0.07, 0.124  # squares on a 16:9 picture
    # a ring that empties as the ability recharges, and a dark clock wipe over the button that uses it
    box("Cooldown ring", x, y, w, h, (0.95, 0.75, 0.25, 1), layer=2, back_color=(0.15, 0.2, 0.35, 0.9), UiRadial={"value": 0.0, "start": 0, "shape": 0, "inner": 0.72})
    use = box("Use button", x + 0.1, y, w, h, (0.25, 0.55, 0.4, 1), layer=2, button=True, text="Use", text_size=0.04, radius=0.015, color2=(0.12, 0.3, 0.22, 1), UseButton={},
              UiTooltip={"text": "Use the ability\nrecharges in 4 s", "delay": 0.4})
    box("Use wipe", x + 0.1, y, w, h, (0.0, 0.0, 0.0, 0.62), layer=3, UiRadial={"value": 0.0, "start": 0, "shape": 1})
    components.append({"name": "UseButton", "doc": "The button that starts the cooldown.", "fields": []})
    resources["cooldown"] = 0
    systems.append({"name": "use_ability", "order": 90, "doc": "A click on the Use button starts the cooldown (1 = just used) when it is not running.", "each": ["UseButton", "Ui"],
                    "do": [{"if": {"and": [{"get": "Ui.clicked"}, {"le": [{"res": "cooldown"}, 0]}]}, "then": [{"res_set": "cooldown", "to": 1}]}]})
    systems.append({"name": "run_cooldown", "order": 100, "doc": "The remaining share of the cooldown is what the dials show; it runs down in four seconds.", "each": ["UiRadial"],
                    "do": [{"set": "UiRadial.value", "to": {"res": "cooldown"}}, {"res_set": "cooldown", "to": {"max": [0, {"sub": [{"res": "cooldown"}, {"mul": [{"dt": True}, 0.25]}]}]}}]})
    return use


# ---------------------------------------------------------------------------------------------------------------- the tooltip box
def tooltip_box():
    # hidden until the pointer has rested on something with a UiTooltip; the engine fills it in and puts it near the pointer
    box("Tooltip", 0, 0, 0.2, 0.08, (0.03, 0.04, 0.09, 0.96), layer=40, text="", text_size=0.034, text_color=(0.95, 0.95, 0.8, 1), radius=0.012, border=0.003, border_color=(0.9, 0.8, 0.4, 0.9), visible=False,
        UiTooltipBox={})


scroll_list()
dropdown()
tabs()
inventory()
cooldown()
tooltip_box()
label("Status", "quality: {ui.quality}   tab: {ui.tab}", 0.36, 0.78, size=0.04, color=(0.6, 0.9, 0.6, 1))

scene ={"lunaris": 1, "components": components, "next_uid": len(entities) + 1, "entities": entities, "resources": resources, "systems": systems}
path = os.path.join(ROOT, "examples", "ui_kit3.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(scene, f, indent=2)
    f.write("\n")
print("wrote", os.path.normpath(path), len(entities), "entities")
