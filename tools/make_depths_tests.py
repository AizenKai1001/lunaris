"""Writes the playtests of "Depths" (examples/tests/depths_*.test.json) from the scenes `tools/build_depths.py` made: the scenes are read as data, and the keys a player would press
are worked out from them, as `tools/make_dungeon_test.py` does for the dungeon.

  depths_boot          the start scene hands over to the menu; the music (a `Persist` entity) is still there in level 1 and has changed track
  depths_menu          keyboard: move the focus with W/S, Enter plays; the run starts with full health, no coins, a fade in
  depths_menu_mouse    mouse: Credits opens and Back closes, Quit asks the player to close, Play starts
  depths_level1/2/3    a bot walks the shortest ways (start -> key, key -> door -> stairs: a breadth-first search over the tiles) and takes the stairs: level 1 leads to 2, 2 to 3,
                       3 to the win screen; health, coins and keys carry over; the locked door stops a hero without a key
  depths_combat        the sword hurts a guard and kills it for points
  depths_guards        guards that see the hero hunt it down: game over; Retry starts the level again with the health it began with
  depths_pause         Esc (and the pad's Start) stop time, the menu comes up, Resume goes on, Main menu goes back to the title
  depths_save          the autosave of every level, and Continue brings back the level, the health, the coins and the score
  depths_win           the win screen shows the rank of the score; Play again starts a new run

    python -I tools/make_depths_tests.py
"""
import collections
import json
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
D = "examples/depths"
TESTS = os.path.join(ROOT, "examples", "tests")
HERO = 1000


def scene(name):
    with open(os.path.join(ROOT, D, f"{name}.lunaris.json"), encoding="utf-8") as f:
        return json.load(f)


def comps(sc, uid):
    for e in sc["entities"]:
        if e["uid"] == uid:
            return e["components"]
    raise SystemExit(f"no entity {uid}")


def named(sc, name):
    for e in sc["entities"]:
        if e["components"].get("Name", {}).get("value") == name:
            return e["uid"], e["components"]
    raise SystemExit(f"no entity named {name}")


class Level:
    def __init__(self, n):
        self.n = n
        self.sc = scene(f"level{n}")
        tm = comps(self.sc, 10)["Tilemap"]
        self.w, self.h = tm["size"]
        rows = [[int(v) for v in line.split()] for line in tm["data"].strip().split("\n")]
        self.floor = [[v < 3 for v in row] for row in rows]
        self.hero = comps(self.sc, HERO)["Transform"]["pos"]
        self.exit = named(self.sc, "Exit")[1]["Transform"]["pos"]
        self.door = named(self.sc, "Door")[1]["Transform"]["pos"]
        self.key = named(self.sc, "Key")[1]["Transform"]["pos"]
        self.guards = [e["uid"] for e in self.sc["entities"] if "Enemy" in e["components"]]
        self.start_cell, self.exit_cell, self.door_cell, self.key_cell = (self.cell(p) for p in (self.hero, self.exit, self.door, self.key))

    def cell(self, pos):
        return int(pos[0] + self.w / 2), int(self.h / 2 - pos[1])

    def world(self, cell):
        return [cell[0] + 0.5 - self.w / 2, self.h / 2 - cell[1] - 0.5]

    def path(self, a, b, blocked=()):
        came = {a: None}
        q = collections.deque([a])
        while q:
            c = q.popleft()
            if c == b:
                break
            for key, (dx, dy) in (("d", (1, 0)), ("a", (-1, 0)), ("s", (0, 1)), ("w", (0, -1))):
                n = (c[0] + dx, c[1] + dy)
                if 0 <= n[0] < self.w and 0 <= n[1] < self.h and self.floor[n[1]][n[0]] and n not in came and n not in blocked:
                    came[n] = (c, key)
                    q.append(n)
        if b not in came:
            raise SystemExit(f"level {self.n}: no way from {a} to {b}")
        cells, c = [b], b
        while came[c] is not None:
            c = came[c][0]
            cells.append(c)
        return cells[::-1]

    def walk(self, cells, to_the_stairs=False):
        """Steps that hold one key at a time until the hero has reached the middle of the last cell of each straight stretch (the last one, when it ends on the stairs, until
        the hero has touched them: it stops there, the level is over)."""
        steps = []
        i = 0
        while i < len(cells) - 1:
            dx, dy = cells[i + 1][0] - cells[i][0], cells[i + 1][1] - cells[i][1]
            j = i + 1
            while j + 1 < len(cells) and (cells[j + 1][0] - cells[j][0], cells[j + 1][1] - cells[j][1]) == (dx, dy):
                j += 1
            key = {(1, 0): "d", (-1, 0): "a", (0, 1): "s", (0, -1): "w"}[(dx, dy)]
            tx, ty = self.world(cells[j])
            field, cmp, target = {"d": ("Transform.pos[0]", "ge", tx), "a": ("Transform.pos[0]", "le", tx), "s": ("Transform.pos[1]", "le", ty), "w": ("Transform.pos[1]", "ge", ty)}[key]
            goal = {"resource": "leave", "gt": 0} if (to_the_stairs and j == len(cells) - 1) else {"entity": HERO, "field": field, cmp: round(target, 3)}
            steps += [{"input": {key: True}}, {"until": {**goal, "max_ticks": 14 * (j - i) + 120}}, {"input": {key: False}}]
            i = j
        return steps


