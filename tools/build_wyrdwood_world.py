"""Builds the scene of a generated Wyrdwood world (examples/wyrdwood/wyrdwood.lunaris.json) from the files Tellus wrote and `tools/wyrdwood_world.py` classified: the terrain in tiles with their splat maps and water, the sea, the
sky and the hours, the hero at the landing, the trees and rocks round it (and where the biome says what grows), the altar of each biome, the camps, the lookout, the HUD and the rules.

    python -I tools/gen_wyrdwood.py [SEED]                                    (the whole pipeline)
    LUNARIS_BIN=path/to/lunaris python -I tools/build_wyrdwood_world.py examples/wyrdwood/worlds/seed23

Only the part of the world near the hero has trees (VEG_RADIUS metres): the whole island is far more than a scene can hold until the engine streams them (docs/GAMEPLAY.md, `StreamZone`).
"""
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import build_wyrdwood as W  # noqa: E402
import make_heightmap as mh  # noqa: E402
import wyrdwood_scripts as S  # noqa: E402
import wyrdwood_world as WW  # noqa: E402

VEG_RADIUS = 380.0
LAYERS = "grass grass uv_scale=1500 bump=0.02\nrock dirt uv_scale=1100 tint=0.85,0.82,0.78 bump=0.06\nsand sand uv_scale=1500 bump=0.01\nsnow concrete uv_scale=1100 tint=1.6,1.6,1.7"
M = "examples/caldera/models"


def hash01(x, z, salt):
    return mh._hash(int(x * 3.17) & 0xFFFF, int(z * 3.17) & 0xFFFF, salt)


