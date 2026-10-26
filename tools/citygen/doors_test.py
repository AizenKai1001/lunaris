"""Walk up to a door of the streamed city, look at it, open it with the interact key and walk in (run on the machine with the workspace, here a Linux test machine):

    python3 tools/citygen/doors_test.py ROOT SCENE [COUNT [inside]]

For COUNT doors (default 4) near the start: the player stands 2 m in front of the door looking at it, `interact_uid` must name the door and the prompt kind be 1; walking forward is blocked while it is closed; E opens it
(`Door.want` 1, `Door.open` follows); walking forward then goes through; E again shuts it. Prints ok / FAIL for each step.
"""
import json
import math
import os
import subprocess
import sys

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")


def main():
    root, scene = sys.argv[1], sys.argv[2]
    count = int(sys.argv[3]) if len(sys.argv) > 3 else 4
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

    def rows(component):
        q = call("query", {"with": [component], "select": []})
        r = q.get("entities") or q.get("results") or q.get("matches") or []
        return [x.get("uid") if isinstance(x, dict) else x for x in r]

    def comp(uid, name):
        r = call("get", {"uid": uid, "component": name})
        return (r.get("components") or {}).get(name) or r.get("value") or r.get(name) or {}

    def res(name):
        r = call("resources", {})
        v = r.get("resources", r)
        return v.get(name, 0) if isinstance(v, dict) else 0

    call("load_scene", {"path": scene})

    call("set_resource", {"name": "continue_enabled", "value": 0})
    hero = rows("Hero")[0]
    eyes = rows("MouseLook")[0]
    call("step", {"ticks": 400})
    if len(sys.argv) > 4 and sys.argv[4] == "inside":
        # into the building behind the first street door, so that the doors of its rooms stream in
        first = rows("Door")[0]
        fd = comp(first, "Door")
        ft = comp(first, "Transform")["pos"]
        call("set", {"uid": first, "component": "Door", "value": {"want": 1.0, "open": 1.0}})
        call("set", {"uid": hero, "component": "Transform", "value": {"pos": [ft[0] - fd["ox"] * 4.0, ft[1] - 0.95, ft[2] - fd["oz"] * 4.0]}})
        call("step", {"ticks": 500})
    doors = rows("Door")
    print("doors streamed in:", len(doors))
    hp = comp(hero, "Transform")["pos"]
    scored = []
    for d in doors:
        t = comp(d, "Transform")["pos"]
        scored.append(((t[0] - hp[0]) ** 2 + (t[2] - hp[2]) ** 2, d))
    scored.sort()
    failures = 0
    for _, d in scored[:count]:
        door = comp(d, "Door")
        pos = comp(d, "Transform")["pos"]
        ox, oz = door["ox"], door["oz"]
        base = [pos[0], pos[1], pos[2]]
        # stand 2 m out from the leaf's middle (closed), looking at it
        start = [base[0] + ox * 2.0, base[1] - 0.95, base[2] + oz * 2.0]
        call("set", {"uid": d, "component": "Door", "value": {"want": 0.0, "open": 0.0}})
        call("set", {"uid": hero, "component": "Transform", "value": {"pos": start}})
        call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": math.atan2(ox, oz), "pitch": 0.0}})
        call("input", {"values": {"w": False, "e": False}})
        call("step", {"ticks": 40})
        target = res("interact_uid")
        kind = res("interact_kind")
        looked = target == d and kind == 1
        # blocked while closed
        call("input", {"values": {"w": True}})
        call("step", {"ticks": 90})
        call("input", {"values": {"w": False}})
        call("step", {"ticks": 10})
        p = comp(hero, "Transform")["pos"]
        depth_closed = -((p[0] - base[0]) * ox + (p[2] - base[2]) * oz)
        blocked = depth_closed < 0.4
        # open it
        call("set", {"uid": hero, "component": "Transform", "value": {"pos": start}})
        call("step", {"ticks": 20})
        call("input", {"values": {"e": True}})
        call("step", {"ticks": 3})
        call("input", {"values": {"e": False}})
        call("step", {"ticks": 60})
        o = comp(d, "Door")
        opened = o["want"] > 0.5 and o["open"] > 0.95
        call("input", {"values": {"w": True}})
        call("step", {"ticks": 150})
        call("input", {"values": {"w": False}})
        call("step", {"ticks": 10})
        p = comp(hero, "Transform")["pos"]
        depth_open = -((p[0] - base[0]) * ox + (p[2] - base[2]) * oz)
        entered = depth_open > 1.2
        # shut it again
        call("set", {"uid": hero, "component": "Transform", "value": {"pos": [base[0] + ox * 2.0, base[1] - 0.95, base[2] + oz * 2.0]}})
        call("step", {"ticks": 20})
        call("input", {"values": {"e": True}})
        call("step", {"ticks": 3})
        call("input", {"values": {"e": False}})
        call("step", {"ticks": 60})
        c = comp(d, "Door")
        closed = c["want"] < 0.5 and c["open"] < 0.05
        ok = looked and blocked and opened and entered and closed
        failures += 0 if ok else 1
        print(f"{'ok  ' if ok else 'FAIL'} door at ({base[0]:.0f}, {base[2]:.0f}): looked {looked} (target {target} kind {kind}), blocked closed {blocked} (depth {depth_closed:.2f}), opened {opened}, entered {entered} (depth {depth_open:.2f}), shut again {closed}", flush=True)
    print("failures:", failures)
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