def spec(name, scene_path, steps, title):
    return {"name": title, "scene": scene_path, "seed": 1, "replay_check": True, "steps": steps}


def write(name, s):
    path = os.path.join(TESTS, f"depths_{name}.test.json")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(s, f, indent=2)
        f.write("\n")
    print("wrote", os.path.relpath(path, ROOT), f"({len(s['steps'])} steps)")


def press(key, hold=2, after=2):
    return [{"input": {key: True}}, {"step": hold}, {"input": {key: False}}, {"step": after}]


def click(x, y):
    return [{"input": {"mouse_x": round(x, 4), "mouse_y": round(y, 4)}}, {"step": 2}, {"input": {"mouse_left": True}}, {"step": 3}, {"input": {"mouse_left": False}}, {"step": 3}]


def centre(sc, name):
    ui = named(sc, name)[1]["Ui"]
    return ui["pos"][0] + ui["size"][0] / 2, ui["pos"][1] + ui["size"][1] / 2


# ------------------------------------------------------------------------------------------------ the tests
def level_steps(n, last=False, first=True):
    """What a bot does in level `n` (its scene is loaded and its first tick has run): through the locked door's refusal, to the key, through the door and to the stairs.
    `first` is false when the level was reached by playing (the health is not the full 100 then). Returns the steps and what the walk was."""
    lv = Level(n)
    to_key = lv.path(lv.start_cell, lv.key_cell, blocked={lv.door_cell})
    to_exit = lv.path(lv.key_cell, lv.exit_cell)
    before_door = lv.path(lv.start_cell, lv.door_cell, blocked=set())[-2]
    steps = [{"expect": {"check": "errors", "eq": 0}}, {"step": 40}] if first else [{"step": 40}]
    steps += [{"expect": {"resource": "level", "eq": n, "message": "the level says which one it is"}},
              {"expect": {"resource": "game.saved", "gt": 0, "message": "the level saved the game when it began"}}]
    if first:
        steps += [{"expect": {"name": "Hero", "field": "HitPoints.value", "eq": 100}}]
    # the guards are about to be tested on their own (depths_guards): here they are sent home so that they do not block the corridors
    steps += [{"call": {"tool": "despawn", "args": {"uid": uid}}} for uid in lv.guards]
    # the locked door stops a hero that has no key: stand in front of it and walk into it
    wx, wy = lv.world(before_door)
    toward = {(1, 0): "d", (-1, 0): "a", (0, 1): "s", (0, -1): "w"}[(lv.door_cell[0] - before_door[0], lv.door_cell[1] - before_door[1])]
    steps += [{"call": {"tool": "set", "args": {"uid": HERO, "component": "Transform", "value": {"pos": [round(wx, 3), round(wy, 3), 0]}}}}, {"step": 2},
              {"input": {toward: True}}, {"step": 45}, {"input": {toward: False}},
              {"expect": {"exists": "Door", "eq": True, "message": "the door is locked: a hero without a key cannot open it"}},
              {"expect": {"resource": "keys", "eq": 0}}]
    sx, sy = lv.world(lv.start_cell)
    steps += [{"call": {"tool": "set", "args": {"uid": HERO, "component": "Transform", "value": {"pos": [round(sx, 3), round(sy, 3), 0]}}}}, {"step": 3}]
    steps += lv.walk(to_key)
    steps += [{"step": 3}, {"expect": {"resource": "keys", "eq": 1, "message": "the hero found the key"}}, {"expect": {"exists": "Key", "eq": False}}]
    steps += lv.walk(to_exit[: to_exit.index(lv.door_cell) + 1])
    steps += [{"step": 10}, {"expect": {"exists": "Door", "eq": False, "message": "the key opened the door"}}, {"expect": {"resource": "keys", "eq": 0, "message": "and was used up"}},
              {"expect": {"check": "errors", "eq": 0, "message": "no graph system has failed while playing (check reports the last error of each)"}}]
    # a wound just before the stairs (the health heals again 6 seconds after a wound, so the last stretch must be shorter than that): it must come with the hero into the next scene
    final = lv.walk(to_exit[to_exit.index(lv.door_cell):], to_the_stairs=True)
    final.insert(len(final) - 3, {"call": {"tool": "set", "args": {"uid": HERO, "component": "HitPoints", "value": {"value": 63}}}})
    steps += final
    steps += [{"expect": {"resource": "score", "ge": 150, "message": "key, door and stairs are worth points"}}]
    if last:
        steps += [{"until": {"exists": "Win title", "eq": True, "max_ticks": 240}}, {"step": 5},
                  {"expect": {"resource": "hp", "between": [60, 66], "message": "the health arrives at the win screen"}},
                  {"expect": {"resource": "score", "ge": 150, "message": "and the score"}}]
        what = "the last level ends in the win screen"
    else:
        steps += [{"until": {"exists": f"Level {n + 1}", "eq": True, "max_ticks": 240}}, {"step": 5},
                  {"expect": {"resource": "level", "eq": n + 1, "message": "the stairs lead to the next level"}},
                  {"expect": {"name": "Hero", "field": "HitPoints.value", "between": [62, 68], "message": "the hero arrives with the health it had"}},
                  {"expect": {"resource": "score", "ge": 150, "message": "and the score"}},
                  {"expect": {"resource": "game.saved", "gt": 0}}]
        what = f"the stairs lead to level {n + 1}"
    return steps, f"a locked door stops the hero; it walks to the key ({len(to_key) - 1} cells), through the door and to the stairs ({len(to_exit) - 1} cells); {what}"


