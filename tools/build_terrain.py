"""Write examples/terrain.lunaris.json, its heightmap examples/terrain/hills.png and the playtest examples/tests/terrain.test.json: an island of
hills made from a 16-bit heightmap picture (`Terrain`, docs/TERRAIN.md). A flat meadow lies in the middle with four balls dropped onto it
and a hero on it (WASD walks, Space jumps, the camera rides on the hero); a ball set on a steep slope rolls away.

    python -I tools/build_terrain.py
"""
import json
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import make_heightmap as mh  # noqa: E402

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
N, SIZE, RELIEF, SEED, MEADOW = 257, 256.0, 60.0, 7, 0.42  # samples per side, metres across, metres from black to white
CELL = SIZE / (N - 1)

rows = mh.samples(N, SEED, MEADOW)
os.makedirs(os.path.join(ROOT, "examples", "terrain"), exist_ok=True)
mh.write_png16(os.path.join(ROOT, "examples", "terrain", "hills.png"), rows)


def ground(x, z):
    """The height of the ground at world (x, z), the way the engine works it out: two triangles to a cell, the diagonal from (i+1, j) to (i, j+1)."""
    fx, fz = (x + SIZE / 2) / CELL, (z + SIZE / 2) / CELL
    i, j = max(0, min(N - 2, int(math.floor(fx)))), max(0, min(N - 2, int(math.floor(fz))))
    u, v = fx - i, fz - j
    s = lambda a, b: rows[b][a] / 65535.0 * RELIEF
    p00, p10, p01, p11 = s(i, j), s(i + 1, j), s(i, j + 1), s(i + 1, j + 1)
    return p00 + (p10 - p00) * u + (p01 - p00) * v if u + v <= 1.0 else p11 + (p01 - p11) * (1 - u) + (p10 - p11) * (1 - v)


meadow_y = ground(0, 0)
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})


add("Environment", Environment={"sky": True, "sky_top": [0.16, 0.38, 0.8], "sky_horizon": [0.82, 0.88, 0.95], "ground": [0.28, 0.3, 0.24], "clouds": 0.35, "fog": 0.0025, "fog_color": [0.82, 0.88, 0.95], "bloom": 0.2})
add("Sun", Transform={"rot": [-0.5, 0.32, 0.12, 0.8]}, Light={"kind": 0, "color": [1.0, 0.93, 0.8], "intensity": 2.3})
add("Hills", Transform={"pos": [0, 0, 0]}, Terrain={"heightmap": "examples/terrain/hills.png", "size": [SIZE, SIZE], "height": RELIEF, "step": 1, "color": [0.42, 0.56, 0.34, 1], "texture": "grass", "uv_scale": [96, 96], "friction": 0.9})

add("Sea", Transform={"pos": [0, 1.4, 0], "scale": [SIZE, 0.1, SIZE]}, MeshRenderer={"mesh": "cube", "color": [0.1, 0.32, 0.55, 0.8], "roughness": 0.15, "shininess": 90})

for k, (x, z) in enumerate([(-6, -4), (-2, 5), (3, -7), (7, 3)]):
    add(f"Ball {k + 1}", Transform={"pos": [x, meadow_y + 8 + 2 * k, z]}, Velocity={}, Collider={"shape": 0, "size": [1, 1, 1]}, RigidBody={"mass": 1, "restitution": 0.25, "friction": 0.8},
        MeshRenderer={"mesh": "sphere", "color": [0.9 - 0.15 * k, 0.35 + 0.15 * k, 0.3, 1], "roughness": 0.4})

