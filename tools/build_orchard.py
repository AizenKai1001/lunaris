"""Write examples/orchard.lunaris.json: prefabs in use. The scene defines three prefabs (a Tree with a collider body, a trunk and a crown; an
Apple; a Ball) and places forty trees as `instances`. A spawner entity runs a graph system with a `spawn_prefab` statement that rains a
ball from the sky every 15 ticks at a position drawn from the engine's deterministic random numbers; balls bounce off the trees and the
ground and are removed after seven seconds.

    python -I tools/build_orchard.py
"""
import json
import math
import os
import random

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})


add("Environment", Environment={"sky": True, "sky_top": [0.14, 0.34, 0.74], "sky_horizon": [0.78, 0.86, 0.92], "ground": [0.25, 0.3, 0.2], "clouds": 0.45, "fog": 0.006, "fog_color": [0.78, 0.86, 0.92], "bloom": 0.15})
add("Sun", Transform={"rot": [-0.5, 0.3, 0.1, 0.8]}, Light={"kind": 0, "color": [1.0, 0.94, 0.82], "intensity": 2.4})
add("Ground", Transform={"pos": [0, -0.5, 0], "scale": [60, 1, 60]}, MeshRenderer={"mesh": "cube", "texture": "grass", "uv_scale": [30, 30], "color": [0.8, 0.9, 0.75, 1], "roughness": 0.95}, Collider={"shape": 1, "size": [1, 1, 1]})
add("Physics", PhysicsSettings={"gravity": [0, -9.8, 0]})
add("Camera", Transform={"pos": [0, 12, 26], "rot": [-0.25, 0, 0, 0.968]}, Camera={"fov_y": 55, "clear": [0.5, 0.65, 0.85, 1]})
add("Spawner", Transform={"pos": [0, 0, 0]}, Spawner={})
add("Count", Text={"value": "balls dropped {dropped}", "pos": [0.02, 0.03], "size": 0.05, "color": [1, 1, 1, 1], "align": 0, "shadow": True})

prefabs = {
    "Tree": {
        "components": {"Name": {"value": "Tree"}, "Transform": {"pos": [0, 0, 0]}},
        "children": [
            {"name": "Trunk", "components": {"LocalTransform": {"pos": [0, 1.2, 0], "scale": [0.45, 2.4, 0.45]},
                                             "MeshRenderer": {"mesh": "cylinder", "color": [0.42, 0.28, 0.18, 1], "roughness": 0.9},
                                             "Collider": {"shape": 1, "size": [1, 1, 1]}}},
            {"name": "Crown", "components": {"LocalTransform": {"pos": [0, 3.3, 0], "scale": [2.4, 2.4, 2.4]},
                                             "MeshRenderer": {"mesh": "sphere", "color": [0.2, 0.55, 0.22, 1], "roughness": 0.85},
                                             "Collider": {"shape": 0, "size": [1, 1, 1]}}},
        ],
    },
    "Ball": {
        "components": {"Name": {"value": "Ball"}, "Transform": {"pos": [0, 8, 0], "scale": [0.6, 0.6, 0.6]}, "Velocity": {},
                       "MeshRenderer": {"mesh": "sphere", "color": [0.95, 0.35, 0.25, 1], "roughness": 0.3, "emissive": 0.25},
                       "Collider": {"shape": 0, "size": [1, 1, 1]}, "RigidBody": {"mass": 1, "restitution": 0.6, "friction": 0.3},
                       "Ball": {"age": 0}},
    },
}
prefabs["Apple"] = {"prefab": "Ball", "components": {"Name": {"value": "Apple"}, "MeshRenderer": {"color": [0.8, 0.1, 0.1, 1]}}}

# forty trees on a jittered grid, none in the middle where the balls land first
rng = random.Random(7)
instances = []
for gx in range(-9, 10, 2):
    for gz in range(-9, 8, 3):
        if len(instances) >= 40:
            break
        x, z = gx * 2.4 + rng.uniform(-0.7, 0.7), gz * 2.2 + rng.uniform(-0.7, 0.7)
        if math.hypot(x, z) < 4:
            continue
        s = rng.uniform(0.8, 1.3)
        instances.append({"prefab": "Tree", "at": [round(x, 2), 0, round(z, 2)], "overrides": {"Transform": {"scale": [round(s, 2)] * 3}}})

doc = {"lunaris": 1,
       "components": [
           {"name": "Spawner", "doc": "Marks the entity that rains balls.", "fields": []},
           {"name": "Ball", "doc": "A dropped ball; removed when it is old.", "fields": [{"name": "age", "type": "f32", "default": 0}]},
       ],
       "next_uid": len(entities) + 1, "entities": entities, "resources": {"dropped": 0},
       "prefabs": prefabs, "instances": instances,
       "systems": [
           {"name": "rain", "doc": "Every 15 ticks drop a Ball from [-10..10, 9, -10..10] with a little sideways speed.", "each": ["Spawner"], "do": [
               {"if": {"eq": [{"mod": [{"tick": True}, 15]}, 0]}, "then": [
                   {"spawn_prefab": "Ball", "at": [{"sub": [{"mul": [{"rand": 1}, 20]}, 10]}, 9, {"sub": [{"mul": [{"rand": 2}, 20]}, 10]}],
                    "velocity": [{"sub": [{"mul": [{"rand": 3}, 2]}, 1]}, 0, {"sub": [{"mul": [{"rand": 4}, 2]}, 1]}]},
                   {"res_add": "dropped", "by": 1}]}]},
           {"name": "age", "doc": "Balls get old and disappear after seven seconds.", "each": ["Ball"], "do": [
               {"set": "Ball.age", "to": {"add": [{"get": "Ball.age"}, {"dt": True}]}},
               {"if": {"gt": [{"get": "Ball.age"}, 7]}, "then": [{"despawn": True}]}]},
       ]}
path = os.path.join(ROOT, "examples", "orchard.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")
print(f"wrote {path}: {len(entities)} entities, {len(instances)} tree instances, prefabs {', '.join(prefabs)}")