def level_test(n, last=False):
    steps, what = level_steps(n, last)
    write(f"level{n}", spec(f"level{n}", f"{D}/level{n}.lunaris.json", steps, f"Depths level {n} ({['rooms', 'caves', 'maze'][n - 1]}): {what}"))


def full_test():
    """The whole game, played by a bot from the title: Play, level 1, level 2, level 3, the win screen; the score only goes up, and the run ends in the win screen."""
    steps = [{"step": 4}, {"until": {"exists": "Play button", "eq": True, "max_ticks": 60}}, {"step": 5}, *press("enter"),
             {"until": {"exists": "Level 1", "eq": True, "max_ticks": 240}}, {"step": 3}]
    for n in (1, 2, 3):
        more, _ = level_steps(n, last=(n == 3), first=False)
        steps += more
    steps += [{"expect": {"resource": "level", "eq": 3}}, {"expect": {"resource": "score", "ge": 450, "message": "three levels of keys, doors and stairs"}},
              {"expect": {"name": "Final score", "field": "Text.visible", "eq": True}}, {"expect": {"exists": "Theme music", "eq": True, "message": "the music has played through every scene"}},
              *press("enter"), {"until": {"exists": "Level 1", "eq": True, "max_ticks": 240}}, {"step": 4},
              {"expect": {"resource": "score", "eq": 0, "message": "Play again on the win screen starts a new run"}}]
    write("full", spec("full", f"{D}/boot.lunaris.json", steps, "Depths from the start to the end: the boot scene, the title, three levels played by a bot (key, door, stairs) and the win screen; Play again starts over"))


