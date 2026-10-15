"""Write examples/animation.lunaris.json: five copies of the skinned model `examples/models/swayer.glb` playing its two clips at different
speeds and offsets, on grass under a sky. Skeletal animation is data too: an `Animator` component names the clip and holds the clock.

    python -I tools/make_models.py     (makes the model)
    python -I tools/build_animation.py
"""
import json
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})


add("Environment", Environment={"sky": True, "sky_top": [0.16, 0.38, 0.8], "sky_horizon": [0.8, 0.88, 0.95], "ground": [0.28, 0.3, 0.24], "clouds": 0.3, "fog": 0.006, "fog_color": [0.8, 0.88, 0.95], "bloom": 0.2})
add("Sun", Transform={"rot": [-0.45, 0.35, 0.1, 0.8]}, Light={"kind": 0, "color": [1.0, 0.93, 0.8], "intensity": 2.2})
add("Camera", Transform={"pos": [0, 2.6, 9.5], "rot": [-0.09, 0, 0, 0.996]}, Camera={"fov_y": 50, "clear": [0.5, 0.65, 0.85, 1]})
add("Ground", Transform={"scale": [300, 1, 300]}, MeshRenderer={"mesh": "plane", "texture": "grass", "uv_scale": [120, 120], "roughness": 0.95, "color": [0.8, 0.9, 0.8, 1]})
# (x, clip, speed, start time, scale, tint)
for x, clip, speed, t0, s, tint in ((-6, "sway", 1.0, 0.0, 1.0, [1.0, 1.0, 1.0]), (-3, "sway", 1.6, 0.4, 0.8, [0.7, 0.95, 1.0]), (0, "nod", 1.0, 0.0, 1.2, [1.0, 0.85, 0.85]), (3, "nod", 0.6, 0.9, 1.0, [0.85, 1.0, 0.8]), (6, "sway", 0.5, 1.2, 1.4, [1.0, 0.95, 0.7])):
    add(f"Swayer {clip} x{speed}", Transform={"pos": [x, 0, 0], "scale": [s, s, s]},
        MeshRenderer={"mesh": "models/swayer.glb".replace("models/", "examples/models/"), "color": tint + [1], "roughness": 0.55},
        Animator={"clip": clip, "time": t0, "speed": speed, "playing": True, "looping": True})

path = os.path.join(ROOT, "examples", "animation.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump({"lunaris": 1, "entities": entities}, f, indent=1)
    f.write("\n")
print(f"wrote {path}: {len(entities)} entities")
