"""Fly a camera through a streamed world (world.py) and watch the cells come and go (run on the machine with the GPU, here a Linux test machine):

    python3 tools/realmap/stream_test.py ROOT SCENE OUTDIR  STOP STOP ...      a stop is x,z   or   x,z,render   or   x,z,render,EYE_ABOVE_GROUND,TX,TY,TZ (look at that world point)

Each stop sets the camera entity's position (the StreamZones follow the active camera), steps the simulation 2 ticks, reports how many entities exist and how long the step took, and with
`render` draws the picture from 2 m above the ground there looking along the path (the first picture of a stop pays for loading what the stop made appear). W, H, Q as for render.py.
"""
import json
import os
import subprocess
import sys
import time

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
W, H = int(os.environ.get("W", "1280")), int(os.environ.get("H", "720"))
Q = os.environ.get("Q", "ultra")


def main():
    root, scene, outdir = sys.argv[1], sys.argv[2], sys.argv[3]
    stops = []
    for s in sys.argv[4:]:
        p = s.split(",")
        stops.append((float(p[0]), float(p[1]), len(p) > 2 and p[2] == "render", [float(v) for v in p[3:]]))
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
            print(name, "FAILED", json.dumps(m)[:400])
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {}
        return {}

    t = time.time()
    r = call("load_scene", {"path": scene})
    print("load_scene", json.dumps(r)[:200], f"{time.time() - t:.2f}s")
    cam = None
    q = call("query", {"with": ["Camera"], "select": []})
    rows = q.get("entities") or q.get("results") or q.get("matches") or []
    if rows:
        cam = rows[0].get("uid") if isinstance(rows[0], dict) else rows[0]
    if cam is None:
        raise SystemExit("no camera found: " + json.dumps(q)[:300])
    print("describe keys:", list(call("describe").keys()))
    prev = None
    for k, (x, z, draw, extra) in enumerate(stops):
        call("set", {"uid": cam, "component": "Transform", "value": {"pos": [x, 80, z]}})
        t = time.time()
        call("step", {"ticks": 2})
        dt = time.time() - t
        d = call("describe")
        count = d.get("entity_count") or d.get("entities") or d.get("count")
        cells = call("query", {"with": ["Terrain"], "count_only": True})
        trees = call("query", {"with": ["MeshRenderer"], "count_only": True})
        rss = ""
        try:
            for ln in open(f"/proc/{proc.pid}/status"):
                if ln.startswith("VmRSS"):
                    rss = f", RSS {int(ln.split()[1]) // 1024} MB"
        except OSError:
            pass
        line = f"stop {k}: camera ({x:.0f}, {z:.0f}) step {dt * 1000:.0f} ms{rss}, entities {count}, ground cells {cells.get('total', cells.get('count'))}, meshes {trees.get('total', trees.get('count'))}"
        if draw:
            h = call("terrain_height", {"x": x, "z": z}).get("height")
            eye_y = (h + (extra[0] if extra else 2.0)) if h is not None else 60.0
            tx, tz = (x - prev[0], z - prev[1]) if prev else (1.0, 0.0)
            norm = max((tx * tx + tz * tz) ** 0.5, 1e-6)
            target = [x + tx / norm * 60, eye_y + 1.0, z + tz / norm * 60]
            if len(extra) >= 4:
                target = extra[1:4]
            line += f", eye y {eye_y:.0f} (ground {h}), target {[round(v) for v in target]}"
            for probe in os.environ.get("PROBES", "").split(";"):
                if probe:
                    px, pz = (float(v) for v in probe.split(","))
                    line += f", ground at {px:.0f},{pz:.0f} = {call('terrain_height', {'x': px, 'z': pz}).get('height')}"
            path = os.path.join(outdir, f"stop{k}.png")
            t = time.time()
            res = call("render", {"quality": Q, "width": W, "height": H, "eye": [x, eye_y, z], "target": target, "fov": float(os.environ.get("FOV", "62")), "far": float(os.environ.get("FAR", "20000")), "near": float(os.environ.get("NEAR", "0.3")), "path": path})
            m = res.get("measured") or {}
            line += f", picture {time.time() - t:.1f} s, texture_mb {m.get('texture_mb')}, triangles {m.get('triangles')}"
            try:
                for ln in open(f"/proc/{proc.pid}/status"):
                    if ln.startswith("VmRSS"):
                        line += f", RSS after {int(ln.split()[1]) // 1024} MB"
            except OSError:
                pass
        frames = []
        for _ in range(int(os.environ.get("EXTRA_FRAMES", "0"))):
            t = time.time()
            call("render", {"quality": "low", "width": 64, "height": 36, "eye": [x, 60, z], "target": [x + 1, 60, z], "fov": 60})
            frames.append(int((time.time() - t) * 1000))
        if frames:
            line += f", frames ms {frames}"
        print(line, flush=True)
        # time passes between stops as it does in a game (STOP_SECONDS), a small picture every tenth of a second: the loader's threads work meanwhile
        waited = 0.0
        while waited < float(os.environ.get("STOP_SECONDS", "0")):
            time.sleep(0.1)
            waited += 0.1
            t = time.time()
            call("render", {"quality": "low", "width": 64, "height": 36, "eye": [x, 60, z], "target": [x + 1, 60, z], "fov": 60})
            waited += time.time() - t
            frames.append(int((time.time() - t) * 1000))
        if waited:
            print(f"   between stops: worst picture {max(frames)} ms", flush=True)
        prev = (x, z)
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
