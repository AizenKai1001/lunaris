"""Water physics for the engine (examples/gfx_lab/floaters_*.png): a raft, planks, crates and balls dropped into a rolling lake. Each is a `Floater`: it is lifted by columns of its own volume, so it rolls and pitches with the
waves, rights itself when it is tipped, and is carried by the `WaterFlow` (docs/TERRAIN.md, "Floaters and waves"). The pictures are the same scene a little apart in time.

    python3 tools/build_water_physics.py
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


call("reset")
uid = [10]


def sp(comps):
    uid[0] += 1
    call("spawn", {"uid": uid[0], "components": comps})
    return uid[0]


sp({"Environment": {"sky": True, "clouds": 0.4, "bloom": 0.4, "bloom_threshold": 0.9, "ssao": 1.0, "shadows": True, "tonemap": 1, "vignette": 0.2, "fog": 0.0002, "ground": [0.2, 0.22, 0.16], "shadow_distance": 120}})
sp({"Light": {"kind": 0, "intensity": 2.0, "color": [1.0, 0.97, 0.92]}, "Transform": {"rot": quat(335.0, -38.0)}})
sp({"Atmosphere": {"turbidity": 2.4, "haze_glow": 1.8}})
sp({"Shadows": {"softness": 0.03}})
sp({"AutoExposure": {"min": 0.3, "max": 3.0, "speed": 4.0, "target": 0.26}})
sp({"Ssr": {"strength": 0.9, "max_distance": 80, "thickness": 0.6}})
sp({"Transform": {"pos": [0, -3.5, -10], "scale": [200, 1, 120]}, "MeshRenderer": {"mesh": "plane", "color": [0.62, 0.55, 0.4, 1.0], "roughness": 0.95}})
sp({"Transform": {"pos": [0, 0, -6]}, "Water": {"size": [160, 100], "color": [0.03, 0.2, 0.28, 0.9], "wave_height": 0.7, "wave_length": 9, "speed": 1.4},
    "WaterFlow": {"velocity": [0.2, 0.0]}, "WaterLook": {"clarity": 3.5, "shallow": [0.3, 0.62, 0.55], "foam": 0.6, "foam_width": 0.8}})
wood = [0.45, 0.3, 0.17, 1.0]


def body(pos, scale, mesh, color, mass, density, columns=3, yaw=0.0, rough=0.7):
    shape = {"cube": 1, "sphere": 0}[mesh]
    sp({"Transform": {"pos": pos, "scale": scale, "rot": quat(yaw, 0)}, "MeshRenderer": {"mesh": mesh, "color": color, "roughness": rough, "metallic": 0.0}, "Velocity": {}, "AngularVelocity": {"damping": 0.1},
        "Collider": {"shape": shape, "size": [1, 1, 1]}, "RigidBody": {"mass": mass, "restitution": 0.1}, "Floater": {"density": density, "columns": columns}})


body([-1.0, 4.0, -2.0], [5.0, 0.35, 3.2], "cube", wood, 120, 450, 5, 12)                # a raft
body([-7.5, 7.5, -4.0], [3.6, 0.22, 0.9], "cube", [0.55, 0.4, 0.22, 1.0], 20, 500, 4, -30)  # a plank, dropped tipped by its spin
body([5.5, 9.0, -1.0], [1.0, 1.0, 1.0], "cube", [0.7, 0.2, 0.15, 1.0], 30, 400, 3, 20)    # crates
body([7.8, 6.0, -4.5], [1.0, 1.0, 1.0], "cube", [0.2, 0.4, 0.7, 1.0], 30, 600, 3, 55)
body([3.2, 12.0, -6.0], [1.2, 1.2, 1.2], "cube", [0.8, 0.7, 0.2, 1.0], 30, 300, 3, 5)
body([-4.0, 10.0, 1.5], [0.9, 0.9, 0.9], "sphere", [0.9, 0.9, 0.85, 1.0], 20, 600, 1, 0, 0.25)  # balls
body([1.5, 14.0, 2.0], [0.7, 0.7, 0.7], "sphere", [0.85, 0.15, 0.15, 1.0], 10, 250, 1, 0, 0.25)
body([-9.0, 5.0, 0.0], [0.6, 0.6, 0.6], "sphere", [0.15, 0.55, 0.2, 1.0], 6, 150, 1, 0, 0.25)
sp({"Transform": {"pos": [-1.0, 5.2, 12.5], "rot": quat(0.0, -17.0)}, "Camera": {"fov_y": 54, "active": True}})
for name, ticks in (("drop", 36), ("settled", 220), ("later", 160)):
    call("step", {"ticks": ticks})
    r = call("render", {"quality": "ultra", "width": W, "height": H, "path": f"floaters_{name}.png", "image": False})
    print(name, r.get("renderer"), r.get("warnings"), (r.get("timing_ms") or {}).get("total"))
proc.stdin.close()
proc.wait(timeout=120)
