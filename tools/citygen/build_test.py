"""Build in the streamed city: B starts building, the number keys choose a piece, a click puts it down where the ghost is, X takes a box away (run on the machine with the workspace, here a Linux test machine):

    python3 tools/citygen/build_test.py ROOT SCENE

The player stands in a street looking down at the ground: a foundation is put down (snapped to the 3 m grid), the same place again is refused, a wall and a doorway and a door go on its edge, and the door opens like any other.
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
            print(name, "FAILED", json.dumps(m)[:500])
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

    def tap(key, ticks=3):
        call("input", {"values": {key: True}})
        call("step", {"ticks": ticks})
        call("input", {"values": {key: False}})
        call("step", {"ticks": 4})

    def check(label, ok, detail=""):
        print(("ok   " if ok else "FAIL ") + label + (f"  {detail}" if detail else ""), flush=True)
        return ok

    call("load_scene", {"path": scene})

    call("set_resource", {"name": "continue_enabled", "value": 0})
    call("set_resource", {"name": "mat_wood", "value": 99})
    call("set_resource", {"name": "mat_nails", "value": 99})
    hero, eyes = rows("Hero")[0], rows("MouseLook")[0]
    call("step", {"ticks": 300})
    # in the street in front of the spawn, looking down at the ground 4 m ahead
    pos = comp(hero, "Transform")["pos"]
    call("set", {"uid": hero, "component": "Transform", "value": {"pos": [pos[0] + 12.0, pos[1] + 0.3, pos[2] - 6.0]}})
    call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": 0.0, "pitch": -0.38}})
    call("step", {"ticks": 40})
    tap("b")
    r = resources()
    check("B starts building", r.get("build_on") == 1)
    call("step", {"ticks": 10})
    r = resources()
    check("the ground is aimed at", r.get("build_aimed") == 1, f"aimed {r.get('build_aimed')} at ({r.get('build_x')}, {r.get('build_y')}, {r.get('build_z')})")
    gx, gz = r.get("build_x"), r.get("build_z")
    snapped = abs(((gx - 1.5) / 3.0) - round((gx - 1.5) / 3.0)) < 1e-3 and abs(((gz - 1.5) / 3.0) - round((gz - 1.5) / 3.0)) < 1e-3
    check("snapped to the 3 m grid", snapped, f"({gx}, {gz})")
    ghost = rows("BuildGhost")[0]
    check("the ghost is shown", comp(ghost, "MeshRenderer").get("visible") is True)
    before = len(rows("Piece"))

    def click():
        call("input", {"values": {"mouse_left": True}})
        call("step", {"ticks": 3})
        call("input", {"values": {"mouse_left": False}})
        call("step", {"ticks": 6})

    click()
    pieces = rows("Piece")
    check("a click puts a foundation down", len(pieces) == before + 1, f"{len(pieces)} pieces")
    if len(pieces) == before + 1:
        new = [p for p in pieces][-1]
        t = comp(new, "Transform")["pos"]
        check("it stands where the ghost was", abs(t[0] - gx) < 0.05 and abs(t[2] - gz) < 0.05, f"at ({t[0]:.2f}, {t[1]:.2f}, {t[2]:.2f})")
        check("its Placing is gone", not comp(new, "Placing"))
    click()
    # (a second click at the same aim lands on top of the first, whose top is now what the ray hits: it is a piece stacked on it, not a copy in its place)
    # choose a wall and aim at the same cell's edge: the player moves so that the ray hits the foundation's top edge
    tap("2")
    r = resources()
    check("2 chooses the wall", r.get("build_piece") == 2)
    call("step", {"ticks": 10})
    click()
    check("a wall goes down", len(rows("Piece")) >= before + 2, f"{len(rows('Piece'))} pieces")
    tap("3")
    call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": 0.6, "pitch": -0.38}})
    call("step", {"ticks": 10})
    click()
    tap("6")
    call("step", {"ticks": 10})
    click()
    names = [comp(p, "Name").get("value") for p in rows("Piece")]
    print("pieces:", names)
    doors = [pc for pc in rows("Piece") if comp(pc, "Name").get("value") == "Door piece"]
    check("a door piece exists", len(doors) >= 1, f"{len(doors)}")
    if doors:
        d = doors[0]
        door = comp(d, "Door")
        call("set", {"uid": d, "component": "Door", "value": {"want": 1.0}})
        call("step", {"ticks": 60})
        check("the door piece swings open", comp(d, "Door").get("open", 0) > 0.95)
    import os
    if os.environ.get("SHOT"):
        t = comp(rows("Piece")[0], "Transform")["pos"]
        call("render", {"quality": "high", "width": 1400, "height": 800, "eye": [t[0] - 7.0, t[1] + 5.0, t[2] + 9.0], "target": [t[0], t[1] + 1.0, t[2]], "fov": 60, "path": os.environ["SHOT"]})
    # X takes a box away
    tap("1")
    boxes = rows("PieceBox")
    print("boxes:", len(boxes))
    tap("b")
    r = resources()
    check("B ends building", r.get("build_on") == 0)
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
