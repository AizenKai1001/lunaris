"""Write examples/walker.lunaris.json: a character controller playground. WASD walks (the camera rides on the hero), Space jumps. There is
a wall to bump into on the left, a staircase of five steps up to a platform on the right, a ledge to walk off, a tall block to jump
onto and a ramp-like pile of spheres. The hero is one entity with a `CharacterBody`; a three-statement graph system turns the keys
into `walk` and `jump`; the engine does the moving, colliding, stepping and falling.

    python -I tools/build_walker.py
"""
import json
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})


def block(name, pos, size, color, texture="", uv=None):
    """A solid box: a mesh and a Collider of the same size (a unit cube scaled)."""
    mesh = {"mesh": "cube", "color": color + [1], "roughness": 0.8}
    if texture:
        mesh.update({"texture": texture, "uv_scale": uv or [1, 1]})
    add(name, Transform={"pos": pos, "scale": size}, MeshRenderer=mesh, Collider={"shape": 1, "size": [1, 1, 1]})


add("Environment", Environment={"sky": True, "sky_top": [0.16, 0.38, 0.8], "sky_horizon": [0.82, 0.88, 0.95], "ground": [0.28, 0.3, 0.24], "clouds": 0.3, "fog": 0.004, "fog_color": [0.82, 0.88, 0.95], "bloom": 0.2})
add("Sun", Transform={"rot": [-0.5, 0.32, 0.12, 0.8]}, Light={"kind": 0, "color": [1.0, 0.93, 0.8], "intensity": 2.3})
add("Ground", Transform={"pos": [0, -0.5, 0], "scale": [80, 1, 60]}, MeshRenderer={"mesh": "cube", "texture": "grass", "uv_scale": [32, 24], "color": [0.8, 0.9, 0.8, 1], "roughness": 0.95}, Collider={"shape": 1, "size": [1, 1, 1]})
block("Wall", [-6, 1.5, 0], [1, 3, 8], [0.75, 0.45, 0.35], "bricks", [1, 1])
for i in range(5):
    h = 0.24 * (i + 1)
    block(f"Step {i + 1}", [4 + i, h / 2, 0], [1, h, 5], [0.7, 0.7, 0.72], "stone", [1, 1])
block("Platform", [13.5, 0.6, 0], [10, 1.2, 8], [0.7, 0.7, 0.72], "concrete", [4, 3])
block("Tall block", [-2, 0.9, 6], [2, 1.8, 2], [0.6, 0.65, 0.85], "tiles", [1, 1])
block("Low block", [2, 0.3, 6], [2, 0.6, 2], [0.85, 0.65, 0.4], "wood", [1, 1])
hero = len(entities) + 1
add("Hero", Transform={"pos": [0, 0, 0]}, CharacterBody={"radius": 0.35, "height": 1.8, "step_height": 0.4})
add("Hero body", Transform={"pos": [0, 0.9, 0], "scale": [0.7, 1.8, 0.7]}, Parent={"target": hero}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.7, 1.8, 0.7]},
    MeshRenderer={"mesh": "cylinder", "color": [0.95, 0.5, 0.25, 1], "roughness": 0.5})
add("Hero head", Transform={"pos": [0, 1.95, 0], "scale": [0.55, 0.55, 0.55]}, Parent={"target": hero}, LocalTransform={"pos": [0, 1.95, 0], "scale": [0.55, 0.55, 0.55]},
    MeshRenderer={"mesh": "sphere", "color": [1.0, 0.85, 0.7, 1], "roughness": 0.45})
add("Camera", Transform={"pos": [0, 4.2, 8], "rot": [-0.2, 0, 0, 0.98]}, Parent={"target": hero}, LocalTransform={"pos": [0, 4.2, 8], "rot": [-0.2, 0, 0, 0.98]}, Camera={"fov_y": 55, "clear": [0.5, 0.65, 0.85, 1]})
add("Hint", Text={"value": "WASD move   Space jump", "pos": [0.5, 0.04], "size": 0.04, "align": 1, "color": [1, 1, 1, 1], "shadow": True})

doc = {"lunaris": 1, "entities": entities, "systems": [
    {"name": "walk_input", "doc": "Keys into the character controller: A/D and W/S set the walking speed (5 m/s), Space asks for a jump (6.5 m/s up) when on the ground.",
     "each": ["CharacterBody"], "do": [
         {"set": "CharacterBody.walk[0]", "to": {"mul": [{"sub": [{"res": "input.d"}, {"res": "input.a"}]}, 5]}},
         {"set": "CharacterBody.walk[1]", "to": {"mul": [{"sub": [{"res": "input.s"}, {"res": "input.w"}]}, 5]}},
         {"if": {"gt": [{"res": "input.space"}, 0.5]}, "then": [{"set": "CharacterBody.jump", "to": 6.5}]},
     ]},
]}
path = os.path.join(ROOT, "examples", "walker.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")
print(f"wrote {path}: {len(entities)} entities, hero is uid {hero}")
