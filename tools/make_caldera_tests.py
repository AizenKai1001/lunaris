"""Writes the playtests of "Caldera" (examples/tests/caldera_*.test.json) from the scenes `tools/build_caldera.py` made: the scenes are read as data, a bot's way over the terrain is computed
with the engine's `path` tool (the same ground the hero walks on), and the keys a player would press are worked out from it.

  caldera_title      the boot scene hands over to the title; Enter plays: the island with its music still playing, a full health bar, no shards
  caldera_walk       a bot walks from the beach start to the first ember shard along a path over the terrain (the stick of a pad, the camera not turned), and takes it
  caldera_combat     a sleeping golem wakes when the hero comes near, hunts it, and is brought down by the sword: hit-stop, a kill, a golem that is gone
  caldera_night      at night wraiths come out of the dark and bite; at the fire they are sent away
  caldera_swim       the hero wades into the lake, swims in the deep water and walks out again (`InWater`)
  caldera_camera     the orbit camera is kept in front of the beacon tower (CameraCollide) and eases out again on the free side
  caldera_death      the hero dies: the game over screen comes after the slow motion fall
  caldera_save       the fire saves the game; the hero dies; Retry brings the saved state back, and so does Continue from the title
  caldera_boss       the Caldera Warden: a slam and a charge, a rest that is the sword's chance, and its death unlocks the last shard
  caldera_win        the six shards on the altar, the beacon, the cutscene, the end screen
  caldera_pause      Escape stops the world, the settings change the mouse, the volume and the language, Resume goes on

    python3 tools/make_caldera_tests.py        (needs target/release/lunaris for the path tool)
"""
import json
import math
import os
import subprocess
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
D = "examples/caldera"
TESTS = os.path.join(ROOT, "examples", "tests")
BIN = os.path.join(ROOT, "target", "release", "lunaris")
HERO = 100


def scene(name):
    with open(os.path.join(ROOT, D, f"{name}.lunaris.json"), encoding="utf-8") as f:
        return json.load(f)


ISLAND = scene("island")


def named(name, sc=None):
    for e in (sc or ISLAND)["entities"]:
        if e["components"].get("Name", {}).get("value") == name:
            return e["uid"], e["components"]
    raise SystemExit(f"no entity named {name}")


def pos_of(name):
    return named(name)[1]["Transform"]["pos"]


