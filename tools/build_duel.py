"""Write examples/duel.lunaris.json: a two-player game for lockstep multiplayer (docs/NETWORK.md). Two heroes (orange and blue) run around
one arena collecting glowing orbs; the first to 12 of the 24 wins. Each player uses WASD and Space on their own computer: the host maps
the first player's keys to `input.<key>` and the second player's to `input.p2.<key>`, and the scene reads both.

    lunaris host --players 2
    lunaris play examples/duel.lunaris.json --join 127.0.0.1:7000 --name Ada      (in two windows or on two computers)

    python -I tools/build_duel.py
"""
import json
import math
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})


def block(name, pos, size, color, texture="", uv=None):
    mesh = {"mesh": "cube", "color": color + [1], "roughness": 0.8}
    if texture:
        mesh.update({"texture": texture, "uv_scale": uv or [1, 1]})
    add(name, Transform={"pos": pos, "scale": size}, MeshRenderer=mesh, Collider={"shape": 1, "size": [1, 1, 1]})


add("Environment", Environment={"sky": True, "sky_top": [0.1, 0.22, 0.55], "sky_horizon": [0.7, 0.62, 0.78], "ground": [0.2, 0.2, 0.25], "clouds": 0.2, "fog": 0.003, "fog_color": [0.7, 0.62, 0.78], "bloom": 0.35})
add("Sun", Transform={"rot": [-0.55, 0.3, 0.1, 0.77]}, Light={"kind": 0, "color": [1.0, 0.9, 0.78], "intensity": 2.0})
add("Floor", Transform={"pos": [0, -0.5, 0], "scale": [34, 1, 34]}, MeshRenderer={"mesh": "cube", "texture": "tiles", "uv_scale": [17, 17], "color": [0.75, 0.78, 0.9, 1], "roughness": 0.6}, Collider={"shape": 1, "size": [1, 1, 1]})
for i, (pos, size) in enumerate([([0, 1, -17], [34, 2, 1]), ([0, 1, 17], [34, 2, 1]), ([-17, 1, 0], [1, 2, 34]), ([17, 1, 0], [1, 2, 34])]):
    block(f"Wall {i + 1}", pos, size, [0.55, 0.4, 0.5], "bricks", [8, 1])
for i, (x, z) in enumerate([(-6, -6), (6, -6), (-6, 6), (6, 6), (0, 0)]):
    block(f"Pillar {i + 1}", [x, 1.5, z], [2.4, 3, 2.4], [0.5, 0.55, 0.8], "stone", [1, 1])
block("Ramp block", [0, 0.4, -11], [6, 0.8, 3], [0.8, 0.7, 0.45], "wood", [2, 1])

for i in range(24):
    ring, k = (0, i) if i < 12 else (1, i - 12)
    r = 8.0 if ring == 0 else 13.0
    a = (k / 12.0) * math.tau + (0.26 if ring else 0.0)
    add(f"Orb {i + 1}", Transform={"pos": [round(r * math.cos(a), 3), 0.7, round(r * math.sin(a), 3)], "scale": [0.6, 0.6, 0.6]},
        MeshRenderer={"mesh": "sphere", "color": [1.0, 0.82, 0.25, 1], "emissive": 2.5, "roughness": 0.2, "cast_shadow": False},
        Collider={"shape": 0, "size": [1, 1, 1], "trigger": True}, Orb={})


def hero(name, tag, pos, color):
    uid = len(entities) + 1
    add(name, Transform={"pos": pos}, CharacterBody={"radius": 0.4, "height": 1.8, "step_height": 0.5}, Collider={"shape": 0, "size": [1.6, 1.6, 1.6], "trigger": True}, **{tag: {}})
    add(name + " body", Transform={"pos": [pos[0], pos[1] + 0.9, pos[2]], "scale": [0.8, 1.8, 0.8]}, Parent={"target": uid}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.8, 1.8, 0.8]},
        MeshRenderer={"mesh": "cylinder", "color": color + [1], "roughness": 0.45})
    add(name + " head", Transform={"pos": [pos[0], pos[1] + 2.0, pos[2]], "scale": [0.6, 0.6, 0.6]}, Parent={"target": uid}, LocalTransform={"pos": [0, 2.0, 0], "scale": [0.6, 0.6, 0.6]},
        MeshRenderer={"mesh": "sphere", "color": [1.0, 0.9, 0.78, 1], "roughness": 0.4})


