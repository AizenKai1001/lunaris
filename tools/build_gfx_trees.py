"""Imported models with several materials (examples/gfx_lab/trees_*.png): the same Poly Haven sapling twice, on the left as `prepare_polyhaven_models.py` made it (one texture for the whole model, the other materials as
their average colour) and on the right as the original glTF with its two materials, their six photographs (colour, normal and metallic-roughness maps for the bark and for the twigs) imported as they are.

    python3 tools/fetch_polyhaven_model.py pine_sapling_small fir_sapling
    python3 tools/build_gfx_trees.py
"""
import json
import math
import os
import subprocess

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
W, H = int(os.environ.get("W", "1600")), int(os.environ.get("H", "900"))
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


M = "examples/gfx_lab/models"
uid = [10]


def sp(comps):
    uid[0] += 1
    call("spawn", {"uid": uid[0], "components": comps})
    return uid[0]


for model in os.environ.get("MODELS", "pine_sapling_small,fir_sapling").split(","):
    call("reset")
    sp({"Environment": {"sky": True, "clouds": 0.3, "ssao": 1.0, "bloom": 0.1, "vignette": 0.15, "tonemap": 1, "shadow_distance": 60, "ambient": 1.0, "ground": [0.25, 0.3, 0.2]}})
    sp({"Light": {"kind": 0, "intensity": 3.6, "color": [1.0, 0.92, 0.8]}, "Transform": {"rot": quat(300, -28)}})
    sp({"Atmosphere": {"turbidity": 2.2, "intensity": 1.0}})
    sp({"Shadows": {"softness": 0.02}})
    sp({"AutoExposure": {"min": 0.3, "max": 2.5, "speed": 3.0, "target": 0.34}})
    sp({"Camera": {"fov_y": 40, "active": True}, "Transform": {"pos": [0, 1.4, 6.0]}})
    sp({"Transform": {"pos": [0, 0, 0], "scale": [40, 1, 40]}, "MeshRenderer": {"mesh": "plane", "color": [0.28, 0.34, 0.18, 1], "roughness": 0.95}})
    for x, name in ((-1.6, f"{model}_lunaris.gltf"), (1.6, f"{model}_1k.gltf")):
        sp({"Transform": {"pos": [x, 0, 0]}, "MeshRenderer": {"mesh": f"{M}/{model}/{name}", "roughness": 0.8}})
    call("step", {"ticks": 3})
    eye = [float(v) for v in os.environ.get("EYE", "0,1.3,5.2").split(",")]
    target = [float(v) for v in os.environ.get("TARGET", "0,1.0,0").split(",")]
    r = call("render", {"quality": os.environ.get("Q", "ultra"), "width": W, "height": H, "eye": eye, "target": target, "fov": float(os.environ.get("FOV", "42")), "path": f"trees_{model.split('_')[0]}{os.environ.get('TAG', '')}.png", "image": False})
    print(model, r.get("renderer"), r.get("warnings"), (r.get("timing_ms") or {}).get("total"), "texture_mb", (r.get("measured") or {}).get("texture_mb"))
proc.stdin.close()
proc.wait(timeout=120)
