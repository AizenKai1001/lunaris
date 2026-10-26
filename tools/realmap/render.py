"""Pictures of a real-place scene with the Lunaris GPU renderer (run on a machine with a GPU, here a Linux test machine):

    python3 tools/realmap/render.py ROOT SCENE OUTDIR [view ...]      a view is NAME=ex,ey,ez,tx,ty,tz[,fov]  (metres in the scene's frame: x east, z south, y up)

ROOT is the workspace folder (the scene's and the models' paths are relative to it), SCENE the scene file inside it. The camera heights are absolute above the ground's lowest point.
"""
import json
import os
import subprocess
import sys
import time

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
W, H = int(os.environ.get("W", "1920")), int(os.environ.get("H", "1080"))
Q = os.environ.get("Q", "ultra")


def main():
    root, scene, outdir = sys.argv[1], sys.argv[2], sys.argv[3]
    views = []
    for v in sys.argv[4:]:
        name, rest = v.split("=")
        nums = [float(a) for a in rest.split(",")]
        views.append((name, nums))
    os.makedirs(outdir, exist_ok=True)
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
            print(name, "FAILED", json.dumps(m)[:600])
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {}
        return {}

    t = time.time()
    r = call("load_scene", {"path": scene})
    print("load_scene", json.dumps(r)[:300], f"{time.time() - t:.1f}s")
    call("step", {"ticks": 3})
    for name, nums in views:
        eye, target = nums[0:3], nums[3:6]
        # a negative height is metres above the ground there
        for pt in (eye, target):
            if pt[1] < 0:
                g = call("terrain_height", {"x": pt[0], "z": pt[2]})
                pt[1] = float(g.get("height", 0.0)) - pt[1]
        fov = nums[6] if len(nums) > 6 else 55.0
        t = time.time()
        path = os.path.join(outdir, name + ".png")
        r = call("render", {"quality": Q, "width": W, "height": H, "eye": eye, "target": target, "fov": fov, "far": float(os.environ.get("FAR", "20000")), "near": float(os.environ.get("NEAR", "0.3")), "path": path})
        m = r.get("measured") or {}
        print(name, r.get("renderer"), (r.get("warnings") or [])[:3], f"{(time.time() - t):.1f}s", "texture_mb", m.get("texture_mb"), "triangles", m.get("triangles"))
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
