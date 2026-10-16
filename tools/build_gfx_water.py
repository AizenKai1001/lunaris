"""A pond for the renderer's water (examples/gfx_lab/water_*.png): a sloping bed, rocks and posts standing in the water, the sun low behind them, the sky of the air, and `WaterLook` giving the water its depth: clear and tinted
over the shallows, dark over the deep, with foam where it touches the shore, the rocks and the posts.

    python3 tools/build_gfx_water.py
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


def scene(look):
    call("reset")
    uid = [10]

    def sp(comps):
        uid[0] += 1
        call("spawn", {"uid": uid[0], "components": comps})

    sp({"Environment": {"sky": True, "clouds": 0.35, "bloom": 0.4, "bloom_threshold": 0.9, "ssao": 1.0, "shadows": True, "tonemap": 1, "vignette": 0.2, "fog": 0.0002, "ground": [0.2, 0.22, 0.16], "shadow_distance": 200}})
    sp({"Light": {"kind": 0, "intensity": 3.4, "color": [1.0, 1.0, 1.0]}, "Transform": {"rot": quat(150.0, -24.0)}})
    sp({"Atmosphere": {"turbidity": 2.4, "haze_glow": 1.8}})
    sp({"Shadows": {"softness": 0.03}})
    sp({"AutoExposure": {"min": 0.3, "max": 3.0, "speed": 4.0, "target": 0.32}})
    if not os.environ.get("NOSSR"):
        sp({"Ssr": {"strength": 0.9, "max_distance": 80, "thickness": 0.6}})
    # the bed: a bank on the near side sloping down into the pond, a sandy far shore
    for i in range(24):
        t = i / 23.0
        z = 12.0 - t * 40.0
        y = -0.1 - 4.5 * math.sin(min(t * 1.4, 1.0) * math.pi / 2) if t < 0.72 else -4.5 + (t - 0.72) * 22.0
        sp({"Transform": {"pos": [0, y, z], "scale": [90, 1, 40.0 / 23 * 1.3], "rot": [0, 0, 0, 1]}, "MeshRenderer": {"mesh": "plane", "color": [0.62, 0.55, 0.4, 1.0], "roughness": 0.95}})
    for x, z, s in ((-6, -3, 1.6), (5, -8, 2.2), (-2, -14, 1.2), (9, -2, 1.0)):
        sp({"Transform": {"pos": [x, -0.2, z], "scale": [s, s * 1.1, s * 0.9], "rot": quat(x * 20.0, 0)}, "MeshRenderer": {"mesh": "sphere", "color": [0.38, 0.37, 0.35, 1.0], "roughness": 0.9}})
    for x, z in ((-3, -1), (-1.4, -5), (2.6, -3)):
        sp({"Transform": {"pos": [x, 0.9, z], "scale": [0.22, 3.2, 0.22]}, "MeshRenderer": {"mesh": "cylinder", "color": [0.35, 0.26, 0.17, 1.0], "roughness": 0.9}})
    water = {"Transform": {"pos": [0, 0, -4]}, "Water": {"size": [120, 80], "color": [0.03, 0.2, 0.28, 0.9], "wave_height": 0.12, "wave_length": 9, "speed": 0.9}}
    if look:
        water["WaterLook"] = look
    sp(water)
    sp({"Transform": {"pos": [0, 1.7, 13.0], "rot": quat(0.0, -2.0)}, "Camera": {"fov_y": 62, "active": True}})


TAG = os.environ.get("TAG", "")
FOAM = float(os.environ.get("FOAM", "0.8"))
for name, look in (("pond" + TAG, {"clarity": 3.5, "shallow": [0.3, 0.62, 0.55], "foam": FOAM, "foam_width": 0.8}),):
    scene(look)
    call("step", {"ticks": 30})
    r = call("render", {"quality": "ultra", "width": 1600, "height": 900, "path": f"water_{name}.png", "image": False})
    print(name, r.get("renderer"), r.get("warnings"), r.get("measured"))
proc.stdin.close()
proc.wait(timeout=120)
