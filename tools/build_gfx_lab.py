"""A Cornell box for the renderer (examples/gfx_lab.lunaris.json): a closed room of white, red and green walls, two white blocks, a metal ball and a glass-bright lamp in the ceiling, lit by that lamp alone. It is how the lighting of the
renderer is judged: with `GlobalIllumination` the red and green walls colour the white ones next to them and the blocks' shadows are not black; without it they are.

    python3 tools/build_gfx_lab.py
"""
import json
import os
import subprocess
import sys

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
proc = subprocess.Popen([BIN, "serve", "--root", "."], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
n_req = 0
uid = [10]


def call(name, args=None):
    global n_req
    n_req += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n_req, "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + chr(10))
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line:
            raise SystemExit(f"the engine stopped during {name}")
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("id") == n_req:
            break
    res = m.get("result") or {}
    if "error" in m or res.get("isError"):
        print("request", n_req, name, "failed:", json.dumps(m.get("error") or res)[:500])
        return {}
    for c in res.get("content") or []:
        if c.get("type") == "text":
            try:
                return json.loads(c["text"])
            except ValueError:
                return {"text": c["text"]}
    return {}


def spawn(name, **c):
    u = uid[0]
    uid[0] += 1
    call("spawn", {"uid": u, "components": {"Name": {"value": name}, **c}})
    return u


def box(name, pos, size, color, rough=0.9, metal=0.0, emissive=0.0, mesh="cube"):
    return spawn(name, Transform={"pos": pos, "scale": size}, MeshRenderer={"mesh": mesh, "color": [*color, 1.0], "roughness": rough, "metallic": metal, "emissive": emissive}, Collider={"shape": 1, "size": [1, 1, 1]})


W, H, D = 6.0, 4.0, 6.0
call("reset")
spawn("Environment", Environment={"sky": False, "ground": [0.02, 0.02, 0.02], "ambient": 0.05, "exposure": 1.1, "ssao": 1.0, "bloom": 0.3, "bloom_threshold": 1.0, "shadows": True, "shadow_distance": 20, "vignette": 0.15,
                                  "sky_top": [0.01, 0.01, 0.01], "sky_horizon": [0.01, 0.01, 0.01], "tonemap": 1})
spawn("Soft shadows", Shadows={"softness": 0.03})
if not os.environ.get("NO_GI"):
    spawn("Bounce", GlobalIllumination={"intensity": 1.0, "radius": 6.0, "directions": 12, "thickness": 0.7})
# the room: floor, ceiling, back, left (red), right (green); the front is open to the camera
white, red, green = (0.78, 0.78, 0.76), (0.72, 0.1, 0.08), (0.12, 0.55, 0.14)
box("Floor", [0, -0.05, 0], [W, 0.1, D], white)
box("Ceiling", [0, H + 0.05, 0], [W, 0.1, D], white)
box("Back", [0, H / 2, -D / 2 - 0.05], [W, H, 0.1], white)
box("Left", [-W / 2 - 0.05, H / 2, 0], [0.1, H, D], red)
box("Right", [W / 2 + 0.05, H / 2, 0], [0.1, H, D], green)
box("Tall block", [-1.3, 1.2, -1.2], [1.5, 2.4, 1.5], white)
box("Short block", [1.2, 0.6, 0.4], [1.4, 1.2, 1.4], white)
spawn("Metal ball", Transform={"pos": [1.2, 1.55, 0.4], "scale": [0.7, 0.7, 0.7]}, MeshRenderer={"mesh": "sphere", "color": [0.95, 0.85, 0.6, 1.0], "roughness": 0.18, "metallic": 1.0})
box("Lamp panel", [0, H - 0.03, -0.3], [1.6, 0.06, 1.6], (1.0, 0.95, 0.85), rough=0.5, emissive=3.0)
spawn("Lamp", Transform={"pos": [0, H - 0.5, -0.3]}, Light={"kind": 1, "color": [1.0, 0.92, 0.78], "intensity": float(os.environ.get("LAMP", "2.2")), "range": 12.0})
spawn("Camera", Transform={"pos": [0, 2.0, 8.4]}, Camera={"fov_y": 60, "near": 0.1, "far": 100, "active": True})
call("step", {"ticks": 2})
call("check")
tag = sys.argv[1] if len(sys.argv) > 1 else "gfx_lab"
r = call("render", {"quality": "high", "width": 1280, "height": 720, "eye": [0, 2.0, 8.4], "target": [0, 1.9, 0], "fov": 60, "path": f"{tag}.png", "image": False})
print(tag, r.get("renderer"), r.get("warnings"), (r.get("measured") or {}))
if not os.environ.get("NO_GI") and len(sys.argv) < 3:
    call("save_scene", {"path": "examples/gfx_lab.lunaris.json"})
proc.stdin.close()
proc.wait(timeout=120)