def combat_test():
    lv = Level(1)
    hx, hy = lv.hero[0], lv.hero[1]
    # the hero faces the first free side (the resource `face` is the way it last walked), the guard is put one cell that way
    fx, fy = next((dx, -dy) for dx, dy in ((0, 1), (1, 0), (-1, 0), (0, -1)) if lv.floor[lv.start_cell[1] + dy][lv.start_cell[0] + dx])
    steps = [{"call": {"tool": "set_resource", "args": {"name": "face.x", "value": fx}}}, {"call": {"tool": "set_resource", "args": {"name": "face.y", "value": fy}}}, {"step": 3}]
    steps += [{"call": {"tool": "despawn", "args": {"uid": uid}}} for uid in lv.guards[1:]]
    place = {"call": {"tool": "set", "args": {"uid": lv.guards[0], "component": "Transform", "value": {"pos": [round(hx + fx, 3), round(hy + fy, 3), 0]}}}}
    stop = {"call": {"tool": "set", "args": {"uid": lv.guards[0], "component": "Velocity", "value": {"lin": [0, 0, 0]}}}}
    steps += [place, stop, {"step": 1}, *press("space", 2, 4),
              {"expect": {"entity": lv.guards[0], "field": "HitPoints.value", "lt": 40, "message": "the sword hurt the guard"}},
              {"expect": {"events": "swing", "ge": 1}}, {"expect": {"events": "hit", "ge": 1}}]
    steps += [{"step": 22}, place, stop, {"step": 1}, *press("space", 2, 4), {"step": 6}]
    steps += [{"expect": {"exists": lv.guards[0], "eq": False, "message": "two blows killed the guard"}}, {"expect": {"resource": "kills", "eq": 1}},
              {"expect": {"resource": "score", "eq": 50, "message": "a grunt is worth 50 points"}}, {"expect": {"events": "kill", "ge": 1}}]
    write("combat", spec("combat", f"{D}/level1.lunaris.json", steps, "Depths combat: Space swings the sword, a guard in front of the hero is hurt, knocked back and killed for points"))


def guards_test():
    lv = Level(1)
    # three guards on the floor cells nearest to the hero, so that they see it at once
    near = [c for c in sorted((c for c in lv.path(lv.start_cell, lv.exit_cell)), key=lambda c: abs(c[0] - lv.start_cell[0]) + abs(c[1] - lv.start_cell[1])) if c != lv.start_cell][:2]
    cells = near + [(lv.start_cell[0] + dx, lv.start_cell[1] + dy) for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)) if lv.floor[lv.start_cell[1] + dy][lv.start_cell[0] + dx]]
    cells = list(dict.fromkeys(cells))[:3]
    steps = [{"step": 3}]
    for uid, c in zip(lv.guards[:3], cells):
        x, y = lv.world(c)
        steps.append({"call": {"tool": "set", "args": {"uid": uid, "component": "Transform", "value": {"pos": [round(x, 3), round(y, 3), 0]}}}})
    steps += [{"step": 12}, {"expect": {"entity": lv.guards[0], "field": "Enemy.alert", "gt": 0, "message": "a guard that sees the hero is alerted (Sensor with line of sight)"}},
              {"until": {"name": "Hero", "field": "HitPoints.dead", "eq": True, "max_ticks": 1800}},
              {"expect": {"events": "hurt", "ge": 3, "message": "the guards bit the hero again and again"}},
              {"until": {"exists": "Game over title", "eq": True, "max_ticks": 240}}, {"step": 40},
              {"expect": {"resource": "level", "eq": 1, "message": "the game over screen knows the level"}},
              {"expect": {"exists": "Hero", "eq": False}},
              *press("enter", 2, 2),
              {"until": {"exists": "Level 1", "eq": True, "max_ticks": 240}}, {"step": 5},
              {"expect": {"name": "Hero", "field": "HitPoints.value", "eq": 100, "message": "Retry starts the level again with the health it began with"}},
              {"expect": {"resource": "hp", "eq": 100}}]
    write("guards", spec("guards", f"{D}/level1.lunaris.json", steps, "Depths guards: guards that see the hero hunt it down and kill it (game over), Retry starts the level again with full health"))


