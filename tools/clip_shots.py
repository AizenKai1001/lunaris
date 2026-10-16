"""Pictures of clips of a rigged model, for judging animations (runs where target/release/lunaris is):

    python3 tools/clip_shots.py MODEL.glb OUT_PREFIX CLIP[:time] [CLIP[:time] ...]
"""
import json
import subprocess
import sys

MODEL, PREFIX, CLIPS = sys.argv[1], sys.argv[2], sys.argv[3:]
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
        print(name, "failed", json.dumps(res)[:400])
    return res


def spawn(uid, **c):
    call("spawn", {"uid": uid, "components": c})


call("reset")
spawn(1, Environment={"sky": True, "sky_top": [0.3, 0.5, 0.85], "sky_horizon": [0.8, 0.86, 0.95], "ground": [0.35, 0.36, 0.34], "bloom": 0.1, "shadows": True, "ssao": 1.0})
spawn(2, Transform={"rot": [-0.5, 0.35, 0.15, 0.78]}, Light={"kind": 0, "color": [1, 0.95, 0.85], "intensity": 3.0})
spawn(3, Transform={"pos": [0, -0.05, 0], "scale": [20, 0.1, 20]}, MeshRenderer={"mesh": "cube", "color": [0.45, 0.5, 0.4, 1], "roughness": 0.95})
spawn(10, Transform={"pos": [0, 0, 0], "rot": [0, 1, 0, 0]}, MeshRenderer={"mesh": MODEL, "color": [1, 1, 1, 1], "roughness": 0.8}, Animator={"clip": CLIPS[0].split(":")[0], "playing": False, "speed": 0})
for c in CLIPS:
    name, _, t = c.partition(":")
    call("set", {"uid": 10, "component": "Animator", "value": {"clip": name, "time": float(t or 0.0), "playing": False}})
    call("render", {"quality": "high", "width": 640, "height": 720, "eye": [2.6, 1.5, 3.2], "target": [0, 0.9, 0], "fov": 40, "path": f"{PREFIX}_{name}_{t or '0'}.png", "image": False})
    print("shot", name, t)
proc.stdin.close()
proc.wait(timeout=60)
