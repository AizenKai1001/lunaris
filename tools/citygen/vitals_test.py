"""Living in the city: the bars run down and hurt at nothing, a click on a thing in the backpack uses it, building spends wood and nails, death wakes the player at the start (run on the machine with the workspace):

    python3 tools/citygen/vitals_test.py ROOT SCENE
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

    call("load_scene", {"path": scene})

    call("set_resource", {"name": "continue_enabled", "value": 0})
    hero, eyes = rows("Hero")[0], rows("MouseLook")[0]
    call("step", {"ticks": 200})
    start = comp(hero, "Transform")["pos"]
    # the bars run down
    call("set_resource", {"name": "food", "value": 80})
    call("set_resource", {"name": "water", "value": 80})
    call("step", {"ticks": 300})
    f, w = res("food"), res("water")
    check("food and water run down", f < 80 and w < 80 and w < f, f"food {f:.1f} water {w:.1f}")
    # at nothing health falls
    call("set_resource", {"name": "food", "value": 0})
    call("set_resource", {"name": "water", "value": 0})
    call("step", {"ticks": 120})
    check("at nothing health falls", res("hp") < 100, f"hp {res('hp'):.1f}")
    # a click on the water in the backpack drinks it
    call("set_resource", {"name": "water", "value": 20})
    call("set_resource", {"name": "food", "value": 80})
    call("set_resource", {"name": "hp", "value": 60})
    call("input", {"values": {"tab": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"tab": False}})
    call("step", {"ticks": 8})
    items = [i for i in rows("Item") if comp(i, "Item").get("owner") == 0]
    texts = {i: comp(i, "Ui").get("text") for i in items}
    water = [i for i, t in texts.items() if t == "Water"]
    bandage = [i for i, t in texts.items() if t == "Bandage"]
    check("the backpack holds water and a bandage", bool(water) and bool(bandage), str(sorted(texts.values())))
    for item, what in ((water[0], "water"), (bandage[0], "hp")):
        before = res(what)
        ui = comp(item, "Ui")
        x, y = ui["pos"][0] + ui["size"][0] / 2, ui["pos"][1] + ui["size"][1] / 2
        call("input", {"values": {"mouse_x": x, "mouse_y": y}})
        call("step", {"ticks": 3})
        call("input", {"values": {"mouse_left": True}})
        call("step", {"ticks": 3})
        call("input", {"values": {"mouse_left": False}})
        call("step", {"ticks": 6})
        after = res(what)
        gone = call("get", {"uid": item}).get("code") is not None or not comp(item, "Item")
        check(f"a click uses the {what} item up", after > before + 5 and gone, f"{what} {before:.1f} -> {after:.1f}, gone {gone}")
    call("input", {"values": {"tab": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"tab": False}})
    call("step", {"ticks": 6})
    # building costs wood and nails
    call("set", {"uid": hero, "component": "Transform", "value": {"pos": [start[0] + 12.0, start[1] + 0.3, start[2] - 6.0]}})
    call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": 0.0, "pitch": -0.38}})
    call("step", {"ticks": 40})
    wood0 = res("mat_wood")
    call("input", {"values": {"b": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"b": False}})
    call("step", {"ticks": 10})
    pieces = len(rows("Piece"))
    call("input", {"values": {"mouse_left": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"mouse_left": False}})
    call("step", {"ticks": 6})
    check("a foundation is paid for in wood", len(rows("Piece")) == pieces + 1 and res("mat_wood") == wood0 - 6, f"wood {wood0} -> {res('mat_wood')}, pieces {pieces} -> {len(rows('Piece'))}")
    # with nothing in stock nothing is built
    call("set_resource", {"name": "mat_wood", "value": 1})
    call("step", {"ticks": 4})
    call("input", {"values": {"2": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"2": False}})
    call("step", {"ticks": 6})
    pieces = len(rows("Piece"))
    call("input", {"values": {"mouse_left": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"mouse_left": False}})
    call("step", {"ticks": 6})
    check("without the wood nothing is built", len(rows("Piece")) == pieces, f"{pieces} -> {len(rows('Piece'))}")
    # death
    call("set_resource", {"name": "hp", "value": 0.005})
    call("step", {"ticks": 6})
    p = comp(hero, "Transform")["pos"]
    check("no health: the player wakes at the start with health back", res("hp") > 90 and math.hypot(p[0] - start[0], p[2] - start[2]) < 5.0, f"hp {res('hp'):.0f} at ({p[0]:.0f}, {p[2]:.0f}), start ({start[0]:.0f}, {start[2]:.0f})")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
