"""Write examples/lighting_showcase.lunaris.json and the playtest examples/tests/lighting_showcase.test.json: a bright courtyard with a glossy floor, a row of metal spheres, a puddle and a low sun
in the camera's view, next to a dark cellar (a cave-like room with torches) whose door stands open: the four lighting effects of the GPU renderer's high qualities at work (docs/GPU.md).

    python -I tools/build_lighting_showcase.py

* `AutoExposure`: walk into the cellar (W, A, S, D move the camera) and the picture first goes dark and then brightens as the eye adapts; walk out and it is dazzled for a moment.
* `Ssr`: the polished stone floor, the puddle and the spheres mirror the facade, the columns, the lanterns and each other.
* `LensFlare`: the sun stands above the cellar's roof in front of the camera, with ghosts and a halo; step behind a column or turn away and it goes.
* `ReflectionProbe`: one in the middle of the courtyard (the spheres reflect the courtyard and the building and not only the sky) and one in the cellar (its walls are what the metal in there reflects).

Everything is made from the standard library and the numbers below, so the same bytes come out every time. The playtest walks the camera through the door and checks the resource `scene.indoors`
that a graph system sets from the camera's place (what the eye does is the renderer's business and never enters the simulation: the `render` tool's reply says it in `measured.exposure`).
"""
import json
import math
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")


def norm(v):
    n = math.sqrt(sum(c * c for c in v))
    return [c / n for c in v]


def quat_from_z_to(s):
    """The rotation that turns +z into the unit vector s: a Light's rotation, since a light shines from `rot * (0, 0, 1)` towards the scene."""
    x, y, z, w = -s[1], s[0], 0.0, 1.0 + s[2]
    n = math.sqrt(x * x + y * y + z * z + w * w)
    return [round(x / n, 6), round(y / n, 6), round(z / n, 6), round(w / n, 6)]


def pitch_quat(radians):
    return [round(math.sin(radians / 2), 6), 0, 0, round(math.cos(radians / 2), 6)]


entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


def mesh(kind, color, rough=-1.0, metal=0.0, emissive=0.0, tex=None, uv=(1, 1), shadow=True):
    m = {"mesh": kind, "color": color, "roughness": rough, "metallic": metal, "emissive": emissive, "cast_shadow": shadow}
    if tex:
        m["texture"] = tex
        m["uv_scale"] = list(uv)
    return m


def box(name, pos, size, color, **kw):
    return add(name, Transform={"pos": pos, "scale": size}, MeshRenderer=mesh("cube", color, **kw))


# ---- the sky, the sun and the picture effects
SUN_TOWARDS = norm([-0.25, 0.42, -0.87])  # where the sun is: ahead of the camera (towards -z), a little left of it, about 25 degrees up
add("Environment", Environment={"sky": True, "sky_top": [0.1, 0.26, 0.62], "sky_horizon": [0.72, 0.8, 0.92], "ground": [0.22, 0.23, 0.26], "clouds": 0.4, "fog": 0.0011, "fog_color": [0.7, 0.78, 0.9],
                                "bloom": 0.25, "ssao": 1.0, "vignette": 0.2, "shadow_distance": 130})
add("Sun", Transform={"rot": quat_from_z_to(SUN_TOWARDS)}, Light={"kind": 0, "color": [1.0, 0.93, 0.8], "intensity": 4.5})
add("Eye adaptation", AutoExposure={"min": 0.08, "max": 14, "speed": 1.5, "target": 0.18})
add("Reflections", Ssr={"strength": 1.0, "max_distance": 70, "thickness": 0.6})
add("Lens flare", LensFlare={"intensity": 0.8, "ghosts": 5, "halo": 0.5})
add("Courtyard probe", Transform={"pos": [0, 2.5, -6]}, ReflectionProbe={"size": [80, 14, 80], "resolution": 128})
add("Cellar probe", Transform={"pos": [0, 2.75, -27.25]}, ReflectionProbe={"size": [24, 5.5, 15.5], "resolution": 128})

# ---- the courtyard: polished dark stone, a row of metal spheres, columns, lanterns, a puddle
add("Courtyard floor", Transform={"pos": [0, 0, 0], "scale": [90, 1, 90]}, MeshRenderer=mesh("plane", [0.22, 0.24, 0.29, 1], rough=0.07, tex="tiles", uv=(26, 26)))
add("Puddle", Transform={"pos": [10.5, 0.03, 4.0]}, Water={"size": [9, 6], "color": [0.03, 0.1, 0.14, 0.72], "wave_height": 0.03, "wave_length": 5, "speed": 0.6})
spheres = [("Gold", [1.0, 0.77, 0.33, 1], 0.16), ("Copper", [0.95, 0.52, 0.36, 1], 0.24), ("Chrome", [0.95, 0.95, 1.0, 1], 0.03), ("Blue steel", [0.36, 0.5, 0.9, 1], 0.12), ("Iron", [0.5, 0.5, 0.55, 1], 0.42)]
for k, (name, colour, rough) in enumerate(spheres):
    add(f"{name} sphere", Transform={"pos": [round(-5.6 + 2.8 * k, 3), 1.0, 1.0], "scale": [2, 2, 2]}, MeshRenderer=mesh("sphere", colour, rough=rough, metal=1.0))
