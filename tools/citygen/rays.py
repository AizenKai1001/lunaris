"""Cast rays in a streamed city scene and print what they hit (to find what stops a walker), on the machine that has the workspace:

    python3 tools/citygen/rays.py ROOT SCENE "ox,oy,oz>dx,dy,dz" ["..." ...]

The scene is loaded and stepped 150 ticks first so that the zones around its camera stream in. Rays are cast at colliders only.
"""
import json
import os
import subprocess
import sys

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")


def main():
    root, scene = sys.argv[1], sys.argv[2]
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

    call("load_scene", {"path": scene})
    if os.environ.get("HERO_TO"):
        # stand the player where the rays are, so that the city streams in there (HERO_TO=x,y,z)
        q = call("query", {"with": ["Hero"], "select": []})
        rows = q.get("entities") or q.get("results") or q.get("matches") or []
        hero = rows[0].get("uid") if rows and isinstance(rows[0], dict) else rows[0]
        call("set", {"uid": hero, "component": "Transform", "value": {"pos": [float(v) for v in os.environ["HERO_TO"].split(",")]}})
    call("step", {"ticks": int(os.environ.get("TICKS", "150"))})
    for spec in sys.argv[3:]:
        o, d = spec.split(">")
        origin = [float(v) for v in o.split(",")]
        direction = [float(v) for v in d.split(",")]
        r = call("raycast", {"origin": origin, "direction": direction, "max_distance": float(os.environ.get("MAXD", "30")), "include": "colliders"})
        if r.get("hit"):
            q = call("get", {"uid": r["uid"], "component": "Name"})
            print(spec, "-> hit", round(r["distance"], 2), "m at", [round(x, 2) for x in r["point"]], "normal", [round(x, 2) for x in r["normal"]], "uid", r["uid"], "name", r.get("name") or q)
        else:
            print(spec, "-> nothing")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
