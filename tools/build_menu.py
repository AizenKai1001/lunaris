"""Write examples/menu.lunaris.json: the on-screen interface (`Ui`). A health bar, three buttons (Heal, Hurt, Reset) and a title panel over a small 3D
backdrop. The buttons are `Ui` entities with `button: true`; one graph system per button reacts to `Ui.clicked` and changes the resource `hp`;
another keeps the bar's `fill` and colour in step with it. Everything is plain data: a model can build, test (by sending mouse input) and
change it without any editor.

    python -I tools/build_menu.py
"""
import json
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})


add("Sun", Transform={"rot": [-0.5, 0.3, 0.1, 0.8]}, Light={"kind": 0, "color": [1.0, 0.95, 0.85], "intensity": 2.0})
add("Environment", Environment={"sky": True, "sky_top": [0.12, 0.2, 0.45], "sky_horizon": [0.6, 0.55, 0.75], "ground": [0.15, 0.15, 0.2], "bloom": 0.3})
add("Backdrop floor", Transform={"pos": [0, -0.5, 0], "scale": [30, 1, 30]}, MeshRenderer={"mesh": "cube", "texture": "tiles", "uv_scale": [15, 15], "color": [0.7, 0.75, 0.9, 1], "roughness": 0.6})
add("Crystal", Transform={"pos": [0, 1.4, 0], "scale": [1.4, 1.4, 1.4], "rot": [0.2, 0.5, 0.1, 0.84]}, MeshRenderer={"mesh": "sphere", "color": [0.5, 0.8, 1.0, 1], "emissive": 0.5, "roughness": 0.15})
add("Camera", Transform={"pos": [0, 2.2, 6], "rot": [-0.12, 0, 0, 0.99]}, Camera={"fov_y": 55, "clear": [0.3, 0.4, 0.6, 1]})
add("Title", Ui={"pos": [0.25, 0.05], "size": [0.5, 0.12], "color": [0.08, 0.1, 0.2, 0.85], "border": 0.004, "border_color": [0.6, 0.75, 1.0, 1], "text": "HEALTH DEMO", "text_size": 0.05, "layer": 1})
add("Health bar", Ui={"pos": [0.1, 0.3], "size": [0.8, 0.09], "color": [0.2, 0.8, 0.3, 1], "back_color": [0.05, 0.05, 0.08, 0.8], "fill": 0.5, "border": 0.004, "border_color": [1, 1, 1, 0.9], "text": "{hp} / 100", "text_size": 0.045, "layer": 1}, HpBar={})
for i, (label, tag) in enumerate([("Heal", "HealButton"), ("Hurt", "HurtButton"), ("Reset", "ResetButton")]):
    add(f"{label} button", Ui={"pos": [0.1 + 0.3 * i, 0.55], "size": [0.2, 0.1], "color": [0.2, 0.3, 0.55, 0.95], "border": 0.003, "border_color": [0.8, 0.9, 1, 1], "text": label, "text_size": 0.045, "button": True, "layer": 2}, **{tag: {}})
add("Hint", Text={"value": "click the buttons", "pos": [0.5, 0.85], "size": 0.035, "color": [0.9, 0.92, 1, 0.85], "align": 1, "shadow": True})


def on_click(tag, new_hp):
    return {"name": tag.lower(), "doc": f"A click on the {tag} changes the health.", "each": [tag, "Ui"],
            "do": [{"if": {"get": "Ui.clicked"}, "then": [{"res_set": "hp", "to": new_hp}]}]}


doc = {"lunaris": 1,
       "components": [{"name": n, "fields": []} for n in ("HpBar", "HealButton", "HurtButton", "ResetButton")],
       "next_uid": len(entities) + 1, "entities": entities, "resources": {"hp": 50},
       "systems": [
           on_click("HealButton", {"min": [100, {"add": [{"res": "hp"}, 10]}]}),
           on_click("HurtButton", {"max": [0, {"sub": [{"res": "hp"}, 15]}]}),
           on_click("ResetButton", 100),
           {"name": "bar", "doc": "The bar shows the health: its fill, and a colour from red (empty) through yellow to green (full).", "each": ["HpBar", "Ui"], "do": [
               {"set": "Ui.fill", "to": {"div": [{"res": "hp"}, 100]}},
               {"set": "Ui.color[0]", "to": {"min": [1, {"mul": [2, {"sub": [1, {"div": [{"res": "hp"}, 100]}]}]}]}},
               {"set": "Ui.color[1]", "to": {"mul": [0.85, {"min": [1, {"mul": [2, {"div": [{"res": "hp"}, 100]}]}]}]}},
               {"set": "Ui.color[2]", "to": 0.15}]},
       ]}
path = os.path.join(ROOT, "examples", "menu.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")
print(f"wrote {path}: {len(entities)} entities")
