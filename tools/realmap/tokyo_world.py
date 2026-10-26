r"""A streamed Tokyo from PLATEAU tiles converted by plateau_tiles.py / plateau_dem.py / plateau_ortho.py: the scene holds only `StreamZone`s; what is near the camera is loaded.

    python3 tokyo_world.py WORKSPACE_ROOT OUT_SCENE [START_X START_Z]

The workspace root has `tokyo/<kind>/` (index.json + glTF tiles; kinds bldg, tran, frn, brid, wtr, veg_SolitaryVegetationObject) and `tokyo/dem/` (cells.json, cell_<i>_<j>/heightmap.png + terrain*.jpg).

Tiles of a hierarchy (buildings, roads: PLATEAU's 3D Tiles with refinement REPLACE) are streamed as a tree: a tile's coarse mesh is a ring zone (loaded beyond the distance at which its children take over), a small "refined" zone of the same
place with that distance as its radius stands for the finer level, and the tiles under it `require` it (`StreamZone.requires`), so exactly one level of every branch is there at a time. The distance is the tile's
geometric error times REFINE (default 100) plus its radius. The ground has three rings like the valley's (full detail, 8 m, 16 m). Other kinds are leaf tiles within R_PROP metres.

Env: REFINE, HYST (margin, default 40 m), PREFETCH (files read ahead this far beyond a zone's reach, default 700), R_GROUND (full-detail ground, default 800), R_PROP (default 1400), SUN, AMBIENT, EXPOSURE,
SUN_YAW, SUN_PITCH (default -40, -42: a sun in the south-west, so that shadows fall to the north-east as they do in the photographs the building textures were made from), START_Y, CAM_YAW, CAM_PITCH (the camera's start height and turn), TOUR="x,y,z;..." (a closed flight path in the scene's frame; TOUR_SPEED m/s, default 45), STEP_L0 (ground mesh step, default 2).
"""
import json
import math
import os
import sys


# the engine's four terrain layers for the city ground (plateau_ground.py writes the weights): real asphalt, paving, concrete and grass; the macro picture gives the colour and the markings
GROUND_LAYERS = "\n".join([
    "asphalt textures/asphalt_02/asphalt_02_diff.jpg uv_scale=333",
    "pavement textures/brick_pavement_03/brick_pavement_03_diff.jpg uv_scale=500",
    "plaza textures/asphalt_04/asphalt_04_diff.jpg uv_scale=250",
    "grass textures/grass_ground/grass_ground_diff.jpg uv_scale=400 tint=0.8,0.95,0.7",
])


def quat(yaw, pitch):
    sy, cy = math.sin(math.radians(yaw) / 2), math.cos(math.radians(yaw) / 2)
    sp, cp = math.sin(math.radians(pitch) / 2), math.cos(math.radians(pitch) / 2)
    return [cy * sp, sy * cp, -sy * sp, cy * cp]


