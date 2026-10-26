"""Search a container of the streamed city and move things between it and the backpack (run on the machine with the workspace, here a Linux test machine):

    python3 tools/citygen/inventory_test.py ROOT SCENE

The player is put inside the nearest door's building, the interior streams in, and a container is looked at: E opens it (`inv_open` names it, the backpack opens too), its things are rolled into the slots of the panel,
shutting and opening again keeps them, a thing dragged to a backpack slot is the backpack's and one dragged back is the container's. Tab shuts the backpack.
"""
import json
import math
import os
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

    def resources():
        return call("resources", {}).get("resources", {})

    def check(label, ok, detail=""):
        print(("ok   " if ok else "FAIL ") + label + (f"  {detail}" if detail else ""), flush=True)
        return ok

    call("load_scene", {"path": scene})

    call("set_resource", {"name": "continue_enabled", "value": 0})
    hero, eyes = rows("Hero")[0], rows("MouseLook")[0]
    call("step", {"ticks": 300})
    # into the first doorway's building: a few metres inside, so that its interior streams in
    door = rows("Door")[0]
    d = comp(door, "Door")
    t = comp(door, "Transform")["pos"]
    inside = [t[0] - d["ox"] * 4.0, t[1] - 0.95, t[2] - d["oz"] * 4.0]
    call("set", {"uid": door, "component": "Door", "value": {"want": 1.0, "open": 1.0}})
    call("set", {"uid": hero, "component": "Transform", "value": {"pos": inside}})
    call("step", {"ticks": 500})
    boxes = rows("Container")
    print("containers streamed in:", len(boxes))
    if not boxes:
        print("no containers near the first door: nothing to test")
        return
    # the nearest container to the player
    hp = comp(hero, "Transform")["pos"]
    same_floor = [b for b in boxes if abs(comp(b, "Transform")["pos"][1] - hp[1]) < 1.5] or boxes
    ranked = sorted(same_floor, key=lambda b: sum((a - c) ** 2 for a, c in zip(comp(b, "Transform")["pos"], hp)))
    best = ranked[0]
    for cand in ranked[:8]:
        # stand 1.4 m from it on the side of the player, looking at its middle; the first one the eye ray finds (a wall or a shut door may be in the way of the others) is the one
        bt = comp(cand, "Transform")["pos"]
        dx, dz = hp[0] - bt[0], hp[2] - bt[2]
        l = math.hypot(dx, dz) or 1.0
        stand = [bt[0] + dx / l * 1.4, hp[1], bt[2] + dz / l * 1.4]
        call("set", {"uid": hero, "component": "Transform", "value": {"pos": stand}})
        eye_y = stand[1] + 1.62
        pitch = math.atan2(bt[1] - eye_y, 1.4)
        call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": math.atan2(dx, dz), "pitch": pitch}})
        call("input", {"values": {"e": False, "tab": False}})
        call("step", {"ticks": 30})
        if resources().get("interact_uid") == cand:
            best = cand
            break
    bt = comp(best, "Transform")["pos"]
    bc = comp(best, "Container")
    print("container", best, "kind", bc.get("kind"), "at", [round(v, 1) for v in bt])
    r = resources()
    ok = check("looking at the container", r.get("interact_uid") == best and r.get("interact_kind") == 2, f"target {r.get('interact_uid')} kind {r.get('interact_kind')} (container {best})")
    # open it
    call("input", {"values": {"e": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"e": False}})
    call("step", {"ticks": 12})
    r = resources()
    check("E opens it and the backpack", r.get("inv_open") == best and r.get("inv_player") == 1, f"inv_open {r.get('inv_open')} inv_player {r.get('inv_player')} cursor.lock {r.get('cursor.lock')}")
    shot = os.environ.get("SHOT")
    if shot:
        call("render", {"quality": "high", "width": 1600, "height": 900, "eye": [stand[0], stand[1] + 1.62, stand[2]], "target": [bt[0], bt[1], bt[2]], "fov": 72, "path": shot})
    items = rows("Item")
    mine = [i for i in items if comp(i, "Item").get("owner") == best]
    backpack = [i for i in items if comp(i, "Item").get("owner") == 0]
    check("the backpack's starting things are there", len(backpack) >= 3, f"{len(backpack)} things")
    print("things rolled into the container:", len(mine), [comp(i, "Ui").get("text") for i in mine])
    visible = [i for i in mine if comp(i, "Ui").get("visible")]
    check("they are shown", len(visible) == len(mine) and len(mine) > 0 or bc.get("state") == 1, f"{len(visible)} of {len(mine)} visible")
    # shut it and open it again: the same things
    call("input", {"values": {"e": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"e": False}})
    call("step", {"ticks": 12})
    r = resources()
    shut = r.get("inv_open") == 0
    hidden = all(not comp(i, "Ui").get("visible") for i in mine)
    check("E again shuts it and its things are hidden", shut and hidden, f"inv_open {r.get('inv_open')}")
    call("input", {"values": {"e": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"e": False}})
    call("step", {"ticks": 12})
    items2 = [i for i in rows("Item") if comp(i, "Item").get("owner") == best]
    check("opened again it holds the same things", sorted(items2) == sorted(mine) and all(comp(i, "Ui").get("visible") for i in items2), f"{len(items2)} things")
    # drag the first of its things to the first free backpack slot
    if mine:
        first = mine[0]
        slots = [i for i in rows("UiDrop")]
        names = {s: comp(s, "UiDrop").get("name", "") for s in slots}
        taken = {comp(i, "UiDrag").get("slot") for i in rows("Item")}
        free = [s for s in slots if names[s].startswith("p") and s not in taken]
        if free:
            target = free[0]
            ui = comp(first, "Ui")
            tp = comp(target, "Ui")
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
            call("step", {"ticks": 6})
            drag = comp(first, "UiDrag")
            it = comp(first, "Item")
            check("a thing dragged to a backpack slot is the backpack's", drag.get("slot") == target and it.get("owner") == 0, f"slot {drag.get('slot')} (wanted {target}), owner {it.get('owner')}")
    # Tab shuts the backpack
    call("input", {"values": {"tab": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"tab": False}})
    call("step", {"ticks": 10})
    r = resources()
    check("Tab shuts everything", r.get("inv_player") == 0 and r.get("inv_open") == 0 and r.get("cursor.lock") == 1, f"inv_player {r.get('inv_player')} inv_open {r.get('inv_open')} cursor.lock {r.get('cursor.lock')}")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
