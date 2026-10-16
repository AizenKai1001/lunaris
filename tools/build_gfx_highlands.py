"""Pictures of a landscape made by Tellus (examples/gfx_lab/highlands_*.png): 8 km of rolling highlands cut by rivers, forests in the valleys and a lake, lit by a low golden sun through the renderer's air, with
the clouds' slab, soft shadows, bounced light and haze that gives the distance its depth.

    tellus export template:highlands --res 1025 --engine examples/gfx_lab/highlands --prefix examples/gfx_lab/highlands/ --scene examples/gfx_lab/highlands.lunaris.json
    python3 tools/build_gfx_highlands.py      (the scene and its pictures are not committed: about 14 MB)
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


call("load_scene", {"path": "examples/gfx_lab/highlands.lunaris.json"})
sun = [e["uid"] for e in call("query", {"with": ["Light"], "select": ["Light"]}).get("entities", []) if (e.get("components") or {}).get("Light", {}).get("kind") == 0][0]
elevation = float(os.environ.get("SUN", "15"))
call("set", {"uid": sun, "component": "Transform", "value": {"rot": quat(215.0, -elevation)}})
call("set", {"uid": sun, "component": "Light", "value": {"intensity": 3.6, "color": [1.0, 0.93, 0.8]}})
env = [e["uid"] for e in call("query", {"with": ["Environment"], "select": []}).get("entities", [])][0]
call("set", {"uid": env, "component": "Environment", "value": {"sky": True, "clouds": 0.5, "fog": 0.00002, "ssao": 1.0, "bloom": 0.4, "bloom_threshold": 0.95, "vignette": 0.2, "saturation": 1.05, "tonemap": 1, "shadow_distance": 3500, "ambient": 1.0, "exposure": 1.0}})
for uid, comps in (
    (900, {"Atmosphere": {"turbidity": 2.4, "haze_glow": 2.0, "intensity": 1.0}}),
    (901, {"Shadows": {"softness": 0.03}}),
    (902, {"GlobalIllumination": {"intensity": 0.8, "radius": 40.0, "directions": 10}}),
    (903, {"AutoExposure": {"min": 0.25, "max": 3.0, "speed": 3.0, "target": 0.3}}),
    (904, {"PostFx": {"shafts": 0.4, "contrast": 1.05, "grain": 0.08}}),
):
    call("spawn", {"uid": uid, "components": comps})
# the ground gets close detail: Poly Haven's grass, rock and soil photographs blended by Tellus's own splat map (R grass and forest, G rock and scree, B soil, A snow), and the water follows the build's water picture
# (every lake and river, at its own level) instead of the flat sheets of the export
tid = [e["uid"] for e in call("query", {"with": ["Terrain"], "select": []}).get("entities", [])][0]
got = call("get", {"uid": tid, "components": ["Terrain", "Transform"]}).get("components", {})
terrain = dict(got["Terrain"])
terrain.update({"texture": "", "color": [1, 1, 1, 1]})
call("despawn", {"uid": tid})
T = "examples/hollowlight/textures"
layers = "\n".join((
    f"grass {T}/sparse_grass/sparse_grass_diff.jpg uv_scale=1400 tint=0.7,0.95,0.55",
    f"rock {T}/aerial_grass_rock/aerial_grass_rock_diff.jpg uv_scale=700 tint=0.8,0.8,0.78",
    f"soil {T}/grass_ground/grass_ground_diff.jpg uv_scale=900 tint=0.85,0.7,0.5",
    f"snow {T}/withered_grass/withered_grass_diff.jpg uv_scale=600 tint=1.6,1.6,1.7",
))
call("spawn", {"uid": tid, "components": {"Transform": got["Transform"], "Terrain": terrain, "TerrainLayers": {"splat": "examples/gfx_lab/highlands/splat.png", "layers": layers}}})
for e in call("query", {"with": ["Water"], "select": []}).get("entities", []):
    call("despawn", {"uid": e["uid"]})
call("spawn", {"uid": 950, "components": {"Transform": {"pos": [0.0, 0.0, 0.0]}, "WaterBody": {"heightmap": "examples/gfx_lab/highlands/water.png", "size": [8000.0, 8000.0], "relief": 1100.0, "color": [0.03, 0.2, 0.28, 0.9], "wave_height": 0.1, "wave_length": 14, "speed": 0.8},
                                            "WaterLook": {"clarity": 3.5, "shallow": [0.3, 0.62, 0.55], "foam": 0.5, "foam_width": 0.8}}})
call("reload_terrain", {})
cam = [e["uid"] for e in call("query", {"with": ["Camera"], "select": []}).get("entities", [])][0]
call("set", {"uid": cam, "component": "Camera", "value": {"fov_y": 60.0, "far": 30000.0}})
lake = [269.53125, 224.86, -1171.875]  # the big lake of this build (its sheet in the export)
print("lake at", lake)


def ground(x, z):
    return call("terrain_height", {"x": x, "z": z}).get("height", 0.0)


call("step", {"ticks": 5})
shots = []
# a ridge above the lake looking down the valley, the lake itself, and a high view over the whole range
ex, ez = lake[0] + 900.0, lake[2] + 700.0
shots.append(("valley", [ex, ground(ex, ez) + 60.0, ez], [lake[0], lake[1], lake[2]], 62))
lx, lz = lake[0] - 260.0, lake[2] + 260.0
shots.append(("lake", [lx, max(ground(lx, lz), lake[1]) + 9.0, lz], [lake[0] + 120.0, lake[1] + 25.0, lake[2] - 120.0], 58))
hx, hz = -2400.0, 3000.0
shots.append(("ridge", [hx, ground(hx, hz) + 500.0, hz], [500.0, 300.0, -500.0], 66))
for name, eye, target, fov in shots:
    if name not in os.environ.get("ONLY", "valley,lake,ridge").split(","):
        continue
    r = call("render", {"quality": "ultra", "width": W, "height": H, "eye": eye, "target": target, "fov": fov, "path": f"highlands_{name}.png", "image": False})
    print(name, r.get("renderer"), r.get("warnings"), r.get("measured"), (r.get("timing_ms") or {}).get("total"))
proc.stdin.close()
proc.wait(timeout=120)