for k, x in enumerate([-14.0, -7.5, 7.5, 14.0]):
    add(f"Column {k + 1}", Transform={"pos": [x, 3.0, -11.0], "scale": [1.3, 6, 1.3]}, MeshRenderer=mesh("cylinder", [0.82, 0.79, 0.72, 1], rough=0.28, tex="concrete", uv=(1, 3)))
for k, x in enumerate([-10.0, 10.0]):
    add(f"Lantern post {k + 1}", Transform={"pos": [x, 1.6, -7.0], "scale": [0.18, 3.2, 0.18]}, MeshRenderer=mesh("cylinder", [0.08, 0.08, 0.09, 1], rough=0.4, metal=1.0))
    add(f"Lantern {k + 1}", Transform={"pos": [x, 3.4, -7.0], "scale": [0.45, 0.45, 0.45]}, MeshRenderer=mesh("sphere", [1.0, 0.85, 0.55, 1], emissive=6.0, shadow=False))
    add(f"Lantern light {k + 1}", Transform={"pos": [x, 3.4, -7.0]}, Light={"kind": 1, "color": [1.0, 0.8, 0.5], "intensity": 3.0, "range": 12})

# ---- the cellar: a room 24 by 15.5 metres, 4.5 high, in the facade at z = -20, its door 4 wide and 3.2 high, lit by torches
stone = dict(rough=0.8, tex="stone", uv=(3, 1.5))
box("Facade left", [-8.0, 2.75, -20], [10, 5.5, 1], [0.5, 0.46, 0.42, 1], **stone)
box("Facade right", [8.0, 2.75, -20], [10, 5.5, 1], [0.5, 0.46, 0.42, 1], **stone)
box("Lintel", [0, 4.65, -20], [6, 1.7, 1], [0.5, 0.46, 0.42, 1], **stone)
box("Cellar left wall", [-12.5, 2.75, -27.25], [1, 5.5, 15.5], [0.4, 0.37, 0.34, 1], **stone)
box("Cellar right wall", [12.5, 2.75, -27.25], [1, 5.5, 15.5], [0.4, 0.37, 0.34, 1], **stone)
box("Cellar back wall", [0, 2.75, -35], [26, 5.5, 1], [0.4, 0.37, 0.34, 1], **stone)
box("Roof", [0, 6.0, -27.25], [27, 1, 16.5], [0.34, 0.32, 0.3, 1], rough=0.9)
add("Cellar floor", Transform={"pos": [0, 0.02, -27.25], "scale": [24, 1, 15.5]}, MeshRenderer=mesh("plane", [0.3, 0.26, 0.23, 1], rough=0.55, tex="stone", uv=(8, 5)))
for k, x in enumerate([-8.5, 8.5]):
    add(f"Pillar {k + 1}", Transform={"pos": [x, 2.75, -27], "scale": [1.3, 5.5, 1.3]}, MeshRenderer=mesh("cylinder", [0.55, 0.52, 0.48, 1], rough=0.7, tex="concrete", uv=(1, 2)))
box("Plinth", [0, 0.45, -31], [1.8, 0.9, 1.8], [0.35, 0.33, 0.31, 1], rough=0.5)
add("Treasure sphere", Transform={"pos": [0, 1.75, -31], "scale": [1.6, 1.6, 1.6]}, MeshRenderer=mesh("sphere", [1.0, 0.77, 0.33, 1], rough=0.1, metal=1.0))
box("Chest", [-7.5, 0.45, -32.5], [2.2, 0.9, 1.3], [0.75, 0.78, 0.82, 1], rough=0.15, metal=1.0)
add("Silver ball", Transform={"pos": [6.5, 0.7, -30], "scale": [1.4, 1.4, 1.4]}, MeshRenderer=mesh("sphere", [0.93, 0.94, 0.97, 1], rough=0.05, metal=1.0))
for k, (x, z) in enumerate([(-4.2, -24.5), (4.2, -24.5), (-11.2, -32.0), (11.2, -32.0)]):
    add(f"Torch {k + 1}", Transform={"pos": [x, 2.6, z], "scale": [0.28, 0.28, 0.28]}, MeshRenderer=mesh("sphere", [1.0, 0.65, 0.3, 1], emissive=3.0, shadow=False))
    add(f"Torch light {k + 1}", Transform={"pos": [x * 0.95, 2.7, z]}, Light={"kind": 1, "color": [1.0, 0.62, 0.3], "intensity": 3.4, "range": 13})