hero("Orange", "P1", [-11, 0, 0], [0.98, 0.5, 0.2])
hero("Blue", "P2", [11, 0, 0], [0.25, 0.5, 0.98])
add("Camera", Transform={"pos": [0, 30, 17], "rot": [-0.43, 0, 0, 0.9]}, Camera={"fov_y": 55, "clear": [0.1, 0.12, 0.2, 1]})
add("Orange score", Text={"value": "Orange {score1} / 12", "pos": [0.02, 0.03], "size": 0.06, "color": [1.0, 0.65, 0.35, 1], "align": 0, "shadow": True})
add("Blue score", Text={"value": "Blue {score2} / 12", "pos": [0.98, 0.03], "size": 0.06, "color": [0.45, 0.65, 1.0, 1], "align": 2, "shadow": True})
add("Hint", Text={"value": "WASD move, Space jump: first to 12 orbs wins", "pos": [0.5, 0.93], "size": 0.04, "color": [0.9, 0.92, 1, 0.85], "align": 1, "shadow": True})
add("Orange wins", Text={"value": "ORANGE WINS", "pos": [0.5, 0.4], "size": 0.16, "color": [1.0, 0.65, 0.35, 1], "align": 1, "shadow": True, "visible": False}, P1Banner={})
add("Blue wins", Text={"value": "BLUE WINS", "pos": [0.5, 0.4], "size": 0.16, "color": [0.45, 0.65, 1.0, 1], "align": 1, "shadow": True, "visible": False}, P2Banner={})


def walk(prefix):
    return [
        {"set": "CharacterBody.walk[0]", "to": {"mul": [{"sub": [{"res": prefix + "d"}, {"res": prefix + "a"}]}, 6]}},
        {"set": "CharacterBody.walk[1]", "to": {"mul": [{"sub": [{"res": prefix + "s"}, {"res": prefix + "w"}]}, 6]}},
        {"if": {"gt": [{"res": prefix + "space"}, 0.5]}, "then": [{"set": "CharacterBody.jump", "to": 7}]},
    ]


doc = {"lunaris": 1,
       "components": [{"name": c, "fields": []} for c in ("P1", "P2", "Orb", "P1Banner", "P2Banner")],
       "next_uid": len(entities) + 1, "entities": entities, "resources": {"score1": 0, "score2": 0},
       "systems": [
           {"name": "p1_walk", "doc": "The first player's keys (input.<key>) move the orange hero.", "each": ["P1", "CharacterBody"], "do": walk("input.")},
           {"name": "p2_walk", "doc": "The second player's keys (input.p2.<key>) move the blue hero.", "each": ["P2", "CharacterBody"], "do": walk("input.p2.")},
           {"name": "p1_pick", "doc": "Touching an orb scores for orange and removes it.", "on_contact": {"a": ["P1"], "b": ["Orb"]}, "do": [{"res_add": "score1", "by": 1}, {"emit": "coin"}, {"despawn": "b"}]},
           {"name": "p2_pick", "doc": "Touching an orb scores for blue and removes it.", "on_contact": {"a": ["P2"], "b": ["Orb"]}, "do": [{"res_add": "score2", "by": 1}, {"emit": "coin"}, {"despawn": "b"}]},
           {"name": "p1_banner", "order": 140, "each": ["P1Banner", "Text"], "do": [{"set": "Text.visible", "to": {"ge": [{"res": "score1"}, 12]}}]},
           {"name": "p2_banner", "order": 140, "each": ["P2Banner", "Text"], "do": [{"set": "Text.visible", "to": {"ge": [{"res": "score2"}, 12]}}]},
       ]}
path = os.path.join(ROOT, "examples", "duel.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")
print(f"wrote {path}: {len(entities)} entities")
