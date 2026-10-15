"""Write examples/ui_showcase.lunaris.json: a HUD made of the interface components of docs/UI.md that go beyond flat rectangles (rounded corners, gradients, drop
shadows, pictures, TrueType text, tags pinned to a character in the world), over a small 3D scene. The GPU comparison test draws it with both renderers.

    python -I tools/build_ui_showcase.py

It also copies the test font next to the scene (examples/fonts/pixel.ttf, made by tools/make_test_font.py).
"""
import json
import os
import shutil

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
FONT = "examples/fonts/pixel.ttf"
os.makedirs(os.path.join(ROOT, "examples", "fonts"), exist_ok=True)
shutil.copyfile(os.path.join(ROOT, "crates", "lunaris-render", "testdata", "pixel.ttf"), os.path.join(ROOT, FONT))

entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


add("Camera", Transform={"pos": [0, 1.9, 7.5]}, Camera={"fov_y": 50, "clear": [0.45, 0.6, 0.8, 1]})
add("Sun", Transform={"rot": [-0.45, 0.3, 0.1, 0.83]}, Light={"kind": 0, "color": [1.0, 0.95, 0.85], "intensity": 2.0})
add("Floor", Transform={"pos": [0, 0, 0], "scale": [30, 1, 30]}, MeshRenderer={"mesh": "plane", "color": [0.35, 0.55, 0.38, 1], "shininess": 8})
hero = add("Hero", Transform={"pos": [0, 0.8, 0], "scale": [1.2, 1.6, 1.2]}, MeshRenderer={"mesh": "cube", "color": [0.9, 0.35, 0.25, 1]})
add("Orb", Transform={"pos": [2.6, 0.6, -1.0], "scale": [1.2, 1.2, 1.2]}, MeshRenderer={"mesh": "sphere", "color": [0.25, 0.5, 0.95, 1], "shininess": 48})

# a panel in the corner: rounded, shaded from top to bottom, with a border and a drop shadow
add("Panel", Ui={"pos": [0.03, 0.04], "size": [0.3, 0.34], "color": [0.2, 0.26, 0.5, 0.95], "border": 0.004, "border_color": [0.7, 0.8, 1.0, 1.0], "layer": 0},
    UiStyle={"radius": 0.03, "color2": [0.06, 0.08, 0.2, 0.95], "shadow_color": [0, 0, 0, 0.55], "shadow_offset": [0.008, 0.012]})
add("Title", Text={"value": "LUNARIS", "pos": [0.05, 0.065], "size": 0.07, "color": [1, 1, 1, 1], "shadow": True}, TextFont={"file": FONT})
add("Health bar", Ui={"pos": [0.05, 0.17], "size": [0.26, 0.05], "color": [0.25, 0.8, 0.3, 1], "back_color": [0.08, 0.1, 0.1, 0.9], "fill": 0.65, "border": 0.003, "border_color": [0, 0, 0, 1],
                      "text": "HP 65", "text_size": 0.035, "text_color": [1, 1, 1, 1], "layer": 1},
    UiStyle={"radius": 0.025, "color2": [0.1, 0.45, 0.15, 1]}, TextFont={"file": FONT})
add("Coin", Ui={"pos": [0.05, 0.25], "size": [0.07, 0.09], "color": [0, 0, 0, 0], "layer": 1}, UiStyle={"image": "circle", "tint": [1.0, 0.8, 0.2, 1.0], "fit": 1})
add("Coin count", Text={"value": "x 12", "pos": [0.13, 0.27], "size": 0.05, "color": [1, 0.9, 0.5, 1], "shadow": True}, TextFont={"file": FONT})

# a picture frame on the right: a texture cropped to fill a rounded box
add("Frame", Ui={"pos": [0.72, 0.04], "size": [0.25, 0.25], "color": [0.1, 0.1, 0.1, 1], "border": 0.005, "border_color": [0.9, 0.85, 0.6, 1], "layer": 0},
    UiStyle={"radius": 0.04, "image": "checker", "fit": 2, "tint": [0.9, 0.95, 1.0, 1.0], "shadow_color": [0, 0, 0, 0.5], "shadow_offset": [0.006, 0.01]})

# buttons along the bottom: gradient fills, one labelled with the TrueType font and one with the built-in one
add("Play", Ui={"pos": [0.3, 0.82], "size": [0.18, 0.11], "color": [0.3, 0.8, 0.4, 1], "button": True, "text": "PLAY", "text_size": 0.05, "text_color": [1, 1, 1, 1], "border": 0.004, "border_color": [1, 1, 1, 0.8], "layer": 2},
    UiStyle={"radius": 0.055, "color2": [0.1, 0.45, 0.2, 1], "shadow_color": [0, 0, 0, 0.5], "shadow_offset": [0.004, 0.008]}, TextFont={"file": FONT})
add("Quit", Ui={"pos": [0.52, 0.82], "size": [0.18, 0.11], "color": [0.9, 0.35, 0.3, 1], "button": True, "text": "Quit", "text_size": 0.05, "text_color": [1, 1, 1, 1], "layer": 2},
    UiStyle={"radius": 0.02, "color2": [0.5, 0.12, 0.1, 1]})

# tags pinned to the hero: its name (TrueType, centred) and a small bar over it
add("Hero name", Text={"value": "Hero", "pos": [0, 0], "size": 0.05, "color": [1, 1, 0.6, 1], "align": 1, "shadow": True}, TextFont={"file": FONT}, UiAnchor={"entity": hero, "world": [0, 1.15, 0], "offset": [0, -0.085]})
add("Hero bar", Ui={"size": [0.12, 0.022], "color": [0.9, 0.2, 0.2, 1], "back_color": [0.15, 0.02, 0.02, 0.9], "fill": 0.8, "layer": 1},
    UiStyle={"radius": 0.011}, UiAnchor={"entity": hero, "world": [0, 1.15, 0], "offset": [0, -0.012]})

scene = {"lunaris": 1, "entities": entities}
path = os.path.join(ROOT, "examples", "ui_showcase.lunaris.json")
with open(path, "w", encoding="utf-8") as f:
    json.dump(scene, f, indent=2)
    f.write("\n")
print("wrote", os.path.normpath(path), len(entities), "entities")
