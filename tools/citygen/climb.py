"""Walk the player up the stairs of many buildings of the streamed city (run on the machine with the workspace, here a Linux test machine):

    python3 tools/citygen/climb.py ROOT SCENE STAIRS_FILE

STAIRS_FILE is the output of `citygen stairs`: `kind block_i block_j` and four points (x y z): the foot of the first flight, the landing after it, the landing in front of the second flight and where the second flight comes
out on the floor above. For every building the player is put at the first point, the zones around it stream in, and `w` is held while the player is turned towards each next point; the height reached is compared with the
height of the last point. Prints one line per building and a summary by kind.
"""
import json
import math
import os
import subprocess
import sys

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")


def main():
    root, scene, stairs_file = sys.argv[1], sys.argv[2], sys.argv[3]
    rows = [line.split() for line in open(stairs_file) if line.strip()]
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
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {}
        return {}

    def first(component):
        q = call("query", {"with": [component], "select": []})
        r = q.get("entities") or q.get("results") or q.get("matches") or []
        return r[0].get("uid") if r and isinstance(r[0], dict) else r[0]

    def pos_of(uid):
        r = call("get", {"uid": uid, "component": "Transform"})
        return ((r.get("components") or {}).get("Transform") or {}).get("pos", [0, 0, 0])

    call("load_scene", {"path": scene})
    hero, eyes = first("Hero"), first("MouseLook")
    summary = {}
    for row in rows:
        kind, bi, bj = row[0], row[1], row[2]
        pts = [[float(v) for v in row[3 + 3 * k: 6 + 3 * k]] for k in range(4)]
        call("set", {"uid": hero, "component": "Transform", "value": {"pos": [pts[0][0], pts[0][1] + 0.3, pts[0][2]]}})
        call("input", {"values": {"w": False}})
        call("step", {"ticks": int(os.environ.get("STREAM", "240"))})
        reached = 0
        for k in range(1, 4):
            tx, tz = pts[k][0], pts[k][2]
            for _ in range(80):
                p = pos_of(hero)
                if os.environ.get("DEBUG"):
                    print(f"   target {k} ({tx:.2f}, {tz:.2f}) player at ({p[0]:.2f}, {p[1]:.2f}, {p[2]:.2f})")
                dx, dz = tx - p[0], tz - p[2]
                if math.hypot(dx, dz) < 0.35:
                    reached = k
                    break
                # face the point: forward is (-sin yaw, -cos yaw)
                call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": math.atan2(-dx, -dz)}})
                call("input", {"values": {"w": True}})
                call("step", {"ticks": 6})
            call("input", {"values": {"w": False}})
            if reached < k:
                break
        call("step", {"ticks": 30})
        p = pos_of(hero)
        ok = reached == 3 and abs(p[1] - (pts[3][1] - 0.05)) < 0.5
        summary.setdefault(kind, []).append(ok)
        print(f"{'ok  ' if ok else 'FAIL'} {kind:10s} block ({bi},{bj}) reached point {reached} of 3, at y {p[1]:.2f} (the upper floor is at {pts[3][1] - 0.05:.2f})", flush=True)
    for kind, v in sorted(summary.items()):
        print(f"{kind:10s} {sum(v)}/{len(v)} climbed")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
