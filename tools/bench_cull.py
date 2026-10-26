"""How much does culling save? A field of many cubes (a forest of boulders: 60 m apart over 3 km) seen from its south edge, drawn by the GPU renderer in high quality with and without culling.

    python3 tools/bench_cull.py          # on a Linux test machine's GPU

The number printed is what the renderer reports as the picture's time (encode and wait for the card), the median of several pictures; the culled share is what the camera's view volume left out.
"""
import json
import os
import statistics
import subprocess
import sys

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
N = int(os.environ.get("N", "50"))  # cubes per side
Q = os.environ.get("Q", "high")
MODEL = os.environ.get("MODEL", "")  # a model file instead of the cube (a heavy one shows what culling is for)
W, H = int(os.environ.get("W", "1280")), int(os.environ.get("H", "720"))


def run(no_cull):
    env = dict(os.environ)
    if no_cull:
        env["LUNARIS_NO_CULL"] = "1"
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
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {}
        return {}

    call("reset")
    batch = [
        {"op": "spawn", "uid": 1, "components": {"Camera": {"fov_y": 60, "active": True, "far": 4000.0}, "Transform": {"pos": [0, 6, 0]}}},
        {"op": "spawn", "uid": 2, "components": {"Light": {"kind": 0, "intensity": 3.0}, "Transform": {"rot": [-0.5, 0.3, 0, 0.81]}}},
        {"op": "spawn", "uid": 3, "components": {"Environment": {"sky": True, "shadows": True, "shadow_distance": 400, "ssao": 1.0, "tonemap": 1}}},
        {"op": "spawn", "uid": 4, "components": {"Transform": {"pos": [0, 0, 0], "scale": [4000, 1, 4000]}, "MeshRenderer": {"mesh": "plane", "color": [0.4, 0.5, 0.3, 1], "roughness": 0.9}}},
    ]
    uid = 10
    for i in range(N):
        for j in range(N):
            uid += 1
            batch.append({"op": "spawn", "uid": uid, "components": {"Transform": {"pos": [(i - N / 2) * 60.0, 3, -j * 60.0 - 20.0], "scale": [6, 6, 6]}, "MeshRenderer": {"mesh": MODEL or "cube", "color": [0.6, 0.55, 0.5, 1], "roughness": 0.8}}})
    for k in range(0, len(batch), 500):
        call("batch", {"commands": batch[k : k + 500]})
    times = []
    for k in range(7):
        r = call("render", {"quality": Q, "width": W, "height": H, "eye": [0, 6, 0], "target": [0, 6, -300], "fov": 60, "image": False})
        if r.get("renderer", "").startswith("cpu"):
            print("the GPU renderer did not draw:", r.get("warnings"))
            break
        times.append((r.get("timing_ms") or {}).get("total", 0.0))
    proc.stdin.close()
    proc.wait(timeout=60)
    return statistics.median(times[2:]) if len(times) > 3 else None


if __name__ == "__main__":
    full = run(True)
    cut = run(False)
    print(f"{N * N} x {MODEL or 'cube'}, {W}x{H} {Q}: without culling {full} ms, with culling {cut} ms" + (f" ({full / cut:.2f}x)" if full and cut else ""))
    sys.exit(0)
