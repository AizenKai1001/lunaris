"""A dense forest seen from the ground: where do the CPU and the GPU spend a picture's time when thousands of real glTF trees (several materials, normal maps, cut-out leaves) are in the scene?

    python3 tools/bench_forest.py                       (a Linux test machine's GPU; N=trees, AREA=metres on a side, W/H, Q, CAMERA=ground|high)

Prints the best of several pictures: the GPU passes by time (timestamp queries), the CPU phases, the triangles drawn and the texture memory.
"""
import json
import os
import subprocess

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
N = int(os.environ.get("N", "3000"))
AREA = float(os.environ.get("AREA", "400"))
W, H = int(os.environ.get("W", "1920")), int(os.environ.get("H", "1080"))
Q = os.environ.get("Q", "high")
CAMERA = os.environ.get("CAMERA", "ground")
M = "examples/gfx_lab/models"
MODELS = [f"{M}/pine_sapling_small/pine_sapling_small_1k.gltf", f"{M}/fir_sapling/fir_sapling_1k.gltf"]

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
        print(name, "FAILED", json.dumps(m)[:300])
    for c in res.get("content") or []:
        if c.get("type") == "text":
            try:
                return json.loads(c["text"])
            except ValueError:
                return {}
    return {}


call("reset")
batch = [
    {"op": "spawn", "uid": 1, "components": {"Camera": {"fov_y": 60, "active": True, "far": 3000.0}, "Transform": {"pos": [0, 1.7, 0]}}},
    {"op": "spawn", "uid": 2, "components": {"Light": {"kind": 0, "intensity": 3.4, "color": [1.0, 0.92, 0.8]}, "Transform": {"rot": [-0.5, 0.3, 0, 0.81]}}},
    {"op": "spawn", "uid": 3, "components": {"Environment": {"sky": True, "clouds": 0.3, "shadows": True, "shadow_distance": 150, "ssao": 1.0, "bloom": 0.1, "tonemap": 1, "ambient": 1.0}}},
    {"op": "spawn", "uid": 4, "components": {"Atmosphere": {"turbidity": 2.2, "intensity": 1.0}}},
    {"op": "spawn", "uid": 5, "components": {"Transform": {"pos": [0, 0, 0], "scale": [AREA * 3, 1, AREA * 3]}, "MeshRenderer": {"mesh": "plane", "color": [0.28, 0.34, 0.18, 1], "roughness": 0.95}}},
]
state = 12345
def rnd():
    global state
    state = (state * 1103515245 + 12345) & 0x7FFFFFFF
    return state / 0x7FFFFFFF
for i in range(N):
    x, z = (rnd() - 0.5) * AREA, -rnd() * AREA - 4.0
    s = 4.0 + rnd() * 4.0
    yaw = rnd() * 6.28318
    batch.append({"op": "spawn", "uid": 100 + i, "components": {"Transform": {"pos": [x, 0, z], "rot": [0, __import__("math").sin(yaw / 2), 0, __import__("math").cos(yaw / 2)], "scale": [s, s, s]}, "MeshRenderer": {"mesh": MODELS[i % 2], "roughness": 0.8}}})
for k in range(0, len(batch), 500):
    call("batch", {"commands": batch[k : k + 500]})
eye = [0, 1.7, 0] if CAMERA == "ground" else [0, 60, 40]
target = [0, 4.0, -150] if CAMERA == "ground" else [0, 0, -150]
best = None
for k in range(int(os.environ.get("REPEAT", "6"))):
    r = call("render", {"quality": Q, "width": W, "height": H, "eye": eye, "target": target, "fov": 60, "image": False})
    m = r.get("measured") or {}
    if not r.get("renderer", "").startswith("gpu"):
        print("not drawn by the GPU:", r.get("warnings"))
        break
    if k >= 2 and "gpu_ms.total" in m and (best is None or m["gpu_ms.total"] < best["gpu_ms.total"]):
        best = m
proc.stdin.close()
proc.wait(timeout=60)
if best:
    gpu = sorted(((k[7:], v) for k, v in best.items() if k.startswith("gpu_ms.") and k != "gpu_ms.total"), key=lambda r: -r[1])
    cpu = sorted(((k[7:], v) for k, v in best.items() if k.startswith("cpu_ms.")), key=lambda r: -r[1])
    print(f"{N} trees on {AREA:.0f} m, {W}x{H} {Q}, camera {CAMERA}: GPU {best['gpu_ms.total']:.2f} ms ({1000.0 / best['gpu_ms.total']:.0f} fps)")
    print("  gpu: " + ", ".join(f"{k} {v:.2f}" for k, v in gpu[:14] if v >= 0.05))
    print("  cpu: " + ", ".join(f"{k} {v:.2f}" for k, v in cpu if v >= 0.05))
    print(f"  texture_mb {best.get('texture_mb')}, triangles in the main view {best.get('triangles', 0) / 1e6:.2f} M, objects culled {best.get('objects_culled')}, drawn at levels " + "/".join(str(int(best.get(f'lod{i}', 0))) for i in range(7)))