# ---- the camera, and a hint
START = [0, 1.9, 9]
add("Camera", Transform={"pos": START, "rot": pitch_quat(0.1)}, Camera={"fov_y": 55, "far": 600, "clear": [0.06, 0.07, 0.1, 1]})
add("Hint", Text={"value": "W A S D walk: into the cellar and out again", "pos": [0.5, 0.93], "size": 0.034, "align": 1, "color": [1, 1, 1, 1], "shadow": True})

doc = {"lunaris": 1, "entities": entities, "systems": [
    {"name": "walk", "doc": "W and S walk the camera along z, A and D along x (6 m/s): nothing stops it, it is a camera.",
     "each": ["Camera", "Transform"], "do": [
         {"set": "Transform.pos[2]", "to": {"add": [{"get": "Transform.pos[2]"}, {"mul": [{"sub": [{"res": "input.s"}, {"res": "input.w"}]}, 6, {"dt": True}]}]}},
         {"set": "Transform.pos[0]", "to": {"add": [{"get": "Transform.pos[0]"}, {"mul": [{"sub": [{"res": "input.d"}, {"res": "input.a"}]}, 6, {"dt": True}]}]}},
     ]},
    {"name": "indoors", "doc": "The resource scene.indoors is 1 while the camera is inside the cellar (past the door at z = -20) and 0 outside: where the renderer's eye adaptation has something to do.",
     "each": ["Camera", "Transform"], "order": 150, "do": [
         {"if": {"lt": [{"get": "Transform.pos[2]"}, -20.5]}, "then": [{"res_set": "scene.indoors", "to": 1}], "else": [{"res_set": "scene.indoors", "to": 0}]},
     ]},
]}
path = os.path.join(ROOT, "examples", "lighting_showcase.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")
print(f"wrote {path}: {len(entities)} entities")

# ---- the playtest: the world a camera walks through (the picture's own business, the eye, is the renderer's and is checked by the GPU tests)
speed = 6.0
steps = [
    {"expect": {"check": "errors", "eq": 0, "message": "the scene has no mistakes"}},
    {"expect": {"name": "Camera", "field": "Transform.pos[2]", "eq": START[2], "message": "the camera starts in the courtyard"}},
    {"step": 2},
    {"expect": {"resource": "scene.indoors", "eq": 0, "message": "outdoors, in the sun"}},
    {"expect": {"count": {"with": ["ReflectionProbe"]}, "eq": 2, "message": "two reflection probes: the courtyard and the cellar"}},
    {"expect": {"count": {"with": ["Light"]}, "eq": 7, "message": "the sun, two lanterns and four torches: seven lights"}},
    {"note": "walk to the door and in"},
    {"input": {"w": True}},
    {"until": {"resource": "scene.indoors", "eq": 1, "max_ticks": 900}},
    {"expect": {"name": "Camera", "field": "Transform.pos[2]", "between": [round(-20.5 - speed / 60 * 1.5, 3), -20.5], "message": "the camera has just passed the door"}},
    {"step": 120},
    {"expect": {"name": "Camera", "field": "Transform.pos[2]", "between": [-34.5, -26.0], "message": "deeper in the cellar"}},
    {"expect": {"resource": "scene.indoors", "eq": 1}},
    {"note": "and back out into the sun"},
    {"input": {"w": False, "s": True}},
    {"until": {"resource": "scene.indoors", "eq": 0, "max_ticks": 1200}},
    {"input": {"s": False}},
    {"expect": {"name": "Camera", "field": "Transform.pos[2]", "between": [-20.5, round(-20.5 + speed / 60 * 1.5, 3)], "message": "out of the door again"}},
    {"expect": {"check": "errors", "eq": 0}},
]
test = {"name": "Lighting showcase: the camera walks from the bright courtyard through the door into the dark cellar and back; the scene has no mistakes (the eye adaptation, reflections, flare and probes are drawn by the GPU renderer: docs/GPU.md)",
        "scene": "examples/lighting_showcase.lunaris.json", "seed": 1, "replay_check": True, "steps": steps}
tpath = os.path.join(ROOT, "examples", "tests", "lighting_showcase.test.json")
with open(tpath, "w", encoding="utf-8", newline="\n") as f:
    json.dump(test, f, indent=2)
    f.write("\n")
print(f"wrote {tpath}")
