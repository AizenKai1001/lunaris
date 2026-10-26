"""Walk the player into the front door of many buildings of the streamed city and report which can be entered (run on the machine with the workspace, here a Linux test machine):

    python3 tools/citygen/enter.py ROOT SCENE DOORS_FILE [SECONDS]

DOORS_FILE is the output of `citygen doors` (`kind block_i block_j x y z out_x out_z width` a line). For every door the player is put 5 m in front of it, facing it, the zones around it stream in, `w` is held for SECONDS
(default 3) and the depth reached behind the door's plane is printed: more than 2 m means it went in. Prints a summary by kind.
"""
import json
import math
import os
import subprocess
import sys

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")


def main():
    root, scene, doors_file = sys.argv[1], sys.argv[2], sys.argv[3]
    seconds = float(sys.argv[4]) if len(sys.argv) > 4 else 3.0
    doors = [line.split() for line in open(doors_file) if line.strip()]
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

    def first(component):
        q = call("query", {"with": [component], "select": []})
        rows = q.get("entities") or q.get("results") or q.get("matches") or []
        return rows[0].get("uid") if rows and isinstance(rows[0], dict) else rows[0]

    def pos_of(uid, default):
        r = call("get", {"uid": uid, "component": "Transform"})
        t = (r.get("components") or {}).get("Transform") or r.get("value") or r.get("Transform") or {}
        return t.get("pos", default)

    call("load_scene", {"path": scene})
    hero, eyes = first("Hero"), first("MouseLook")
    summary = {}
    for d in doors:
        kind, bi, bj = d[0], d[1], d[2]
        x, y, z, ox, oz, w = [float(v) for v in d[3:9]]
        start = [x + ox * 5.0, y + 0.1, z + oz * 5.0]
        call("set", {"uid": hero, "component": "Transform", "value": {"pos": start}})
        call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": math.atan2(ox, oz)}})
        call("input", {"values": {"w": False}})
        call("step", {"ticks": int(os.environ.get("STREAM", "240"))})
        p0 = pos_of(hero, start)
        call("input", {"values": {"w": True}})
        call("step", {"ticks": int(seconds * 60)})
        call("input", {"values": {"w": False}})
        call("step", {"ticks": 10})
        p = pos_of(hero, start)
        depth = -((p[0] - x) * ox + (p[2] - z) * oz)
        ok = depth > 2.0 and -0.5 < p[1] - y < 1.0
        summary.setdefault(kind, []).append(ok)
        print(f"{'ok  ' if ok else 'FAIL'} {kind:10s} block ({bi},{bj}) door width {w:.1f}: stood at y {p0[1]:.2f}, walked to depth {depth:5.1f} m, y {p[1]:.2f}", flush=True)
    for kind, v in sorted(summary.items()):
        print(f"{kind:10s} {sum(v)}/{len(v)} entered")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
