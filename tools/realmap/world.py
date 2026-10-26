"""A streamed world from a built real-place folder: the scene holds no ground at all, only `StreamZone`s and the prefabs those load while the camera is near. This is how a map of a county or a country is held:
what is far away is not in memory, in the simulation or on the card.

    python tools/realmap/world.py NAME [START_X START_Z]

NAME's folder is either a region built by swiss_region.py (world.json and cells/) or a single window built by swiss_build.py (scene/info.json). The scene goes next to it as NAME.world.lunaris.json
(in the folder for a region, in scene/ for a window); a workspace root that has the folder NAME/ (realmap-data, or ~/realmap on a Linux test machine) and models/ and textures/ can load it.

Each ground tile has three levels of detail, each a zone that is a ring of distances from the camera (StreamZone.min_radius): up close the full tile (0.5 m ground drawn at 1 m, the 8192 px photograph with its detail
layers, grass, buildings, water), then the same ground at 8 m (STEP_L1) with a 2048 px photograph, then at 16 m with a 512 px photograph (no physics, no buildings). The rings meet exactly at the hysteresis margin, so a tile is
always exactly one of them; a skirt under each tile hides the cracks between tiles of different levels. The measured trees are prefabs of up to 4,000 for each 500 m of ground.

Env: R_CELL (metres from the camera at which a ground tile is full detail, default 1100), R_L1 (the coarse ring ends, default 4500), R_L2 (the far ring ends, default 17000), R_TREE (trees, default 650),
HYST (the margin before it unloads again, default 150), PREFETCH (how far beyond a zone's radius its files are read ahead on other threads, default 900 m, 0 for not at all).
"""
import json
import math
import os
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import scene as S  # noqa: E402

ROOT = S.ROOT
MAX_PREFAB = 4000  # a prefab makes at most 4,096 entities
# the layers of a region built by swiss_region.py (the fourth is snow and ice) and of a window built by swiss_build.py (the fourth is bare ground)
ALPINE_LINES = ["grass textures/sparse_grass/sparse_grass_diff.jpg uv_scale=450 tint=0.9,1.0,0.8", "rock textures/rock_face_03/rock_face_03_diff.jpg uv_scale=260 cliff=1", "forest textures/grass_ground/grass_ground_diff.jpg uv_scale=380 tint=0.7,0.85,0.6", "snow textures/snow_02/snow_02_diff.jpg uv_scale=300"]
# the far levels keep the photograph's own colour (a forest stays dark green, snow white): their layers are a plain grey but for the rock, which steep ground shows from the side (cliff=1)
FAR_LINES = ["photo textures/neutral/neutral.png uv_scale=1", "rock textures/rock_face_03/rock_face_03_diff.jpg uv_scale=260 cliff=1", "forest textures/neutral/neutral.png uv_scale=1", "snow textures/neutral/neutral.png uv_scale=1"]
LAYER_LINES = ["grass textures/sparse_grass/sparse_grass_diff.jpg uv_scale=450 tint=0.9,1.0,0.8", "rock textures/rock_face_03/rock_face_03_diff.jpg uv_scale=260", "forest textures/grass_ground/grass_ground_diff.jpg uv_scale=380 tint=0.7,0.85,0.6", "bare textures/coast_sand_01/coast_sand_01_diff.jpg uv_scale=320 tint=0.9,0.85,0.8"]


def cells_of(name):
    """The ground tiles of a built folder, one dict each, whatever built it: id, centre (x, z), y of the lowest ground, relief, the files (relative to the workspace root) and the trees (world positions)."""
    base = os.path.join(ROOT, name)
    wj = os.path.join(base, "world.json")
    out = []
    if os.path.exists(wj):
        w = json.load(open(wj))
        for c in w["cells"]:
            kx, ky = c["kx"], c["ky"]
            rel = f"{name}/cells/cell_{kx}_{ky}"
            d = os.path.join(base, "cells", f"cell_{kx}_{ky}")
            trees = json.load(open(os.path.join(d, "trees.json"))) if os.path.exists(os.path.join(d, "trees.json")) else []
            fits = json.load(open(os.path.join(d, "fits.json"))) if os.path.exists(os.path.join(d, "fits.json")) else []
            out.append({"fits": fits, "layers": ALPINE_LINES, "id": f"{kx}_{ky}", "cx": c["cx"], "cz": c["cz"], "y0": c["y0"], "relief": c["relief"], "size": 1000, "heightmap": f"{rel}/heightmap.png", "texture": f"{rel}/terrain.jpg" if c.get("photo") else None,
                        "l1": f"{rel}/terrain_l1.jpg" if c.get("photo") else None, "l2": f"{rel}/terrain_l2.jpg" if c.get("photo") else None, "splat": f"{rel}/splat.png" if c.get("photo") else None,
                        "water": f"{rel}/water.png" if c.get("water") else None, "buildings": f"{name}/buildings_{kx}_{ky}.gltf" if c.get("has_buildings") else None, "trees": trees})
        return out, base
    d = os.path.join(base, "scene")
    info = json.load(open(os.path.join(d, "info.json")))
    trees = json.load(open(os.path.join(d, "trees.json"))) if os.path.exists(os.path.join(d, "trees.json")) else []
    for t in info["tiles"]:
        i, j = t["i"], t["j"]
        sub = [tr for tr in trees if abs(tr["x"] - t["cx"]) <= t["size"] / 2 and abs(tr["z"] - t["cz"]) <= t["size"] / 2]
        out.append({"layers": LAYER_LINES, "id": f"{i}_{j}", "cx": t["cx"], "cz": t["cz"], "y0": 0.0, "relief": float(info["relief"]), "size": t["size"], "heightmap": f"{name}/{t['heightmap']}", "texture": f"{name}/{t['texture']}",
                    "l1": f"{name}/terrain_{i}_{j}_l1.jpg", "l2": f"{name}/terrain_{i}_{j}_l2.jpg", "splat": f"{name}/{t['splat']}" if t.get("splat") else None, "water": None,
                    "buildings": f"{name}/buildings_{i}_{j}.gltf" if os.path.exists(os.path.join(d, f"buildings_{i}_{j}.gltf")) else None, "trees": sub, "legacy_water": f"{name}/water.png", "legacy_size": float(info["size"])})
    return out, d


