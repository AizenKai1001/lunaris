"""Working what stands in the street: a tree looked at gives wood to a hatchet (E), a parked car gives scrap to a crowbar, each a few times; without the tool the prompt says what is missing (run on the machine with the
workspace):

    python3 tools/citygen/harvest_test.py ROOT SCENE
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

    def face(hero, eyes, target, dist, pitch=0.0):
        """Put the player `dist` metres from `target` (on the side it already is), looking at it."""
        hp = comp(hero, "Transform")["pos"]
        dx, dz = target[0] - hp[0], target[2] - hp[2]
        d = math.hypot(dx, dz) or 1.0
        ux, uz = dx / d, dz / d
        px, pz = target[0] - ux * dist, target[2] - uz * dist
        call("set", {"uid": hero, "component": "Transform", "value": {"pos": [px, hp[1], pz]}})
        yaw = math.atan2(-ux, -uz)
        call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": yaw, "pitch": pitch}})
        call("step", {"ticks": 25})

    call("load_scene", {"path": scene})
    call("set_resource", {"name": "continue_enabled", "value": 0})
    hero, eyes = rows("Hero")[0], rows("MouseLook")[0]
    call("step", {"ticks": 240})
    spots = rows("Harvest")
    kinds = {u: comp(u, "Harvest") for u in spots}
    hp = comp(hero, "Transform")["pos"]

    def near(kind):
        c = [(math.hypot(comp(u, "Transform")["pos"][0] - hp[0], comp(u, "Transform")["pos"][2] - hp[2]), u) for u, h in kinds.items() if h.get("kind") == kind]
        return sorted(c)
    trees, cars = near(1.0), near(2.0)
    check("trees and cars can be worked", len(trees) >= 1 and len(cars) >= 1, f"{len(trees)} trees, {len(cars)} cars streamed in")
    # a tree, with the hatchet the backpack starts with
    tree = trees[0][1]
    face(hero, eyes, comp(tree, "Transform")["pos"], 2.0)
    check("the tree looked at asks for E to chop it", res("interact_uid") == tree and res("interact_kind") == 4, f"target {res('interact_uid')} (tree {tree}) kind {res('interact_kind')}, hatchet {res('has_hatchet')}")
    wood0 = res("mat_wood")
    tap("e")
    check("E gives wood and uses up a chop", res("mat_wood") == wood0 + 3 and comp(tree, "Harvest").get("left") == 3, f"wood {wood0} -> {res('mat_wood')}, left {comp(tree, 'Harvest').get('left')}")
    for _ in range(3):
        tap("e")
    check("the tree is used up after its chops", comp(tree, "Harvest").get("left") == 0 and res("mat_wood") == wood0 + 12 and res("interact_kind") == 0, f"wood {res('mat_wood')}, left {comp(tree, 'Harvest').get('left')}, kind {res('interact_kind')}")
    # without the hatchet nothing is given
    items = [i for i in rows("Item") if comp(i, "Item").get("owner") == 0]
    hatchets = [i for i in items if comp(i, "Ui").get("text") == "Hatchet"]
    for h in hatchets:
        call("despawn", {"uid": h})
    call("step", {"ticks": 6})
    if len(trees) > 1:
        tree2 = trees[1][1]
        face(hero, eyes, comp(tree2, "Transform")["pos"], 2.0)
        wood1 = res("mat_wood")
        tap("e")
        check("without a hatchet the prompt says so and no wood comes", res("interact_kind") == 6 and res("mat_wood") == wood1, f"kind {res('interact_kind')}, wood {wood1} -> {res('mat_wood')}")
    # a car, with a crowbar put in the backpack
    car = cars[0][1]
    slots = [s for s in rows("UiDrop") if comp(s, "UiDrop").get("name", "").startswith("p")]
    taken = {comp(i, "UiDrag").get("slot") for i in rows("Item")}
    free = [s for s in slots if s not in taken]
    face(hero, eyes, comp(car, "Transform")["pos"], 3.0, -0.32)
    nails0 = res("mat_nails")
    tap("e")
    check("without a crowbar a car gives nothing", res("interact_kind") == 7 and res("mat_nails") == nails0, f"kind {res('interact_kind')}")
    call("spawn_prefab", {"prefab": "Item_7", "overrides": {"UiDrag": {"slot": free[0]}}})
    call("step", {"ticks": 10})
    tap("e")
    check("with a crowbar E strips scrap into nails", res("mat_nails") == nails0 + 4 and comp(car, "Harvest").get("left") == 2, f"nails {nails0} -> {res('mat_nails')}, left {comp(car, 'Harvest').get('left')}, crowbar {res('has_crowbar')}")
    # a hydrant gives water to drink
    hydrants = near(3.0)
    if hydrants:
        h = hydrants[0][1]
        face(hero, eyes, comp(h, "Transform")["pos"], 2.0, -0.58)
        call("set_resource", {"name": "water", "value": 30})
        call("step", {"ticks": 2})
        check("a hydrant looked at offers a drink", res("interact_kind") == 10, f"kind {res('interact_kind')}")
        w0 = res("water")
        tap("e")
        check("E drinks from it", res("water") > w0 + 25 and comp(h, "Harvest").get("left") == 2, f"water {w0:.0f} -> {res('water'):.0f}, left {comp(h, 'Harvest').get('left')}")
    else:
        check("a hydrant is near", False, "none streamed in")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
