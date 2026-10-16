"""Pictures of Wyrdwood at different hours and in different weather, for judging the look (runs where target/release/lunaris is):

    python3 tools/wyrdwood_shots.py [scene] OUT_PREFIX     writes OUT_PREFIX_<name>.png for each of the views below
"""
import json
import subprocess
import sys

SCENE = sys.argv[1] if len(sys.argv) > 2 else "examples/wyrdwood/wyrdwood.lunaris.json"
PREFIX = sys.argv[-1]
META = json.load(open("examples/wyrdwood/worlds/seed23/wyrdwood.json"))
SX, SZ = META["start"]["x"], META["start"]["z"]
VIEWS = [("dawn", 6.2, None), ("morning", 8.0, None), ("noon", 12.0, None), ("afternoon", 16.5, None), ("sunset", 18.6, None), ("dusk", 20.2, None), ("night", 23.0, None), ("mist", 7.5, 1), ("rain", 14.0, 4), ("storm", 15.0, 5)]
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
    for c in res.get("content") or []:
        if c.get("type") == "text":
            try:
                return json.loads(c["text"])
            except ValueError:
                return {}
    return {}


call("load_scene", {"path": SCENE})
for name, hour, wx in VIEWS:
    call("set_resource", {"name": "clock.init", "value": 1})
    call("set_resource", {"name": "clock.t", "value": hour / 24 * 1500})
    call("set_resource", {"name": "wx.force.on", "value": 1 if wx is not None else 0})
    call("set_resource", {"name": "wx.force", "value": wx or 0})
    call("step", {"ticks": 4})
    call("render", {"quality": "high", "width": 1280, "height": 720, "eye": [SX + 18, META["start"]["y"] + 9, SZ + 24], "target": [SX - 10, META["start"]["y"] + 6, SZ - 60], "fov": 62, "path": f"{PREFIX}_{name}.png", "image": False})
    print("shot", name, hour, wx)
proc.stdin.close()
proc.wait(timeout=60)
