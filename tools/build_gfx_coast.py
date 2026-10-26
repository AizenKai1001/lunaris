"""The coast of the renderer (examples/gfx_lab/coast_*.png): meadow, dune grass, a beach and the sea with a shore that looks like one. Built from `tools/make_coast.py` (the terrain and its paint), Poly Haven's photographs on the ground,
blade grass on the meadow and the dunes, boulders and driftwood on the sand, and the renderer's air and water.

    python3 -I tools/make_coast.py examples/gfx_lab/coast
    python3 tools/fetch_polyhaven.py sparse_grass rock_face_03 aerial_beach_01 withered_grass
    python3 tools/fetch_polyhaven_model.py boulder_01 rock_07 dead_tree_trunk_02; python3 tools/prepare_polyhaven_models.py
    python3 tools/build_gfx_coast.py
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


def coast_x(z):
    return -80.0 + 130.0 * math.sin(z / 520.0 + 0.8) + 55.0 * math.sin(z / 190.0 + 2.0) + 18.0 * math.sin(z / 71.0)


D = "examples/gfx_lab/coast"
T = "examples/hollowlight/textures"
M = "examples/gfx_lab/models"
meta = json.load(open(f"{D}/coast.json"))
SIZE, BASE, RELIEF = meta["size_m"], meta["base_m"], meta["relief_m"]
call("reset")
SUN_YAW = float(os.environ.get("SUN_YAW", "319"))
SUN_EL = float(os.environ.get("SUN", "8"))
uid = [10]


def sp(comps, fixed=None):
    uid[0] += 1
    u = fixed or uid[0]
    call("spawn", {"uid": u, "components": comps})
    return u


_env = {"sky": True, "clouds": float(os.environ.get("CLOUDS", "0.5")), "fog": 0.00002, "ssao": 1.0, "bloom": float(os.environ.get("BLOOM", "0.14")), "bloom_threshold": 2.2, "vignette": 0.22, "saturation": 1.1, "tonemap": 1, "shadow_distance": 900, "ambient": 1.0, "exposure": 1.0, "ground": [0.2, 0.3, 0.38]}
_env.update(json.loads(os.environ.get("ENVOVR", "{}")))
sp({"Environment": _env})
sun = sp({"Light": {"kind": 0, "intensity": 3.9, "color": [1.0, 0.88, 0.7]}, "Transform": {"rot": quat(SUN_YAW, -SUN_EL)}})
sp({"Atmosphere": {"turbidity": 2.4, "haze_glow": 1.6, "intensity": 1.0}})
sp({"Shadows": {"softness": 0.03}})
sp({"GlobalIllumination": {"intensity": float(os.environ.get("GI", "0.8")), "radius": 14.0, "directions": 10}})
sp({"AutoExposure": {"min": 0.25, "max": 3.0, "speed": 3.0, "target": 0.36}})
sp({"PostFx": {"shafts": float(os.environ.get("SHAFTS", "0.5")), "contrast": 1.06, "grain": 0.06}})
if not os.environ.get("NOSSR"):
    sp({"Ssr": {"strength": 1.0, "max_distance": 160, "thickness": 0.8}})
tid = sp({"Transform": {"pos": [0.0, BASE, 0.0]}, "Terrain": {"heightmap": f"{D}/heightmap.png", "size": [SIZE, SIZE], "height": RELIEF, "step": 1, "color": [1, 1, 1, 1], "friction": 0.9},
          "TerrainLayers": {"splat": f"{D}/splat.png", "layers": "\n".join((
              f"meadow {T}/sparse_grass/sparse_grass_diff.jpg uv_scale={int(SIZE / 3)} tint=0.42,0.6,0.3",
              f"rock {T}/rock_face_03/rock_face_03_diff.jpg uv_scale={int(SIZE / 6)} tint=0.8,0.8,0.78",
              f"sand {T}/aerial_beach_01/aerial_beach_01_diff.jpg uv_scale={int(SIZE / 3)} tint=1.0,0.97,0.9",
              f"dune {T}/withered_grass/withered_grass_diff.jpg uv_scale={int(SIZE / 3)} tint=0.9,0.82,0.5"))}})
sp({"Transform": {"pos": [0.0, 0.0, 0.0]}, "Water": {"size": [60000.0, 60000.0], "color": [0.02, 0.2, 0.26, 0.92], "wave_height": float(os.environ.get("WH", "0.5")), "wave_length": float(os.environ.get("WL", "14.0")), "speed": 1.0},
    "WaterLook": {"clarity": 5.0, "shallow": [0.3, 0.62, 0.55], "foam": 0.9, "foam_width": 0.9}})
cam = sp({"Camera": {"fov_y": 62, "active": True, "far": 60000.0}, "Transform": {"pos": [0, 30, 0]}})


def ground(x, z):
    return call("terrain_height", {"x": x, "z": z}).get("height", 0.0)


zc = float(os.environ.get("Z", "40"))
cx = coast_x(zc)
# blade grass on the meadow (paint layer 1) and the tall dune grass (layer 4)
sp({"GrassField": {"terrain": tid, "radius": 160, "density": 22.0, "height": [0.28, 0.6], "blade_width": 0.04, "mask_layer": 1, "color_a": [0.9, 1.0, 0.8], "color_b": [1.4, 1.15, 0.8], "patch_size": 70.0}})
sp({"GrassField": {"terrain": tid, "radius": 160, "density": 12.0, "height": [0.5, 1.1], "blade_width": 0.045, "mask_layer": 4, "base_color": [0.045, 0.075, 0.03], "tip_color": [0.34, 0.38, 0.15], "color_a": [0.95, 1.0, 0.85], "color_b": [1.25, 1.1, 0.8],
                  "bend": 0.7, "clump_size": 1.6, "seed": 9.0, "patch_size": 50.0}})
ROCKS = (("boulder_01", 14, (1.2, 2.6), 3, 2), ("rock_07", 60, (4.0, 9.0), 4, 2), ("rock_09", 60, (5.0, 11.0), 5, 2), ("dead_tree_trunk_02", 6, (0.9, 1.6), 6, 2))
for mid, count, scale, seed, layer in (() if os.environ.get("NOROCKS") else ROCKS):
    if os.path.exists(f"{M}/{mid}/{mid}_lunaris.gltf"):
        r = call("scatter", {"terrain": tid, "components": {"MeshRenderer": {"mesh": f"{M}/{mid}/{mid}_lunaris.gltf", "roughness": 0.9}}, "count": count, "slope": [0, 40], "min_distance": 8.0, "scale": list(scale), "seed": seed, "yaw": True, "sink": 0.12,
                                  "area": [cx - 30, zc - 260, cx + 60, zc + 260], "layer": layer, "layer_min": 0.5})
        print(mid, "placed", r.get("placed"))
call("step", {"ticks": 5})

shots = []
ex, ez = cx + 62.0, zc
shots.append(("dune", [ex, ground(ex, ez) + 1.5, ez], [ex - 90.0, 1.4, ez + 160.0], 66))
bx, bz = cx + 7.0, zc - 20.0
shots.append(("beach", [bx, ground(bx, bz) + 1.3, bz], [bx - 130.0, 0.6, bz + 150.0], 62))
shots.append(("aerial", [cx + 40.0, 130.0, zc - 260.0], [cx - 150.0, 0.0, zc + 200.0], 66))
OFFSETS = {"dune": 24.0, "beach": 30.0, "aerial": -22.0}
for name, eye, target, fov in shots:
    if name not in os.environ.get("ONLY", "dune,beach,aerial").split(","):
        continue
    # the sun a little off the middle of the picture, so that its path on the water leads to the camera across the frame (the light shines the way the sun's yaw says, from the opposite side)
    heading = math.degrees(math.atan2(target[0] - eye[0], target[2] - eye[2]))
    call("set", {"uid": sun, "component": "Transform", "value": {"rot": quat(heading + OFFSETS[name], -SUN_EL)}})
    r = call("render", {"quality": os.environ.get("Q", "ultra"), "width": W, "height": H, "eye": eye, "target": target, "fov": fov, "path": f"coast_{name}{os.environ.get('TAG', '')}.png", "image": False})
    # REPEAT=n (with LUNARIS_GPU_PROFILE=1): draw it n times and keep the picture whose pass times add up to the least (the card's clock changes from one picture to the next)
    for _ in range(int(os.environ.get("REPEAT", "1")) - 1):
        r2 = call("render", {"quality": os.environ.get("Q", "ultra"), "width": W, "height": H, "eye": eye, "target": target, "fov": fov, "image": False})
        if (r2.get("measured") or {}).get("gpu_ms.total", 1e9) < (r.get("measured") or {}).get("gpu_ms.total", 1e9):
            r = r2
    if os.environ.get("PICK"):
        px, py = [int(v) for v in os.environ["PICK"].split(",")]
        print("pick", call("pick", {"x": px, "y": py, "width": W, "height": H, "eye": eye, "target": target, "fov": fov}))
    print(name, r.get("renderer"), r.get("warnings"), r.get("measured"), (r.get("timing_ms") or {}).get("total"))
proc.stdin.close()
proc.wait(timeout=120)
