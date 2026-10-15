"""Takes the pictures of "Depths" in examples/depths/shots/ (1280 x 720, the GPU renderer at its high quality) with the `render` tool: each is a scene, a few things done to it
(a hero put next to a guard, a key pressed) and a few ticks of play. It needs a GPU and `target/release/lunaris` (on a Linux test machine: `a shell --get ...`).

    python3 tools/depths_shots.py [name ...]        (without names: the six pictures kept in examples/depths/shots/)
"""
import json
import os
import subprocess
import sys

D = "examples/depths"
OUT = f"{D}/shots"
BIN = "target/release/lunaris"


def scene(name):
    with open(f"{D}/{name}.lunaris.json", encoding="utf-8") as f:
        return json.load(f)


def pos_of(sc, name):
    for e in sc["entities"]:
        if e["components"].get("Name", {}).get("value") == name:
            return e["components"]["Transform"]["pos"]
    raise SystemExit(f"no {name}")


def guard_uid(sc, which=0):
    return [e["uid"] for e in sc["entities"] if "Enemy" in e["components"]][which]


def load(name):
    return [{"name": "load_scene", "arguments": {"path": f"{D}/{name}.lunaris.json"}}]


def step(n):
    return [{"name": "step", "arguments": {"ticks": n}}]


def move(uid, x, y):
    return [{"name": "set", "arguments": {"uid": uid, "component": "Transform", "value": {"pos": [x, y, 0]}}}]


def key(k, down):
    return [{"name": "input", "arguments": {"values": {k: down}}}]


def shot(name):
    return [{"name": "render", "arguments": {"width": 1280, "height": 720, "quality": "high", "image": False, "path": f"{OUT}/{name}.png"}}]


def build():
    l1, l2, l3 = scene("level1"), scene("level2"), scene("level3")
    shots = {}
    shots["menu"] = load("menu") + step(110) + shot("menu")
    shots["level1_start"] = load("level1") + step(150) + shot("level1_start")
    wx, wy, _ = pos_of(l1, "Warden")
    shots["level1_talk"] = load("level1") + step(3) + move(1000, wx, wy) + step(40) + key("e", True) + step(3) + key("e", False) + step(200) + shot("level1_talk")
    g = guard_uid(l1)
    hx, hy, _ = pos_of(l1, "Hero")
    # the first free side of the hero's cell: the guard stands there, the hero faces it
    tm = next(e["components"]["Tilemap"] for e in l1["entities"] if e["uid"] == 10)
    w, h = tm["size"]
    rows = [[int(v) for v in line.split()] for line in tm["data"].strip().split("\n")]
    cx, cy = int(hx + w / 2), int(h / 2 - hy)
    fx, fy = next((dx, -dy) for dx, dy in ((1, 0), (0, 1), (-1, 0), (0, -1)) if rows[cy + dy][cx + dx] < 3)
    shots["level1_fight"] = (load("level1") + [{"name": "set_resource", "arguments": {"name": "face.x", "value": fx}}, {"name": "set_resource", "arguments": {"name": "face.y", "value": fy}}] + step(200)
                             + move(g, hx + fx, hy + fy) + step(1) + key("space", True) + step(3) + key("space", False) + step(4) + shot("level1_fight"))
    kx, ky, _ = pos_of(l1, "Key")
    shots["level1_key"] = load("level1") + step(3) + move(1000, kx - 2.5, ky) + step(240) + shot("level1_key")
    hx2, hy2, _ = pos_of(l2, "Heart 2")
    shots["level2_caves"] = load("level2") + step(3) + move(1000, hx2 + 2.0, hy2) + step(120) + shot("level2_caves")
    ex, ey, _ = pos_of(l3, "Key")
    shots["level3_maze"] = load("level3") + step(3) + move(1000, ex, ey) + step(260) + shot("level3_maze")
    shots["pause"] = load("level1") + step(200) + key("escape", True) + step(3) + key("escape", False) + step(6) + shot("pause")
    shots["gameover"] = load("gameover") + step(100) + shot("gameover")
    shots["win"] = load("win") + [{"name": "set_resource", "arguments": {"name": "score", "value": 1720}}, {"name": "set_resource", "arguments": {"name": "coins", "value": 41}},
                                  {"name": "set_resource", "arguments": {"name": "kills", "value": 12}}] + step(150) + shot("win")
    return shots


def main():
    os.makedirs(OUT, exist_ok=True)
    shots = build()
    wanted = sys.argv[1:] or ["menu", "level1_talk", "level1_fight", "level2_caves", "level3_maze", "win"]  # the six kept in the repository; level1_start, level1_key, pause and gameover on request
    for name in wanted:
        calls = shots[name]
        p = subprocess.run([BIN, "serve", "--root", "."], input="\n".join(json.dumps({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": c}) for i, c in enumerate(calls)) + "\n", capture_output=True, text=True, timeout=240)
        last = None
        for line in p.stdout.splitlines():
            try:
                m = json.loads(line)
            except ValueError:
                continue
            res = m.get("result") or {}
            if "error" in m or res.get("isError"):
                print(name, "FAILED:", json.dumps(m.get("error") or res)[:300])
            for c in res.get("content") or []:
                if c.get("type") == "text":
                    last = c["text"]
        info = json.loads(last) if last and last.startswith("{") else {}
        print(f"{name}: {info.get('renderer', '?')} {info.get('hash', '')}")


if __name__ == "__main__":
    main()
