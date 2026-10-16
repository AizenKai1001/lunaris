"""Finds how a tool sits in the hero's right hand: renders an axe fixed to the hand joint with several turns (runs where target/release/lunaris is): python3 tools/hold_shots.py PREFIX"""
import json
import math
import subprocess
import sys

PREFIX = sys.argv[1]
proc = subprocess.Popen(["target/release/lunaris", "serve", "--root", "."], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
n = 0


def call(name, args=None):
    global n
    n += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + "\n")
    proc.stdin.flush()
    while True:
        m = json.loads(proc.stdout.readline())
        if m.get("id") == n:
            break
    res = m.get("result") or {}
    if res.get("isError"):
        print(name, "failed", json.dumps(res)[:300])


def q(axis, deg):
    h = math.radians(deg) / 2
    s = math.sin(h)
    return [s * (axis == "x"), s * (axis == "y"), s * (axis == "z"), math.cos(h)]


def spawn(uid, **c):
    call("spawn", {"uid": uid, "components": c})


call("reset")
spawn(1, Environment={"sky": True, "sky_top": [0.3, 0.5, 0.85], "sky_horizon": [0.8, 0.86, 0.95], "ground": [0.35, 0.36, 0.34], "shadows": True})
spawn(2, Transform={"rot": [-0.5, 0.35, 0.15, 0.78]}, Light={"kind": 0, "color": [1, 0.95, 0.85], "intensity": 3.0})
spawn(3, Transform={"pos": [0, -0.05, 0], "scale": [20, 0.1, 20]}, MeshRenderer={"mesh": "cube", "color": [0.45, 0.5, 0.4, 1], "roughness": 0.95})
spawn(10, Transform={"pos": [0, 0, 0]}, MeshRenderer={"mesh": "examples/wyrdwood/models/hero.glb", "color": [1, 1, 1, 1], "roughness": 0.8}, Animator={"clip": "Idle_Loop", "playing": False, "speed": 0, "time": 0.5})
for k, (name, rot) in enumerate([("id", [0, 0, 0, 1]), ("x90", q("x", 90)), ("xm90", q("x", -90)), ("z90", q("z", 90)), ("zm90", q("z", -90)), ("x180", q("x", 180))]):
    # the axe in its own frame: the handle up +y, the head at the top towards +x
    for uid in (20, 21):
        call("despawn", {"uid": uid}) if k else None
    spawn(20, Transform={}, MeshRenderer={"mesh": "cylinder", "color": [0.45, 0.3, 0.18, 1]}, BoneAttach={"target": 10, "joint": "DEF-hand.R", "pos": [0, 0.0, 0], "rot": rot, "scale": [0.05, 0.6, 0.05]})
    spawn(21, Transform={}, MeshRenderer={"mesh": "cube", "color": [0.6, 0.6, 0.66, 1]}, BoneAttach={"target": 10, "joint": "DEF-hand.R", "pos": [0.0, 0.0, 0], "rot": rot, "scale": [0.2, 0.12, 0.05]})
    call("render", {"quality": "high", "width": 640, "height": 640, "eye": [1.2, 1.3, 2.2], "target": [-0.2, 0.9, 0], "fov": 38, "path": f"{PREFIX}_{name}.png", "image": False})
    print("shot", name)
proc.stdin.close()
proc.wait(timeout=60)
