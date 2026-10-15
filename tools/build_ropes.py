"""Write examples/ropes.lunaris.json and the playtest examples/tests/ropes.test.json: a playground for `Joint` (docs/PHYSICS.md). A heavy ball on
a rod swings into a tower of blocks, a chain of rods hangs from a hook and swings, and a ball on a spring bounces until it settles where the
spring's pull balances its weight (rest length 3 m, 60 N/m, 1 kg: 3 + 9.8 / 60 below the hook).

    python -I tools/build_ropes.py
"""
import json
import math
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


def sphere(name, pos, radius, mass, joint, color):
    return add(name, Transform={"pos": pos, "scale": [radius * 2] * 3}, Velocity={}, Collider={"shape": 0, "size": [1, 1, 1]}, RigidBody={"mass": mass, "restitution": 0.15, "friction": 0.6},
               MeshRenderer={"mesh": "sphere", "color": color + [1], "roughness": 0.35}, Joint=joint)


add("Environment", Environment={"sky": True, "sky_top": [0.16, 0.38, 0.8], "sky_horizon": [0.82, 0.88, 0.95], "ground": [0.28, 0.3, 0.24], "clouds": 0.3, "fog": 0.003, "fog_color": [0.82, 0.88, 0.95], "bloom": 0.15})
add("Sun", Transform={"rot": [-0.5, 0.32, 0.12, 0.8]}, Light={"kind": 0, "color": [1.0, 0.93, 0.8], "intensity": 2.4})
add("Ground", Transform={"pos": [0, -0.5, 0], "scale": [60, 1, 30]}, MeshRenderer={"mesh": "cube", "texture": "grass", "uv_scale": [24, 12], "color": [0.8, 0.9, 0.8, 1], "roughness": 0.95}, Collider={"shape": 1, "size": [1, 1, 1]})
add("Beam", Transform={"pos": [0, 11.2, 0], "scale": [34, 0.4, 0.8]}, MeshRenderer={"mesh": "cube", "color": [0.4, 0.3, 0.22, 1], "roughness": 0.8})
add("Camera", Transform={"pos": [0, 6.0, 26], "rot": [-0.0499, 0, 0, 0.9988]}, Camera={"fov_y": 55, "clear": [0.5, 0.65, 0.85, 1]})

# a heavy ball on a 8 m rod from (-4, 11, 0), let go from the side: 7 m left of its hook, 3.87 m below it
HOOK = 11.0
swing = sphere("Wrecking ball", [-11.0, HOOK - math.sqrt(64 - 49), 0.0], 1.1, 30, {"kind": 1, "length": 8, "anchor": [-4, HOOK, 0], "width": 0.12, "color": [0.15, 0.15, 0.17, 1]}, [0.25, 0.25, 0.3])
for k in range(4):
    for z in (-0.55, 0.55):
        add(f"Block {k * 2 + (1 if z > 0 else 0) + 1}", Transform={"pos": [-2.2, 0.5 + k * 1.0, z]}, Velocity={}, Collider={"shape": 1, "size": [1, 1, 1]}, RigidBody={"mass": 1, "friction": 0.7, "restitution": 0.1},
            MeshRenderer={"mesh": "cube", "color": [0.85 - 0.1 * k, 0.55 + 0.08 * k, 0.3, 1], "roughness": 0.6})

# a chain of five balls on rods from a hook at (4, 10, 0), thrown out sideways
hook = add("Hook", Transform={"pos": [4, 10, 0], "scale": [0.3, 0.3, 0.3]}, MeshRenderer={"mesh": "sphere", "color": [0.7, 0.7, 0.2, 1], "roughness": 0.3})
prev = hook
for k in range(5):
    prev = sphere(f"Chain {k + 1}", [4 + 1.2 * (k + 1), 10, 0], 0.35, 1, {"kind": 1, "target": prev, "length": 1.2, "width": 0.07, "color": [0.55, 0.45, 0.2, 1]}, [0.9, 0.35 + 0.1 * k, 0.3])

# a ball on a spring from (10, 10, 0)
SPRING = {"kind": 2, "length": 3, "stiffness": 60, "damping": 1.5, "anchor": [10, 10, 0], "width": 0.06, "color": [0.8, 0.8, 0.85, 1]}
sphere("Spring ball", [10, 6.0, 0], 0.5, 1, SPRING, [0.3, 0.7, 0.95])

doc = {"lunaris": 1, "entities": entities}
path = os.path.join(ROOT, "examples", "ropes.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")

rest = 10 - 3 - 9.8 / 60
test = {
    "name": "Ropes: a heavy ball on a rod swings into a tower of blocks, a chain of rods hangs and swings, a ball on a spring settles where the pull balances the weight",
    "scene": "examples/ropes.lunaris.json",
    "seed": 1,
    "replay_check": True,
    "steps": [
        {"expect": {"check": "errors", "eq": 0}},
        {"step": 600},
        {"expect": {"name": "Wrecking ball", "field": "Transform.pos[1]", "ge": 2.8, "message": "the rod keeps the ball 8 m from its hook: never lower than 3 m"}},
        {"expect": {"name": "Wrecking ball", "field": "Transform.pos[1]", "le": 7.4, "message": "and never higher than where it was let go (a little slack for the correction)"}},
        {"expect": {"name": "Block 7", "field": "Transform.pos[0]", "gt": 3.0, "message": "the ball knocked the top of the tower more than 5 m to the right"}},
        {"expect": {"name": "Block 8", "field": "Transform.pos[0]", "gt": 3.0}},
        {"expect": {"name": "Block 1", "field": "Transform.pos[0]", "approx": [-2.2, 0.15], "message": "while the bottom of the tower stayed where it was"}},
        {"expect": {"name": "Chain 5", "field": "Transform.pos[1]", "between": [3.0, 10.5], "message": "the end of the chain swings below the hook"}},
        {"step": 1500},
        {"expect": {"name": "Spring ball", "field": "Transform.pos[1]", "approx": [round(rest, 3), 0.01], "message": "settled where 60 N/m balance 9.8 N: 3.16 m below the hook"}},
        {"expect": {"name": "Spring ball", "field": "Transform.pos[0]", "approx": [10, 0.01]}},
    ],
}
tpath = os.path.join(ROOT, "examples", "tests", "ropes.test.json")
with open(tpath, "w", encoding="utf-8", newline="\n") as f:
    json.dump(test, f, indent=2)
    f.write("\n")
print(f"wrote {path}, {tpath}: {len(entities)} entities, swing ball is uid {swing}")
