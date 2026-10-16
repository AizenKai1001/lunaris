"""Pictures of the `Atmosphere` for the renderer's docs (examples/gfx_lab/sky_*.png): a plain of grass-green ground under the sky at noon, in the late afternoon, at sunset and at dusk, the same camera.

    python3 tools/build_gfx_sky.py
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


for name, elevation in (("noon", 70.0), ("afternoon", 28.0), ("sunset", 3.0), ("dusk", -4.5)):
    call("reset")
    call("spawn", {"uid": 1, "components": {"Environment": {"sky": True, "clouds": 0.0, "bloom": 0.35, "bloom_threshold": 1.0, "ssao": 1.0, "shadows": True, "tonemap": 1, "exposure": 1.0, "ground": [0.2, 0.26, 0.14], "fog": 0.00045, "vignette": 0.15}}})
    call("spawn", {"uid": 2, "components": {"Light": {"kind": 0, "intensity": 3.2, "color": [1.0, 1.0, 1.0]}, "Transform": {"rot": quat(90.0, -elevation)}}})
    call("spawn", {"uid": 3, "components": {"Atmosphere": {"turbidity": 2.5, "haze_glow": 2.0}}})
    call("spawn", {"uid": 4, "components": {"Shadows": {"softness": 0.03}}})
    call("spawn", {"uid": 5, "components": {"AutoExposure": {"min": 0.05, "max": 6.0, "speed": 8.0, "target": 0.3}}})
    call("spawn", {"uid": 6, "components": {"Transform": {"pos": [0, -0.5, 0], "scale": [3000, 1, 3000]}, "MeshRenderer": {"mesh": "plane", "color": [0.2, 0.32, 0.12, 1.0], "roughness": 0.95}}})
    for i, (x, z, h) in enumerate(((60, -90, 14), (-40, -150, 22), (120, -60, 9), (-110, -40, 11), (30, -220, 28))):
        call("spawn", {"uid": 10 + i, "components": {"Transform": {"pos": [x, h / 2, z], "scale": [3 + h * 0.25, h, 3 + h * 0.25]}, "MeshRenderer": {"mesh": "cylinder", "color": [0.28, 0.26, 0.22, 1.0], "roughness": 0.9}}})
    call("spawn", {"uid": 30, "components": {"Transform": {"pos": [0, 1.7, 0], "rot": quat(-90.0, 6.0)}, "Camera": {"fov_y": 70, "active": True}}})
    call("step", {"ticks": 2})
    r = call("render", {"quality": "high", "width": 1280, "height": 720, "path": f"sky_{name}.png", "image": False})
    print(name, r.get("renderer"), r.get("warnings"), r.get("measured"))
proc.stdin.close()
proc.wait(timeout=60)
