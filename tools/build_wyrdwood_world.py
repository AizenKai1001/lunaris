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

def qmul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return [aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz]


def qrot(q, v):
    x, y, z = v
    qx, qy, qz, qw = q
    tx, ty, tz = 2 * (qy * z - qz * y), 2 * (qz * x - qx * z), 2 * (qx * y - qy * x)
    return [x + qw * tx + qy * tz - qz * ty, y + qw * ty + qz * tx - qx * tz, z + qw * tz + qx * ty - qy * tx]


def equipment(hero_uid):
    """What the hero holds, drawn in its right hand (a `BoneAttach` to the hand joint, shown while the hotbar's selected slot holds that item: `HeldPart`), and the hair and beard on its head."""
    import wyrdwood_items as I
    # the grip: the item's own frame has the handle along +y and the head towards +x; this turns it to sit in the fist, pointing up and forward
    base = [-0.7071068, 0.0, 0.0, 0.7071068]
    hold = [0.0, 0.0, 0.03]
    wood, steel, flint = [0.45, 0.3, 0.18, 1], [0.62, 0.62, 0.68, 1], [0.28, 0.33, 0.4, 1]
    handle = ("cylinder", [0.045, 0.6, 0.045], [0, 0.18, 0], wood)
    parts = {
        "stone_axe": [handle, ("cube", [0.22, 0.13, 0.05], [0.09, 0.43, 0], steel)],
        "flint_axe": [handle, ("cube", [0.22, 0.13, 0.05], [0.09, 0.43, 0], flint)],
        "club": [("cylinder", [0.075, 0.62, 0.075], [0, 0.2, 0], wood), ("sphere", [0.13, 0.18, 0.13], [0, 0.5, 0], [0.5, 0.34, 0.2, 1])],
        "hammer": [handle, ("cube", [0.2, 0.12, 0.12], [0, 0.46, 0], steel)],
        "torch": [("cylinder", [0.05, 0.42, 0.05], [0, 0.12, 0], wood), ("sphere", [0.12, 0.17, 0.12], [0, 0.38, 0], [1.0, 0.55, 0.15, 1])],
        "horn_pick": [handle, ("cube", [0.34, 0.06, 0.05], [0, 0.46, 0], [0.88, 0.85, 0.72, 1])],
        "flint_knife": [("cube", [0.05, 0.3, 0.02], [0, 0.25, 0], flint), ("cylinder", [0.04, 0.14, 0.04], [0, 0.05, 0], wood)],
        "bow": [("cylinder", [0.03, 0.9, 0.03], [0, 0.3, 0], wood)],
    }
    uid = 3000
    for key, items in parts.items():
        for mesh, scale, off, color in items:
            pos = [h + o for h, o in zip(hold, qrot(base, off))]
            comps = {"HeldPart": {"item": I.ID[key]}, "Transform": {}, "MeshRenderer": {"mesh": mesh, "color": color, "roughness": 0.8, "visible": False, "cast_shadow": True}, "BoneAttach": {"target": hero_uid, "joint": "DEF-hand.R", "pos": pos, "rot": base, "scale": scale}}
            if key == "torch" and mesh == "sphere":
                comps["MeshRenderer"]["emissive"] = 1.6
            W.spawn(uid, f"Held {key}", **comps)
            uid += 1
    # the hair and the beard
    skin_hair, beard = [0.24, 0.16, 0.1, 1], [0.3, 0.2, 0.12, 1]
    W.spawn(uid, "Hair", Transform={}, MeshRenderer={"mesh": "sphere", "color": skin_hair, "roughness": 0.9}, BoneAttach={"target": hero_uid, "joint": "DEF-head", "pos": [0, 0.15, -0.02], "scale": [0.215, 0.17, 0.225]})
    W.spawn(uid + 1, "Hair back", Transform={}, MeshRenderer={"mesh": "sphere", "color": skin_hair, "roughness": 0.9}, BoneAttach={"target": hero_uid, "joint": "DEF-head", "pos": [0, 0.06, -0.09], "scale": [0.16, 0.2, 0.12]})
    W.spawn(uid + 2, "Beard", Transform={}, MeshRenderer={"mesh": "sphere", "color": beard, "roughness": 0.9}, BoneAttach={"target": hero_uid, "joint": "DEF-head", "pos": [0, 0.03, 0.085], "scale": [0.13, 0.17, 0.09]})


ICONS = "examples/wyrdwood/icons.png"
SLOT_W, SLOT_H, GAP, X0, Y0 = 0.0478, 0.085, 0.0045, 0.03, 0.03


def icon_uv(num):
    col, row = num % 8, num // 8
    return [col / 8, row / 8, (col + 1) / 8, (row + 1) / 8]


