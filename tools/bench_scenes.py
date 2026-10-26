"""GPU time per pass of the example scenes (the high qualities, one picture each, the best of several): where does a picture of a real scene spend its time?

    python3 tools/bench_scenes.py [scene.lunaris.json ...]      (on a machine with a GPU; the passes are timed by timestamp queries)
"""
import ast
import json
import os
import subprocess
import sys

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
W, H = int(os.environ.get("W", "1920")), int(os.environ.get("H", "1080"))
Q = os.environ.get("Q", "high")
SCENES = sys.argv[1:] or ["examples/lighting_showcase.lunaris.json", "examples/gfx_lab.lunaris.json", "examples/showcase.lunaris.json", "examples/models.lunaris.json", "examples/caldera/caldera.lunaris.json"]


def run(scene):
    env = dict(os.environ, LUNARIS_GPU_PROFILE="1")
    proc = subprocess.Popen([BIN, "serve", "--root", "."], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1, env=env)
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
            return {"error": json.dumps(m)[:300]}
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {}
        return {}

    r = call("load_scene", {"path": scene})
    if "error" in r:
        proc.stdin.close()
        return None, r["error"]
    call("step", {"ticks": 30})
    best = None
    for _ in range(int(os.environ.get("REPEAT", "5"))):
        r = call("render", {"quality": Q, "width": W, "height": H, "image": False})
        m = r.get("measured") or {}
        if not r.get("renderer", "").startswith("gpu"):
            proc.stdin.close()
            return None, "not drawn by the GPU: " + str(r.get("warnings"))[:200] + str(r.get("error"))
        if "gpu_ms.total" in m and (best is None or m["gpu_ms.total"] < best["gpu_ms.total"]):
            best = m
    proc.stdin.close()
    proc.wait(timeout=60)
    return best, None


for scene in SCENES:
    if not os.path.exists(scene):
        print(scene, "missing")
        continue
    best, err = run(scene)
    if not best:
        print(scene, "FAILED", err)
        continue
    rows = sorted(((k[7:], v) for k, v in best.items() if k.startswith("gpu_ms.") and k != "gpu_ms.total"), key=lambda r: -r[1])
    print(f"{scene}  {W}x{H} {Q}: {best['gpu_ms.total']:.2f} ms ({1000.0 / best['gpu_ms.total']:.0f} fps)  " + ", ".join(f"{k} {v:.2f}" for k, v in rows[:9] if v >= 0.05))
