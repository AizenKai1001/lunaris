"""Write examples/particle_clouds.lunaris.json: a night camp that shows every kind of `Particles` cloud (docs/GAMEPLAY.md): a fire (added light), its smoke (alpha-blended,
sorted), flying sparks (stretched along their velocity), snow (a big box), a burst frozen at one moment, and a spinning ring of magic.

    python -I tools/build_particle_clouds.py
"""
import json
import math
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


add("Environment", Environment={"sky": True, "sky_top": [0.02, 0.03, 0.09], "sky_horizon": [0.08, 0.1, 0.2], "ground": [0.03, 0.04, 0.05], "ambient": 0.5, "bloom": 0.5, "bloom_threshold": 0.8, "fog": 0.01, "fog_color": [0.05, 0.06, 0.12], "exposure": 1.1})
add("Moon", Transform={"rot": [-0.55, 0.3, 0.1, 0.77]}, Light={"kind": 0, "color": [0.45, 0.55, 0.9], "intensity": 0.8})
add("Fire light", Transform={"pos": [0, 0.9, 0]}, Light={"kind": 1, "color": [1.0, 0.55, 0.2], "intensity": 4.0, "range": 9})
add("Ground", Transform={"pos": [0, 0, 0], "scale": [40, 1, 40]}, MeshRenderer={"mesh": "plane", "color": [0.14, 0.18, 0.14, 1], "roughness": 0.95})
add("Camera", Transform={"pos": [0, 2.6, 9.5], "rot": [-0.12, 0, 0, 0.99]}, Camera={"fov_y": 55, "clear": [0.02, 0.03, 0.08, 1]})
for k, (x, z, ry) in enumerate([(-0.35, 0.0, 0.2), (0.35, 0.0, -0.2), (0.0, 0.3, 1.5)]):
    s, c = math.sin(ry / 2), math.cos(ry / 2)
    add(f"Log {k + 1}", Transform={"pos": [x, 0.12, z], "rot": [0, s, 0, c], "scale": [1.1, 0.18, 0.18]}, MeshRenderer={"mesh": "cube", "color": [0.3, 0.18, 0.1, 1], "roughness": 0.9})
add("Fire", Transform={"pos": [0, 0.2, 0]}, Particles={"count": 700, "lifetime": 1.2, "shape": 3, "extent": [0.3, 0, 0], "direction": [0, 1, 0], "cone": 14, "speed_range": [0.8, 1.8], "gravity": [0, 1.2, 0],
    "turbulence": 0.18, "turbulence_scale": 1.5, "size": [0.55, 0.12], "color": [1.0, 0.65, 0.18, 0.9], "color_end": [0.9, 0.12, 0.02, 0.0], "additive": True, "fade_in": 0.1, "seed": 11})
add("Smoke", Transform={"pos": [0, 1.0, 0]}, Particles={"count": 160, "lifetime": 4.0, "shape": 3, "extent": [0.15, 0, 0], "cone": 20, "speed_range": [0.5, 1.0], "gravity": [0.3, 0.6, 0.0], "drag": 0.25,
    "turbulence": 0.5, "turbulence_scale": 0.6, "size": [0.35, 1.5], "size_jitter": 0.25, "color": [0.32, 0.32, 0.36, 0.5], "color_end": [0.16, 0.16, 0.2, 0.0], "additive": False, "fade_in": 0.2, "spin": 0.4, "seed": 5})
add("Sparks", Transform={"pos": [0, 0.6, 0]}, Particles={"count": 120, "lifetime": 1.6, "shape": 0, "cone": 55, "speed_range": [2.0, 5.0], "gravity": [0, -5, 0], "drag": 0.4, "size": [0.05, 0.02], "stretch": 0.05,
    "color": [1.0, 0.85, 0.4, 1.0], "color_end": [1.0, 0.3, 0.05, 0.0], "additive": True, "fade_in": 0.0, "seed": 21})
add("Snow", Transform={"pos": [0, 7, 0]}, Particles={"count": 4000, "lifetime": 9.0, "shape": 2, "extent": [12, 0.2, 9], "direction": [0, -1, 0], "cone": 12, "speed_range": [0.5, 1.0], "gravity": [0.0, 0.0, 0.0],
    "turbulence": 0.6, "turbulence_scale": 0.3, "size": [0.05, 0.05], "size_jitter": 0.5, "color": [0.9, 0.95, 1.0, 0.9], "color_end": [0.9, 0.95, 1.0, 0.9], "additive": False, "fade_in": 0.02, "seed": 3})
# a burst, stopped half a second after it went off (time 0.5; `playing` false keeps the clock still, so the picture is the same every time)
add("Burst", Transform={"pos": [-5.5, 1.5, -1]}, Particles={"count": 500, "lifetime": 2.0, "looping": False, "playing": False, "time": 0.55, "shape": 1, "extent": [0.2, 0, 0], "cone": 180, "speed_range": [3.0, 7.0],
    "gravity": [0, -4, 0], "drag": 1.6, "size": [0.18, 0.04], "color": [1.0, 0.95, 0.5, 1.0], "color_end": [1.0, 0.25, 0.05, 0.0], "additive": True, "fade_in": 0.0, "seed": 9})
add("Magic ring", Transform={"pos": [5.2, 1.1, -0.5]}, Particles={"count": 300, "lifetime": 2.5, "shape": 4, "extent": [1.0, 0, 0], "direction": [0, 1, 0], "cone": 8, "speed_range": [0.2, 0.6], "gravity": [0, 0.4, 0],
    "size": [0.16, 0.02], "color": [0.5, 0.8, 1.0, 1.0], "color_end": [0.7, 0.3, 1.0, 0.0], "additive": True, "spin": 3.0, "texture": "ring", "seed": 17})

scene = {"lunaris": 1, "entities": entities}
path = os.path.join(ROOT, "examples", "particle_clouds.lunaris.json")
with open(path, "w", encoding="utf-8") as f:
    json.dump(scene, f, indent=2)
    f.write("\n")
print("wrote", os.path.normpath(path), len(entities), "entities")
