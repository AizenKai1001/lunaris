"""Fly the camera of a streamed city scene along a line, drawing small pictures as it goes, and watch the memory, the entities and the time (run on the machine with the GPU, here a Linux test machine):

    python3 tools/citygen/fly.py ROOT SCENE [HOPS] [HOP_METRES] [HEIGHT] [DIRX,DIRZ]

Every hop moves the camera, steps 90 ticks (the zones around it load and unload) and draws a 320 x 180 picture (so that the renderer loads what is in view). Prints the resident memory of the engine, the entities,
the triangles drawn and the seconds the hop took. A picture every PICEVERY hops (default 0 = none) is kept as fly_<n>.png.
"""
import json
import os
import subprocess
import sys
import time

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")


def rss_mb(pid):
    try:
        for line in open(f"/proc/{pid}/status"):
            if line.startswith("VmRSS"):
                return int(line.split()[1]) // 1024
    except OSError:
        pass
    return -1


def main():
    root, scene = sys.argv[1], sys.argv[2]
    hops = int(sys.argv[3]) if len(sys.argv) > 3 else 30
    hop = float(sys.argv[4]) if len(sys.argv) > 4 else 150.0
    height = float(sys.argv[5]) if len(sys.argv) > 5 else 40.0
    d = [float(v) for v in sys.argv[6].split(",")] if len(sys.argv) > 6 else [1.0, 0.0]
    norm = (d[0] ** 2 + d[1] ** 2) ** 0.5
    d = [d[0] / norm, d[1] / norm]
    pic_every = int(os.environ.get("PICEVERY", "0"))
    q = os.environ.get("Q", "low")
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
            print(name, "FAILED", json.dumps(m)[:400])
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {}
        return {}

    call("load_scene", {"path": scene})
    qy = call("query", {"with": ["Camera"], "select": []})
    rows = qy.get("entities") or qy.get("results") or qy.get("matches") or []
    cam = rows[0].get("uid") if rows and isinstance(rows[0], dict) else rows[0]
    print(f"engine pid {proc.pid}, rss {rss_mb(proc.pid)} MB", flush=True)
    x, z = 0.0, 0.0
    peak = 0
    for k in range(hops):
        t = time.time()
        x += d[0] * hop
        z += d[1] * hop
        call("set", {"uid": cam, "component": "Transform", "value": {"pos": [x, height, z]}})
        call("step", {"ticks": 90})
        path = f"fly_{k}.png" if pic_every and k % pic_every == 0 else "fly_tmp.png"
        w, h = (1280, 720) if path != "fly_tmp.png" else (320, 180)
        r = {} if os.environ.get("NORENDER") else call("render", {"quality": q if path == "fly_tmp.png" else "high", "width": w, "height": h, "eye": [x, height, z], "target": [x + d[0] * 300, height * 0.5, z + d[1] * 300], "far": 9000.0, "path": path})
        m = r.get("measured") or {}
        dsc = call("describe")
        rss = rss_mb(proc.pid)
        peak = max(peak, rss)
        print(f"hop {k:3d} at ({x:7.0f},{z:7.0f})  rss {rss:5d} MB  entities {dsc.get('entities')}  triangles {m.get('triangles')}  texture_mb {m.get('texture_mb')}  {time.time() - t:5.1f} s", flush=True)
    print(f"peak rss {peak} MB")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
