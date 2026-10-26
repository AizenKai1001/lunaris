r"""The whole of Tokyo as one streamed world: the scene holds only `StreamZone`s; what is near the camera is loaded.

    python3 tokyo23_world.py WORKSPACE_ROOT OUT_SCENE [START_X START_Z]

WORKSPACE_ROOT holds `tokyo23/` as tokyo_city_build.py and the steps before it made it: `dem/` (cells.json, cell_<i>_<j>/heightmap.png, terrain*.jpg, splat.png), `bldg/` (units_<code>.json of every ward with the glTF files of `near/`, `far/`
and `rest/`), `tran_elev/<code>/`, `frn_<code>/`, `brid_<code>/`, `wtr_<code>/`, `vegs_<code>/` (index.json + glTF). Only what is there is used, so a world of the wards done so far can be tried at any time.

For every building unit (a leaf tile of the LoD2 set: its buildings and one picture) there are two zones of the same middle: the LoD2 mesh within R_NEAR, the LoD1 mesh of the same buildings from there out to R_FAR (a unit is one or the
other, never both: the two meshes hold the same buildings), and a ring of HYST metres in which whichever is loaded stays. Buildings that only have LoD1 are one zone out to R_FAR. The ground has three rings (full detail, 8 m, 16 m) as in
tokyo_world.py; props (road surfaces off the ground, city furniture, bridges, water, trees) are leaf tiles within R_PROP. Files are read ahead (PREFETCH metres beyond a zone's reach) and zones appear a few a tick (LOADS_PER_TICK, default 6).

Env: R_NEAR (default 600), R_FAR (4500), R_GROUND (800), R_PROP (1200), HYST (60), PREFETCH (700), LOADS_PER_TICK (6), START_LAT/START_LON (the camera's start; default the Shibuya crossing), START_Y (default 40), CAM_YAW, CAM_PITCH,
TOUR="x,y,z;..." (a closed flight path; TOUR_SPEED m/s, default 45), WARDS="13101,13113" (only these), SKIP="ground,bldg,props", STEP_L0 (default 2), SUN, AMBIENT, EXPOSURE, SUN_YAW, SUN_PITCH, FAR (camera far plane, default 14000).
"""
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from plateau_tiles import Frame  # noqa: E402
from tokyo_world import GROUND_LAYERS, quat  # noqa: E402

REL = "tokyo23"