def interface():
    """The hotbar, the inventory with its crafting list, the bars of health and stamina, the three foods and the message about the wrong tool."""
    import wyrdwood_items as I
    font = {"TextFont": {"file": W.FONT}}
    for i in range(I.INV_SLOTS):
        r, c = divmod(i, 8)
        x, y = X0 + c * (SLOT_W + GAP), Y0 + r * (SLOT_H + GAP)
        shown = i < I.HOT_SLOTS
        W.spawn(2000 + i, f"Slot {i}", InvSlot={"n": i}, Ui={"pos": [x, y], "size": [SLOT_W, SLOT_H], "color": [0.06, 0.07, 0.09, 0.8], "border": 0.002, "border_color": [0.7, 0.6, 0.45, 0.55], "layer": 2, "button": True, "visible": shown},
                UiStyle={"radius": 0.006})
        W.spawn(2100 + i, f"Icon {i}", InvIcon={"n": i}, Ui={"pos": [x + 0.004, y + 0.007], "size": [SLOT_W - 0.008, SLOT_H - 0.014], "color": [1, 1, 1, 0], "layer": 3, "visible": False},
                UiStyle={"image": ICONS, "fit": 1, "image_uv": icon_uv(1), "smooth": True})
        W.spawn(2200 + i, f"Count {i}", InvCount={"n": i}, Text={"value": "{inv.%d.n}" % i, "pos": [x + SLOT_W * 0.8, y + SLOT_H * 0.74], "size": 0.026, "align": 1, "color": [1, 1, 1, 1], "shadow": True, "visible": False}, **font)
        if i < I.HOT_SLOTS:
            W.spawn(2300 + i, f"Key {i + 1}", Text={"value": str(i + 1), "pos": [x + 0.007, y + 0.012], "size": 0.02, "align": 0, "color": [1, 0.92, 0.75, 0.8], "shadow": True}, **font)
    # the panel behind the inventory, and the crafting list
    W.spawn(2400, "Inventory panel", InvPanel={}, Ui={"pos": [0.018, 0.014], "size": [8 * (SLOT_W + GAP) + 0.026, 4 * (SLOT_H + GAP) + 0.026], "color": [0.03, 0.035, 0.05, 0.88], "border": 0.002, "border_color": [0.6, 0.5, 0.35, 0.6], "layer": 1, "visible": False}, UiStyle={"radius": 0.01})
    rows = len(I.RECIPES)
    W.spawn(2401, "Crafting panel", InvPanel={}, Ui={"pos": [0.462, 0.014], "size": [0.352, 0.062 + rows * 0.0605], "color": [0.03, 0.035, 0.05, 0.88], "border": 0.002, "border_color": [0.6, 0.5, 0.35, 0.6], "layer": 1, "visible": False}, UiStyle={"radius": 0.01})
    W.spawn(2402, "Crafting title", InvPanel={}, Text={"value": "Crafting", "pos": [0.47, 0.02], "size": 0.03, "align": 0, "color": [1, 0.9, 0.65, 1], "shadow": True, "visible": False}, **font)
    for r, (res, count, station, ingr) in enumerate(I.RECIPES):
        name = dict((it[0], it[1]) for it in I.ITEMS)[res]
        need = ", ".join(f"{dict((it[0], it[1]) for it in I.ITEMS)[k].lower()} {n}" for k, n in ingr)
        label = f"{name}{' x%d' % count if count > 1 else ''}\n{need}{'   [workbench]' if station else ''}"
        y = 0.07 + r * 0.0605
        W.spawn(2500 + r, f"Recipe icon {r}", RecipeIcon={"n": r}, Ui={"pos": [0.468, y], "size": [0.031, 0.0535], "color": [1, 1, 1, 0], "layer": 3, "visible": False}, UiStyle={"image": ICONS, "fit": 1, "image_uv": icon_uv(I.ID[res])})
        W.spawn(2600 + r, f"Recipe {r}", RecipeBtn={"n": r}, Ui={"pos": [0.503, y], "size": [0.306, 0.0535], "color": [0.07, 0.08, 0.09, 0.9], "text": label, "text_size": 0.0172, "text_color": [0.5, 0.5, 0.5, 1], "layer": 3, "button": True, "visible": False,
                                                                 "border": 0.0015, "border_color": [0.5, 0.42, 0.3, 0.5]}, UiStyle={"radius": 0.006}, **font)
    # health, stamina and the three foods, bottom left
    W.spawn(1002, "Health bar", HudHp={}, Ui={"pos": [0.03, 0.915], "size": [0.17, 0.03], "color": [0.8, 0.22, 0.18, 1], "back_color": [0.2, 0.05, 0.06, 0.9], "fill": 1, "border": 0.002, "border_color": [1, 0.9, 0.8, 0.7], "layer": 3}, UiStyle={"radius": 0.004})
    W.spawn(1004, "Stamina bar", HudSt={}, Ui={"pos": [0.03, 0.955], "size": [0.17, 0.02], "color": [0.9, 0.82, 0.25, 1], "back_color": [0.1, 0.1, 0.05, 0.9], "fill": 1, "border": 0.002, "border_color": [0.95, 1, 0.8, 0.6], "layer": 3}, UiStyle={"radius": 0.004})
    for n in range(3):
        W.spawn(1010 + n, f"Food {n}", HudFood={"n": n}, Ui={"pos": [0.03 + n * 0.057, 0.885], "size": [0.052, 0.014], "color": [0.9, 0.55, 0.2, 1], "back_color": [0.12, 0.08, 0.04, 0.9], "fill": 1, "border": 0.0015, "border_color": [1, 0.9, 0.7, 0.6], "layer": 3, "visible": False})
    W.spawn(1020, "Tool message", ToastText={}, Text={"value": "That needs the right tool: an axe for a tree, a pick for a rock", "pos": [0.5, 0.2], "size": 0.026, "align": 1, "color": [1, 0.85, 0.5, 1], "shadow": True, "visible": False}, **font)
    W.spawn(1021, "Help", Text={"value": "WASD walk   Shift run   Ctrl sneak   Space jump / rise   C dive   RMB or Q roll   LMB swing   1-8 hotbar (food is eaten)   Tab inventory and crafting", "pos": [0.03, 0.985], "size": 0.02, "align": 0, "color": [1, 1, 1, 0.7], "shadow": True}, **font)

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
        W.spawn(170 + i, f"Rough sea {i + 1}", Transform={"pos": [px, sea_y + 20, pz], "scale": [sx, 80, sz]}, Collider={"shape": 1, "size": [1, 1, 1]})
    W.call("step", {"ticks": 1})  # the terrains are read at the first step
    th = W.th
    sx, sz = meta["start"]["x"], meta["start"]["z"]
    sy = th(sx, sz - 4.0)
    # ---- the hero, its camera, the HUD, the rules
    W.spawn(7, "Camera", Transform={"pos": [sx, sy + 3.5, sz + 6.5]}, Camera={"fov_y": 62, "near": 0.15, "far": 2500},
            CameraRig={"target": 100 + 900, "mode": 1, "look_at": True, "look_offset": [0, 1.55, 0], "follow_speed": 16, "yaw": 0.0, "pitch": 0.3, "distance": 6.6, "shake_decay": 1.8, "shake_amount": 0.5}, CameraCollide={"radius": 0.4, "min_distance": 1.3})
    W.spawn(100 + 900, "Hero", Hero={"stamina": 100, "fx": 0, "fz": -1}, Transform={"pos": [sx, sy + 0.05, sz - 4.0]}, CharacterBody={"radius": 0.38, "height": 1.8, "step_height": 0.55, "max_slope": 52},
            CharacterMotion={"swim_speed": 2.6, "roll_speed": 8.5, "roll_time": 0.62}, HitPoints={"value": 100, "max": 100, "regen": 0.8, "regen_delay": 9},
            MeshRenderer={"mesh": "examples/wyrdwood/models/hero.glb", "color": [1, 1, 1, 1], "roughness": 0.85}, Animator={"clip": "Idle_Loop", "looping": True}, StateMachine={"machine": S.HERO_MACHINE})
    interface()
    equipment(100 + 900)
    import wyrdwood_build as B
    B.ghosts(W.spawn)
    B.texts(W.spawn, {"file": W.FONT})
    # ---- what grows where: the trees Tellus listed, near the hero, by the biome
    kinds = {
        "pine": ("pine", [0.7, 3.4, 0.7], 40, 1), "broad": ("broadleaf", [0.8, 3.4, 0.8], 50, 1), "palm": ("palm", [0.5, 3.0, 0.5], 30, 1), "dead": ("deadtree", [0.5, 3.0, 0.5], 30, 1),
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
            comp = {"Tree": {}, "Choppable": {"kind": 2, "reach": 0.4}, "HitPoints": {"value": 60, "max": 60, "regen": 0}, "MeshRenderer": {"mesh": f"{M}/rock{n}.glb", "roughness": 0.95}, "Collider": {"shape": 1, "size": [1.8, 1.4, 1.8]}}
            r = W.call("scatter", {"terrain": tuid, "components": comp, "count": count, "layer": "rock", "layer_min": 0.35, "slope": [0, 55], "min_distance": 6.0, "scale": [0.7, 2.0], "seed": 40 + n, "yaw": True, "sink": 0.1, "area": area}, quiet=True)
            rocks += r.get("placed", 0)
        r = W.call("scatter", {"terrain": tuid, "components": {"Tree": {}, "Choppable": {"kind": 2, "reach": 0.4}, "HitPoints": {"value": 45, "max": 45, "regen": 0}, "MeshRenderer": {"mesh": f"{M}/rock2.glb", "roughness": 0.95}, "Collider": {"shape": 1, "size": [1.8, 1.4, 1.8]}},
                                    "count": 26, "layer": "grass", "layer_min": 0.6, "slope": [0, 25], "min_distance": 16.0, "scale": [0.5, 1.1], "seed": 60, "yaw": True, "sink": 0.2, "area": area}, quiet=True)
        rocks += r.get("placed", 0)
    print(f"  rocks: {rocks}")
    # what lies about on the ground to begin with: branches, stones, flint, berries, mushrooms
    lying = 0
    for (tx_, ty_), (tuid, pos, tsize) in tiles.items():
        x0, z0 = pos[0], pos[2]
        area = [max(x0, sx - 260.0), max(z0, sz - 260.0), min(x0 + tsize[0], sx + 260.0), min(z0 + tsize[1], sz + 260.0)]
        if area[0] >= area[2] or area[1] >= area[3]:
            continue
        for prefab, count, layer, lmin, apart, seed in (("Branch", 70, "grass", 0.5, 6.0, 51), ("GroundStone", 50, "grass", 0.5, 9.0, 52), ("GroundFlint", 40, "sand", 0.5, 8.0, 53), ("BerryBush", 16, "grass", 0.7, 24.0, 54), ("MushroomPatch", 18, "grass", 0.7, 20.0, 55)):
            r = W.call("scatter", {"terrain": tuid, "prefab": prefab, "count": count, "layer": layer, "layer_min": lmin, "slope": [0, 30], "min_distance": apart, "seed": seed, "yaw": True, "area": area}, quiet=True)
            lying += r.get("placed", 0)
    print(f"  things on the ground: {lying}")
    # ---- places: the lookout at the landing, the camps, the altars
    u_counter = [6000]

    def u():
        u_counter[0] += 1
        return u_counter[0] - 1

    tower_x, tower_z = sx + 26.0, sz - 30.0
    W.lookout(u, tower_x, tower_z)
    for name, a in meta["altars"].items():
        gy = th(a["x"], a["z"])
        W.spawn(u(), f"{name.capitalize()} altar", Prop={}, Transform={"pos": [a["x"], gy + 0.6, a["z"]], "scale": [2.4, 1.2, 1.2]}, MeshRenderer={"mesh": "cube", "color": [0.46, 0.46, 0.5, 1], "roughness": 0.9}, Collider={"shape": 1, "size": [1, 1, 1]})
        W.spawn(u(), f"{name.capitalize()} altar light", Transform={"pos": [a["x"], gy + 2.2, a["z"]]}, Light={"kind": 1, "color": [0.55, 0.8, 1.0], "intensity": 3.0, "range": 18})
        for k in range(6):
            ang = k * math.pi / 3
            px, pz = a["x"] + math.cos(ang) * 7.0, a["z"] + math.sin(ang) * 7.0
            W.spawn(u(), "Standing stone", Prop={}, Transform={"pos": [px, th(px, pz) + 1.7, pz], "scale": [1.1, 3.6, 0.9], "rot": W.quat_y(math.degrees(ang))}, MeshRenderer={"mesh": "cube", "color": [0.42, 0.42, 0.45, 1], "roughness": 0.95}, Collider={"shape": 1, "size": [1, 1, 1]})
    for k, c in enumerate(meta["camps"]):
        gy = th(c["x"], c["z"])
        W.spawn(u(), "Camp fire", Transform={"pos": [c["x"], gy + 0.6, c["z"]]}, Light={"kind": 1, "color": [1.0, 0.62, 0.28], "intensity": 3.0, "range": 20})
        W.spawn(u(), "Camp crate", Prop={}, Transform={"pos": [c["x"] + 2.2, gy + 0.55, c["z"] + 1.0], "scale": [1.1, 1.1, 1.1]}, MeshRenderer={"mesh": "cube", "color": [0.55, 0.4, 0.22, 1], "roughness": 0.85}, Collider={"shape": 1, "size": [1, 1, 1]})
    res = W.call("define_script", {"source": S.ALL.replace("@SEA@", f"{sea_y:.3f}")})
    print("define_script:", json.dumps(res)[:200])
    W.call("check")
    saved = W.call("save_scene", {"path": out_scene})
    print("saved", out_scene, saved)
    print("failed requests:", W.failed)
    W.proc.stdin.close()
    W.proc.wait(timeout=300)


if __name__ == "__main__":
    main()
