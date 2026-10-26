"""Pictures of a streamed procedural city from given eyes (run on the machine with the GPU, here a Linux test machine):

    python3 tools/citygen/views.py ROOT SCENE OUTDIR NAME=ex,ey,ez,tx,ty,tz[,fov] ...

For every view the scene's camera entity is moved to the eye (so that the StreamZones around it load: the floors of the building it stands in, the street), the world is stepped a few ticks, and a picture is drawn from
the eye looking at the target. W, H, Q, FAR set the picture; TICKS (default 6) the steps before each picture. RES="wx_target=2,wx=0.95,rain=1" sets resources of the scene first (a weather); HERO=1 moves the
player to each eye too (the rain and the lamps' reach follow the player, not the camera).
"""
import json
import os
import subprocess
import sys
import time

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
W, H = int(os.environ.get("W", "1600")), int(os.environ.get("H", "900"))
Q = os.environ.get("Q", "high")


def main():
    root, scene, outdir = sys.argv[1], sys.argv[2], sys.argv[3]
    views = []
    for v in sys.argv[4:]:
        name, rest = v.split("=")
        views.append((name, [float(a) for a in rest.split(",")]))
    os.makedirs(os.path.join(root, outdir), exist_ok=True)
    proc = subprocess.Popen([BIN, "serve", "--root", root], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    n = [0]

    def call(name, args=None):
        n[0] += 1
        proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n[0], "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + chr(10))
        proc.stdin.flush()
        while True:
            m = json.loads(proc.stdout.readline())
            if m.get("id") == n[0]:
                break
        res = m.get("result") or {}
        if res.get("isError"):
            print(name, "FAILED", json.dumps(m)[:500])
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {}
        return {}

    t = time.time()
    r = call("load_scene", {"path": scene})
    call("set_resource", {"name": "continue_enabled", "value": 0})
    print("load_scene", json.dumps(r)[:160], f"{time.time() - t:.1f}s")
    q = call("query", {"with": ["Camera"], "select": []})
    rows = q.get("entities") or q.get("results") or q.get("matches") or []
    cam = rows[0].get("uid") if rows and isinstance(rows[0], dict) else (rows[0] if rows else None)
    if cam is None:
        raise SystemExit("no camera: " + json.dumps(q)[:300])
    for kv in filter(None, os.environ.get("RES", "").split(",")):
        k, v = kv.split("=")
        call("set_resource", {"name": k, "value": float(v)})
    hero = None
    if os.environ.get("HERO"):
        hq = call("query", {"with": ["Hero"], "select": []})
        hrows = hq.get("entities") or hq.get("results") or hq.get("matches") or []
        hero = hrows[0].get("uid") if hrows and isinstance(hrows[0], dict) else (hrows[0] if hrows else None)
    for name, nums in views:
        eye, target = nums[0:3], nums[3:6]
        if hero is not None:
            call("set", {"uid": hero, "component": "Transform", "value": {"pos": [eye[0], eye[1] - 1.7, eye[2]]}})
        fov = nums[6] if len(nums) > 6 else 60.0
        call("set", {"uid": cam, "component": "Transform", "value": {"pos": eye}})
        t = time.time()
        call("step", {"ticks": int(os.environ.get("TICKS", "6"))})
        d = call("describe")
        path = os.path.join(outdir, name + ".png")
        t2 = time.time()
        r = call("render", {"quality": Q, "width": W, "height": H, "eye": eye, "target": target, "fov": fov, "far": float(os.environ.get("FAR", "3000")), "near": 0.1, "path": path})
        m = r.get("measured") or {}
        print(name, f"step {time.time() - t2:.1f}s", "entities", d.get("entities"), "triangles", m.get("triangles"), "texture_mb", m.get("texture_mb"), (r.get("warnings") or [])[:int(os.environ.get("NWARN", "2"))], flush=True)
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
