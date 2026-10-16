"""Takes the pictures of "Caldera" in examples/caldera/shots/ (1280 x 720, the GPU renderer at its high quality) with the `render` tool: each is a scene, a few things done to it (the hour of
the day, the hero put somewhere, a golem woken) and a few ticks of play, and the camera put where the picture wants it, a number of metres over the ground (`terrain_height` says where the ground is).
It needs a GPU and `target/release/lunaris` (on the Linux test machine: `a shell` and `--get`).

    python3 tools/caldera_shots.py [name ...]        (without names: the eight pictures kept in examples/caldera/shots/; others on request)
"""
import json
import math
import os
import subprocess
import sys

D = "examples/caldera"
BIN = "target/release/lunaris"
WORLD = json.load(open(f"{D}/world/world.json"))
OUT_DIR = os.environ.get("CALDERA_SHOTS", f"{D}/shots")
ISLAND = json.load(open(f"{D}/island.lunaris.json"))


def named(name):
    for e in ISLAND["entities"]:
        if e["components"].get("Name", {}).get("value") == name:
            return e["components"]
    raise SystemExit(f"no entity named {name}")


def serve(calls, timeout=280):
    p = subprocess.run([BIN, "serve", "--root", "."], input="\n".join(json.dumps({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": c}) for i, c in enumerate(calls)) + "\n",
                       capture_output=True, text=True, timeout=timeout)
    out = {}
    for line in p.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        res = m.get("result") or {}
        if "error" in m or res.get("isError"):
            print("request", m.get("id"), calls[m["id"] - 1]["name"], "FAILED:", json.dumps(m.get("error") or res)[:300])
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    out[m["id"]] = json.loads(c["text"])
                except ValueError:
                    out[m["id"]] = {"text": c["text"]}
    return out


_ground = {}


def ground_of(points):
    need = [p for p in points if p not in _ground]
    if need:
        out = serve([{"name": "load_scene", "arguments": {"path": f"{D}/island.lunaris.json"}}] + [{"name": "terrain_height", "arguments": {"x": x, "z": z}} for x, z in need])
        for i, p in enumerate(need):
            _ground[p] = (out.get(i + 2) or {}).get("height", 10.0)
    return {p: _ground[p] for p in points}


def load(name):
    return [{"name": "load_scene", "arguments": {"path": f"{D}/{name}.lunaris.json"}}]


def step(n):
    return [{"name": "step", "arguments": {"ticks": n}}]


def put(uid, comp, value):
    return [{"name": "set", "arguments": {"uid": uid, "component": comp, "value": value}}]


def res(name, value):
    return [{"name": "set_resource", "arguments": {"name": name, "value": value}}]


def key(k, down):
    return [{"name": "input", "arguments": {"values": {k: down}}}]


def hour(h):
    return put(3, "TimeOfDay", {"time": h})


def hero(x, z, face=(0, -1)):
    y = ground_of([(x, z)])[(x, z)]
    return put(100, "Transform", {"pos": [x, y + 0.05, z]}) + put(100, "Hero", {"fx": face[0], "fz": face[1]})


def shot(name, eye, target, fov=58, quality="high"):
    """`eye` and `target` are (x, metres over the ground, z)."""
    g = ground_of([(eye[0], eye[2]), (target[0], target[2])])
    e = [eye[0], g[(eye[0], eye[2])] + eye[1], eye[2]]
    t = [target[0], g[(target[0], target[2])] + target[1], target[2]]
    return [{"name": "render", "arguments": {"width": 1280, "height": 720, "quality": quality, "image": False, "path": f"{OUT_DIR}/{name}.png", "eye": e, "target": t, "fov": fov}}]


def shot_abs(name, eye, target, fov=58, quality="high"):
    """The same with heights that are metres over the heightmap's black (the sea is at 5)."""
    return [{"name": "render", "arguments": {"width": 1280, "height": 720, "quality": quality, "image": False, "path": f"{OUT_DIR}/{name}.png", "eye": list(eye), "target": list(target), "fov": fov}}]


def no_hud():
    return res("cutscene", 1)


def forest_spot():
    """The middle of the thickest bunch of broadleaf trees (the most within 14 metres), and a way out of it towards the sun."""
    trees = [e["components"]["Transform"]["pos"] for e in ISLAND["entities"] if "broadleaf" in str(e["components"].get("MeshRenderer", {}).get("mesh", ""))]
    best = max(trees, key=lambda p: sum(1 for q in trees if (p[0] - q[0]) ** 2 + (p[2] - q[2]) ** 2 < 196))
    return best[0], best[2]


def build():
    shots = {}
    cx, cz = WORLD["camp"]["x"], WORLD["camp"]["z"]
    sx, sz = WORLD["start"]["x"], WORLD["start"]["z"]
    ar = WORLD["arena"]
    lk = WORLD["lake_at"]
    fx, fz = forest_spot()
    # the island from the beach at sunrise: the hero on the sand, the land and the volcano behind him
    gx = ground_of([(sx - 3, sz + 8)])[(sx - 3, sz + 8)]
    shots["01_beach_sunrise"] = (load("island") + hour(6.25) + hero(sx + 9, sz + 1, (1, 0.1)) + no_hud() + step(8)
                                + shot_abs("01_beach_sunrise", (sx - 3, gx + 1.5, sz + 8), (sx + 60, gx + 5.4, sz + 6), 60))
    # the forest, in the morning light, with the hero walking between the trees
    shots["02_forest"] = load("island") + hour(9.0) + hero(fx + 4, fz + 4, (-1, -1)) + no_hud() + step(8) + shot("02_forest", (fx + 15, 5.2, fz + 17), (fx - 3, 1.5, fz - 3), 62)
    # the crater rim at dusk from the side of the hill: the pillars, the sea behind, the hero in front of the Warden (nine metres over the ground to see the plateau, which a low eye cannot)
    shots["03_crater_dusk"] = (load("island") + hour(17.4) + hero(ar["x"] - 2.0, ar["z"] + 7.0, (0.2, -1)) + no_hud() + step(8)
                               + shot("03_crater_dusk", (ar["x"] - 24.0, 9.0, ar["z"] + 10.0), (ar["x"] + 2.0, 3.0, ar["z"] - 2.0), 60))
    shots["04_camp_night"] = (load("island") + hour(22.3) + hero(cx - 2.4, cz + 2.2, (1, -0.4)) + step(10)
                              + shot("04_camp_night", (cx - 4.5, 2.1, cz + 11.5), (cx + 3.0, 3.2, cz - 5.0), 64))
    gp = named("Golem highland 1")["Transform"]["pos"]
    shots["05_golem_fight"] = (load("island") + hour(10.5) + hero(gp[0] + 2.4, gp[2], (-1, 0)) + step(100) + key("mouse_left", True) + step(2) + key("mouse_left", False) + step(18)
                               + shot("05_golem_fight", (gp[0] + 3.5, 2.4, gp[2] + 6.0), (gp[0] - 0.5, 1.8, gp[2]), 56))
    shots["06_lake"] = (load("island") + hour(12.0) + hero(lk["x"] + 21, lk["z"] + 9, (-1, -0.3)) + no_hud() + step(8)
                        + shot("06_lake", (lk["x"] + 31, 4.5, lk["z"] + 20), (lk["x"] - 6, 2.0, lk["z"] - 6), 62))
    shots["07_title"] = load("title") + step(30) + shot("07_title", (0, 0, 0), (0, 0, 0), 55)[:0] + [{"name": "render", "arguments": {"width": 1280, "height": 720, "quality": "high", "image": False, "path": f"{OUT_DIR}/07_title.png"}}]
    bx, bz = named("Beacon")["Transform"]["pos"][0], named("Beacon")["Transform"]["pos"][2]
    ax, az = named("Altar")["Transform"]["pos"][0], named("Altar")["Transform"]["pos"][2]
    shots["08_win_cutscene"] = (load("island") + hour(21.8) + res("g.dep", 6) + res("g.s1", 1) + res("g.s2", 1) + res("g.s3", 1) + res("g.s4", 1) + res("g.s5", 1) + res("g.s6", 1)
                                + hero(bx - 3.0, bz + 3.0) + step(6) + key("e", True) + step(2) + key("e", False) + step(300)
                                + [{"name": "render", "arguments": {"width": 1280, "height": 720, "quality": "high", "image": False, "path": f"{OUT_DIR}/08_win_cutscene.png"}}])
    # on request
    shots["09_boss"] = (load("island") + hour(18.6) + hero(ar["x"], ar["z"] + 11, (0, -1)) + step(90) + shot("09_boss", (ar["x"] + 7, 3.2, ar["z"] + 15), (ar["x"], 3.0, ar["z"] + 3), 58))
    shots["10_swim"] = (load("island") + hour(11.0) + hero(lk["x"] + 6, lk["z"] + 4, (-1, -0.3)) + no_hud() + step(60) + shot("10_swim", (lk["x"] + 12, 3.0, lk["z"] + 11), (lk["x"] + 5, 0.5, lk["z"] + 4), 58))
    shots["11_hud_start"] = load("island") + hour(7.2) + step(20) + [{"name": "render", "arguments": {"width": 1280, "height": 720, "quality": "high", "image": False, "path": f"{OUT_DIR}/11_hud_start.png"}}]
    shots["12_dialogue"] = (load("island") + hour(15.0) + hero(cx - 3.0, cz + 3.5, (-1, 0.5)) + step(8) + key("e", True) + step(2) + key("e", False) + step(40)
                            + [{"name": "render", "arguments": {"width": 1280, "height": 720, "quality": "high", "image": False, "path": f"{OUT_DIR}/12_dialogue.png"}}])
    shots["13_quest_log"] = (load("island") + hour(12.0) + res("g.s1", 1) + res("g.s2", 1) + step(5) + key("l", True) + step(2) + key("l", False) + step(8)
                             + [{"name": "render", "arguments": {"width": 1280, "height": 720, "quality": "high", "image": False, "path": f"{OUT_DIR}/13_quest_log.png"}}])
    shots["14_gameover"] = load("gameover") + step(60) + [{"name": "render", "arguments": {"width": 1280, "height": 720, "quality": "high", "image": False, "path": f"{OUT_DIR}/14_gameover.png"}}]
    shots["15_settings"] = (load("title") + step(30) + res("settings", 1) + step(5) + [{"name": "render", "arguments": {"width": 1280, "height": 720, "quality": "high", "image": False, "path": f"{OUT_DIR}/15_settings.png"}}])
    return shots


KEPT = ["01_beach_sunrise", "02_forest", "03_crater_dusk", "04_camp_night", "05_golem_fight", "06_lake", "07_title", "08_win_cutscene"]


def run(name, calls):
    out = serve(calls)
    last = out.get(len(calls)) or {}
    print(f"{name}: {last.get('renderer', '?')} measured={last.get('measured')} warnings={last.get('warnings')}")


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    shots = build()
    for name in sys.argv[1:] or KEPT:
        run(name, shots[name])


if __name__ == "__main__":
    main()
