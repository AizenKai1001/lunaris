"""Footsteps and the world's sounds: walking makes a step every 1.7 m on the ground, standing still none; the day's birds and the night's crickets are looping sounds whose volume follows the light (run on the machine
with the workspace):

    python3 tools/citygen/footsteps_test.py ROOT SCENE
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

    call("load_scene", {"path": scene})
    call("set_resource", {"name": "continue_enabled", "value": 0})
    hero = rows("Hero")[0]
    call("step", {"ticks": 240})
    s0 = res("steps_taken")
    call("step", {"ticks": 120})
    check("standing still makes no step", res("steps_taken") == s0, f"{s0} -> {res('steps_taken')}")
    call("input", {"values": {"w": True}})
    p0 = comp(hero, "Transform")["pos"]
    call("step", {"ticks": 300})
    p1 = comp(hero, "Transform")["pos"]
    call("input", {"values": {"w": False}})
    walked = ((p1[0] - p0[0]) ** 2 + (p1[2] - p0[2]) ** 2) ** 0.5
    steps = res("steps_taken") - s0
    check("walking makes a step every 1.7 m", walked > 5 and abs(steps - walked / 1.7) <= 2.5, f"{walked:.1f} m, {steps} steps (expected about {walked / 1.7:.0f})")
    vols = {comp(u, "WeatherSound").get("kind"): comp(u, "AudioSource").get("volume") for u in rows("WeatherSound")}
    check("by day the birds sound and the crickets do not", vols.get(3.0, 0) > 0.3 and vols.get(4.0, 1) < 0.05, str(vols))
    call("set", {"uid": rows("TimeOfDay")[0], "component": "TimeOfDay", "value": {"time": 23.0}})
    call("step", {"ticks": 3})
    vols = {comp(u, "WeatherSound").get("kind"): comp(u, "AudioSource").get("volume") for u in rows("WeatherSound")}
    check("by night the crickets sound and the birds do not", vols.get(4.0, 0) > 0.3 and vols.get(3.0, 1) < 0.05, str(vols))
    # the compass shows the way the player faces (north is -z)
    eyes = rows("MouseLook")[0]
    labels = {comp(u, "Compass").get("sector"): u for u in rows("Compass")}
    for yaw, want, name in ((0.0, 0.0, "north"), (-1.5708, 2.0, "east"), (3.14159, 4.0, "south")):
        call("set", {"uid": eyes, "component": "MouseLook", "value": {"yaw": yaw}})
        call("step", {"ticks": 3})
        shown = [k for k, u in labels.items() if comp(u, "Text").get("color", [0, 0, 0, 0])[3] > 0.5]
        check(f"facing {name} the compass says so", shown == [want], f"shown sectors {shown}")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
