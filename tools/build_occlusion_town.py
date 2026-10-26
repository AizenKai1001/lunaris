"""Write a town to try the wallhack defence on (docs/NETWORK_SCALE.md, "Wallhacks"): a grid of closed buildings (four walls and a roof each, boxes with colliders, nothing drawn), streets between them, spawn points in the
streets, and a `NetHost` with `occlusion` set. It exists to measure what the defence costs and what it hides with a few hundred bots (`tools/occlusion_bench.py`), not to be played.

    python3 tools/build_occlusion_town.py OUT.json [--lots 30] [--pitch 50] [--size 20] [--height 8] [--occlusion 16] [--spawns 200] [--spawn-area M]

`--lots` buildings a side, `--pitch` metres from the middle of one to the next, `--size` the side of a building, `--occlusion` the `NetHost.occlusion` (0: off), `--spawns` how many places the players start from (spread over the streets, or within `--spawn-area` metres of the middle: a crowd).
"""
import json
import random
import sys


def arg(name, default):
    return type(default)(sys.argv[sys.argv.index("--" + name) + 1]) if "--" + name in sys.argv else default


out = sys.argv[1] if len(sys.argv) > 1 and not sys.argv[1].startswith("--") else "occlusion_town.lunaris.json"
lots, pitch, size, height = arg("lots", 30), arg("pitch", 50.0), arg("size", 20.0), arg("height", 8.0)
occlusion, spawns, area = arg("occlusion", 16.0), arg("spawns", 200), arg("spawn-area", 0.0)
thick = 0.5
half = lots * pitch / 2
entities = []
uid = [10]


def add(name, **components):
    uid[0] += 1
    entities.append({"uid": uid[0], "components": {"Name": {"value": name}, **components}})


def box(name, pos, size_xyz):
    add(name, Transform={"pos": [round(v, 2) for v in pos]}, Collider={"shape": 1, "size": [round(v, 2) for v in size_xyz]})


add("Ground", Transform={"pos": [0, -0.5, 0]}, Collider={"shape": 1, "size": [half * 2 + 400, 1, half * 2 + 400]})
add("Server", NetHost={"player": "Avatar", "static_every": 15, "occlusion": occlusion})
for i in range(lots):
    for j in range(lots):
        cx, cz = (i + 0.5) * pitch - half, (j + 0.5) * pitch - half
        s = size / 2
        box(f"Wall {i},{j} N", [cx, height / 2, cz - s], [size, height, thick])
        box(f"Wall {i},{j} S", [cx, height / 2, cz + s], [size, height, thick])
        box(f"Wall {i},{j} W", [cx - s, height / 2, cz], [thick, height, size])
        box(f"Wall {i},{j} E", [cx + s, height / 2, cz], [thick, height, size])
        box(f"Roof {i},{j}", [cx, height + thick / 2, cz], [size, thick, size])
rng = random.Random(7)
for k in range(spawns):
    # (the crossing of two streets: between the buildings)
    while True:
        i, j = rng.randrange(lots - 1), rng.randrange(lots - 1)
        if area <= 0 or max(abs((i + 1) * pitch - half), abs((j + 1) * pitch - half)) <= area:
            break
    add(f"Spawn {k}", Transform={"pos": [round((i + 1) * pitch - half, 2), 0, round((j + 1) * pitch - half, 2)]}, SpawnPoint={})
scene = {
    "lunaris": 1,
    "components": [],
    "next_uid": uid[0] + 100,
    "entities": entities,
    "resources": {},
    "systems": [],
    "prefabs": {"Avatar": {"components": {"Name": {"value": "Avatar"}, "Transform": {}, "HitPoints": {"value": 100, "max": 100, "last": 100}, "NetAvatar": {}}}},
}
with open(out, "w", encoding="utf-8", newline="\n") as f:
    json.dump(scene, f)
    f.write("\n")
print(f"wrote {out}: {len(entities)} entities ({lots * lots} buildings, {spawns} spawn points), the map is {2 * half:.0f} m across, NetHost.occlusion {occlusion}")
