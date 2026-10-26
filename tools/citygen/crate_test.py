"""The storage crate: C puts one down in front of the player for 8 wood (not with less), it can be looked at and opened with E like any furniture, and its things have no place on the map to be swept from (run on the
machine with the workspace):

    python3 tools/citygen/crate_test.py ROOT SCENE
"""
import json
import subprocess
import sys

BIN = "target/release/lunaris"


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
        if res.get("isError"):
            print(name, "FAILED", json.dumps(m)[:300])
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {"text": c["text"]}
        return {}

    def rows(component):
        q = call("query", {"with": [component], "select": [], "limit": 2000})
        r = q.get("entities") or q.get("results") or q.get("matches") or []
        return [x.get("uid") if isinstance(x, dict) else x for x in r]

    def comp(uid, name):
        r = call("get", {"uid": uid, "component": name})
        return (r.get("components") or {}).get(name) or {}

    def res(name):
        return call("resources", {}).get("resources", {}).get(name)

    def check(label, ok, detail=""):
        print(("ok   " if ok else "FAIL ") + label + (f"  {detail}" if detail else ""), flush=True)

    def tap(key):
        call("input", {"values": {key: True}})
        call("step", {"ticks": 3})
        call("input", {"values": {key: False}})
        call("step", {"ticks": 8})

    def crates():
        return [u for u in rows("Container") if comp(u, "Container").get("kind") == 8.0]

    call("load_scene", {"path": scene})
    call("set_resource", {"name": "continue_enabled", "value": 0})
    hero, eyes = rows("Hero")[0], rows("MouseLook")[0]
    call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": 0.0, "pitch": -0.72}})
    call("step", {"ticks": 120})
    call("set_resource", {"name": "mat_wood", "value": 5})
    tap("c")
    check("with too little wood no crate is made", len(crates()) == 0 and res("mat_wood") == 5, f"{len(crates())} crates, wood {res('mat_wood')}")
    call("set_resource", {"name": "mat_wood", "value": 20})
    tap("c")
    made = crates()
    check("C makes a crate for 8 wood", len(made) == 1 and res("mat_wood") == 12, f"{len(made)} crates, wood {res('mat_wood')}")
    call("step", {"ticks": 20})
    check("looking at it makes it the target", bool(made) and res("interact_uid") == made[0] and res("interact_kind") == 2, f"target {res('interact_uid')} kind {res('interact_kind')}")
    tap("e")
    check("E opens it beside the backpack, with nothing to sweep", bool(made) and res("inv_open") == made[0] and res("inv_player") == 1 and res("inv_x") == 0 and res("inv_z") == 0, f"inv_open {res('inv_open')}, inv_x {res('inv_x')}")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