def menu_test():
    sc = scene("menu")
    steps = [{"expect": {"check": "errors", "eq": 0}}, {"step": 10},
             {"expect": {"name": "Nav", "field": "UiNav.current", "eq": 1301, "message": "the focus starts on Play"}},
             *press("s"), {"expect": {"name": "Nav", "field": "UiNav.current", "eq": 1302, "message": "S moves the focus down to Continue"}},
             *press("arrowdown"), {"expect": {"name": "Nav", "field": "UiNav.current", "eq": 1303}},
             *press("w"), *press("arrowup"), {"expect": {"name": "Nav", "field": "UiNav.current", "eq": 1301, "message": "W and the arrow go back up"}},
             {"expect": {"name": "Continue button", "field": "Ui.hover", "eq": False}},
             {"call": {"tool": "set_resource", "args": {"name": "coins", "value": 7}}},
             *press("enter"),
             {"expect": {"resource": "leave", "eq": 1, "message": "Enter on Play starts the run"}},
             {"until": {"exists": "Level 1", "eq": True, "max_ticks": 240}}, {"step": 4},
             {"expect": {"resource": "level", "eq": 1}}, {"expect": {"resource": "coins", "eq": 0, "message": "a new run starts with no coins"}},
             {"expect": {"name": "Hero", "field": "HitPoints.value", "eq": 100}}, {"expect": {"resource": "score", "eq": 0}},
             {"expect": {"name": "Fader", "field": "ColorGrade.fade[3]", "gt": 0.5, "message": "the new level fades in from black"}},
             {"step": 60}, {"expect": {"name": "Fader", "field": "ColorGrade.fade[3]", "eq": 0}}]
    write("menu", spec("menu", f"{D}/menu.lunaris.json", steps, "Depths main menu with the keyboard: W/S and the arrows move the focus (UiNav), Enter on Play fades out and starts level 1 with a fresh run"))
    px, py = centre(sc, "Play button")
    cx, cy = centre(sc, "Credits button")
    qx, qy = centre(sc, "Quit button")
    bx, by = centre(sc, "Back button")
    steps = [{"step": 10}, *click(cx, cy), {"expect": {"resource": "credits", "eq": 1, "message": "a click on Credits opens them"}},
             {"expect": {"name": "Credits title", "field": "Text.visible", "eq": True}}, {"expect": {"name": "Play button", "field": "Ui.visible", "eq": False}},
             *click(bx, by), {"expect": {"resource": "credits", "eq": 0, "message": "Back closes them"}}, {"expect": {"name": "Play button", "field": "Ui.visible", "eq": True}},
             {"expect": {"resource": "app.quit", "eq": 0}}, *click(qx, qy), {"expect": {"resource": "app.quit", "eq": 1, "message": "Quit asks the player program to close"}},
             {"call": {"tool": "set_resource", "args": {"name": "app.quit", "value": 0}}},
             *click(px, py), {"expect": {"resource": "leave", "eq": 1, "message": "a click on Play starts the run"}},
             {"until": {"exists": "Level 1", "eq": True, "max_ticks": 240}}]
    write("menu_mouse", spec("menu_mouse", f"{D}/menu.lunaris.json", steps, "Depths main menu with the mouse: Credits opens and Back closes, Quit asks the player to close, Play starts level 1"))


def boot_test():
    steps = [{"step": 4}, {"until": {"exists": "Play button", "eq": True, "max_ticks": 60}}, {"step": 2},
             {"expect": {"exists": "To the menu", "eq": False, "message": "the boot scene is gone"}},
             {"expect": {"exists": "Theme music", "eq": True, "message": "the music (Persist) stayed through the scene change"}},
             {"expect": {"name": "Theme music", "field": "MusicPlayer.playing", "eq": True}}, {"expect": {"name": "Depths music", "field": "MusicPlayer.playing", "eq": False}},
             *press("enter"), {"until": {"exists": "Level 1", "eq": True, "max_ticks": 240}}, {"step": 4},
             {"expect": {"exists": "Theme music", "eq": True, "message": "and through the next one"}},
             {"expect": {"name": "Depths music", "field": "MusicPlayer.playing", "eq": True, "message": "the dungeon's track takes over in a level"}},
             {"expect": {"name": "Theme music", "field": "MusicPlayer.playing", "eq": False}}]
    write("boot", spec("boot", f"{D}/boot.lunaris.json", steps, "Depths start: the boot scene switches to the menu and the music (Persist) outlives every scene change, handing over to the dungeon's track"))


