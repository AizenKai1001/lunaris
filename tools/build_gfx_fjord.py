"""The landscape of the renderer (examples/gfx_lab/fjord_*.png): a Tellus fjord (8 km, 1.5 km of relief: glaciated mountains with snow, forested slopes, the sea in the valleys), painted with Poly Haven's photographs by
Tellus's own splat map (hex-tiled, with macro variation), blade grass on the meadows (`GrassField.mask_layer`), boulders on the rock, and the renderer's air: the sky of the atmosphere, clouds, soft shadows, bounced light, haze that
gives the distance its depth, the sun low over the water.

    python3 tools/fetch_polyhaven.py snow_02 rock_face_03 aerial_rocks_02 coast_sand_01 sparse_grass          (the ground's photographs)
    python3 tools/fetch_polyhaven_model.py boulder_01 rock_07 rock_09; python3 tools/prepare_polyhaven_models.py
    tellus export template:fjord --res 1025 --engine examples/gfx_lab/fjord --prefix examples/gfx_lab/fjord/ --scene examples/gfx_lab/fjord.lunaris.json
    python3 tools/build_gfx_fjord.py
"""
import json
import math
import os
import subprocess

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
W, H = int(os.environ.get("W", "1920")), int(os.environ.get("H", "1080"))
proc = subprocess.Popen([BIN, "serve", "--root", "."], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
n = 0


def call(name, args=None):
    global n
    n += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + chr(10))
    proc.stdin.flush()
    while True:
        m = json.loads(proc.stdout.readline())
        if m.get("id") == n:
            break
    res = m.get("result") or {}
    if res.get("isError"):
        print(name, "FAILED", json.dumps(m)[:300])
    for c in res.get("content") or []:
        if c.get("type") == "text":
            try:
                return json.loads(c["text"])
            except ValueError:
                return {}
    return {}


def quat(yaw, pitch):
    sy, cy = math.sin(math.radians(yaw) / 2), math.cos(math.radians(yaw) / 2)
    sp, cp = math.sin(math.radians(pitch) / 2), math.cos(math.radians(pitch) / 2)
    return [cy * sp, sy * cp, -sy * sp, cy * cp]


NAME = os.environ.get("SCENE", "fjord")
call("load_scene", {"path": f"examples/gfx_lab/{NAME}.lunaris.json"})
sun = [e["uid"] for e in call("query", {"with": ["Light"], "select": ["Light"]}).get("entities", []) if (e.get("components") or {}).get("Light", {}).get("kind") == 0][0]
SUN_YAW = float(os.environ.get("SUN_YAW", "215"))
SUN_EL = float(os.environ.get("SUN", "11"))
call("set", {"uid": sun, "component": "Transform", "value": {"rot": quat(SUN_YAW, -SUN_EL)}})
call("set", {"uid": sun, "component": "Light", "value": {"intensity": 3.8, "color": [1.0, 0.9, 0.74]}})
env = [e["uid"] for e in call("query", {"with": ["Environment"], "select": []}).get("entities", [])][0]
call("set", {"uid": env, "component": "Environment", "value": {"sky": True, "clouds": 0.55, "fog": 0.00002, "ssao": 1.0, "bloom": 0.45, "bloom_threshold": 0.95, "vignette": 0.22, "saturation": 1.08, "tonemap": 1, "shadow_distance": 4000, "ambient": 1.0, "exposure": 1.0}})
for uid, comps in (
    (900, {"Atmosphere": {"turbidity": 2.6, "haze_glow": 1.4, "intensity": 1.0}}),
    (901, {"Shadows": {"softness": 0.03}}),
    (902, {"GlobalIllumination": {"intensity": 0.8, "radius": 40.0, "directions": 10}}),
    (903, {"AutoExposure": {"min": 0.25, "max": 3.0, "speed": 3.0, "target": 0.3}}),
    (904, {"PostFx": {"shafts": 0.45, "contrast": 1.06, "grain": 0.06}}),
    (905, {"HeightFog": {"density": 0.0004, "falloff": 0.01, "height": 190, "color": [0.72, 0.78, 0.9]}}),
):
    call("spawn", {"uid": uid, "components": comps})
cam = [e["uid"] for e in call("query", {"with": ["Camera"], "select": []}).get("entities", [])][0]
call("set", {"uid": cam, "component": "Camera", "value": {"fov_y": 62.0, "far": 40000.0}})

# the ground: Poly Haven photographs blended by Tellus's own splat map (R grass and forest, G rock and scree, B soil and sand, A snow)
tid = [e["uid"] for e in call("query", {"with": ["Terrain"], "select": []}).get("entities", [])][0]
got = call("get", {"uid": tid, "components": ["Terrain", "Transform"]}).get("components", {})
terrain = dict(got["Terrain"])
size = terrain["size"][0]
terrain.update({"color": [1, 1, 1, 1]})  # the albedo stays: the ground takes its colour from it, the photographs give the detail
call("despawn", {"uid": tid})
T = "examples/hollowlight/textures"
rep = lambda metres: int(size / metres)
layers = "\n".join((
    f"grass {T}/sparse_grass/sparse_grass_diff.jpg uv_scale={rep(3.2)} tint=0.45,0.62,0.34",
    f"rock {T}/rock_face_03/rock_face_03_diff.jpg uv_scale={rep(6)} tint=0.85,0.85,0.85",
    f"soil {T}/coast_sand_01/coast_sand_01_diff.jpg uv_scale={rep(3.5)} tint=0.8,0.74,0.62",
    f"snow {T}/snow_02/snow_02_diff.jpg uv_scale={rep(4)} tint=1.0,1.0,1.02",
))
call("spawn", {"uid": tid, "components": {"Transform": got["Transform"], "Terrain": terrain, "TerrainLayers": {"splat": f"examples/gfx_lab/{NAME}/splat.png", "layers": layers}}})
call("reload_terrain", {})