def main():
    root, out = sys.argv[1], sys.argv[2]
    base = os.path.join(root, REL)
    r_near = float(os.environ.get("R_NEAR", "600"))
    r_far = float(os.environ.get("R_FAR", "4500"))
    r_ground = float(os.environ.get("R_GROUND", "800"))
    r_prop = float(os.environ.get("R_PROP", "1200"))
    hyst = float(os.environ.get("HYST", "60"))
    prefetch = float(os.environ.get("PREFETCH", "700"))
    skip = set(os.environ.get("SKIP", "").split(","))
    wards = [w for w in os.environ.get("WARDS", "").split(",") if w]
    ents, prefabs = [], {}

    def add(c):
        ents.append({"uid": len(ents) + 1, "components": c})
        return len(ents)

    o = json.load(open(os.path.join(base, "dem", "cells.json")))["origin"]
    frame = Frame(o["lat"], o["lon"], o["h"])
    if len(sys.argv) > 4:
        fx, fz = float(sys.argv[3]), float(sys.argv[4])
    else:
        import numpy as np
        la, lo = float(os.environ.get("START_LAT", "35.6595")), float(os.environ.get("START_LON", "139.7005"))
        a, e2 = 6378137.0, 0.00669438002290
        n = a / math.sqrt(1 - e2 * math.sin(math.radians(la)) ** 2)
        ecef = np.array([[(n + o["h"]) * math.cos(math.radians(la)) * math.cos(math.radians(lo)), (n + o["h"]) * math.cos(math.radians(la)) * math.sin(math.radians(lo)), (n * (1 - e2) + o["h"]) * math.sin(math.radians(la))]])
        fx, _, fz = frame.to_local(ecef)[0]
        fx, fz = float(fx), float(fz)

    add({"Environment": {"sky": True, "clouds": 0.2, "shadows": True, "shadow_distance": float(os.environ.get("SHADOW_DISTANCE", "600")), "ssao": 1.0, "bloom": 0.1, "vignette": 0.1, "tonemap": 1, "ambient": float(os.environ.get("AMBIENT", "0.9")), "ground": [float(c) for c in os.environ.get("GROUND", "0.38,0.38,0.36").split(",")]}})
    add({"Light": {"kind": 0, "intensity": float(os.environ.get("SUN", "2.8")), "color": [1.0, 0.95, 0.88]}, "Transform": {"rot": quat(float(os.environ.get("SUN_YAW", "-40")), float(os.environ.get("SUN_PITCH", "-42")))}})
    add({"Atmosphere": {"turbidity": float(os.environ.get("TURBIDITY", "3.2")), "intensity": 1.0}})
    add({"Shadows": {"softness": 0.02}})
    add({"AutoExposure": {"min": 0.25, "max": 2.5, "speed": 3.0, "target": float(os.environ.get("EXPOSURE", "0.2"))}})
    if os.environ.get("GI", "1") != "0":
        add({"GlobalIllumination": {"intensity": float(os.environ.get("GI_INTENSITY", "1.0")), "radius": 10.0}})
    cam = add({"Camera": {"fov_y": 55, "active": True, "far": float(os.environ.get("FAR", "14000"))}, "Transform": {"pos": [fx, float(os.environ.get("START_Y", "40")), fz], "rot": quat(float(os.environ.get("CAM_YAW", "0")), float(os.environ.get("CAM_PITCH", "0")))}})
    tour = os.environ.get("TOUR")
    if tour:
        path = add({"Name": {"value": "Tour path"}, "Transform": {"pos": [0, 0, 0]}, "Spline": {"points": tour.replace(",", " "), "smooth": True, "closed": True}})
        ents[cam - 1]["components"]["PathFollower"] = {"path": path, "speed": float(os.environ.get("TOUR_SPEED", "45")), "mode": 1, "align": True}

    counts = {}

    # ---- the ground: three rings of every kilometre cell
    if "ground" not in skip:
        dem = json.load(open(os.path.join(base, "dem", "cells.json")))
        n_ground = 0
        for c in dem["cells"]:
            cid = f"{c['i']}_{c['j']}"
            rel = f"{REL}/dem/cell_{cid}"
            has_photo = os.path.exists(os.path.join(base, "dem", f"cell_{cid}", "terrain.jpg"))
            has_splat = os.path.exists(os.path.join(base, "dem", f"cell_{cid}", "splat.png"))
            levels = [(int(os.environ.get("STEP_L0", "2")), "terrain.jpg", 4.0, 0.0, r_ground), (8, "terrain_l1.jpg", 8.0, r_ground + hyst, 3500.0), (16, "terrain_l2.jpg", 20.0, 3500.0 + hyst, 12000.0)]
            for level, (step, photo, skirt, near, far) in enumerate(levels):
                terr = {"heightmap": f"{rel}/heightmap.png", "size": [1000, 1000], "height": c["relief"], "step": step, "color": [0.55, 0.55, 0.55, 1], "uv_scale": [1, 1], "friction": 0.9, "skirt": skirt, "solid": level == 0}
                if has_photo:
                    terr["texture"] = f"{rel}/{photo}"
                    terr["color"] = [1, 1, 1, 1]
                comps = {"Transform": {"pos": [0, 0, 0]}, "Terrain": terr}
                splat = level == 0 and has_splat
                if splat:
                    comps["TerrainLayers"] = {"splat": f"{rel}/splat.png", "layers": GROUND_LAYERS, "photo": float(os.environ.get("GROUND_PHOTO", "0.9"))}
                pname = f"Ground_{cid}_L{level}"
                prefabs[pname] = {"components": comps}
                assets = f"heights {rel}/heightmap.png" + (f"\ntexture {rel}/{photo}" if has_photo else "") + (f"\nsplat {rel}/splat.png" if splat else "")
                zone = {"prefab": pname, "radius": far, "hysteresis": hyst, "prefetch": prefetch, "assets": assets}
                if near > 0:
                    zone["min_radius"] = near
                add({"Name": {"value": f"Ground {cid} level {level}"}, "Transform": {"pos": [c["cx"], c["y0"], c["cz"]]}, "StreamZone": zone})
                n_ground += 1
        counts["ground"] = n_ground

    def mesh_prefab(pname, rel_file, cx, cz):
        prefabs[pname] = {"components": {"Transform": {"pos": [0, 0, 0]}}, "children": [{"name": "Tile", "components": {"LocalTransform": {"pos": [-cx, 0, -cz]}, "MeshRenderer": {"mesh": rel_file, "roughness": 0.85}}}]}
        return pname

    # ---- the buildings
    if "bldg" not in skip:
        n_unit = n_rest = 0
        for f in sorted(os.listdir(os.path.join(base, "bldg"))) if os.path.isdir(os.path.join(base, "bldg")) else []:
            if not (f.startswith("units_") and f.endswith(".json")):
                continue
            code = f[len("units_"):-len(".json")]
            if wards and code not in wards:
                continue
            idx = json.load(open(os.path.join(base, "bldg", f)))
            for u in idx["units"]:
                cx, cy, cz = u["centre"]
                near_file = f"{REL}/bldg/near/{u['near']['file']}"
                pics = "".join(f"\ntexture {REL}/bldg/near/{t}" for t in u["near"].get("textures", []))
                zn = {"prefab": mesh_prefab(f"n_{u['name']}", near_file, cx, cz), "radius": r_near, "hysteresis": hyst, "prefetch": prefetch, "assets": f"model {near_file}"}
                add({"Name": {"value": f"bldg {u['name']} near"}, "Transform": {"pos": [cx, 0, cz]}, "StreamZone": zn})
                if "far" in u:
                    far_file = f"{REL}/bldg/far/{u['far']['file']}"
                    zf = {"prefab": mesh_prefab(f"f_{u['name']}", far_file, cx, cz), "radius": r_far, "min_radius": r_near + hyst, "hysteresis": hyst, "prefetch": prefetch, "assets": f"model {far_file}"}
                    add({"Name": {"value": f"bldg {u['name']} far"}, "Transform": {"pos": [cx, 0, cz]}, "StreamZone": zf})
                n_unit += 1
            for r in idx.get("rest", []):
                cx, cy, cz = r["centre"]
                rest_file = f"{REL}/bldg/rest/{r['file']}"
                zr = {"prefab": mesh_prefab(f"r_{r['file'][:-5]}", rest_file, cx, cz), "radius": r_far + r["radius"], "hysteresis": hyst, "prefetch": prefetch, "assets": f"model {rest_file}"}
                add({"Name": {"value": f"bldg {r['file']}"}, "Transform": {"pos": [cx, 0, cz]}, "StreamZone": zr})
                n_rest += 1
        counts["bldg units"], counts["bldg rest"] = n_unit, n_rest

    # ---- props: leaf tiles within R_PROP
    if "props" not in skip:
        folders = []
        for d in sorted(os.listdir(base)):
            if d.startswith(("frn_", "brid_", "wtr_", "vegs_")):
                folders.append((d, d.split("_")[0], d.split("_")[1]))
        te = os.path.join(base, "tran_elev")
        if os.path.isdir(te):
            folders += [(f"tran_elev/{d}", "tran", d) for d in sorted(os.listdir(te)) if os.path.isdir(os.path.join(te, d))]
        for d, kind, code in folders:
            if wards and code not in wards:
                continue
            ix = os.path.join(base, d, "index.json")
            if not os.path.exists(ix):
                continue
            n = 0
            for t in json.load(open(ix))["tiles"]:
                if not t.get("leaf", True):
                    continue
                cx, cy, cz = t["centre"]
                f_rel = f"{REL}/{d}/{t['file']}"
                add({"Name": {"value": f"{kind} {code} {t['name']}"}, "Transform": {"pos": [cx, 0, cz]}, "StreamZone": {"prefab": mesh_prefab(f"p_{kind}_{code}_{t['name']}", f_rel, cx, cz), "radius": r_prop + t["radius"], "hysteresis": hyst, "prefetch": prefetch, "assets": f"model {f_rel}"}})
                n += 1
            counts[kind] = counts.get(kind, 0) + n

    json.dump({"lunaris": 1, "prefabs": prefabs, "entities": ents, "resources": {"stream.loads_per_tick": float(os.environ.get("LOADS_PER_TICK", "6"))}}, open(out, "w"), separators=(",", ":"))
    print(f"world: {counts}, {len(ents)} entities, {len(prefabs)} prefabs, {os.path.getsize(out) / 1e6:.1f} MB -> {out}; start {fx:.0f}, {fz:.0f}")


if __name__ == "__main__":
    main()