def pause_test():
    lv = Level(1)
    cells = lv.path(lv.start_cell, lv.exit_cell)
    d = (cells[1][0] - cells[0][0], cells[1][1] - cells[0][1])
    key = {(1, 0): "d", (-1, 0): "a", (0, 1): "s", (0, -1): "w"}[d]
    sx, sy = (round(v, 3) for v in lv.world(lv.start_cell))
    axis, sign = ("Transform.pos[0]", d[0]) if d[0] else ("Transform.pos[1]", -d[1])
    home = {"call": {"tool": "set", "args": {"uid": HERO, "component": "Transform", "value": {"pos": [sx, sy, 0]}}}}
    moved = {"gt": (sx if d[0] else sy) + 0.4} if sign > 0 else {"lt": (sx if d[0] else sy) - 0.4}
    steps = [{"step": 30}, {"expect": {"resource": "paused", "eq": 0}}, *press("escape", 2, 3),
             {"expect": {"resource": "paused", "eq": 1, "message": "Esc pauses"}}, {"expect": {"resource": "time.scale", "eq": 0, "message": "and stops time"}},
             {"expect": {"name": "Resume", "field": "Ui.visible", "eq": True, "message": "the pause menu comes up"}}, {"expect": {"name": "Nav", "field": "UiNav.current", "eq": 1201, "message": "with the focus on Resume"}},
             home, {"step": 2}, {"input": {key: True}}, {"step": 40}, {"input": {key: False}},
             {"expect": {"name": "Hero", "field": axis, "approx": [(sx if d[0] else sy), 0.01], "message": "time stands still: holding a key moves nothing"}},
             {"expect": {"tick": True, "gt": 70, "message": "(the clock of the interface goes on)"}},
             *press("escape", 2, 3), {"expect": {"resource": "paused", "eq": 0, "message": "a second Esc goes on"}}, {"expect": {"resource": "time.scale", "eq": 1}},
             {"expect": {"name": "Resume", "field": "Ui.visible", "eq": False}},
             {"input": {key: True}}, {"step": 12}, {"input": {key: False}}, {"expect": {"name": "Hero", "field": axis, **moved, "message": "and the hero walks again"}},
             # the pad's Start button pauses too
             {"input": {"gamepad.start": True}}, {"step": 2}, {"input": {"gamepad.start": False}}, {"step": 3},
             {"expect": {"resource": "paused", "eq": 1, "message": "Start on a pad pauses"}},
             *press("s"), {"expect": {"name": "Nav", "field": "UiNav.current", "eq": 1202, "message": "S moves the focus to Main menu"}},
             *press("enter"),
             {"until": {"exists": "Play button", "eq": True, "max_ticks": 240}}, {"step": 3},
             {"expect": {"resource": "time.scale", "eq": 1, "message": "back at the title, time runs again"}}, {"expect": {"resource": "paused", "eq": 0}}]
    write("pause", spec("pause", f"{D}/level1.lunaris.json", steps, "Depths pause: Esc (or the pad's Start) stops time (time.scale 0) and shows the menu, Esc again resumes, Main menu goes back to the title"))


