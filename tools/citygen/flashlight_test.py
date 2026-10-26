"""The flashlight: F lights it when one is in the backpack, the battery runs down while it shines, dims at the end and puts it out, and batteries (a click in the backpack) charge it again (run on the machine with the
workspace):

    python3 tools/citygen/flashlight_test.py ROOT SCENE
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

    def free_slot():
        slots = [s for s in rows("UiDrop") if comp(s, "UiDrop").get("name", "").startswith("p")]
        taken = {comp(i, "UiDrag").get("slot") for i in rows("Item")}
        return [s for s in slots if s not in taken][0]

    call("load_scene", {"path": scene})
    call("set_resource", {"name": "continue_enabled", "value": 0})
    light = rows("Flashlight")[0]
    call("step", {"ticks": 100})
    tap("f")
    check("with no flashlight F lights nothing", res("torch_on") == 0 and comp(light, "Light").get("intensity", 0) == 0, f"torch_on {res('torch_on')}")
    bar = [u for u in rows("Vital") if comp(u, "Vital").get("kind") == 4.0][0]
    check("the battery bar is hidden without a flashlight", comp(bar, "Ui").get("visible") is False, str(comp(bar, "Ui")))
    call("spawn_prefab", {"prefab": "Item_8", "overrides": {"UiDrag": {"slot": free_slot()}}})
    call("step", {"ticks": 10})
    check("and shown with one", comp(bar, "Ui").get("visible") is True and abs(comp(bar, "Ui").get("fill", 0) - 1.0) < 0.01, str(comp(bar, "Ui")))
    tap("f")
    call("step", {"ticks": 4})
    i0, b0 = comp(light, "Light").get("intensity", 0), res("battery")
    check("with one F lights it", res("torch_on") == 1 and i0 > 5, f"torch_on {res('torch_on')}, intensity {i0:.1f}, battery {b0:.1f}")
    call("step", {"ticks": 600})
    b1 = res("battery")
    check("the battery runs down while it shines", 3.0 < b0 - b1 < 6.0, f"{b0:.1f} -> {b1:.1f} in ten seconds")
    call("set_resource", {"name": "battery", "value": 8})
    call("step", {"ticks": 3})
    i2 = comp(light, "Light").get("intensity", 0)
    check("with little charge it dims", 0 < i2 < i0 * 0.9, f"intensity {i0:.1f} -> {i2:.1f}")
    call("set_resource", {"name": "battery", "value": 0})
    call("step", {"ticks": 4})
    check("an empty battery puts it out", res("torch_on") == 0 and comp(light, "Light").get("intensity", 0) == 0, f"torch_on {res('torch_on')}")
    # batteries: opened backpack, a click on the item
    call("spawn_prefab", {"prefab": "Item_14", "overrides": {"UiDrag": {"slot": free_slot()}}})
    tap("tab")
    cells = [i for i in rows("Item") if comp(i, "Ui").get("text") == "Cells"]
    ui = comp(cells[0], "Ui")
    x, y = ui["pos"][0] + ui["size"][0] / 2, ui["pos"][1] + ui["size"][1] / 2
    call("input", {"values": {"mouse_x": x, "mouse_y": y}})
    call("step", {"ticks": 3})
    call("input", {"values": {"mouse_left": True}})
    call("step", {"ticks": 3})
    call("input", {"values": {"mouse_left": False}})
    call("step", {"ticks": 8})
    gone = call("get", {"uid": cells[0]}).get("code") is not None or not comp(cells[0], "Item")
    check("a click on batteries charges the flashlight", res("battery") > 50 and gone, f"battery {res('battery'):.1f}, item gone {gone}")
    tap("tab")
    tap("f")
    call("step", {"ticks": 4})
    check("and it lights again", res("torch_on") == 1 and comp(light, "Light").get("intensity", 0) > 5, f"intensity {comp(light, 'Light').get('intensity', 0):.1f}")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
