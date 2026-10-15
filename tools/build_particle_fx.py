"""Write examples/particle_fx.lunaris.json: five effects that use the Niagara-style `ParticleModules` (docs/GAMEPLAY.md), left to right:

  fountain   water drops that bounce on the floor and lose speed (a `collide plane`), colour and size over the life from gradients
  campfire   flames and then smoke, with a wind that gusts (a `wind` with `gust`) and flame gradients with more than two stops
  sparks     hot sparks that bounce off the floor and leave a ribbon behind them (a `trail`)
  tornado    dust drawn into a vortex that also lifts it (a `vortex`, a `wind` upwards, an `attract` at the top)
  debris     little solid cubes (a `mesh`) tumbling and coming to rest on the floor (a `collide plane` with `stick`)

    python -I tools/build_particle_fx.py
"""
import json
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


add("Environment", Environment={"sky": True, "sky_top": [0.03, 0.05, 0.12], "sky_horizon": [0.12, 0.16, 0.26], "ground": [0.05, 0.06, 0.07], "ambient": 0.55, "bloom": 0.45, "bloom_threshold": 0.8, "fog": 0.006, "fog_color": [0.1, 0.13, 0.2]})
add("Moon", Transform={"rot": [-0.55, 0.3, 0.1, 0.77]}, Light={"kind": 0, "color": [0.5, 0.6, 0.95], "intensity": 1.0})
add("Camp light", Transform={"pos": [-3.5, 1.0, 0.5]}, Light={"kind": 1, "color": [1.0, 0.55, 0.2], "intensity": 4.0, "range": 8})
add("Ground", Transform={"pos": [0, 0, 0], "scale": [60, 1, 40]}, MeshRenderer={"mesh": "plane", "texture": "stone", "uv_scale": [24, 16], "color": [0.5, 0.52, 0.56, 1], "roughness": 0.95})
add("Camera", Transform={"pos": [0.5, 3.4, 12.5], "rot": [-0.14, 0, 0, 0.99]}, Camera={"fov_y": 58, "clear": [0.02, 0.03, 0.08, 1]})
for k, (x, name) in enumerate([(-7.0, "Basin"), (-3.5, "Fire pit"), (0.0, "Spark pad"), (3.5, "Tornado pad"), (7.0, "Debris pad")]):
    add(name, Transform={"pos": [x, 0.04, 0], "scale": [1.3, 0.08, 1.3]}, MeshRenderer={"mesh": "cylinder", "color": [0.22, 0.22, 0.26, 1], "roughness": 0.8})

# the fountain: drops thrown up and out, falling back to bounce on the floor
add("Fountain", Transform={"pos": [-7.0, 0.05, 0]}, Particles={"count": 1100, "lifetime": 2.0, "shape": 3, "extent": [0.1, 0, 0], "direction": [0, 1, 0], "cone": 11, "speed_range": [6.8, 8.6], "gravity": [0, -9.8, 0],
    "size": [0.09, 0.05], "size_jitter": 0.3, "color": [0.7, 0.85, 1.0, 0.8], "additive": True, "fade_in": 0.0, "seed": 4},
    ParticleModules={"modules": "color 0:0.6,0.8,1,0.85 0.6:0.7,0.9,1,0.7 1:1,1,1,0\nsize 0:0.09 0.7:0.07 1:0.03\ncollide plane 0,1,0 d=-0.05 bounce=0.35 friction=0.3\n"})

# the campfire: flames with a five-stop gradient and a size curve that swells and dies, and smoke that the wind carries off
add("Flames", Transform={"pos": [-3.5, 0.2, 0]}, Particles={"count": 800, "lifetime": 1.3, "shape": 3, "extent": [0.3, 0, 0], "direction": [0, 1, 0], "cone": 10, "speed_range": [0.9, 1.9], "gravity": [0, 1.4, 0],
    "turbulence": 0.15, "turbulence_scale": 1.5, "size_jitter": 0.3, "fade_in": 0.08, "additive": True, "seed": 11},
    ParticleModules={"modules": "color 0:1,0.95,0.7,0 0.12:1,0.8,0.3,0.95 0.45:1,0.45,0.1,0.8 0.8:0.7,0.15,0.04,0.4 1:0.2,0.05,0.02,0\nsize 0:0.2 0.25:0.62 1:0.08\nwind 0.5,0,0 gust=0.8 freq=0.6\n"})