# ------------------------------------------------------------------------------------------------ the engine's path tool
def engine_calls(calls):
    p = subprocess.run([BIN, "serve", "--root", ROOT, "--no-gpu"], input="\n".join(json.dumps({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": c}) for i, c in enumerate(calls)) + "\n",
                       capture_output=True, text=True, cwd=ROOT, timeout=240)
    out = {}
    for line in p.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        for c in (m.get("result") or {}).get("content") or []:
            if c.get("type") == "text":
                try:
                    out[m["id"]] = json.loads(c["text"])
                except ValueError:
                    out[m["id"]] = {}
    return out


def way(a, b):
    out = engine_calls([{"name": "load_scene", "arguments": {"path": f"{D}/island.lunaris.json"}},
                        {"name": "path", "arguments": {"from": a, "to": b, "radius": 0.45, "height": 1.8, "step_height": 0.55}}])
    pts = (out.get(2) or {}).get("points")
    if not pts:
        raise SystemExit(f"no way from {a} to {b}: {out.get(2)}")
    return pts


# ------------------------------------------------------------------------------------------------ steps
def write(name, s):
    path = os.path.join(TESTS, f"caldera_{name}.test.json")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(s, f, indent=1)
        f.write("\n")
    print("wrote", os.path.relpath(path, ROOT), f"({len(s['steps'])} steps)")


def spec(title, scene_path, steps):
    return {"name": title, "scene": scene_path, "seed": 1, "replay_check": True, "steps": steps}


def press(key, hold=2, after=2):
    return [{"input": {key: True}}, {"step": hold}, {"input": {key: False}}, {"step": after}]


def put(uid, comp, value):
    return {"call": {"tool": "set", "args": {"uid": uid, "component": comp, "value": value}}}


def res(name, value):
    return {"call": {"tool": "set_resource", "args": {"name": name, "value": value}}}


def hero_to(x, y, z, face=(0, -1)):
    return [put(HERO, "Transform", {"pos": [round(x, 3), round(y, 3), round(z, 3)]}), put(HERO, "Hero", {"fx": face[0], "fz": face[1]}), {"step": 6}]


def expect(**kw):
    return {"expect": kw}


def stick(dx, dz):
    """The pad's stick that walks along the world direction (dx, dz) when the camera has not been turned (yaw 0: up is -z, right is +x)."""
    n = math.hypot(dx, dz) or 1.0
    return {"input": {"gamepad.lx": round(dx / n, 4), "gamepad.ly": round(-dz / n, 4)}}


def walk(points, chunk=11.0):
    """Steps that hold the stick towards each next place on the way until the hero's x or z has passed it (the axis it travels most along)."""
    steps = []
    for a, b in zip(points, points[1:]):
        dx, dz = b[0] - a[0], b[2] - a[2]
        length = math.hypot(dx, dz)
        n = max(1, math.ceil(length / chunk))
        for k in range(1, n + 1):
            tx, tz = a[0] + dx * k / n, a[2] + dz * k / n
            if abs(dx) >= abs(dz):
                field, cmp, target = "Transform.pos[0]", ("ge" if dx > 0 else "le"), tx
            else:
                field, cmp, target = "Transform.pos[2]", ("ge" if dz > 0 else "le"), tz
            steps += [stick(dx, dz), {"until": {"entity": HERO, "field": field, cmp: round(target, 3), "max_ticks": int(length / n / 3.0 * 60) + 240}}]
    return steps


def converge(tx, tz, slack=90):
    """Close in on a point from wherever the walk ended: walk towards -x until x is at or below it (nothing to do when it already is), then towards +x until x is at or above it, and the same in z.
    Two `until` steps in a row reach the point from either side, so the bot does not need to know which side of it the walk ended on."""
    out = []
    for axis, field, vec_lo, vec_hi in (("x", "Transform.pos[0]", (-1, 0), (1, 0)), ("z", "Transform.pos[2]", (0, -1), (0, 1))):
        t = tx if axis == "x" else tz
        out += [stick(*vec_lo), {"until": {"entity": HERO, "field": field, "le": round(t, 3), "max_ticks": slack}}, stick(*vec_hi), {"until": {"entity": HERO, "field": field, "ge": round(t, 3), "max_ticks": slack}}]
    return out


def set_hour(h):
    return put(3, "TimeOfDay", {"time": h})


# ------------------------------------------------------------------------------------------------ the tests
def t_title():
    steps = [{"until": {"exists": "Play button", "eq": True, "max_ticks": 40}}, {"step": 12},
             expect(exists="Music 1", eq=True, message="the music is a Persist entity of the boot scene"),
             expect(resource="music", eq=1, message="the title asks for the title tune"),
             *press("enter"), {"until": {"exists": "Hero", "eq": True, "max_ticks": 300}}, {"step": 20},
             expect(exists="Music 2", eq=True, message="the music went on into the island"),
             expect(name="Hero", field="HitPoints.value", eq=100), expect(resource="g.shards", eq=0), expect(resource="music", eq=2, message="and now plays the day's tune"),
             expect(check="errors", eq=0, message="the island passes check"),
             expect(count={"with": ["Shard"]}, eq=6, message="six ember shards on the island")]
    write("title", spec("Caldera: the boot scene leads to the title, Play (Enter) leads to the island with the music playing on", f"{D}/boot.lunaris.json", steps))


def t_walk():
    start = pos_of("Hero")
    s1 = pos_of("Shard 1")
    pts = way(start, [s1[0], s1[1] - 1.0, s1[2]])
    print(f"  the way to the first shard: {len(pts)} points, {sum(math.dist(a, b) for a, b in zip(pts, pts[1:])):.0f} m")
    steps = [{"step": 10}, expect(resource="g.shards", eq=0), expect(exists="Shard 1", eq=True)]
    steps += walk(pts)
    # the last metres: towards the shard from wherever the walk ended, until it is picked up (the pick-up reaches further than the walk's slack)
    steps += [stick(s1[0] - pts[-1][0], s1[2] - pts[-1][2]), {"until": {"resource": "g.shards", "ge": 1, "max_ticks": 240}}]
    steps += [stick(0, 0), {"step": 12}, expect(resource="g.shards", eq=1, message="the bot picked up the shard"), expect(resource="g.s1", eq=1), expect(exists="Shard 1", eq=False),
              expect(events="shard", ge=1), expect(check="errors", eq=0)]
    write("walk", spec(f"Caldera: a bot walks the computed path over the terrain ({len(pts) - 1} legs) from the beach to the first ember shard and takes it", f"{D}/island.lunaris.json", steps))


def t_combat():
    gid, g = named("Golem forest")
    gp = g["Transform"]["pos"]
    steps = [hero_to(gp[0] + 30, gp[1] + 1, gp[2], (-1, 0))[0], {"step": 40}, expect(name="Golem forest", field="Golem.mode", eq=0, message="far away the golem sleeps"),
             *hero_to(gp[0] + 2.2, gp[1] + 0.6, gp[2], (-1, 0)),
             {"until": {"name": "Golem forest", "field": "Golem.mode", "ge": 1, "max_ticks": 60}}, expect(events="golem_wake", ge=1, message="it wakes when the hero is close and in its sight"),
             {"until": {"name": "Golem forest", "field": "Golem.mode", "eq": 2, "max_ticks": 200}}]
    for k in range(7):
        steps += [put(HERO, "Hero", {"fx": -1, "fz": 0}), *press("mouse_left", 2, 44)]
        if k == 1:
            steps += [expect(name="Golem forest", field="HitPoints.value", lt=100, message="the sword hurts it")]
    steps += [{"until": {"exists": "Golem forest", "eq": False, "max_ticks": 400}}, {"step": 30},
              expect(resource="g.kills", eq=1), expect(events="golem_hit", ge=2), expect(events="golem_break", ge=1), expect(name="Hero", field="HitPoints.dead", eq=False, message="the hero stands"),
              expect(check="errors", eq=0)]
    write("combat", spec("Caldera: a sleeping golem wakes near the hero, hunts it and is brought down by the sword", f"{D}/island.lunaris.json", steps))


def t_night():
    steps = [{"step": 10}, expect(resource="night", eq=0, message="the game starts at dawn"), expect(name="Wraith 1", field="Wraith.state", eq=0, message="the wraiths wait at the bottom of the world"),
             set_hour(21.0), {"step": 4}, expect(resource="night", eq=1), {"until": {"name": "Wraith 1", "field": "Wraith.state", "eq": 1, "max_ticks": 60}},
             expect(events="wraith_rise", ge=1), {"until": {"name": "Hero", "field": "HitPoints.value", "lt": 100, "max_ticks": 900}}, expect(events="wraith_bite", ge=1, message="a wraith bit the hero")]
    # the fire sends them away
    cx, cy, cz = pos_of("Campfire")
    steps += [*hero_to(cx + 2.5, cy + 0.5, cz + 2.5), {"step": 120}, expect(name="Wraith 1", field="Wraith.state", eq=0, message="at the fire the wraiths go away"),
              expect(resource="dist.fire", lt=4), expect(check="errors", eq=0)]
    write("night", spec("Caldera: at night wraiths come out of the dark and bite; the fire sends them away", f"{D}/island.lunaris.json", steps))


def t_swim():
    lx, lz = -72.0, 44.0
    steps = [{"step": 10}, *hero_to(lx + 12, 14.0, lz + 6, (-1, 0)), {"step": 90}, expect(count={"with": ["InWater"]}, eq=1, message="the hero is in the lake"),
             stick(-1, -0.5), {"step": 100}, expect(name="Hero", field="Hero.swim", eq=1, message="in the deep it swims"), expect(resource="hero.depth", gt=1.2), expect(events="splash", ge=1),
             stick(0, 0), {"step": 20}, *hero_to(lx + 31, 20.0, lz + 12, (-1, 0)), {"step": 120}, expect(count={"with": ["InWater"]}, eq=0, message="and out again"), expect(name="Hero", field="Hero.swim", eq=0),
             expect(check="errors", eq=0)]
    write("swim", spec("Caldera: the hero wades into the lake, swims in the deep water and walks out again", f"{D}/island.lunaris.json", steps))


def t_camera():
    bx, by, bz = pos_of("Beacon tower")
    hx = bx + 4.6
    side = math.pi / 2
    steps = [{"step": 10}, *hero_to(hx, by + 1.6, bz, (1, 0)), {"step": 20},
             put(7, "CameraRig", {"yaw": round(-side, 4), "pitch": 0.25, "distance": 6.0}), {"step": 80},
             expect(name="Camera", field="Transform.pos[0]", ge=round(bx + 1.8, 3), message="the camera hangs west of the hero, where the beacon tower stands: it is kept in front of the tower"),
             expect(name="Camera", field="Transform.pos[0]", le=round(hx - 0.4, 3), message="and nearer to the hero than the tower's far side"),
             put(7, "CameraRig", {"yaw": round(side, 4)}), {"step": 120},
             expect(name="Camera", field="Transform.pos[0]", ge=round(hx + 4.5, 3), message="turned to the free side it eases out to its full distance again"),
             expect(check="errors", eq=0)]
    write("camera", spec("Caldera: the orbit camera is kept out of the beacon tower and eases out again where the way is free", f"{D}/island.lunaris.json", steps))


def t_death():
    steps = [{"step": 20}, put(HERO, "HitPoints", {"value": 0}), {"step": 3}, expect(name="Hero", field="HitPoints.dead", eq=True), expect(resource="dead", eq=1), expect(events="lose", ge=1),
             {"until": {"exists": "Retry button", "eq": True, "max_ticks": 600}}, {"step": 20}, expect(name="Retry button", field="Ui.visible", eq=True), expect(resource="music", eq=1, message="the game over screen has the title's tune")]
    write("death", spec("Caldera: when the hero is dead the world slows, the picture goes red and the game over screen follows", f"{D}/island.lunaris.json", steps))


def t_save():
    cx, cy, cz = pos_of("Campfire")
    steps = [{"step": 10}, res("g.dep", 3), res("g.s1", 1), res("g.s2", 1), res("g.s3", 1), *hero_to(cx + 2.5, cy + 0.5, cz + 2.5), {"step": 6}, expect(resource="prompt", eq=2, message="the fire offers to save"),
             *press("e"), {"step": 10}, expect(resource="game.saved", gt=0, message="E at the fire saves the game"), expect(events="save", ge=1),
             # away from the fire (which heals), a wound, then death: Retry goes back to the fire
             *hero_to(*pos_of("Hero")), put(HERO, "HitPoints", {"value": 0}), {"until": {"exists": "Retry button", "eq": True, "max_ticks": 700}}, {"step": 10}, res("g.dep", 0), res("g.s1", 0),
             {"input": {"enter": True}}, {"step": 3}, {"input": {"enter": False}}, {"until": {"exists": "Hero", "eq": True, "max_ticks": 120}}, {"step": 10},
             expect(resource="g.dep", eq=3, message="Retry loaded the saved run: three shards on the altar"), expect(name="Hero", field="HitPoints.dead", eq=False), expect(resource="g.s2", eq=1),
             # the title's Continue brings the same run back
             res("g.dep", 0), res("leave", 3), {"until": {"exists": "Play button", "eq": True, "max_ticks": 300}}, {"step": 20}, expect(name="Continue button", field="Ui.color[3]", gt=0.9, message="Continue is lit: there is a saved game"),
             *press("s"), *press("enter"), {"until": {"exists": "Hero", "eq": True, "max_ticks": 120}}, {"step": 10}, expect(resource="g.dep", eq=3, message="Continue brought it back"), expect(check="errors", eq=0)]
    write("save", spec("Caldera: the fire saves the game; Retry after a death and Continue from the title bring the run back", f"{D}/island.lunaris.json", steps))


def t_boss():
    bid, b = named("Caldera Warden")
    bp = b["Transform"]["pos"]
    steps = [put(HERO, "HitPoints", {"value": 3000, "max": 3000}), *hero_to(bp[0], bp[1] + 0.6, bp[2] + 12, (0, -1)), expect(name="Caldera Warden", field="Golem.mode", ge=0),
             {"until": {"name": "Caldera Warden", "field": "Golem.mode", "ge": 1, "max_ticks": 120}}, {"until": {"name": "Caldera Warden", "field": "Golem.attack", "eq": 2, "max_ticks": 900}}, {"step": 4},
             expect(events="golem_roar", ge=1, message="its first attack from a distance is a charge"),
             {"until": {"name": "Caldera Warden", "field": "Golem.mode", "eq": 7, "max_ticks": 2400}}, expect(events="boss_tired", ge=1, message="after three attacks it kneels: the sword's chance"),
             expect(name="Shard 6", field="Shard.locked", eq=1, message="its shard waits until it is beaten"), expect(events="boss_slam", ge=1, message="it slammed the ground")]
    # in its rest the blade does double damage: the Warden is put where its home was, the hero three metres south of it, and one blow is enough for what is left of it
    steps += [put(bid, "HitPoints", {"value": 60}), put(bid, "Transform", {"pos": [bp[0], bp[1], bp[2]]}), *hero_to(bp[0], bp[1] + 0.3, bp[2] + 3.0, (0, -1)),
              expect(name="Caldera Warden", field="Golem.mode", eq=7, message="still kneeling")]
    for k in range(3):
        steps += [put(HERO, "Hero", {"fx": 0, "fz": -1}), *press("mouse_left", 2, 44)]
    steps += [{"until": {"exists": "Caldera Warden", "eq": False, "max_ticks": 900}}, {"step": 10}, expect(resource="g.boss", eq=1, message="the Warden is beaten"),
              expect(name="Shard 6", field="Shard.locked", eq=0, message="and its shard is free"), expect(check="errors", eq=0)]
    write("boss", spec("Caldera: the Caldera Warden charges, slams and kneels; its defeat frees the last shard", f"{D}/island.lunaris.json", steps))


def t_win():
    bx, by, bz = pos_of("Beacon")
    ax, ay, az = pos_of("Altar")
    steps = [{"step": 10}, res("g.shards", 6), res("g.s1", 1), res("g.s2", 1), res("g.s3", 1), res("g.s4", 1), res("g.s5", 1), res("g.s6", 1),
             *hero_to(ax + 1.8, ay, az + 1.8), expect(resource="prompt", eq=3, message="at the altar with the shards"), *press("e"), {"step": 8},
             expect(resource="g.dep", eq=6), expect(resource="g.shards", eq=0), expect(name="Altar socket 6", field="Light.intensity", gt=0.5, message="all six sockets glow"),
             *hero_to(bx - 3.0, by - 7.0, bz + 3.0), expect(resource="prompt", eq=4, message="at the beacon: light it"), *press("e"), {"step": 30},
             expect(resource="cutscene", eq=1), expect(resource="g.lit", eq=1), expect(events="beacon", ge=1), expect(name="Beacon fire", field="Light.intensity", gt=1),
             {"until": {"exists": "Win title", "eq": True, "max_ticks": 1400}}, {"step": 30}, expect(resource="g.lit", eq=1, message="the end screen knows the beacon burns"), expect(events="win", ge=1)]
    write("win", spec("Caldera: six shards on the altar, the beacon is lit, the cutscene plays and the end screen follows", f"{D}/island.lunaris.json", steps))


def t_talk():
    nx, ny, nz = pos_of("Maren")
    steps = [{"step": 10}, *hero_to(nx + 2.2, ny + 0.4, nz + 1.0, (-1, 0)), expect(resource="prompt", eq=1, message="next to Maren the prompt says talk"), expect(name="Choice 1", field="Ui.visible", eq=False),
             *press("e"), {"step": 6}, expect(resource="talking", eq=1, message="E starts the conversation"), expect(events="select", ge=1), expect(name="Dialogue text", field="Ui.visible", eq=True),
             expect(name="Hero", field="CharacterBody.walk[0]", eq=0, message="the hero stands still while it talks"),
             *press("enter"), {"step": 4}, expect(name="Choice 1", field="Ui.visible", eq=True, message="after the greeting four things to ask"), expect(name="Choice 4", field="Ui.visible", eq=True),
             *press("2"), {"step": 4}, *press("enter"), {"step": 4}, *press("2"), {"step": 4}, *press("enter"), {"step": 4}, *press("4"), {"step": 8},
             expect(resource="talking", eq=0, message="Farewell ends it"), expect(name="Dialogue text", field="Ui.visible", eq=False), expect(check="errors", eq=0)]
    write("talk", spec("Caldera: E next to Maren starts a conversation (typed with the keys 1 to 4 and Enter); asking where the shards are, then farewell ends it", f"{D}/island.lunaris.json", steps))


def t_pause():
    steps = [{"step": 20}, *press("escape"), {"step": 6}, expect(resource="paused", eq=1), expect(resource="time.scale", eq=0), expect(name="Resume", field="Ui.visible", eq=True),
             {"step": 30}, expect(resource="cursor.lock", eq=0, message="the mouse is let go in the menu"),
             *press("s"), *press("enter"), {"step": 4}, expect(resource="settings", eq=1), expect(name="Setting slider 1", field="Ui.visible", eq=True),
             expect(resource="opt.sens", eq=1.0), *press("a", 3, 3), *press("a", 3, 3), *press("a", 3, 3), expect(resource="opt.sens", lt=0.99, message="the left arrow lowers the sensitivity of the mouse"),
             *press("s"), *press("s"), *press("s"), *press("enter"), {"step": 4}, expect(resource="language.index", eq=1, message="the language button picks German"),
             expect(name="Localization", field="Localization.language", eq="examples/caldera/lang/de"),
             *press("s"), *press("enter"), {"step": 4}, expect(resource="settings", eq=0, message="Back closes the settings"),
             *press("escape"), {"step": 4}, expect(resource="paused", eq=0), expect(resource="time.scale", eq=1), expect(check="errors", eq=0)]
    write("pause", spec("Caldera: Escape pauses and unlocks the mouse; the settings change the mouse, the volume and the language; Escape goes on", f"{D}/island.lunaris.json", steps))


def main():
    only = sys.argv[1:]
    for name, fn in (("title", t_title), ("walk", t_walk), ("combat", t_combat), ("night", t_night), ("swim", t_swim), ("camera", t_camera), ("death", t_death), ("save", t_save), ("boss", t_boss), ("win", t_win), ("talk", t_talk), ("pause", t_pause)):
        if not only or name in only:
            fn()


if __name__ == "__main__":
    main()