# a ball on the steepest slope within the middle of the island, outside the meadow
best = (0.0, 0, 0)
for j in range(N // 2 - 70, N // 2 + 70):
    for i in range(N // 2 - 70, N // 2 + 70):
        if math.hypot(i - N // 2, j - N // 2) < 40:
            continue
        slope = math.hypot(rows[j][i + 1] - rows[j][i], rows[j + 1][i] - rows[j][i]) / 65535.0 * RELIEF / CELL
        if slope > best[0]:
            best = (slope, i, j)
slope, bi, bj = best
sx, sz = (bi - (N - 1) / 2) * CELL, (bj - (N - 1) / 2) * CELL
add("Rolling ball", Transform={"pos": [sx, ground(sx, sz) + 0.9, sz]}, Velocity={}, Collider={"shape": 0, "size": [1, 1, 1]}, RigidBody={"mass": 1, "restitution": 0.1, "friction": 0.05},
    MeshRenderer={"mesh": "sphere", "color": [0.95, 0.85, 0.25, 1], "roughness": 0.4})

# pines on the gentle ground above the beach and outside the meadow: decoration only (no collider), at least 7 m apart
rng = random.Random(11)
trees = []
for _ in range(4000):
    if len(trees) >= 140:
        break
    x, z = rng.uniform(-110, 110), rng.uniform(-110, 110)
    if math.hypot(x, z) < 34:
        continue
    gx, gz = (ground(x + 1, z) - ground(x - 1, z)) / 2, (ground(x, z + 1) - ground(x, z - 1)) / 2
    h = ground(x, z)
    if math.atan(math.hypot(gx, gz)) > math.radians(26) or not 3.0 < h < 38.0:
        continue
    if any((x - a) ** 2 + (z - b) ** 2 < 49 for a, b, _ in trees):
        continue
    trees.append((x, z, h))
for k, (x, z, h) in enumerate(trees):
    big = 0.8 + 0.6 * rng.random()
    add(f"Pine {k + 1} trunk", Transform={"pos": [round(x, 3), round(h + 1.2 * big, 3), round(z, 3)], "scale": [0.5 * big, 2.4 * big, 0.5 * big]}, MeshRenderer={"mesh": "cylinder", "color": [0.36, 0.25, 0.16, 1], "roughness": 0.9})
    add(f"Pine {k + 1} top", Transform={"pos": [round(x, 3), round(h + 3.4 * big, 3), round(z, 3)], "scale": [2.2 * big, 3.2 * big, 2.2 * big]}, MeshRenderer={"mesh": "sphere", "color": [0.12, 0.34, 0.16, 1], "roughness": 0.8})

# the hero: the way east is a gentle slope up out of the meadow (found by looking at the picture)
hero = len(entities) + 1
add("Hero", Transform={"pos": [0, meadow_y + 0.3, 12]}, CharacterBody={"radius": 0.35, "height": 1.8, "step_height": 0.4})
add("Hero body", Transform={"pos": [0, meadow_y + 1.2, 12], "scale": [0.7, 1.8, 0.7]}, Parent={"target": hero}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.7, 1.8, 0.7]},
    MeshRenderer={"mesh": "cylinder", "color": [0.95, 0.5, 0.25, 1], "roughness": 0.5})
add("Hero head", Transform={"pos": [0, meadow_y + 2.25, 12], "scale": [0.55, 0.55, 0.55]}, Parent={"target": hero}, LocalTransform={"pos": [0, 1.95, 0], "scale": [0.55, 0.55, 0.55]},
    MeshRenderer={"mesh": "sphere", "color": [1.0, 0.85, 0.7, 1], "roughness": 0.45})
add("Camera", Transform={"pos": [0, meadow_y + 4.5, 20], "rot": [-0.2, 0, 0, 0.98]}, Parent={"target": hero}, LocalTransform={"pos": [0, 4.2, 8], "rot": [-0.2, 0, 0, 0.98]}, Camera={"fov_y": 55, "clear": [0.5, 0.65, 0.85, 1]})
add("Hint", Text={"value": "WASD walk   Space jump", "pos": [0.5, 0.04], "size": 0.04, "align": 1, "color": [1, 1, 1, 1], "shadow": True})

doc = {"lunaris": 1, "entities": entities, "systems": [
    {"name": "walk_input", "doc": "Keys into the character controller: A/D and W/S set the walking speed (6 m/s), Space asks for a jump (6.5 m/s up) when on the ground.",
     "each": ["CharacterBody"], "do": [
         {"set": "CharacterBody.walk[0]", "to": {"mul": [{"sub": [{"res": "input.d"}, {"res": "input.a"}]}, 6]}},
         {"set": "CharacterBody.walk[1]", "to": {"mul": [{"sub": [{"res": "input.s"}, {"res": "input.w"}]}, 6]}},
         {"if": {"gt": [{"res": "input.space"}, 0.5]}, "then": [{"set": "CharacterBody.jump", "to": 6.5}]},
     ]},
]}
path = os.path.join(ROOT, "examples", "terrain.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")

# the playtest: what the numbers of the picture say the world should do
climb = [ground(0, z * 0.5 + 0.25 * dz) for z in range(80, 121) for dz in (-1, 0, 1)]
span = [ground(x, 12 + dz) for x in range(0, 120) for dz in (-1, 0, 1)]
test = {
    "name": "Terrain: balls come to rest on the meadow (15 m up, level ground), a ball on a steep slope rolls away, the hero stands on the ground and walks across it",
    "scene": "examples/terrain.lunaris.json",
    "seed": 1,
    "replay_check": True,
    "steps": [
        {"expect": {"check": "errors", "eq": 0}},
        {"step": 240},
        *[{"expect": {"name": f"Ball {k + 1}", "field": "Transform.pos[1]", "approx": [round(meadow_y + 0.5, 3), 0.06], "message": f"ball {k + 1} lies on the meadow: the ground is {meadow_y:.3f} m up, the ball 0.5 m across its middle"}} for k in range(4)],
        {"expect": {"name": "Rolling ball", "field": "Transform.pos[1]", "lt": round(ground(sx, sz) - 0.6, 3), "message": "the ball set on a steep slope rolled down it"}},
        {"expect": {"name": "Hero", "field": "CharacterBody.grounded", "eq": True, "message": "the hero stands on the ground"}},
        {"expect": {"name": "Hero", "field": "Transform.pos[1]", "approx": [round(meadow_y, 3), 0.03], "message": "with its feet on the meadow"}},
        {"input": {"d": True}},
        {"step": 150},
        {"expect": {"name": "Hero", "field": "CharacterBody.grounded", "eq": True}},
        {"expect": {"name": "Hero", "field": "Transform.pos[0]", "between": [10.0, 16.0], "message": "walked east for 2.5 s at 6 m/s"}},
        {"expect": {"name": "Hero", "field": "Transform.pos[1]", "between": [round(min(span) - 0.05, 3), round(max(span) + 0.05, 3)], "message": "and is still on the ground"}},
        {"input": {"d": False, "a": True}},
        {"step": 150},
        {"input": {"a": False, "s": True}},
        {"step": 480},
        {"expect": {"name": "Hero", "field": "CharacterBody.grounded", "eq": True}},
        {"expect": {"name": "Hero", "field": "Transform.pos[2]", "between": [40.0, 60.0], "message": "walked 8 s towards +z, up the rim of the meadow's bowl (about 20 degrees)"}},
        {"expect": {"name": "Hero", "field": "Transform.pos[1]", "between": [round(min(climb) - 0.05, 3), round(max(climb) + 0.05, 3)], "message": f"and climbed with the ground to {min(climb):.1f} m or more"}},
    ],
}
tpath = os.path.join(ROOT, "examples", "tests", "terrain.test.json")
with open(tpath, "w", encoding="utf-8", newline="\n") as f:
    json.dump(test, f, indent=2)
    f.write("\n")
print(f"wrote {path}, {tpath}: {len(entities)} entities ({len(trees)} pines), meadow at {meadow_y:.3f} m, steepest slope {math.degrees(math.atan(slope)):.0f} degrees at ({sx:.0f}, {sz:.0f}), hero is uid {hero}")
