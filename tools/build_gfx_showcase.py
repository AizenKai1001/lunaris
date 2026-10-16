"""The renderer's best picture (examples/gfx_lab/showcase_*.png): the hills of `tools/build_hollowlight_hills.py` with the GPU's grass in the wind, a sun low in the sky and the air round it (`Atmosphere`), soft contact-hardening
shadows (`Shadows`), bounced light (`GlobalIllumination`), eye adaptation, and the temporal anti-aliasing of the `high` quality (the still is eight pictures averaged).

    python3 tools/build_gfx_showcase.py      (needs examples/hollowlight/hills.lunaris.json and its textures)
"""
import json
import math
import os
import subprocess
import sys

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


call("load_scene", {"path": "examples/hollowlight/hills.lunaris.json"})
for comp in ("Lens", "PostFx", "AutoExposure", "HeightFog"):
    for e in call("query", {"with": [comp], "select": []}).get("entities", []):
        call("despawn", {"uid": e["uid"]})
for comp in ("Text", "RecDot", "Ui"):
    for e in call("query", {"with": [comp], "select": []}).get("entities", []):
        call("despawn", {"uid": e["uid"]})
sun = [e["uid"] for e in call("query", {"with": ["Light"], "select": ["Light"]}).get("entities", []) if (e.get("components") or {}).get("Light", {}).get("kind") == 0][0]
elevation = float(os.environ.get("SUN", "9"))
call("set", {"uid": sun, "component": "Transform", "value": {"rot": quat(300.0, -elevation)}})
call("set", {"uid": sun, "component": "Light", "value": {"intensity": 3.6, "color": [1.0, 1.0, 1.0]}})
env = [e["uid"] for e in call("query", {"with": ["Environment"], "select": []}).get("entities", [])][0]
call("set", {"uid": env, "component": "Environment", "value": {"sky": True, "clouds": 0.45, "fog": 0.00006, "ssao": 1.0, "bloom": 0.5, "bloom_threshold": 0.9, "vignette": 0.22, "saturation": 1.08, "tonemap": 1, "shadow_distance": 600, "ambient": 1.0, "exposure": 1.0}})
for uid, comps in (
    (900, {"Atmosphere": {"turbidity": 2.6, "haze_glow": 2.4, "intensity": 1.0}}),
    (901, {"Shadows": {"softness": 0.03}}),
    (902, {"GlobalIllumination": {"intensity": 1.0, "radius": 8.0, "directions": 12}}),
    (903, {"AutoExposure": {"min": 0.25, "max": 3.0, "speed": 3.0, "target": 0.32}}),
    (904, {"PostFx": {"shafts": 0.5, "contrast": 1.06, "grain": 0.12}}),
    (905, {"HeightFog": {"density": 0.0007, "falloff": 0.05, "height": 6, "color": [0.8, 0.82, 0.86]}}),
):
    call("spawn", {"uid": uid, "components": comps})
# a plain that looks like grass in a photograph: a deep natural green that goes to straw in patches, not the lime of the texture's own colour
for e in call("query", {"with": ["GrassField"], "select": []}).get("entities", []):
    call("set", {"uid": e["uid"], "component": "GrassField", "value": {"color_a": [0.5, 0.66, 0.38], "color_b": [0.95, 0.82, 0.52], "patch_size": 90.0, "density": 14.0, "size": [0.4, 0.8]}})
# trees and stones on the plain, so that the sun has something to throw shadows with and the haze something to hide: Poly Haven's CC0 models when `tools/fetch_polyhaven_model.py` and
# `tools/prepare_polyhaven_models.py` have been run (young conifers in threes, boulders, stones, a fallen trunk), the low-poly pine of the caldera example otherwise
ground = [e["uid"] for e in call("query", {"with": ["Terrain"], "select": []}).get("entities", [])][0]


def model(mid):
    return f"examples/gfx_lab/models/{mid}/{mid}_lunaris.gltf"


if os.path.exists(model("fir_sapling")):
    layers = (
        # model, how many, half the side of the area, least distance apart, scale, steepest slope, seed
        ("pine_sapling_small", 26, 110.0, 12.0, (2.6, 4.2), 25, 5),
        ("fir_sapling", 26, 110.0, 12.0, (2.6, 4.2), 25, 6),
        ("pine_sapling_small", 110, 700.0, 18.0, (3.5, 6.5), 25, 7),
        ("fir_sapling", 110, 700.0, 18.0, (3.5, 6.5), 25, 8),
        ("boulder_01", 16, 140.0, 15.0, (1.2, 3.2), 35, 9),
        ("rock_07", 60, 120.0, 5.0, (6.0, 14.0), 40, 10),
        ("rock_09", 60, 120.0, 5.0, (8.0, 18.0), 40, 11),
        ("dead_tree_trunk_02", 8, 110.0, 25.0, (1.0, 1.8), 20, 12),
    )
    for mid, count, half, apart, scale, slope, seed in layers:
        r = call("scatter", {"terrain": ground, "components": {"MeshRenderer": {"mesh": model(mid), "roughness": 0.9}}, "count": count, "slope": [0, slope], "min_distance": apart, "scale": list(scale), "seed": seed, "yaw": True, "sink": 0.04, "area": [-half, -half, half, half]})
        print(mid, "placed:", r.get("placed"))
else:
    tree = {"MeshRenderer": {"mesh": "examples/caldera/models/pine.glb", "roughness": 0.9}, "Collider": {"shape": 1, "size": [0.7, 3.4, 0.7]}}
    for count, half, apart, seed in ((140, 120.0, 14.0, 5), (500, 900.0, 22.0, 6)):
        r = call("scatter", {"terrain": ground, "components": tree, "count": count, "slope": [0, 25], "min_distance": apart, "scale": [1.6, 3.2], "seed": seed, "yaw": True, "sink": 0.05, "area": [-half, -half, half, half]})
        print("trees:", r.get("placed"))
call("step", {"ticks": 40})
gy = call("terrain_height", {"x": 0.0, "z": 0.0}).get("height", 10.5)
shots = (
    ("sunset", [0.0, gy + 1.5, 0.0], [-80.0, gy + 9.0, 20.0], 62),
    ("low", [4.0, gy + 0.55, -3.0], [-60.0, gy + 6.0, 6.0], 58),
    ("back", [0.0, gy + 1.6, 0.0], [80.0, gy + 12.0, -20.0], 64),
)
for name, eye, target, fov in [x for x in shots if x[0] in os.environ.get("ONLY", "sunset,low,back").split(",")]:
    r = call("render", {"quality": "ultra", "width": W, "height": H, "eye": eye, "target": target, "fov": fov, "path": f"showcase_{name}.png", "image": False})
    print(name, r.get("renderer"), r.get("warnings"), r.get("measured"), (r.get("timing_ms") or {}).get("total"))
proc.stdin.close()
proc.wait(timeout=120)
