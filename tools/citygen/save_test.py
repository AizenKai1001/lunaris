"""Saved games in the city: what you built, carry and have found comes back after a save and a load (run on the machine with the workspace):

    python3 tools/citygen/save_test.py ROOT SCENE

Builds a foundation, spends stock, saves (the `save_game` tool writes ROOT/saves/<slot>.json), loads a fresh scene, loads the save, and looks at the pieces, the stock, the
backpack and the number of doors (a zone that streamed in twice would double it). Prints the size of the file.
"""
import json
import os
import subprocess
import sys
import time

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

    def total(component):
        return call("query", {"with": [component], "select": []}).get("total", 0)

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

    call("load_scene", {"path": scene})

    call("set_resource", {"name": "continue_enabled", "value": 0})
    hero, eyes = rows("Hero")[0], rows("MouseLook")[0]
    call("step", {"ticks": 200})
    start = comp(hero, "Transform")["pos"]
    call("set", {"uid": hero, "component": "Transform", "value": {"pos": [start[0] + 12.0, start[1] + 0.3, start[2] - 6.0]}})
    call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": 0.0, "pitch": -0.38}})
    call("step", {"ticks": 40})
    call("input", {"values": {"b": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"b": False}})
    call("step", {"ticks": 10})
    call("input", {"values": {"mouse_left": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"mouse_left": False}})
    call("step", {"ticks": 6})
    pieces, doors, containers, lamps = total("Piece"), total("Door"), total("Container"), total("StreetLamp")
    wood = res("mat_wood")
    call("set_resource", {"name": "food", "value": 33})
    call("set_resource", {"name": "hp", "value": 71})
    call("step", {"ticks": 2})
    check("one foundation stands", pieces >= 1, f"pieces {pieces}, wood {wood}, doors {doors}, containers {containers}, lamps {lamps}")
    call("save_game", {"slot": "citytest"})
    path = os.path.join(root, "saves", "citytest.json")
    check("the save is on disk", os.path.exists(path), f"{os.path.getsize(path) / 1e6:.1f} MB" if os.path.exists(path) else "")
    # a fresh world, then the save
    call("load_scene", {"path": scene})
    call("set_resource", {"name": "continue_enabled", "value": 0})
    call("step", {"ticks": 60})
    check("the fresh world has no foundation of ours", total("Piece") == 0, str(total("Piece")))
    out = call("load_game", {"slot": "citytest"})
    call("step", {"ticks": 2})
    check("the foundation is back", total("Piece") == pieces, f"{total('Piece')} of {pieces}")
    check("stock and bars are back", res("mat_wood") == wood and abs(res("food") - 33) < 3 and abs(res("hp") - 71) < 3, f"wood {res('mat_wood')}, food {res('food'):.0f}, hp {res('hp'):.0f}")
    call("step", {"ticks": 240})
    check("play goes on without doubling the city", total("Door") <= doors * 1.2 and total("Container") <= containers * 1.2 and total("StreetLamp") <= lamps * 1.5, f"doors {total('Door')} (was {doors}), containers {total('Container')} (was {containers}), lamps {total('StreetLamp')} (was {lamps})")
    p = comp(rows("Hero")[0], "Transform")["pos"]
    check("the hero stands where it stood", abs(p[0] - (start[0] + 12.0)) < 3.0, str([round(v, 1) for v in p]))
    # the keys: F5 saves slot 1 (and the HUD says so), F9 loads it (an empty slot says so too)
    slot1 = os.path.join(root, "saves", f"slot{int(res('quick_slot'))}.json")
    if os.path.exists(slot1):
        os.remove(slot1)
    call("input", {"values": {"f9": True}})
    call("step", {"ticks": 2})
    call("input", {"values": {"f9": False}})
    call("step", {"ticks": 2})
    toasts = {comp(u, "Toast").get("kind"): comp(u, "Text").get("color") for u in rows("Toast")}
    check("F9 with nothing saved says so", toasts.get(3.0, [0, 0, 0, 0])[3] > 0.5, str(toasts.get(3.0)))
    call("input", {"values": {"f5": True}})
    t = time.time()
    call("step", {"ticks": 2})
    took = (time.time() - t) * 1000
    call("input", {"values": {"f5": False}})
    call("step", {"ticks": 3})
    toasts = {comp(u, "Toast").get("kind"): comp(u, "Text").get("color") for u in rows("Toast")}
    check("F5 writes slot 1 and says so", os.path.exists(slot1) and toasts.get(1.0, [0, 0, 0, 0])[3] > 0.5 and toasts.get(3.0, [0, 0, 0, 1])[3] < 0.5, f"{took:.0f} ms for two ticks with the save in them, file {os.path.getsize(slot1) / 1e6:.1f} MB" if os.path.exists(slot1) else "no file")
    call("set_resource", {"name": "food", "value": 5})
    call("input", {"values": {"f9": True}})
    call("step", {"ticks": 2})
    call("input", {"values": {"f9": False}})
    call("step", {"ticks": 3})
    toasts = {comp(u, "Toast").get("kind"): comp(u, "Text").get("color") for u in rows("Toast")}
    check("F9 brings the saved bars back and says so", res("food") > 20 and toasts.get(2.0, [0, 0, 0, 0])[3] > 0.5, f"food {res('food'):.0f}")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