add("Smoke", Transform={"pos": [-3.5, 1.1, 0]}, Particles={"count": 200, "lifetime": 5.0, "shape": 3, "extent": [0.18, 0, 0], "cone": 18, "speed_range": [0.5, 0.9], "gravity": [0, 0.5, 0], "drag": 0.3,
    "turbulence": 0.4, "turbulence_scale": 0.6, "size_jitter": 0.25, "fade_in": 0.15, "spin": 0.35, "additive": False, "seed": 5},
    ParticleModules={"modules": "color 0:0.3,0.3,0.34,0 0.15:0.34,0.34,0.38,0.5 1:0.14,0.14,0.18,0\nsize 0:0.3 1:1.7\nwind 0.9,0.2,0 gust=0.9 freq=0.5\n"})

# sparks: bounce on the pad, each with a ribbon behind it
add("Sparks", Transform={"pos": [0.0, 0.1, 0]}, Particles={"count": 140, "lifetime": 1.8, "shape": 0, "direction": [0, 1, 0], "cone": 58, "speed_range": [3.0, 6.0], "gravity": [0, -9, 0], "size": [0.1, 0.1],
    "size_jitter": 0.2, "color": [1.0, 0.8, 0.35, 1.0], "additive": True, "fade_in": 0.0, "seed": 21},
    ParticleModules={"modules": "color 0:1,0.95,0.6,1 0.5:1,0.55,0.15,0.9 1:0.8,0.1,0.02,0\ncollide plane 0,1,0 d=-0.1 bounce=0.55 friction=0.15\ntrail segments=8 time=0.28 width=0.55\n"})

# the tornado: dust spun round a vertical axis, lifted, and drawn back in at the top
add("Tornado", Transform={"pos": [3.5, 0.1, 0]}, Particles={"count": 3500, "lifetime": 3.2, "shape": 4, "extent": [1.3, 0, 0], "direction": [0, 1, 0], "cone": 6, "speed_range": [0.2, 0.8], "gravity": [0, 0, 0], "drag": 1.0,
    "size": [0.1, 0.1], "size_jitter": 0.4, "color": [0.7, 0.6, 0.5, 0.7], "additive": False, "fade_in": 0.05, "seed": 8},
    ParticleModules={"modules": "color 0:0.62,0.52,0.42,0 0.15:0.66,0.56,0.46,0.65 0.85:0.5,0.42,0.36,0.5 1:0.4,0.34,0.3,0\nsize 0:0.07 1:0.2\nvortex 0,0,0 axis=0,1,0 strength=6 radius=3 pull=9\nwind 0,1.7,0\n"})

# debris: solid cubes thrown out, tumbling, bouncing once or twice and lying where they stop
add("Debris", Transform={"pos": [7.0, 0.1, 0]}, Particles={"count": 90, "lifetime": 4.0, "looping": True, "shape": 0, "direction": [0, 1, 0], "cone": 42, "speed_range": [2.5, 4.6], "gravity": [0, -9.8, 0],
    "size": [0.28, 0.28], "size_jitter": 0.35, "color": [0.65, 0.52, 0.4, 1.0], "additive": False, "fade_in": 0.0, "spin": 2.5, "seed": 31},
    ParticleModules={"modules": "color 0:0.7,0.56,0.42,1 0.8:0.6,0.48,0.38,1 1:0.5,0.4,0.32,0\ncollide plane 0,1,0 d=-0.1 bounce=0.35 friction=0.5\nmesh cube\n"})

path = os.path.join(ROOT, "examples", "particle_fx.lunaris.json")
with open(path, "w", encoding="utf-8") as f:
    json.dump({"lunaris": 1, "entities": entities}, f, indent=2)
    f.write("\n")
print("wrote", os.path.normpath(path), len(entities), "entities")
