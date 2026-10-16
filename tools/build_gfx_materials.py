"""The surfaces `MaterialExt` adds (examples/gfx_lab/materials.png): a plain rough ball, the same red under a clear coat (car paint), a velvet ball (sheen), a wax ball with light bleeding through (subsurface) and a leaf-green
ball with subsurface and sheen, on a dark floor with the sun low to one side.

    python3 tools/build_gfx_materials.py
"""
import json
import math
import os
import subprocess

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
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


call("reset")
uid = [10]


def sp(comps):
    uid[0] += 1
    call("spawn", {"uid": uid[0], "components": comps})


sp({"Environment": {"sky": True, "clouds": 0.35, "bloom": 0.35, "bloom_threshold": 1.0, "ssao": 1.0, "shadows": True, "tonemap": 1, "vignette": 0.2, "fog": 0.0001, "ground": [0.1, 0.1, 0.11], "shadow_distance": 60}})
sp({"Light": {"kind": 0, "intensity": 3.2, "color": [1.0, 0.95, 0.88]}, "Transform": {"rot": quat(60.0, -22.0)}})
sp({"Atmosphere": {"turbidity": 2.2, "haze_glow": 1.4}})
sp({"Shadows": {"softness": 0.025}})
sp({"AutoExposure": {"min": 0.3, "max": 3.0, "speed": 4.0, "target": 0.3}})
sp({"Ssr": {"strength": 0.8, "max_distance": 40, "thickness": 0.5}})
sp({"Transform": {"pos": [0, 0, 0], "scale": [80, 1, 40]}, "MeshRenderer": {"mesh": "plane", "color": [0.045, 0.045, 0.05, 1.0], "roughness": 0.35, "metallic": 0.0}})
balls = (
    ("plain", {"color": [0.55, 0.04, 0.04, 1.0], "roughness": 0.8}, None),
    ("coat", {"color": [0.55, 0.04, 0.04, 1.0], "roughness": 0.8}, {"clearcoat": 1.0, "clearcoat_roughness": 0.04}),
    ("velvet", {"color": [0.04, 0.07, 0.3, 1.0], "roughness": 0.9}, {"sheen": 1.0}),
    ("wax", {"color": [0.85, 0.7, 0.55, 1.0], "roughness": 0.45}, {"subsurface": 1.0, "subsurface_color": [1.0, 0.25, 0.12], "clearcoat": 0.3, "clearcoat_roughness": 0.25}),
    ("leaf", {"color": [0.12, 0.3, 0.04, 1.0], "roughness": 0.6}, {"subsurface": 0.9, "subsurface_color": [0.55, 0.85, 0.15], "sheen": 0.5}),
)
for i, (name, mesh, ext) in enumerate(balls):
    comps = {"Transform": {"pos": [-6.4 + i * 3.2, 1.45, 0.0], "scale": [2.9, 2.9, 2.9]}, "MeshRenderer": dict(mesh="sphere", **mesh)}
    if ext:
        comps["MaterialExt"] = ext
    sp(comps)
sp({"Transform": {"pos": [0.0, 2.6, 15.5], "rot": quat(0.0, -4.0)}, "Camera": {"fov_y": 34, "active": True}})
call("step", {"ticks": 5})
r = call("render", {"quality": "ultra", "width": 1800, "height": 620, "path": "materials.png", "image": False})
print(r.get("renderer"), r.get("warnings"), (r.get("timing_ms") or {}).get("total"))
proc.stdin.close()
