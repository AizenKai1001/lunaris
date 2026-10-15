"""Write examples/flock.lunaris.json: ninety sheep on a fenced field that keep together, keep apart and run from a dog that roams the field (`Steer`, docs/GAMEPLAY.md).
No graph system at all: every behaviour is data on the entities. The sheep flock (`group`), run from the dog (`flee`), drift when left alone (`wander`) and stay in the
field (`bounds`); the dog just wanders, fast.

    python -I tools/build_flock.py
"""
import json
import math
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


BOUNDS = [-12, -7, 12, 7]
add("Environment", Environment={"sky": True, "sky_top": [0.3, 0.5, 0.85], "sky_horizon": [0.8, 0.88, 0.95], "ambient": 0.9, "ssao": 0.8, "bloom": 0.15, "fog": 0.003})
add("Sun", Transform={"rot": [-0.52, 0.3, 0.12, 0.79]}, Light={"kind": 0, "intensity": 3.0})
add("Field", Transform={"pos": [0, 0, 0], "scale": [60, 1, 40]}, MeshRenderer={"mesh": "plane", "texture": "grass", "uv_scale": [24, 16], "color": [0.8, 0.95, 0.8, 1], "roughness": 0.95})
add("Camera", Transform={"pos": [0, 15, 14], "rot": [-0.45, 0, 0, 0.89]}, Camera={"fov_y": 52})
# the fence: four rails of posts
for k, x in enumerate(range(-12, 13, 3)):
    for z in (-7.4, 7.4):
        add(f"Post {k}", Transform={"pos": [x, 0.4, z], "scale": [0.15, 0.8, 0.15]}, MeshRenderer={"mesh": "cube", "color": [0.45, 0.32, 0.2, 1], "roughness": 0.9})
for k, z in enumerate((-7.4, 7.4)):
    add(f"Rail {k}", Transform={"pos": [0, 0.6, z], "scale": [24.4, 0.08, 0.08]}, MeshRenderer={"mesh": "cube", "color": [0.55, 0.4, 0.26, 1], "roughness": 0.9})
for k, x in enumerate((-12.4, 12.4)):
    add(f"End rail {k}", Transform={"pos": [x, 0.6, 0]}, MeshRenderer={"mesh": "cube", "color": [0.55, 0.4, 0.26, 1], "roughness": 0.9})
    entities[-1]["components"]["Transform"]["scale"] = [0.08, 0.08, 14.8]

dog = add("Dog", Transform={"pos": [-9, 0.35, 0], "scale": [0.55, 0.55, 0.8]}, MeshRenderer={"mesh": "sphere", "color": [0.2, 0.15, 0.12, 1], "roughness": 0.6}, Velocity={},
          Steer={"wander": 1.0, "max_speed": 5.5, "max_accel": 14, "bounds": BOUNDS, "edge": 3, "face": True})
for k in range(90):
    a, r = k * 2.399963, 0.6 + 4.2 * math.sqrt(k / 90)
    x, z = 4 + r * math.cos(a), 0.5 * r * math.sin(a)
    add(f"Sheep {k + 1}", Transform={"pos": [x, 0.3, z], "scale": [0.5, 0.42, 0.62]}, MeshRenderer={"mesh": "sphere", "color": [0.78, 0.78, 0.75, 1], "roughness": 0.95}, Velocity={},
        Steer={"group": "sheep", "radius": 3.0, "personal": 0.9, "separation": 1.6, "alignment": 1.0, "cohesion": 0.8, "max_speed": 3.0, "max_accel": 9, "flee": dog, "flee_radius": 6.5, "flee_weight": 3.0,
               "wander": 0.25, "bounds": BOUNDS, "edge": 2.5, "face": False})

path = os.path.join(ROOT, "examples", "flock.lunaris.json")
with open(path, "w", encoding="utf-8") as f:
    json.dump({"lunaris": 1, "entities": entities}, f, indent=2)
    f.write("\n")
print("wrote", os.path.normpath(path), len(entities), "entities")
