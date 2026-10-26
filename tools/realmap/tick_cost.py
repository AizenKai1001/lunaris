"""What one simulation tick of a streamed world costs when nothing is loading (run on any machine; no GPU needed):

    python3 tools/realmap/tick_cost.py ROOT SCENE [WARM_TICKS] [MEASURE_TICKS]

Loads the scene, steps WARM_TICKS ticks (the zones around the camera load, a few a tick), then times MEASURE_TICKS ticks one `step` call each and in one call, and prints the entity count.
The camera does not move, so what is measured is the cost of the zones that are there (the pass over all of them every tick) plus everything else that runs every tick.
"""
import json
import os
import subprocess
import sys
import time

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")


def main():
    root, scene = sys.argv[1], sys.argv[2]
    warm = int(sys.argv[3]) if len(sys.argv) > 3 else 400
    measure = int(sys.argv[4]) if len(sys.argv) > 4 else 200
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
        for c in (m.get("result") or {}).get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {}
        return {}

    r = call("load_scene", {"path": scene})
    print("load_scene", r.get("entities"), "entities")
    t = time.time()
    call("step", {"ticks": warm})
    print(f"warm-up {warm} ticks: {(time.time() - t) * 1000:.0f} ms")
    q = call("query", {"components": ["StreamZone"], "limit": 1})
    print("after warm-up:", json.dumps(call("describe") if False else q)[:200])
    t = time.time()
    for _ in range(measure):
        call("step", {"ticks": 1})
    one = (time.time() - t) * 1000 / measure
    t = time.time()
    call("step", {"ticks": measure})
    many = (time.time() - t) * 1000 / measure
    print(f"per tick: {one:.2f} ms (a call each, with the call's own cost), {many:.2f} ms (one call of {measure})")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
