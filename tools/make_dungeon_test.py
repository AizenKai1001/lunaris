"""Write examples/tests/dungeon.test.json: a playtest that walks the generated dungeon from the start to the exit.

The scene (`examples/dungeon.lunaris.json`) is read as data: the autotiled map says which cells are floor (tile 0), the hero's and the exit's positions say where
to go. A breadth-first search finds the shortest way between those cells, and every straight stretch of it becomes "hold this key for N ticks" (the hero walks 5 units
a second, a cell is 1 unit, the simulation runs 60 ticks a second: 12 ticks a cell, so the hero turns exactly at the middle of a cell). The test then expects `won`.

    python -I tools/make_dungeon_test.py
"""
import collections
import json
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
scene = json.load(open(os.path.join(ROOT, "examples", "dungeon.lunaris.json"), encoding="utf-8"))


def entity(name):
    for e in scene["entities"]:
        if e["components"].get("Name", {}).get("value") == name:
            return e["components"]
    raise SystemExit(f"no entity named {name}")


tilemap = entity("Dungeon")["Tilemap"]
w, h = tilemap["size"]
tile = tilemap["tile"]
rows = [[int(n) for n in line.split()] for line in tilemap["data"].split("\n")]
floor = [[rows[y][x] == 0 for x in range(w)] for y in range(h)]


def cell_of(pos):
    # the map is centred on the origin (the entity is at 0, 0), the top row first
    return int((pos[0] / tile) + w / 2), int(h / 2 - (pos[1] / tile))


start = cell_of(entity("Hero")["Transform"]["pos"])
goal = cell_of(entity("Exit")["Transform"]["pos"])
assert floor[start[1]][start[0]] and floor[goal[1]][goal[0]], (start, goal)

# breadth first, neighbours in a fixed order so the shortest way is always the same one
came = {start: None}
queue = collections.deque([start])
while queue:
    c = queue.popleft()
    if c == goal:
        break
    for key, (dx, dy) in (("d", (1, 0)), ("a", (-1, 0)), ("s", (0, 1)), ("w", (0, -1))):
        n = (c[0] + dx, c[1] + dy)
        if 0 <= n[0] < w and 0 <= n[1] < h and floor[n[1]][n[0]] and n not in came:
            came[n] = (c, key)
            queue.append(n)
keys = []
c = goal
while came[c] is not None:
    c, key = came[c][0], came[c][1]
    keys.append(key)
keys.reverse()
runs = []
for k in keys:
    if runs and runs[-1][0] == k:
        runs[-1][1] += 1
    else:
        runs.append([k, 1])

guards = [e["uid"] for e in scene["entities"] if "Enemy" in e["components"]]
steps = [{"expect": {"check": "errors", "eq": 0}}, {"step": 30}, {"expect": {"resource": "hero.hp", "eq": 100, "message": "nobody has found the hero yet"}}]
# the guards would block the corridors (they are solid and the hero cannot push past them at the same speed): this test is about the level, so they go home first
steps += [{"call": {"tool": "despawn", "args": {"uid": uid}}} for uid in guards]
for k, cells in runs:
    steps += [{"input": {k: True}}, {"step": 12 * cells}, {"input": {k: False}}]
steps += [
    {"step": 10},
    {"expect": {"resource": "won", "eq": 1, "message": f"the hero walked the {len(keys)} cells from the start to the golden ring and won"}},
    {"expect": {"resource": "hero.hp", "gt": 0, "message": "and is still on its feet"}},
]
spec = {"name": f"Dungeon: walk the shortest way ({len(keys)} cells, {len(runs)} straight stretches) through a generated level to the exit (generate_map, autotile, tilemap_colliders, contacts)", "scene": "examples/dungeon.lunaris.json", "seed": 1, "replay_check": True, "steps": steps}
out = os.path.join(ROOT, "examples", "tests", "dungeon.test.json")
json.dump(spec, open(out, "w", encoding="utf-8", newline="\n"), indent=2)
print(f"wrote {os.path.relpath(out, ROOT)}: {len(keys)} cells in {len(runs)} stretches")

# ---- a second test: a guard that sees the hero hunts it down and bites
hero_cell = start
gx, gy = start[0] + 2, start[1]
assert floor[gy][gx], "the start room is wide enough for a guard two cells away"
guard_pos = [(gx + 0.5 - w / 2) * tile, (h / 2 - gy - 0.5) * tile, 0]
spec2 = {
    "name": "Dungeon: a guard that sees the hero hunts it and bites (Sensor with line of sight, chase, contacts, HitPoints)",
    "scene": "examples/dungeon.lunaris.json",
    "seed": 1,
    "replay_check": True,
    "steps": [
        {"expect": {"entity": 1100, "field": "Enemy.alert", "eq": 0, "message": "the guards start calm"}},
        {"call": {"tool": "set", "args": {"uid": 1100, "component": "Transform", "value": {"pos": guard_pos}}}},
        {"step": 20},
        {"expect": {"entity": 1100, "field": "Enemy.alert", "gt": 0, "message": "the guard sees the hero across the room and is alerted"}},
        {"step": 120},
        {"expect": {"resource": "hero.hp", "lt": 100, "message": "the guard caught the hero and bit it"}},
        {"expect": {"resource": "hero.hp", "gt": 0, "message": "the hero survives one guard"}},
        {"expect": {"entity": 1101, "field": "Enemy.alert", "eq": 0, "message": "a guard behind walls never noticed anything"}},
    ],
}
out2 = os.path.join(ROOT, "examples", "tests", "dungeon_guards.test.json")
json.dump(spec2, open(out2, "w", encoding="utf-8", newline="\n"), indent=2)
print(f"wrote {os.path.relpath(out2, ROOT)}")