def save_test():
    steps = [{"step": 5}, {"expect": {"resource": "game.saved", "gt": 0, "message": "level 1 saved the game when it began"}},
             {"call": {"tool": "set", "args": {"uid": HERO, "component": "HitPoints", "value": {"value": 63}}}}, {"call": {"tool": "set_resource", "args": {"name": "coins", "value": 5}}},
             {"call": {"tool": "set_resource", "args": {"name": "score", "value": 250}}}, {"step": 2},
             {"call": {"tool": "set", "args": {"uid": 901, "component": "SceneSwitch", "value": {"go": True}}}},
             {"until": {"exists": "Level 2", "eq": True, "max_ticks": 30}}, {"step": 8},
             {"expect": {"resource": "level", "eq": 2}}, {"expect": {"name": "Hero", "field": "HitPoints.value", "between": [62, 66], "message": "level 2 starts with the health the hero had"}},
             {"expect": {"resource": "game.saved", "gt": 7, "message": "and saved the game again"}},
             # leave for the title and forget everything, as a new run of the program would
             {"call": {"tool": "set", "args": {"uid": 903, "component": "SceneSwitch", "value": {"go": True}}}},
             {"until": {"exists": "Play button", "eq": True, "max_ticks": 30}}, {"step": 5},
             {"call": {"tool": "set_resource", "args": {"name": "hp", "value": 100}}}, {"call": {"tool": "set_resource", "args": {"name": "coins", "value": 0}}},
             {"call": {"tool": "set_resource", "args": {"name": "score", "value": 0}}}, {"call": {"tool": "set_resource", "args": {"name": "level", "value": 1}}}, {"step": 3},
             {"expect": {"resource": "game.has.1", "eq": 1, "message": "the title knows there is a saved game"}},
             {"expect": {"name": "Continue button", "field": "Ui.color[3]", "gt": 0.9, "message": "so Continue is lit"}},
             *press("s"), {"expect": {"name": "Nav", "field": "UiNav.current", "eq": 1302}}, *press("enter"),
             {"until": {"exists": "Level 2", "eq": True, "max_ticks": 60}}, {"step": 3},
             {"expect": {"resource": "game.loaded", "gt": 0, "message": "Continue loaded the saved game"}},
             {"expect": {"resource": "level", "eq": 2, "message": "the level comes back"}}, {"expect": {"resource": "coins", "eq": 5, "message": "the coins"}},
             {"expect": {"resource": "score", "eq": 250, "message": "the score"}}, {"expect": {"name": "Hero", "field": "HitPoints.value", "between": [62, 66], "message": "and the health"}}]
    write("save", spec("save", f"{D}/level1.lunaris.json", steps, "Depths saving: every level autosaves to slot 1 when it begins; from the title, Continue brings back the level, the health, the coins and the score"))


def win_test():
    steps = [{"step": 5}, {"call": {"tool": "set_resource", "args": {"name": "score", "value": 1700}}}, {"step": 2},
             {"expect": {"name": "Rank S", "field": "Text.visible", "eq": True, "message": "1700 points is rank S"}}, {"expect": {"name": "Rank A", "field": "Text.visible", "eq": False}},
             {"call": {"tool": "set_resource", "args": {"name": "score", "value": 700}}}, {"step": 2},
             {"expect": {"name": "Rank B", "field": "Text.visible", "eq": True, "message": "700 is rank B"}}, {"expect": {"name": "Rank S", "field": "Text.visible", "eq": False}},
             {"call": {"tool": "set_resource", "args": {"name": "score", "value": 120}}}, {"step": 2}, {"expect": {"name": "Rank C", "field": "Text.visible", "eq": True}},
             *press("enter"),
             {"until": {"exists": "Level 1", "eq": True, "max_ticks": 240}}, {"step": 4},
             {"expect": {"resource": "score", "eq": 0, "message": "Play again starts a fresh run"}}, {"expect": {"resource": "level", "eq": 1}},
             {"expect": {"name": "Hero", "field": "HitPoints.value", "eq": 100}}]
    write("win", spec("win", f"{D}/win.lunaris.json", steps, "Depths win screen: the rank follows the score, Play again starts a fresh run at level 1"))


def dialogue_test():
    lv = Level(1)
    _, w = named(lv.sc, "Warden")
    wx, wy = (round(v, 3) for v in w["Transform"]["pos"][:2])

    def say():
        """One line of the talk: wait for it to type out, then go on."""
        return [{"step": 130}, *press("space", 2, 3)]

    steps = [{"step": 3}] + [{"call": {"tool": "despawn", "args": {"uid": uid}}} for uid in lv.guards]
    steps += [{"expect": {"resource": "near", "eq": 0, "message": "far from the warden there is nobody to talk to"}},
              {"call": {"tool": "set", "args": {"uid": HERO, "component": "Transform", "value": {"pos": [wx, wy, 0]}}}}, {"step": 4},
              {"expect": {"resource": "near", "eq": 1, "message": "the hero's Sensor sees the warden"}}, {"expect": {"name": "Talk hint", "field": "Text.visible", "eq": True}},
              *press("e"), {"step": 3},
              {"expect": {"resource": "talking", "eq": 1, "message": "E starts the Dialogue"}}, {"expect": {"name": "Talk hint", "field": "Text.visible", "eq": False}},
              {"expect": {"name": "Dialogue text", "field": "Ui.visible", "eq": True}},
              {"input": {"d": True}}, {"step": 20}, {"input": {"d": False}},
              {"expect": {"name": "Hero", "field": "Transform.pos[0]", "approx": [wx, 0.05], "message": "the hero stands still while it listens"}},
              *say(), *say(), {"step": 30},
              {"expect": {"name": "Choice 1", "field": "Ui.visible", "eq": True, "message": "two lines, then a choice"}},
              *press("1"), {"step": 3}, *say(),
              {"expect": {"resource": "talking", "eq": 0, "message": "the talk is over"}}, {"expect": {"resource": "score", "eq": 20, "message": "the warden paid 20"}}, {"expect": {"resource": "gift", "eq": 1}},
              # a second talk gives nothing more
              *press("e"), {"step": 3}, *say(), *say(), {"step": 30}, *press("1"), {"step": 3}, *say(),
              {"expect": {"resource": "talking", "eq": 0}}, {"expect": {"resource": "score", "eq": 20, "message": "he gives nothing twice"}}]
    write("dialogue", spec("dialogue", f"{D}/level1.lunaris.json", steps, "Depths dialogue: the old warden of level 1 talks when E is pressed near him (Dialogue, Sensor), the hero listens, a choice pays 20 once"))