def main():
    root, out = sys.argv[1], sys.argv[2]
    fx = float(sys.argv[3]) if len(sys.argv) > 3 else 0.0
    fz = float(sys.argv[4]) if len(sys.argv) > 4 else 0.0
    refine = float(os.environ.get("REFINE", "100"))
    hyst = float(os.environ.get("HYST", "40"))
    prefetch = float(os.environ.get("PREFETCH", "700"))
    r_ground = float(os.environ.get("R_GROUND", "800"))
    r_prop = float(os.environ.get("R_PROP", "1400"))
    ents, prefabs = [], {}

    def add(c):
        ents.append({"uid": len(ents) + 1, "components": c})
        return len(ents)

    add({"Environment": {"sky": True, "clouds": 0.2, "shadows": True, "shadow_distance": 600, "ssao": 1.0, "bloom": 0.1, "vignette": 0.1, "tonemap": 1, "ambient": float(os.environ.get("AMBIENT", "0.9")), "ground": [float(c) for c in os.environ.get("GROUND", "0.38,0.38,0.36").split(",")]}})
    add({"Light": {"kind": 0, "intensity": float(os.environ.get("SUN", "2.8")), "color": [1.0, 0.95, 0.88]}, "Transform": {"rot": quat(float(os.environ.get("SUN_YAW", "-40")), float(os.environ.get("SUN_PITCH", "-42")))}})
    add({"Atmosphere": {"turbidity": float(os.environ.get("TURBIDITY", "3.2")), "intensity": 1.0}})
    add({"Shadows": {"softness": 0.02}})
    add({"AutoExposure": {"min": 0.25, "max": 2.5, "speed": 3.0, "target": float(os.environ.get("EXPOSURE", "0.2"))}})
    if os.environ.get("GI", "1") != "0":
        add({"GlobalIllumination": {"intensity": float(os.environ.get("GI_INTENSITY", "1.0")), "radius": 10.0}})
    cam = add({"Camera": {"fov_y": 55, "active": True, "far": 12000.0}, "Transform": {"pos": [fx, float(os.environ.get("START_Y", "30")), fz + (0 if "START_Y" in os.environ else 60)], "rot": quat(float(os.environ.get("CAM_YAW", "0")), float(os.environ.get("CAM_PITCH", "0")))}})
    tour = os.environ.get("TOUR")
    if tour:
        path = add({"Name": {"value": "Tour path"}, "Transform": {"pos": [0, 0, 0]}, "Spline": {"points": tour.replace(",", " "), "smooth": True, "closed": True}})
        ents[cam - 1]["components"]["PathFollower"] = {"path": path, "speed": float(os.environ.get("TOUR_SPEED", "45")), "mode": 1, "align": True}
    prefabs["Flag"] = {"components": {"Name": {"value": "refined"}, "Transform": {"pos": [0, 0, 0]}}}

    # ---- the ground: three rings of every kilometre cell
    dem = json.load(open(os.path.join(root, "tokyo", "dem", "cells.json")))
    skip = set(os.environ.get("SKIP", "").split(","))  # parts to leave out (to find what is what): ground, bldg, tran, frn, brid, wtr, veg_SolitaryVegetationObject
    n_ground = 0
    for c in ([] if "ground" in skip else dem["cells"]):
        cid = f"{c['i']}_{c['j']}"
        rel = f"tokyo/dem/cell_{cid}"
        zone_pos = [c["cx"], c["y0"], c["cz"]]
        has_photo = os.path.exists(os.path.join(root, rel, "terrain.jpg"))
        levels = [(int(os.environ.get("STEP_L0", "2")), "terrain.jpg", 4.0, 0.0, r_ground), (8, "terrain_l1.jpg", 8.0, r_ground + hyst, 3500.0), (16, "terrain_l2.jpg", 20.0, 3500.0 + hyst, 12000.0)]
        for level, (step, photo, skirt, near, far) in enumerate(levels):
            terr = {"heightmap": f"{rel}/heightmap.png", "size": [1000, 1000], "height": c["relief"], "step": step, "color": [0.55, 0.55, 0.55, 1], "uv_scale": [1, 1], "friction": 0.9, "skirt": skirt, "solid": level == 0}
            if has_photo:
                terr["texture"] = f"{rel}/{photo}"
                terr["color"] = [1, 1, 1, 1]
            pname = f"Ground_{cid}_L{level}"
            comps = {"Transform": {"pos": [0, 0, 0]}, "Terrain": terr}
            has_splat = level == 0 and os.path.exists(os.path.join(root, rel, "splat.png"))
            if has_splat:
                comps["TerrainLayers"] = {"splat": f"{rel}/splat.png", "layers": GROUND_LAYERS, "photo": float(os.environ.get("GROUND_PHOTO", "0.9"))}
            prefabs[pname] = {"components": comps}
            assets = f"heights {rel}/heightmap.png" + (f"\ntexture {rel}/{photo}" if has_photo else "") + (f"\nsplat {rel}/splat.png" if has_splat else "")
            zone = {"prefab": pname, "radius": far, "hysteresis": hyst, "prefetch": prefetch, "assets": assets}
            if near > 0:
                zone["min_radius"] = near
            add({"Name": {"value": f"Ground {cid} level {level}"}, "Transform": {"pos": zone_pos}, "StreamZone": zone})
            n_ground += 1

    # ---- tiles
    def tile_prefab(kind, t):
        pname = f"{kind}_{t['name']}"
        cx, cy, cz = t["centre"]
        prefabs[pname] = {"components": {"Transform": {"pos": [0, 0, 0]}}, "children": [{"name": "Tile", "components": {"LocalTransform": {"pos": [-cx, 0, -cz]}, "MeshRenderer": {"mesh": f"tokyo/{kind}/{t['file']}", "roughness": 0.85}}}]}
        return pname

    counts = {}
    for kind in ("bldg",):
        ix = os.path.join(root, "tokyo", kind, "index.json")
        if not os.path.exists(ix) or kind in skip:
            continue
        tiles = {t["name"]: t for t in json.load(open(ix))["tiles"]}
        flag_of = {}
        r_ref_of = {}
        n = 0
        for t in sorted(tiles.values(), key=lambda t: (t["depth"], t["name"])):
            r_ref = t["geometric_error"] * refine + t["radius"]
            r_ref_of[t["name"]] = r_ref
            p = tiles.get(t["parent"]) if t["parent"] else None
            req = flag_of.get(t["parent"], 0) if p else 0
            reach = (r_ref_of[p["name"]] + p["radius"]) if p else 20000.0
            pos = [t["centre"][0], 0, t["centre"][2]]
            zone = {"prefab": tile_prefab(kind, t), "radius": reach, "hysteresis": hyst, "prefetch": prefetch, "assets": f"model tokyo/{kind}/{t['file']}", "requires": req}
            if not t["leaf"]:
                zone["min_radius"] = r_ref + hyst
                flag_of[t["name"]] = add({"Name": {"value": f"{kind} {t['name']} refined"}, "Transform": {"pos": pos}, "StreamZone": {"prefab": "Flag", "radius": r_ref, "hysteresis": hyst, "requires": req}})
            add({"Name": {"value": f"{kind} {t['name']}"}, "Transform": {"pos": pos}, "StreamZone": zone})
            n += 1
        counts[kind] = n
    for kind in ("tran_elev", "frn", "brid", "wtr", "veg_SolitaryVegetationObject"):
        ix = os.path.join(root, "tokyo", kind, "index.json")
        if not os.path.exists(ix) or kind in skip:
            continue
        n = 0
        for t in json.load(open(ix))["tiles"]:
            if not t["leaf"]:
                continue
            add({"Name": {"value": f"{kind} {t['name']}"}, "Transform": {"pos": [t["centre"][0], 0, t["centre"][2]]}, "StreamZone": {"prefab": tile_prefab(kind, t), "radius": r_prop + t["radius"], "hysteresis": hyst, "prefetch": prefetch, "assets": f"model tokyo/{kind}/{t['file']}"}})
            n += 1
        counts[kind] = n
    json.dump({"lunaris": 1, "prefabs": prefabs, "entities": ents}, open(out, "w"), separators=(",", ":"))
    print(f"world: {n_ground} ground zones, tiles {counts}, {len(ents)} entities, {len(prefabs)} prefabs, {os.path.getsize(out) / 1e6:.1f} MB -> {out}")


if __name__ == "__main__":
    main()
