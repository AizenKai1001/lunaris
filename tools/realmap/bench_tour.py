"""How fast a streamed world draws while the camera flies through it (run on the machine with the GPU, here a Linux test machine):

    LUNARIS_GPU_PROFILE=1 python3 tools/realmap/bench_tour.py ROOT TOUR_SCENE [FRAMES] [TICKS_PER_FRAME]

The tour scene (world.py with TOUR=...) has a camera that follows a spline by itself. Every frame the simulation steps TICKS_PER_FRAME ticks (so the camera moves and the StreamZones load and unload as they would in
play) and the world's own camera draws a 1920 x 1080 picture (W, H, Q set it; quality `high` is what `lunaris play` uses). Prints the wall time of every frame as a short list, the average, the 95th percentile
and the slowest, and the median and 95th percentile of every number the renderer reports (`gpu_ms.*` with LUNARIS_GPU_PROFILE=1, `cpu_ms.*`, triangles, draws).
"""
import json
import os
import subprocess
import sys
import time

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
W, H = int(os.environ.get("W", "1920")), int(os.environ.get("H", "1080"))
Q = os.environ.get("Q", "high")


def main():
    root, scene = sys.argv[1], sys.argv[2]
    frames = int(sys.argv[3]) if len(sys.argv) > 3 else 120
    per = int(sys.argv[4]) if len(sys.argv) > 4 else 15
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
            print(name, "FAILED", json.dumps(m)[:300])
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {}
        return {}

    call("load_scene", {"path": scene})
    # the first pictures load the world round the start: not counted
    for _ in range(int(os.environ.get("WARMUP", "6"))):
        call("step", {"ticks": per})
        call("render", {"quality": Q, "width": W, "height": H})
    times, sums, step_ms = [], {}, []
    for k in range(frames):
        t = time.time()
        call("step", {"ticks": per})
        step_ms.append((time.time() - t) * 1000)
        t = time.time()
        r = call("render", {"quality": Q, "width": W, "height": H})
        times.append((time.time() - t) * 1000)
        for key, v in (r.get("measured") or {}).items():
            if isinstance(v, (int, float)):
                sums.setdefault(key, []).append(v)
    srt = sorted(times)
    print(f"{frames} frames at {W}x{H} {Q}: render wall ms avg {sum(times) / len(times):.1f}, p50 {srt[len(srt) // 2]:.1f}, p95 {srt[int(len(srt) * 0.95)]:.1f}, worst {srt[-1]:.1f}; the step of {per} ticks avg {sum(step_ms) / len(step_ms):.1f} ms, worst {max(step_ms):.1f}")
    print("slowest frames (index: ms):", ", ".join(f"{i}: {t:.0f}" for i, t in sorted(enumerate(times), key=lambda it: -it[1])[:8]))
    print("  (median / 95th percentile over the frames)")
    for key in sorted(sums):
        v = sorted(sums[key])
        print(f"  {key}: {v[len(v) // 2]:.2f} / {v[int(len(v) * 0.95)]:.2f}")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