def pad_test():
    lv = Level(1)
    cells = lv.path(lv.start_cell, lv.exit_cell)
    d = (cells[1][0] - cells[0][0], cells[1][1] - cells[0][1])
    sx, sy = lv.world(lv.start_cell)
    axis, lx_ly, sign = ("Transform.pos[0]", "gamepad.lx", d[0]) if d[0] else ("Transform.pos[1]", "gamepad.ly", -d[1])
    start = sx if d[0] else sy
    moved = {"gt": start + 0.5} if sign > 0 else {"lt": start - 0.5}
    steps = [{"step": 10}, {"expect": {"name": "Nav", "field": "UiNav.current", "eq": 1301}},
             {"input": {"gamepad.dpad_down": True}}, {"step": 2}, {"input": {"gamepad.dpad_down": False}}, {"step": 2},
             {"expect": {"name": "Nav", "field": "UiNav.current", "eq": 1302, "message": "the d-pad moves the focus"}},
             {"input": {"gamepad.dpad_up": True}}, {"step": 2}, {"input": {"gamepad.dpad_up": False}}, {"step": 2},
             {"expect": {"name": "Nav", "field": "UiNav.current", "eq": 1301}},
             {"input": {"gamepad.a": True}}, {"step": 2}, {"input": {"gamepad.a": False}},
             {"until": {"exists": "Level 1", "eq": True, "max_ticks": 240}}, {"step": 40},
             {"expect": {"name": "Hero", "field": "HitPoints.value", "eq": 100}}]
    steps += [{"call": {"tool": "despawn", "args": {"uid": uid}}} for uid in lv.guards]
    steps += [{"input": {lx_ly: float(sign)}}, {"step": 20}, {"input": {lx_ly: 0}}, {"step": 4},
              {"expect": {"name": "Hero", "field": axis, **moved, "message": "the left stick walks the hero"}},
              {"input": {"gamepad.x": True}}, {"step": 2}, {"input": {"gamepad.x": False}}, {"step": 3},
              {"expect": {"events": "swing", "ge": 1, "message": "X swings the sword"}},
              {"input": {"gamepad.start": True}}, {"step": 2}, {"input": {"gamepad.start": False}}, {"step": 3},
              {"expect": {"resource": "paused", "eq": 1, "message": "Start pauses"}},
              {"input": {"gamepad.a": True}}, {"step": 2}, {"input": {"gamepad.a": False}}, {"step": 3},
              {"expect": {"resource": "paused", "eq": 0, "message": "A on Resume goes on"}}]
    write("pad", spec("pad", f"{D}/menu.lunaris.json", steps, "Depths with a game pad: the d-pad and A in the menu, the left stick to walk, X to swing, Start to pause, A to resume"))


def main():
    os.makedirs(TESTS, exist_ok=True)
    pad_test()
    dialogue_test()
    level_test(1)
    level_test(2)
    level_test(3, last=True)
    full_test()
    combat_test()
    guards_test()
    menu_test()
    boot_test()
    pause_test()
    save_test()
    win_test()


if __name__ == "__main__":
    main()
