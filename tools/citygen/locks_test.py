"""Locked doors: some street doors are locked; without a hatchet or a crowbar E only rattles it and the prompt says so, with one the prompt offers to break the lock and E breaks it and the door swings open (run on the
machine with the workspace):

    python3 tools/citygen/locks_test.py ROOT SCENE
"""
import json
import math
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
            print(name, "FAILED", json.dumps(m)[:400])
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {"text": c["text"]}
        return {}

    def rows(component):
        q = call("query", {"with": [component], "select": []})
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
        call("step", {"ticks": 10})

    call("load_scene", {"path": scene})
    call("set_resource", {"name": "continue_enabled", "value": 0})
    hero, eyes = rows("Hero")[0], rows("MouseLook")[0]
    call("step", {"ticks": 240})
    doors = rows("Door")
    locked = [d for d in doors if comp(d, "Door").get("locked", 0) > 0.5]
    check("some street doors are locked, not all", 0 < len(locked) < len(doors), f"{len(locked)} of {len(doors)}")
    door = locked[0]
    dd = comp(door, "Door")
    dp = comp(door, "Transform")["pos"]

    def stand_before():
        ox, oz = dd["ox"], dd["oz"]
        call("set", {"uid": hero, "component": "Transform", "value": {"pos": [dp[0] + ox * 1.7, 0.2, dp[2] + oz * 1.7]}})
        call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": math.atan2(ox, oz), "pitch": 0.0}})
        call("step", {"ticks": 25})

    # no tool: the prompt says locked, E rattles it
    for i in [i for i in rows("Item") if comp(i, "Ui").get("text") in ("Hatchet", "Crowbar")]:
        call("despawn", {"uid": i})
    call("step", {"ticks": 6})
    stand_before()
    check("a locked door looked at says locked", res("interact_uid") == door and res("interact_kind") == 8, f"target {res('interact_uid')} (door {door}) kind {res('interact_kind')}")
    tap("e")
    check("E does not open it", comp(door, "Door").get("locked") == 1 and comp(door, "Door").get("want") == 0, str(comp(door, "Door").get("want")))
    # with the hatchet: the prompt offers to break the lock, E does it and the door opens
    slots = [s for s in rows("UiDrop") if comp(s, "UiDrop").get("name", "").startswith("p")]
    taken = {comp(i, "UiDrag").get("slot") for i in rows("Item")}
    free = [s for s in slots if s not in taken]
    call("spawn_prefab", {"prefab": "Item_6", "overrides": {"UiDrag": {"slot": free[0]}}})
    call("step", {"ticks": 10})
    check("with a hatchet the prompt offers to break the lock", res("interact_kind") == 9, f"kind {res('interact_kind')}")
    tap("e")
    call("step", {"ticks": 40})
    d2 = comp(door, "Door")
    check("E breaks the lock and the door swings open", d2.get("locked") == 0 and d2.get("want") == 1 and d2.get("open", 0) > 0.8, f"locked {d2.get('locked')}, want {d2.get('want')}, open {d2.get('open')}")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
