"""Walk the player of a streamed city scene forward (keys held), and report where it gets to and how long the ticks take (run on the machine that has the workspace, here a Linux test machine):

    python3 tools/citygen/walk.py ROOT SCENE [SECONDS] [KEY]

The scene's `Hero` entity is the player. It streams the city in for 120 ticks first, then holds KEY (default `w`) for SECONDS (default 8) of simulated time (60 ticks a second) and prints the position every second.
A picture from the player's eyes at the end goes to ROOT/walk_end.png (set PIC=0 for none).
"""
import json
import os
import subprocess
import sys
import time

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")


def main():
    root, scene = sys.argv[1], sys.argv[2]
    seconds = float(sys.argv[3]) if len(sys.argv) > 3 else 8.0
    key = sys.argv[4] if len(sys.argv) > 4 else "w"
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

    call("load_scene", {"path": scene})
    q = call("query", {"with": ["Hero"], "select": []})
    rows = q.get("entities") or q.get("results") or q.get("matches") or []
    hero = rows[0].get("uid") if rows and isinstance(rows[0], dict) else rows[0]

    def pos():
        r = call("get", {"uid": hero, "component": "Transform"})
        v = r.get("value") or r.get("Transform") or r.get("components", {}).get("Transform") or r
        if not v.get("pos") and os.environ.get("DEBUG"):
            print("get gave", json.dumps(r)[:300])
        return [round(x, 2) for x in v.get("pos", [])]

    t = time.time()
    call("step", {"ticks": 120})
    print(f"streamed in 120 ticks: {time.time() - t:.1f} s, player at {pos()}", flush=True)
    call("input", {"values": {key: True}})
    worst = 0.0
    for s in range(int(seconds)):
        t = time.time()
        call("step", {"ticks": 60})
        dt = time.time() - t
        worst = max(worst, dt)
        d = call("describe")
        print(f"t={s + 1:2d}s player {pos()}  entities {d.get('entities')}  60 ticks took {dt * 1000:.0f} ms", flush=True)
    call("input", {"values": {key: False}})
    call("step", {"ticks": 30})
    print("stopped at", pos(), f"worst second {worst * 1000:.0f} ms", flush=True)
    if os.environ.get("PIC", "1") != "0":
        p = pos()
        r = call("render", {"quality": os.environ.get("Q", "high"), "width": 1600, "height": 900, "far": 9000.0, "path": "walk_end.png"})
        print("render", json.dumps(r.get("measured") or {})[:200], (r.get("warnings") or [])[:3])
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