def main():
    folder = sys.argv[1] if len(sys.argv) > 1 else "examples/wyrdwood/worlds/seed23"
    info = json.load(open(os.path.join(folder, "world.json")))
    meta = json.load(open(os.path.join(folder, "wyrdwood.json")))
    rel = folder.replace("\\", "/").rstrip("/")
    out_scene = "examples/wyrdwood/wyrdwood.lunaris.json"
    W.BIN = os.environ.get("LUNARIS_BIN", W.BIN)
    size, sea_y = info["size_m"], meta["sea_y"]
    biome_w, biome_h, _depth, biome_rows = WW.read_png(os.path.join(folder, meta["biome_picture"]))
    cell = meta["biome_cell_m"]

    def biome_at(x, z):
        i = max(0, min(biome_w - 1, int((x + size / 2) / cell)))
        j = max(0, min(biome_h - 1, int((z + size / 2) / cell)))
        return biome_rows[j][i] // meta["biome_scale"]

    W.start()
    W.call("reset")
    W.define_all()
    W.dress_world()
    # dress_world's island: take it away, the tiles come instead
    for uid in (3, 20, 21, 22, 34, 35, 36, 37):  # 3 is the engine's TimeOfDay: the sky is the game's own (tools/wyrdwood_sky.py)
        W.call("despawn", {"uid": uid}, quiet=True)
    W.call("add_component", {"uid": 2, "component": "SunTag", "value": {}})
    # the sky for a bigger world
    W.call("set", {"uid": 1, "component": "Environment", "value": {"shadow_distance": 220, "fog": 0.0006}}, quiet=True)
    # ---- the terrain in tiles, with their splat and water
    uid = 100
    tiles = {}
    for t in info["tile_list"]:
        tj = json.load(open(os.path.join(folder, f"heightmap_{t['x']}_{t['y']}.terrain.json")))
        terrain = dict(tj["Terrain"])
        terrain.update({"heightmap": f"{rel}/{terrain['heightmap']}", "color": [1, 1, 1, 1]})
        terrain.pop("texture", None)
        W.spawn(uid, f"Ground {t['x']} {t['y']}", Transform=tj["Transform"], Terrain=terrain, TerrainLayers={"splat": f"{rel}/splat_{t['x']}_{t['y']}.png", "layers": LAYERS})
        wfile = next((f for f in t["files"] if f["kind"] == "water"), None)
        if wfile:
            W.spawn(uid + 50, f"Water {t['x']} {t['y']}", Transform={"pos": [tj["Transform"]["pos"][0], 0, tj["Transform"]["pos"][2]]},
                    WaterBody={"heightmap": f"{rel}/{wfile['file']}", "size": terrain["size"], "relief": wfile.get("scale_m", info["relief_m"]), "color": [0.04, 0.3, 0.4, 0.86], "wave_height": 0.3, "wave_length": 10, "speed": 1.0})
        tiles[(t["x"], t["y"])] = (uid, tj["Transform"]["pos"], terrain["size"])
        uid += 1
    W.spawn(160, "Open sea", Transform={"pos": [0, sea_y - 0.05, 0]}, Water={"size": [size * 3, size * 3], "color": [0.04, 0.28, 0.4, 1.0], "wave_height": 0.3, "wave_length": 14, "speed": 0.9})
    # invisible walls far out at sea
    half = size / 2 + 260
    for i, (px, pz, sx, sz) in enumerate([(0, -half, 2 * half, 4), (0, half, 2 * half, 4), (-half, 0, 4, 2 * half), (half, 0, 4, 2 * half)]):
        W.spawn(170 + i, f"Rough sea {i + 1}", Transform={"pos": [px, sea_y + 20, pz], "scale": [sx, 80, sz]}, Collider={"shape": 1, "size": [sx, 80, sz]})
    W.call("step", {"ticks": 1})  # the terrains are read at the first step
    th = W.th
    sx, sz = meta["start"]["x"], meta["start"]["z"]
    sy = th(sx, sz - 4.0)
    # ---- the hero, its camera, the HUD, the rules
    W.spawn(7, "Camera", Transform={"pos": [sx, sy + 3.5, sz + 6.5]}, Camera={"fov_y": 62, "near": 0.15, "far": 2500},
            CameraRig={"target": 100 + 900, "mode": 1, "look_at": True, "look_offset": [0, 1.55, 0], "follow_speed": 16, "yaw": 0.0, "pitch": 0.3, "distance": 6.6, "shake_decay": 1.8, "shake_amount": 0.5}, CameraCollide={"radius": 0.4, "min_distance": 1.3})
    W.spawn(100 + 900, "Hero", Hero={"stamina": 100, "fx": 0, "fz": -1}, Transform={"pos": [sx, sy + 0.05, sz - 4.0]}, CharacterBody={"radius": 0.38, "height": 1.8, "step_height": 0.55, "max_slope": 52},
            CharacterMotion={"swim_speed": 2.6, "roll_speed": 8.5, "roll_time": 0.62}, HitPoints={"value": 100, "max": 100, "regen": 0.8, "regen_delay": 9},
            MeshRenderer={"mesh": f"{M}/hero.glb", "roughness": 0.8}, Animator={"clip": "idle", "looping": True}, StateMachine={"machine": S.HERO_MACHINE})
    W.hud()
    # ---- what grows where: the trees Tellus listed, near the hero, by the biome
    kinds = {
        "pine": ("pine", [0.7, 3.4, 0.7], 4, 1), "broad": ("broadleaf", [0.8, 3.4, 0.8], 5, 1), "palm": ("palm", [0.5, 3.0, 0.5], 3, 1), "dead": ("deadtree", [0.5, 3.0, 0.5], 3, 1),
    }
    batch, planted = [], 0
    for t in info["tile_list"]:
        lst = json.load(open(os.path.join(folder, f"trees_{t['x']}_{t['y']}.scatter.json")))
        for p in lst["points"]:
            tx, ty, tz = p["Transform"]["pos"]
            if math.hypot(tx - sx, tz - sz) > VEG_RADIUS:
                continue
            b = biome_at(tx, tz)
            r = hash01(tx, tz, 3)
            r2 = hash01(tx, tz, 8)
            if b == 0:
                continue
            if ty < sea_y + 7.5 and ty > sea_y + 1.0:
                k = "palm" if r < 0.6 else "broad"
            elif b == 1:
                k = "broad" if r < 0.62 else "pine"
                if r2 < 0.3:
                    continue
            elif b == 2:
                k = "pine" if r < 0.82 else "broad"
            elif b == 3:
                if r2 < 0.5:
                    continue
                k = "dead" if r < 0.45 else "broad"
            elif b == 4:
                if r2 < 0.85:
                    continue
                k = "pine"
            else:
                if r2 < 0.72:
                    continue
                k = "dead"
            model, box, hp, kind = kinds[k]
            tr = dict(p["Transform"])
            batch.append({"op": "spawn", "components": {"Name": {"value": k.capitalize()}, "Tree": {}, "Choppable": {"kind": kind, "reach": 0.4}, "HitPoints": {"value": hp, "max": hp, "regen": 0}, "Transform": tr,
                                                         "MeshRenderer": {"mesh": f"{M}/{model}.glb", "roughness": 0.9}, "Collider": {"shape": 1, "size": box}}})
            planted += 1
    for i in range(0, len(batch), 400):
        W.call("batch", {"commands": batch[i:i + 400]})
    print(f"  trees: {planted} near the landing")
    # rocks: by the rock layer of each tile that is near the hero
    rocks = 0
    for (tx_, ty_), (tuid, pos, tsize) in tiles.items():
        x0, z0 = pos[0], pos[2]
        area = [max(x0, sx - VEG_RADIUS), max(z0, sz - VEG_RADIUS), min(x0 + tsize[0], sx + VEG_RADIUS), min(z0 + tsize[1], sz + VEG_RADIUS)]
        if area[0] >= area[2] or area[1] >= area[3]:
            continue
        for n, count in ((1, 22), (2, 18), (3, 14)):
            comp = {"Tree": {}, "Choppable": {"kind": 2, "reach": 0.4}, "HitPoints": {"value": 5, "max": 5, "regen": 0}, "MeshRenderer": {"mesh": f"{M}/rock{n}.glb", "roughness": 0.95}, "Collider": {"shape": 1, "size": [1.8, 1.4, 1.8]}}
            r = W.call("scatter", {"terrain": tuid, "components": comp, "count": count, "layer": "rock", "layer_min": 0.35, "slope": [0, 55], "min_distance": 6.0, "scale": [0.7, 2.0], "seed": 40 + n, "yaw": True, "sink": 0.1, "area": area}, quiet=True)
            rocks += r.get("placed", 0)
        r = W.call("scatter", {"terrain": tuid, "components": {"Tree": {}, "Choppable": {"kind": 2, "reach": 0.4}, "HitPoints": {"value": 4, "max": 4, "regen": 0}, "MeshRenderer": {"mesh": f"{M}/rock2.glb", "roughness": 0.95}, "Collider": {"shape": 1, "size": [1.8, 1.4, 1.8]}},
                                    "count": 26, "layer": "grass", "layer_min": 0.6, "slope": [0, 25], "min_distance": 16.0, "scale": [0.5, 1.1], "seed": 60, "yaw": True, "sink": 0.2, "area": area}, quiet=True)
        rocks += r.get("placed", 0)
    print(f"  rocks: {rocks}")
    # ---- places: the lookout at the landing, the camps, the altars
    u_counter = [6000]

    def u():
        u_counter[0] += 1
        return u_counter[0] - 1

    tower_x, tower_z = sx + 26.0, sz - 30.0
    W.lookout(u, tower_x, tower_z)
    for name, a in meta["altars"].items():
        gy = th(a["x"], a["z"])
        W.spawn(u(), f"{name.capitalize()} altar", Prop={}, Transform={"pos": [a["x"], gy + 0.6, a["z"]], "scale": [2.4, 1.2, 1.2]}, MeshRenderer={"mesh": "cube", "color": [0.46, 0.46, 0.5, 1], "roughness": 0.9}, Collider={"shape": 1, "size": [2.4, 1.2, 1.2]})
        W.spawn(u(), f"{name.capitalize()} altar light", Transform={"pos": [a["x"], gy + 2.2, a["z"]]}, Light={"kind": 1, "color": [0.55, 0.8, 1.0], "intensity": 3.0, "range": 18})
        for k in range(6):
            ang = k * math.pi / 3
            px, pz = a["x"] + math.cos(ang) * 7.0, a["z"] + math.sin(ang) * 7.0
            W.spawn(u(), "Standing stone", Prop={}, Transform={"pos": [px, th(px, pz) + 1.7, pz], "scale": [1.1, 3.6, 0.9], "rot": W.quat_y(math.degrees(ang))}, MeshRenderer={"mesh": "cube", "color": [0.42, 0.42, 0.45, 1], "roughness": 0.95}, Collider={"shape": 1, "size": [1.1, 3.6, 0.9]})
    for k, c in enumerate(meta["camps"]):
        gy = th(c["x"], c["z"])
        W.spawn(u(), "Camp fire", Transform={"pos": [c["x"], gy + 0.6, c["z"]]}, Light={"kind": 1, "color": [1.0, 0.62, 0.28], "intensity": 3.0, "range": 20})
        W.spawn(u(), "Camp crate", Prop={}, Transform={"pos": [c["x"] + 2.2, gy + 0.55, c["z"] + 1.0], "scale": [1.1, 1.1, 1.1]}, MeshRenderer={"mesh": "cube", "color": [0.55, 0.4, 0.22, 1], "roughness": 0.85}, Collider={"shape": 1, "size": [1.1, 1.1, 1.1]})
    res = W.call("define_script", {"source": S.ALL})
    print("define_script:", json.dumps(res)[:200])
    W.call("check")
    saved = W.call("save_scene", {"path": out_scene})
    print("saved", out_scene, saved)
    print("failed requests:", W.failed)
    W.proc.stdin.close()
    W.proc.wait(timeout=300)


if __name__ == "__main__":
    main()