def ground(x, z):
    return call("terrain_height", {"x": x, "z": z}).get("height", 0.0)


# the sea and lakes of the export are flat sheets: give them a look with depth
for e in call("query", {"with": ["Water"], "select": []}).get("entities", []):
    c = call("get", {"uid": e["uid"], "components": ["Water", "Transform"]}).get("components", {})
    w = dict(c["Water"])
    w.update({"size": [600000.0, 600000.0], "wave_height": 0.35, "wave_length": 22.0, "speed": 0.7})
    call("despawn", {"uid": e["uid"]})
    call("spawn", {"uid": e["uid"], "components": {"Transform": c["Transform"], "Water": w, "WaterLook": {"clarity": 6.0, "shallow": [0.25, 0.55, 0.5], "foam": 0.4, "foam_width": 1.2}}})

# a viewpoint: a meadow on the coast, 2.5 km from the highest ground, with the peaks in view
best = None
peak = (0.0, 0.0, -1.0)
step = size / 48.0
pts = []
for i in range(49):
    for j in range(49):
        x, z = -size / 2 + i * step, -size / 2 + j * step
        h = ground(x, z)
        pts.append((x, z, h))
        if h > peak[2]:
            peak = (x, z, h)
sea = 180.0
for x, z, h in pts:
    if not (sea + 6 < h < sea + 70):
        continue
    d = math.hypot(peak[0] - x, peak[1] - z)
    if not (1800 < d < 3600):
        continue
    # the line to the peak must stay above the ground (the eye 40 m up)
    ok = all(ground(x + (peak[0] - x) * k / 10.0, z + (peak[1] - z) * k / 10.0) < (h + 40.0) + (peak[2] - h - 40.0) * k / 10.0 + 60.0 for k in range(1, 9))
    # and the sea should be near
    near_sea = any(abs(ground(x + dx, z + dz) - sea) < 12 for dx in (-300, 0, 300) for dz in (-300, 0, 300))
    if ok and near_sea:
        score = -abs(d - 2600.0)
        if best is None or score > best[0]:
            best = (score, x, z, h)
print("peak", peak, "viewpoint", best)
_, vx, vz, vh = best if best else (0, peak[0] - 1500, peak[1] + 1500, ground(peak[0] - 1500, peak[1] + 1500))

# blade grass on the meadows round the viewpoint, boulders on the rock
call("spawn", {"uid": 950, "components": {"GrassField": {"terrain": tid, "radius": 140, "density": 20.0, "height": [0.3, 0.7], "blade_width": 0.04, "mask_layer": 1, "color_a": [0.9, 1.0, 0.8], "color_b": [1.5, 1.2, 0.8], "patch_size": 80.0}}})
M = "examples/gfx_lab/models"
for mid, count, scale, seed in (("boulder_01", 30, (1.5, 4.0), 3), ("rock_07", 120, (6.0, 16.0), 4), ("rock_09", 120, (8.0, 20.0), 5)):
    if os.path.exists(f"{M}/{mid}/{mid}_lunaris.gltf"):
        r = call("scatter", {"terrain": tid, "components": {"MeshRenderer": {"mesh": f"{M}/{mid}/{mid}_lunaris.gltf", "roughness": 0.9}}, "count": count, "slope": [0, 60], "min_distance": 6.0, "scale": list(scale), "seed": seed, "yaw": True, "sink": 0.1,
                                  "area": [vx - 250, vz - 250, vx + 250, vz + 250], "layer": 1, "layer_min": 0.5})
        print(mid, "placed", r.get("placed"))
call("step", {"ticks": 5})

look = [peak[0], peak[2] - 250.0, peak[1]]
dx, dz = look[0] - vx, look[2] - vz
yaw = math.degrees(math.atan2(-dx, -dz))
shots = [
    ("meadow", [vx, vh + 1.7, vz], [vx + dx, vh + 1.7 + (look[1] - vh - 1.7) * 0.12, vz + dz], 64),
    ("shore", [vx, vh + 4.0, vz], [vx + dx * 0.9 - dz * 0.35, vh + 40.0, vz + dz * 0.9 + dx * 0.35], 60),
    ("aerial", [vx - dx * 0.35, vh + 420.0, vz - dz * 0.35], [peak[0], peak[2] - 400.0, peak[1]], 70),
]
for name, eye, target, fov in shots:
    if name not in os.environ.get("ONLY", "meadow,shore,aerial").split(","):
        continue
    r = call("render", {"quality": "ultra", "width": W, "height": H, "eye": eye, "target": target, "fov": fov, "path": f"fjord_{name}.png", "image": False})
    print(name, r.get("renderer"), r.get("warnings"), r.get("measured"), (r.get("timing_ms") or {}).get("total"))
proc.stdin.close()
proc.wait(timeout=120)
