"""Putting things down and picking them up: a thing dragged from the backpack onto the drop box lies on the ground in front of the player, looked at it is the target (prompt kind 3), E puts it back in the backpack
(run on the machine with the workspace):

    python3 tools/citygen/loot_test.py ROOT SCENE
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
        call("step", {"ticks": 6})

    call("load_scene", {"path": scene})

    call("set_resource", {"name": "continue_enabled", "value": 0})
    hero, eyes = rows("Hero")[0], rows("MouseLook")[0]
    call("step", {"ticks": 200})
    start = comp(hero, "Transform")["pos"]
    call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": 0.0, "pitch": -0.85}})
    call("step", {"ticks": 20})
    tap("tab")
    mine = [i for i in rows("Item") if comp(i, "Item").get("owner") == 0]
    texts = {i: comp(i, "Ui").get("text") for i in mine}
    hatchet = [i for i, t in texts.items() if t == "Hatchet"]
    check("the backpack holds a hatchet", bool(hatchet), str(sorted(texts.values())))
    bin_box = [p for p in rows("Panel") if comp(p, "Panel").get("kind") == 3.0]
    check("the drop box is there", bool(bin_box))
    ui, tp = comp(hatchet[0], "Ui"), comp(bin_box[0], "Ui")
    fx, fy = ui["pos"][0] + ui["size"][0] / 2, ui["pos"][1] + ui["size"][1] / 2
    gx, gy = tp["pos"][0] + tp["size"][0] / 2, tp["pos"][1] + tp["size"][1] / 2
    call("input", {"values": {"mouse_x": fx, "mouse_y": fy}})
    call("step", {"ticks": 2})
    call("input", {"values": {"mouse_left": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"mouse_x": (fx + gx) / 2, "mouse_y": (fy + gy) / 2}})
    call("step", {"ticks": 3})
    call("input", {"values": {"mouse_x": gx, "mouse_y": gy}})
    call("step", {"ticks": 3})
    call("input", {"values": {"mouse_left": False}})
    call("step", {"ticks": 8})
    loot = rows("Loot")
    gone = call("get", {"uid": hatchet[0]}).get("code") is not None or not comp(hatchet[0], "Item")
    check("dragged to the drop box it leaves the backpack and lies on the ground", len(loot) == 1 and gone, f"{len(loot)} on the ground, item gone {gone}")
    if loot:
        lp = comp(loot[0], "Transform")["pos"]
        d = math.hypot(lp[0] - start[0], lp[2] - start[2])
        check("it lies a step in front of the player", 0.6 < d < 2.2 and lp[1] < 1.0, f"at {[round(v, 2) for v in lp]}, {d:.2f} m from the player")
    tap("tab")
    call("step", {"ticks": 20})
    check("looking at it makes it the target (prompt 3)", loot and res("interact_uid") == loot[0] and res("interact_kind") == 3, f"target {res('interact_uid')} kind {res('interact_kind')} (loot {loot})")
    tap("e")
    call("step", {"ticks": 8})
    mine = [i for i in rows("Item") if comp(i, "Item").get("owner") == 0]
    names = sorted(comp(i, "Ui").get("text") for i in mine)
    check("E picks it up: gone from the ground, back in the backpack", len(rows("Loot")) == 0 and "Hatchet" in names, f"{len(rows('Loot'))} on the ground, backpack {names}")
    # a full backpack leaves it where it lies: fill the free slots with things, then drop one and try again
    slots = [s for s in rows("UiDrop") if comp(s, "UiDrop").get("name", "").startswith("p")]
    taken = {comp(i, "UiDrag").get("slot") for i in rows("Item")}
    free = [s for s in slots if s not in taken]
    for s in free:
        call("spawn_prefab", {"prefab": "Item_8", "overrides": {"UiDrag": {"slot": s}}})
    call("step", {"ticks": 4})
    call("spawn_prefab", {"prefab": "Loot_3", "at": [start[0], start[1] + 0.13, start[2] - 1.3]})
    call("step", {"ticks": 20})
    loot = rows("Loot")
    aimed = res("interact_uid") == (loot[0] if loot else -1)
    tap("e")
    call("step", {"ticks": 12})
    toast = [comp(u, "Text").get("color") for u in rows("Toast") if comp(u, "Toast").get("kind") == 4.0]
    check("a full backpack leaves it on the ground and says so", aimed and len(rows("Loot")) == len(loot) and len(loot) >= 1 and res("pickup_item") == 0 and res("full_tick") > 0 and bool(toast) and toast[0][3] > 0.5, f"aimed {aimed}, {len(rows('Loot'))} on the ground, pickup_item {res('pickup_item')}, full_tick {res('full_tick')}, toast {toast}")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