def main():
    name = sys.argv[1]
    fx = float(sys.argv[2]) if len(sys.argv) > 2 else 0.0
    fz = float(sys.argv[3]) if len(sys.argv) > 3 else 0.0
    r_cell = float(os.environ.get("R_CELL", "1100"))
    r_l1 = float(os.environ.get("R_L1", "4500"))
    r_l2 = float(os.environ.get("R_L2", "17000"))
    r_tree = float(os.environ.get("R_TREE", "650"))
    hyst = float(os.environ.get("HYST", "150"))
    prefetch = float(os.environ.get("PREFETCH", "900"))  # a zone's files are read ahead from this far beyond its radius (0: not at all)
    cells, out_dir = cells_of(name)
    ents = []
    prefabs = {}

    def add(comps):
        ents.append({"uid": len(ents) + 1, "components": comps})
        return len(ents)

    cam = S.sky_and_camera(add, fx, fz)
    # TOUR="x,y,z;x,y,z;..." (world metres, y above the region's datum): the camera flies a closed smooth path over the world by itself, at TOUR_SPEED m/s (for watching a streamed world live: `lunaris play NAME.tour.lunaris.json`)
    tour = os.environ.get("TOUR")
    if tour:
        path = add({"Name": {"value": "Tour path"}, "Transform": {"pos": [0, 0, 0]}, "Spline": {"points": tour.replace(",", " "), "smooth": True, "closed": True}})
        ents[cam - 1]["components"]["PathFollower"] = {"path": path, "speed": float(os.environ.get("TOUR_SPEED", "70")), "mode": 1, "align": True}
        ents[cam - 1]["components"]["Camera"]["far"] = 20000.0
    if cells and cells[0].get("legacy_water"):
        c0 = cells[0]
        add({"Name": {"value": "Water"}, "Transform": {"pos": [0, 0, 0]}, "WaterBody": {"heightmap": c0["legacy_water"], "size": [c0["legacy_size"]] * 2, "relief": c0["relief"], "step": 2, "color": [0.05, 0.22, 0.28, 0.85], "wave_height": 0.05, "wave_length": 3.0}})
    # the two tree models are the bases of the tree prefabs: one child of a cell is then a few numbers
    for k, (model, mh, mr) in enumerate(S.TREE_MODELS):
        prefabs[f"TreeModel{k}"] = {"components": {"MeshRenderer": {"mesh": model, "roughness": 0.8}}}
    all_trees = []
    for c in cells:
        cid = c["id"]
        root = {"Transform": {"pos": [0, 0, 0]}, "Terrain": {"heightmap": c["heightmap"], "size": [c["size"], c["size"]], "height": c["relief"], "step": int(os.environ.get("STEP_L0", "2")), "color": [1, 1, 1, 1], "uv_scale": [1, 1], "friction": 0.9, "skirt": 4.0}}
        if c["texture"]:
            root["Terrain"]["texture"] = c["texture"]
        if c["splat"]:
            root["TerrainLayers"] = {"splat": c["splat"], "layers": chr(10).join(c["layers"])}
            root["GrassField"] = {"radius": 90, "density": 20, "mask_layer": 1, "height": [0.10, 0.34], "seed": 1.0 + (zlib.crc32(cid.encode()) % 97)}
        children = []
        if c["buildings"]:
            children.append({"name": "Buildings", "components": {"LocalTransform": {"pos": [0, 0, 0]}, "MeshRenderer": {"mesh": c["buildings"], "roughness": 0.85}}})
        for f in c.get("fits", []):
            children.append({"components": {"LocalTransform": {"pos": f["pos"], "rot": f["rot"], "scale": f["scale"]}, "MeshRenderer": {"mesh": f["model"], "roughness": 0.85}}})
        if c["water"]:
            children.append({"name": "Water", "components": {"LocalTransform": {"pos": [0, 0, 0]}, "WaterBody": {"heightmap": c["water"], "size": [c["size"], c["size"]], "relief": c["relief"], "step": 2, "color": [0.05, 0.22, 0.28, 0.85], "wave_height": 0.05, "wave_length": 3.0}}})
        prefabs[f"Cell_{cid}"] = {"components": root, "children": children}
        zone_pos = [c["cx"], c["y0"], c["cz"]]
        assets = [("texture", c["texture"]), ("heights", c["heightmap"]), ("splat", c["splat"]), ("model", c["buildings"])]
        add({"Name": {"value": f"Zone cell {cid}"}, "Transform": {"pos": zone_pos}, "StreamZone": {"prefab": f"Cell_{cid}", "radius": r_cell, "hysteresis": hyst, "prefetch": prefetch, "assets": chr(10).join(f"{k} {v}" for k, v in assets if v)}})
        # the coarser levels: the same ground, further away (the ring starts where the finer one ends plus its margin, so that exactly one of them is loaded)
        for level, (step, key, skirt, near, far) in enumerate(((int(os.environ.get("STEP_L1", "8")), "l1", 8.0, r_cell + hyst, r_l1), (16, "l2", 20.0, r_l1 + hyst, r_l2)), start=1):
            if not c[key]:
                continue
            far_ground = {"Transform": {"pos": [0, 0, 0]}, "Terrain": {"heightmap": c["heightmap"], "size": [c["size"], c["size"]], "height": c["relief"], "step": step, "color": [1, 1, 1, 1], "texture": c[key], "uv_scale": [1, 1], "solid": False, "skirt": skirt}}
            if c["splat"] and c["layers"] is ALPINE_LINES and os.environ.get("FAR_LAYERS", "1") != "0":
                # the photograph's own colour stays (macro), the layers give the cliffs their rock seen from the side
                far_ground["TerrainLayers"] = {"splat": c["splat"], "layers": chr(10).join(FAR_LINES), "photo": 1.0}
            prefabs[f"Cell_{cid}_L{level}"] = {"components": far_ground}
            add({"Name": {"value": f"Zone cell {cid} level {level}"}, "Transform": {"pos": zone_pos}, "StreamZone": {"prefab": f"Cell_{cid}_L{level}", "radius": far, "min_radius": near, "hysteresis": hyst, "prefetch": prefetch, "assets": f"texture {c[key]}" + chr(10) + f"heights {c['heightmap']}" + (chr(10) + f"splat {c['splat']}" if c["splat"] else "")}})
        all_trees += c["trees"]
    # trees: every 500 m of ground is a prefab of measured trees (a model of the library scaled to each measured height and crown)
    sub = 500.0
    groups = {}
    for k, t in enumerate(all_trees):
        groups.setdefault((math.floor(t["x"] / sub), math.floor(t["z"] / sub)), []).append((k, t))
    dropped = 0
    for (a, b), items in sorted(groups.items()):
        if len(items) > MAX_PREFAB:
            dropped += len(items) - MAX_PREFAB
            items = sorted(items, key=lambda kt: -kt[1]["h"])[:MAX_PREFAB]
        cx, cz = (a + 0.5) * sub, (b + 0.5) * sub
        children = []
        for k, t in items:
            mk = (k * 7 + int(abs(t["x"]) * 3)) % 2
            model, mh, mr = S.TREE_MODELS[mk]
            sy = t["h"] / mh
            sxz = max(0.6, min(8.0, t["r"] / mr))
            yaw = (k * 137.5) % 360
            children.append({"prefab": f"TreeModel{mk}", "components": {"LocalTransform": {"pos": [round(t["x"] - cx, 2), round(t["y"] - 0.1, 2), round(t["z"] - cz, 2)], "rot": [0, round(math.sin(math.radians(yaw) / 2), 4), 0, round(math.cos(math.radians(yaw) / 2), 4)], "scale": [round(sxz, 2), round(sy, 2), round(sxz, 2)]}}})
        pname = f"Trees_{a}_{b}"
        prefabs[pname] = {"components": {"Transform": {"pos": [0, 0, 0]}}, "children": children}
        add({"Name": {"value": f"Zone trees {a},{b}"}, "Transform": {"pos": [cx, 0, cz]}, "StreamZone": {"prefab": pname, "radius": r_tree, "hysteresis": hyst}})
    out = os.path.join(out_dir, f"{name}.{'tour' if tour else 'world'}.lunaris.json")
    # one zone loaded a tick, the nearest first: a cell of three thousand entities costs about 20 ms to create
    json.dump({"lunaris": 1, "resources": {"stream.loads_per_tick": 1}, "prefabs": prefabs, "entities": ents}, open(out, "w"), separators=(",", ":"))
    print(f"world: {len(cells)} ground cells, {len(groups)} tree cells ({len(all_trees) - dropped} of {len(all_trees)} trees), {len(ents)} entities, {len(prefabs)} prefabs, {os.path.getsize(out) / 1e6:.1f} MB -> {out}")


if __name__ == "__main__":
    main()
