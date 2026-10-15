"""Write examples/maze.lunaris.json: navigation in use. A serpentine maze of walls, a Seeker (a CharacterBody with a NavAgent) that patrols four flags
by itself, and a Hero with a NavAgent for a mind or a person to send somewhere (`set NavAgent {target: [x, y, z], active: true}`, or the mind verb
`go_to`). The patrol is one graph system: when the Seeker has arrived (`NavAgent.status` is 2) it picks the next flag and sets `active`.

    python -I tools/build_maze.py
"""
import json
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})


def wall(name, x0, z0, x1, z1):
    """A wall covering the rectangle (x0, z0)-(x1, z1), 3 m high."""
    add(name, Transform={"pos": [(x0 + x1) / 2, 1.5, (z0 + z1) / 2], "scale": [x1 - x0, 3, z1 - z0]},
        MeshRenderer={"mesh": "cube", "texture": "bricks", "uv_scale": [max(x1 - x0, z1 - z0) / 2, 1], "color": [0.8, 0.62, 0.55, 1], "roughness": 0.85},
        Collider={"shape": 1, "size": [1, 1, 1]})


add("Environment", Environment={"sky": True, "sky_top": [0.18, 0.3, 0.6], "sky_horizon": [0.8, 0.75, 0.8], "ground": [0.2, 0.2, 0.2], "clouds": 0.2, "fog": 0.002, "bloom": 0.25})
add("Sun", Transform={"rot": [-0.6, 0.25, 0.1, 0.75]}, Light={"kind": 0, "color": [1.0, 0.92, 0.8], "intensity": 2.2})
add("Floor", Transform={"pos": [0, -0.5, 0], "scale": [28, 1, 28]}, MeshRenderer={"mesh": "cube", "texture": "tiles", "uv_scale": [14, 14], "color": [0.78, 0.8, 0.9, 1], "roughness": 0.6}, Collider={"shape": 1, "size": [1, 1, 1]})
# outer walls
wall("North wall", -13, -13, 13, -12)
wall("South wall", -13, 12, 13, 13)
wall("West wall", -13, -12, -12, 12)
wall("East wall", 12, -12, 13, 12)
# the serpentine: each wall leaves a gap at one end
wall("Wall A", -6.5, -12, -5.5, 4)   # gap at the south end (z > 4)
wall("Wall B", -0.5, -4, 0.5, 12)    # gap at the north end (z < -4)
wall("Wall C", 5.5, -12, 6.5, 4)     # gap at the south end
flags = [(-9, 0, -9), (-3, 0, 9), (3, 0, -9), (9, 0, 9)]
for i, (x, y, z) in enumerate(flags):
    add(f"Flag {i + 1}", Transform={"pos": [x, 0.75, z], "scale": [0.2, 1.5, 0.2]}, MeshRenderer={"mesh": "cylinder", "color": [1.0, 0.8, 0.2, 1], "emissive": 0.6})
seeker = len(entities) + 1
add("Seeker", Transform={"pos": [-9, 0, -9]}, CharacterBody={"radius": 0.35, "height": 1.8}, NavAgent={"radius": 0.4, "speed": 5, "arrive": 0.4}, Patrol={"leg": 0})
add("Seeker body", Transform={"pos": [-9, 0.9, -9], "scale": [0.7, 1.8, 0.7]}, Parent={"target": seeker}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.7, 1.8, 0.7]},
    MeshRenderer={"mesh": "cylinder", "color": [0.9, 0.3, 0.3, 1], "roughness": 0.5})
hero = len(entities) + 1
add("Hero", Transform={"pos": [9, 0, -9]}, CharacterBody={"radius": 0.35, "height": 1.8}, NavAgent={"radius": 0.4, "speed": 5, "arrive": 0.4})
add("Hero body", Transform={"pos": [9, 0.9, -9], "scale": [0.7, 1.8, 0.7]}, Parent={"target": hero}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.7, 1.8, 0.7]},
    MeshRenderer={"mesh": "cylinder", "color": [0.3, 0.6, 0.95, 1], "roughness": 0.5})
add("Camera", Transform={"pos": [0, 30, 14], "rot": [-0.6, 0, 0, 0.8]}, Camera={"fov_y": 50, "clear": [0.5, 0.6, 0.8, 1]})
add("Legs", Text={"value": "flags reached {legs}", "pos": [0.02, 0.03], "size": 0.055, "color": [1, 1, 1, 1], "align": 0, "shadow": True})


def branch(n):
    x, _, z = flags[n]
    return {"if": {"eq": [{"var": "n"}, n]}, "then": [
        {"set": "NavAgent.target[0]", "to": x}, {"set": "NavAgent.target[1]", "to": 0}, {"set": "NavAgent.target[2]", "to": z}]}


doc = {"lunaris": 1,
       "components": [{"name": "Patrol", "doc": "Which flag a patrolling entity is walking to.", "fields": [{"name": "leg", "type": "f32", "default": 0}]}],
       "next_uid": len(entities) + 1, "entities": entities, "resources": {"legs": 0},
       "systems": [
           {"name": "patrol", "doc": "When the Seeker has arrived (NavAgent.status = 2), count the flag, choose the next one of the four and set off.",
            "each": ["NavAgent", "Patrol"], "do": [
                {"if": {"eq": [{"get": "NavAgent.status"}, 2]}, "then": [
                    {"res_add": "legs", "by": 1},
                    {"let": "n", "be": {"mod": [{"add": [{"get": "Patrol.leg"}, 1]}, 4]}},
                    {"set": "Patrol.leg", "to": {"var": "n"}},
                    *[branch(n) for n in range(4)],
                    {"set": "NavAgent.active", "to": True}]},
                {"if": {"eq": [{"get": "NavAgent.status"}, 0]}, "then": [
                    {"set": "NavAgent.target[0]", "to": flags[1][0]}, {"set": "NavAgent.target[1]", "to": 0}, {"set": "NavAgent.target[2]", "to": flags[1][2]},
                    {"set": "Patrol.leg", "to": 1},
                    {"set": "NavAgent.active", "to": True}]}]},
       ]}
path = os.path.join(ROOT, "examples", "maze.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")
print(f"wrote {path}: {len(entities)} entities, seeker uid {seeker}, hero uid {hero}")
