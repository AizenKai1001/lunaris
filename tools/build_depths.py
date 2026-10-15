"""Builds "Depths", a complete small game (a top-down dungeon crawler), only by calling the engine's tools: every scene file of `examples/depths/` is written by
`lunaris serve` from this script, the way an AI agent would build it.

  boot.lunaris.json       the start scene: the music (a `Persist` entity that survives every scene change), the mixer and a switch to the menu
  menu.lunaris.json       title, Play / Continue / Credits / Quit (keyboard, pad and mouse: `UiNav`), a camera drifting over a dungeon on a `Spline`
  level1/2/3.lunaris.json three levels made by `generate_map` (rooms, caves, a maze), walls from `autotile` + `tilemap_colliders`, guards that hunt with `Steer` + `Sensor`
                          line of sight, coins, keys, hearts, a locked door that needs a key, torches (`ParticleModules`), a lantern of darkness round the hero, a pause menu, a HUD
  gameover.lunaris.json   retry the level (the health, coins, keys and score of its start come back) or go to the menu
  win.lunaris.json        the final score
  lang/en.json, de.json   every text of the game (`Localization`, `{#key}`)

    python -I tools/make_depths_art.py && python -I tools/make_depths_audio.py        (once: the pictures and the sound)
    python3 tools/build_depths.py                                  (needs target/release/lunaris; see the end of this file for the options)

The state of the run travels between scenes in resources (`level`, `hp`, `coins`, `keys`, `score`: `SceneSwitch` keeps them), and every level saves itself to slot 1 on
its first tick (`game.save`), so Continue brings back the level, the health and the score.
"""
import collections
import json
import math
import os
import subprocess
import sys

BIN = "target/release/lunaris"
D = "examples/depths"
FONT = "examples/fonts/pixel.ttf"
LANG = f"{D}/lang/en"
TICKS_PER_CELL = 12  # the hero walks 4.5 units a second, a cell is a unit, the simulation runs at 60 a second

# ------------------------------------------------------------------------------------------------ the engine, spoken to over stdin/stdout
proc = None
n_requests = 0
failed = 0


def start():
    global proc, n_requests, failed
    proc = subprocess.Popen([BIN, "serve", "--root", ".", "--no-gpu"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    n_requests = failed = 0


def call(name, args=None):
    """One tool call; returns the tool's own JSON answer (an empty dict, and a line on stdout, when it failed)."""
    global failed, n_requests
    n_requests += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n_requests, "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + "\n")
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line:
            raise SystemExit(f"the engine stopped during {name}")
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("id") == n_requests:
            break
    res = m.get("result") or {}
    if "error" in m or res.get("isError"):
        failed += 1
        print("request", n_requests, name, "failed:", json.dumps(m.get("error") or res)[:700])
        return {}
    for c in res.get("content") or []:
        if c.get("type") == "text":
            try:
                return json.loads(c["text"])
            except ValueError:
                return {"text": c["text"]}
    return {}


def spawn(uid, name, **components):
    call("spawn", {"uid": uid, "components": {"Name": {"value": name}, **components}})


def finish():
    proc.stdin.close()
    proc.wait(timeout=60)


def show_check(what):
    """`check` of the scene just built: the errors and the warnings, each message once with how many entities have it."""
    chk = call("check", {})
    seen = collections.OrderedDict()
    for p in chk.get("problems", []):
        key = (p.get("severity"), p.get("message"))
        seen.setdefault(key, []).append(p.get("entity"))
    print(f"  {what}: check says {chk.get('errors')} errors, {chk.get('warnings')} warnings")
    for (sev, msg), ents in seen.items():
        print(f"    {sev}: {msg}  (x{len(ents)}, e.g. entity {ents[0]})")


# ------------------------------------------------------------------------------------------------ the data types of the game
F = "f32"
COMPONENTS = {
    "Hero": [("ready", F), ("cd", F)],
    "Enemy": [("alert", F), ("damage", F), ("chase", F), ("idle", F), ("wander", F), ("worth", F)],
    "Coin": [], "Key": [], "Heart": [], "Door": [], "Exit": [], "Wall": [],
    "Fader": [("t", F), ("out", F), ("leaving", F), ("target", F), ("fired", F)],
    "SwitchId": [("n", F)],
    "PauseUi": [], "PauseButton": [("kind", F)], "HudHp": [], "HurtFlash": [], "SlashFx": [("t", F)], "FxPool": [("kind", F)], "Npc": [], "TalkHint": [],
    "Hint": [("t", F)], "Banner": [("t", F)], "ShakeCam": [], "MenuButton": [("kind", F)], "ContinueButton": [], "NoSave": [], "CreditsUi": [],
    "OverButton": [("kind", F)], "WinButton": [("kind", F)], "Rank": [("min", F), ("max", F)], "MainUi": [], "ThemeTrack": [], "DepthsTrack": [],
}


def define_components(names):
    for n in names:
        call("define_component", {"name": n, "fields": [{"name": f, "type": t, "default": 0} for f, t in COMPONENTS[n]]})


# ------------------------------------------------------------------------------------------------ small helpers for the interface
def ui_text(uid, name, text, pos, size, color=(1, 1, 1, 1), align=1, visible=True, **extra):
    spawn(uid, name, Text={"value": text, "pos": list(pos), "size": size, "align": align, "color": list(color), "shadow": True, "visible": visible}, TextFont={"file": FONT}, **extra)


def panel(uid, name, pos, size, color=(0, 0, 0, 0.55), radius=0.012, layer=1, **extra):
    comps = {"Ui": {"pos": list(pos), "size": list(size), "color": list(color), "layer": layer}, "UiStyle": {"radius": radius}}
    comps.update(extra)
    spawn(uid, name, **comps)


def button(uid, name, text, pos, size, tags, layer=3, color=(0.2, 0.28, 0.55, 0.96), color2=(0.1, 0.14, 0.34, 0.96), text_size=0.04, visible=True):
    comps = {"Ui": {"pos": list(pos), "size": list(size), "color": list(color), "text": text, "text_size": text_size, "text_color": [1, 1, 1, 1], "button": True, "layer": layer, "visible": visible, "border": 0.003, "border_color": [0.75, 0.82, 1, 0.9]},
             "UiStyle": {"radius": 0.018, "color2": list(color2), "shadow_color": [0, 0, 0, 0.5], "shadow_offset": [0.004, 0.008]},
             "TextFont": {"file": FONT}}
    comps.update(tags)
    spawn(uid, name, **comps)


def icon(uid, name, frame, frames, sheet, pos, size, layer=3):
    spawn(uid, name, Ui={"pos": list(pos), "size": list(size), "color": [1, 1, 1, 1], "layer": layer},
          UiStyle={"image": f"{D}/{sheet}", "image_uv": [frame / frames, 0, (frame + 1) / frames, 1], "smooth": False, "fit": 1})


SOUNDS = [  # name, wave (0 sine 1 square 2 saw 3 triangle 4 noise), freq, freq_end, seconds, volume
    ("coin", 1, 880, 1320, 0.12, 0.16), ("key", 3, 660, 1320, 0.28, 0.22), ("unlock", 2, 180, 420, 0.28, 0.2), ("hurt", 2, 210, 70, 0.22, 0.3),
    ("swing", 4, 400, 0, 0.09, 0.12), ("hit", 1, 320, 110, 0.09, 0.22), ("kill", 2, 420, 50, 0.3, 0.26), ("heal", 0, 440, 880, 0.3, 0.22),
    ("stairs", 3, 330, 110, 0.55, 0.26), ("select", 1, 520, 780, 0.08, 0.14), ("win", 3, 523, 1046, 0.8, 0.28), ("lose", 2, 220, 55, 1.0, 0.3),
]


def sounds():
    for i, (name, wave, f0, f1, dur, vol) in enumerate(SOUNDS):
        spawn(8000 + i, f"Sound {name}", Sound={"name": name, "wave": wave, "freq": f0, "freq_end": f1, "duration": dur, "volume": vol})


def input_map(uid=4):
    spawn(uid, "Input", InputMap={"bindings": "\n".join([
        "move_x = d, a:-1, gamepad.lx, arrowright, arrowleft:-1", "move_y = w, s:-1, gamepad.ly, arrowup, arrowdown:-1",
        "attack = space, j, gamepad.x, gamepad.rt", "pause = escape, gamepad.start", "interact = e, gamepad.y", "dialogue_next = space, enter, gamepad.a", "dialogue_choice_1 = 1, gamepad.x", "dialogue_choice_2 = 2, gamepad.b",
        "ui_up = w, arrowup, gamepad.dpad_up", "ui_down = s, arrowdown, gamepad.dpad_down", "ui_left = a, arrowleft, gamepad.dpad_left", "ui_right = d, arrowright, gamepad.dpad_right",
        "ui_accept = enter, space, gamepad.a", "deadzone = 0.15"])})


def fader(uid, switches):
    """The fade in at the start of a scene, the fade out before leaving it, and the switches (`SceneSwitch`) the scene can throw: `switches` maps a number to a scene file."""
    spawn(uid, "Fader", Fader={}, ColorGrade={"fade": [0, 0, 0, 1]})
    for i, (n, path) in enumerate(switches.items()):
        spawn(uid + 1 + i, f"Switch {n}", SwitchId={"n": n}, SceneSwitch={"path": path})


FADER_SCRIPT = """
/// The first tick of a scene: what the last scene left behind (a pause, a switch on its way) is cleared.
system scene_init each Fader order 0 {
    if Fader.t == 0 {
        Fader.t = 0.0001;
        $paused = 0;
        $leave = 0;
        $fire = 0;
        $atk = 0;
        $time.scale = 1;
@INIT@
    }
}

/// Anything that wants to leave sets `$leave` to the number of a switch; the picture fades to black, then the switch is thrown.
system fader each Fader, ColorGrade order 60 {
    Fader.t += dt;
    if $leave > 0 && Fader.leaving == 0 {
        Fader.leaving = 1;
        Fader.target = $leave;
    }
    if Fader.leaving > 0 {
        Fader.out += dt / 0.55;
        if Fader.out >= 1 && Fader.fired == 0 {
            Fader.fired = 1;
            $fire = Fader.target;
        }
    }
    ColorGrade.fade[3] = max(clamp(1 - Fader.t / 0.7, 0, 1), clamp(Fader.out, 0, 1));
}

system fire_switch each SwitchId, SceneSwitch order 70 {
    if $fire > 0 && $fire == SwitchId.n {
        SceneSwitch.go = true;
        $fire = 0;
    }
}
"""


def fader_script(init=""):
    return FADER_SCRIPT.replace("@INIT@", init)


# ------------------------------------------------------------------------------------------------ the level maths (plain Python on the grid the engine generated)
def parse_grid(data):
    return [[int(n) for n in line.split()] for line in data.strip().split("\n")]


def neighbours(c, w, h):
    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        n = (c[0] + dx, c[1] + dy)
        if 0 <= n[0] < w and 0 <= n[1] < h:
            yield n


def bfs(floor, src, blocked=frozenset()):
    h, w = len(floor), len(floor[0])
    dist = {src: 0}
    came = {src: None}
    q = collections.deque([src])
    while q:
        c = q.popleft()
        for n in neighbours(c, w, h):
            if n not in dist and floor[n[1]][n[0]] and n not in blocked:
                dist[n] = dist[c] + 1
                came[n] = c
                q.append(n)
    return dist, came


def path_to(came, goal):
    out = []
    while goal is not None:
        out.append(goal)
        goal = came[goal]
    return out[::-1]


def plan_level(floor, start, exit_cell, want_door=True):
    """The door goes on a cell of the way from the start to the exit that the whole level hangs on (take it away and the exit cannot be reached), about 55 to 70 percent of the
    way; the key is the farthest cell the start can reach without passing the door. Returns (path, door, key) with door and key None when the level has no such cell."""
    dist, came = bfs(floor, start)
    path = path_to(came, exit_cell)
    door = key = None
    if want_door:
        lo, hi = int(len(path) * 0.35), int(len(path) * 0.8)
        best = None
        for i in range(lo, hi):
            c = path[i]
            d2, _ = bfs(floor, start, frozenset([c]))
            if exit_cell not in d2:
                score = abs(i - len(path) * 0.62)
                if best is None or score < best[0]:
                    best = (score, c, d2)
        if best:
            _, door, d2 = best
            # the key lies far from the start and from the door both (not in the corridor just before the door), on the start's side of it
            all_floor = {(x, y) for y, row in enumerate(floor) for x, v in enumerate(row) if v}
            dd, _ = bfs(floor, door, frozenset(all_floor - set(d2) - {door}))
            key = max((c for c in d2 if d2[c] >= 8 and c != door and c in dd), key=lambda c: (min(d2[c], dd[c]), d2[c], c), default=None)
            if key is None:
                door = None
    return path, door, key


def hash2(x, y, seed):
    h = (x * 374761393 + y * 668265263 + seed * 1442695041) & 0xFFFFFFFF
    h = (h ^ (h >> 13)) * 1274126177 & 0xFFFFFFFF
    return (h ^ (h >> 16)) & 255


LEVELS = [
    dict(n=1, name="The Cellars", npc=True, mode="rooms", seeds=range(11, 60), size=(52, 34), theme="dungeon", enemy="grunt", enemies=6, coins=16, hearts=3, torches=16, props=22, extra={"loops": 1}),
    dict(n=2, name="The Caves", mode="caves", seeds=range(7, 70), size=(58, 38), theme="cave", enemy="bat", enemies=9, coins=18, hearts=3, torches=12, props=26, extra={"fill": 46, "smooth": 5}),
    dict(n=3, name="The Catacombs", mode="maze", seeds=range(5, 40), size=(41, 27), theme="maze", enemy="skeleton", enemies=8, coins=20, hearts=3, torches=14, props=12, extra={"braid": 18}),
]
ENEMIES = {  # sprite sheet, frames, hit points, bite, chase speed, idle speed, wander, worth, sensor radius, line of sight
    "grunt": dict(sheet="grunt.png", frames=4, hp=40, damage=14, chase=3.2, idle=0.7, wander=0.5, worth=50, radius=7.5, los=True),
    "bat": dict(sheet="bat.png", frames=2, hp=20, damage=10, chase=4.6, idle=1.6, wander=1.6, worth=40, radius=7.0, los=False),
    "skeleton": dict(sheet="skeleton.png", frames=4, hp=70, damage=18, chase=3.7, idle=0.5, wander=0.4, worth=80, radius=8.0, los=True),
}


def level_file(n):
    return f"{D}/level{n}.lunaris.json"


def cell_world(cell, w, h):
    return [cell[0] + 0.5 - w / 2, h / 2 - cell[1] - 0.5]


# ------------------------------------------------------------------------------------------------ the systems of a level, as Lunaris Script
LEVEL_SCRIPT = """
/// Once per scene the hero takes the health of the run (the resource `hp`), notes what the level started with (for `Retry`) and saves the game to slot 1.
system hero_init each Hero, HitPoints order 1 {
    if Hero.ready == 0 {
        Hero.ready = 1;
        HitPoints.value = select($hp > 0, $hp, 100);
        $s.hp = select($hp > 0, $hp, 100);
        $s.coins = $coins;
        $s.keys = $keys;
        $s.score = $score;
        $game.save = 1;
    }
}

/// Escape (or Start on a pad) pauses: time stops (`time.scale` 0), a second press goes on.
system pause_toggle each Fader order 4 {
    if $action.pause.pressed > 0.5 && $leave == 0 && $hp > 0 {
        if $paused == 0 {
            $paused = 1;
            $time.scale = 0;
            emit "select";
        } else {
            $paused = 0;
            $time.scale = 1;
        }
    }
}

system pause_focus each UiNav order 5 {
    if $action.pause.pressed > 0.5 && $paused == 1 {
        UiNav.current = 1201;
    }
}

system pause_show each PauseUi, Ui order 6 {
    Ui.visible = $paused != 0;
}

system pause_show_text each PauseUi, Text order 6 {
    Text.visible = $paused != 0;
}

system pause_buttons each PauseButton, Ui order 7 {
    if Ui.clicked && $paused != 0 {
        $paused = 0;
        $time.scale = 1;
        emit "select";
        if PauseButton.kind == 2 {
            $leave = 3;
        }
    }
}

/// The keys or the stick become the hero's walk; it stands still when dead, paused or on its way out.
system hero_walk each Hero, Velocity, HitPoints order 10 {
    let mx = $action.move_x;
    let my = $action.move_y;
    let k = select(hypot(mx, my) > 1, 1 / max(hypot(mx, my), 0.001), 1);
    if HitPoints.dead || $paused != 0 || $leave != 0 || $talking != 0 {
        Velocity.lin[0] = 0;
        Velocity.lin[1] = 0;
    } else {
        Velocity.lin[0] = mx * k * 4.5;
        Velocity.lin[1] = my * k * 4.5;
    }
}

/// What everything else needs to know about the hero: where it is, which way it faces, its health for the HUD.
system hero_status each Hero, Transform, HitPoints, Velocity order 11 {
    $hero.x = Transform.pos[0];
    $hero.y = Transform.pos[1];
    $hp = floor(HitPoints.value);
    if abs(Velocity.lin[0]) + abs(Velocity.lin[1]) > 0.3 {
        let l = max(hypot(Velocity.lin[0], Velocity.lin[1]), 0.001);
        $face.x = Velocity.lin[0] / l;
        $face.y = Velocity.lin[1] / l;
    }
    if Velocity.lin[0] < -0.3 {
        Transform.scale[0] = -1;
    }
    if Velocity.lin[0] > 0.3 {
        Transform.scale[0] = 1;
    }
}

/// Space swings the sword: for one tick `$atk` is 1 and `$atk.x`, `$atk.y` is where the blow lands; whatever is near that point is hit (see `enemy_hurt`).
system hero_attack each Hero, HitPoints, Transform order 14 {
    Hero.cd = max(Hero.cd - dt, 0);
    let go = $action.attack.pressed > 0.5 && Hero.cd <= 0 && !HitPoints.dead && $paused == 0 && $leave == 0 && $talking == 0;
    $atk = select(go, 1, 0);
    if go {
        Hero.cd = 0.32;
        $atk.x = Transform.pos[0] + $face.x * 0.9;
        $atk.y = Transform.pos[1] + $face.y * 0.9;
        emit "swing";
    }
}

/// The picture of the swing: a crescent in front of the hero for a moment.
system slash_fx each SlashFx, Transform, Sprite order 16 {
    if $atk == 1 {
        SlashFx.t = 0.16;
    }
    SlashFx.t = max(SlashFx.t - dt, 0);
    Sprite.visible = SlashFx.t > 0;
    Sprite.frame = select(SlashFx.t > 0.08, 6, 7);
    let a = atan2($face.y, $face.x);
    Transform.pos[0] = $hero.x + $face.x * 0.95;
    Transform.pos[1] = $hero.y + $face.y * 0.95;
    Transform.rot[2] = sin(a / 2);
    Transform.rot[3] = cos(a / 2);
}

/// A guard that sees the hero is alerted for a few seconds; an alerted guard seeks the hero (`Steer`), otherwise it mooches about.
system guard_ai each Enemy, Sensor, Steer, Transform, Velocity, Sprite order 20 {
    if Sensor.seen != 0 {
        Enemy.alert = 3.5;
    }
    if Enemy.alert > 0 && $leave == 0 {
        Enemy.alert -= dt;
        Steer.seek = 1000;
        Steer.max_speed = Enemy.chase;
        Steer.wander = 0;
    } else {
        Steer.seek = 0;
        Steer.max_speed = Enemy.idle;
        Steer.wander = Enemy.wander;
    }
    if $talking != 0 {
        Steer.seek = 0;
        Steer.max_speed = 0;
        Steer.wander = 0;
    }
    Transform.scale[0] = select(Velocity.lin[0] < -0.25, -1, select(Velocity.lin[0] > 0.25, 1, Transform.scale[0]));
    Sprite.color[1] = select(Enemy.alert > 0, 0.6, 1);
    Sprite.color[2] = select(Enemy.alert > 0, 0.6, 1);
}

/// The hero's sensor sees the warden when it is close: `$near`.
system npc_near each Hero, Sensor order 12 {
    $near = select(Sensor.count > 0, 1, 0);
}

system talk_state each Npc, Dialogue order 13 {
    $talking = select(Dialogue.active, 1, 0);
}

/// E talks to the warden when the hero is near (the `Dialogue` takes over: lines, choices, the typing).
system npc_interact each Npc, Dialogue order 15 {
    if $action.interact.pressed > 0.5 && $near > 0.5 && $talking < 0.5 && $leave == 0 && $paused == 0 {
        Dialogue.active = true;
    }
}

system talk_hint each TalkHint, Text order 50 {
    Text.visible = $near > 0.5 && $talking < 0.5 && $paused == 0;
}

system enemy_hurt each Enemy, Transform, HitPoints, Velocity order 22 {
    if $atk == 1 && !HitPoints.dead {
        if hypot(Transform.pos[0] - $atk.x, Transform.pos[1] - $atk.y) < 1.15 {
            let l = max(hypot(Transform.pos[0] - $hero.x, Transform.pos[1] - $hero.y), 0.001);
            HitPoints.value -= 22;
            Velocity.lin[0] = (Transform.pos[0] - $hero.x) / l * 7;
            Velocity.lin[1] = (Transform.pos[1] - $hero.y) / l * 7;
            Enemy.alert = 4;
            emit "hit";
        }
    }
}

system enemy_die each Enemy, HitPoints, Transform order 23 {
    if HitPoints.dead {
        $score += Enemy.worth;
        $kills += 1;
        $dx = Transform.pos[0];
        $dy = Transform.pos[1];
        $dgo = 1;
        emit "kill";
        despawn;
    }
}

/// A guard that touches the hero bites it, once in a while (a moment of grace after each hit).
system guard_bites on_contact a(Hero, HitPoints) b(Enemy) order 30 {
    if a.HitPoints.invulnerable <= 0 && !a.HitPoints.dead {
        a.HitPoints.value -= b.Enemy.damage;
        a.HitPoints.invulnerable = 0.8;
        $shake = 0.8;
        $flash = 1;
        emit "hurt";
    }
}

system coin_pickup on_contact a(Hero) b(Coin, Transform) order 31 {
    $coins += 1;
    $score += 10;
    $sx = b.Transform.pos[0];
    $sy = b.Transform.pos[1];
    $sgo = 1;
    emit "coin";
    despawn b;
}

system key_pickup on_contact a(Hero) b(Key, Transform) order 32 {
    $keys += 1;
    $score += 25;
    $sx = b.Transform.pos[0];
    $sy = b.Transform.pos[1];
    $sgo = 1;
    emit "key";
    despawn b;
}

system heart_pickup on_contact a(Hero, HitPoints) b(Heart, Transform) order 33 {
    if a.HitPoints.value < a.HitPoints.max {
        a.HitPoints.value = min(a.HitPoints.max, a.HitPoints.value + 30);
        $sx = b.Transform.pos[0];
        $sy = b.Transform.pos[1];
        $sgo = 1;
        emit "heal";
        despawn b;
    }
}

/// The locked door opens for a hero that has a key, and takes it.
system door_open on_contact a(Hero) b(Door, Transform) order 34 {
    if $keys > 0 {
        $keys -= 1;
        $score += 25;
        $sx = b.Transform.pos[0];
        $sy = b.Transform.pos[1];
        $sgo = 1;
        emit "unlock";
        despawn b;
    }
}

system reach_exit on_contact a(Hero, HitPoints) b(Exit) order 35 {
    if $leave == 0 && !a.HitPoints.dead {
        $score += 100;
        $leave = 1;
        emit "stairs";
    }
}

/// The hero falls over when dead, and the game is over after a moment.
system hero_death each Hero, HitPoints, Transform, Sprite order 40 {
    if HitPoints.dead {
        Sprite.color[1] = 0.45;
        Sprite.color[2] = 0.45;
        Transform.rot[2] = 0.7071;
        Transform.rot[3] = 0.7071;
        if $leave == 0 {
            $leave = 2;
            emit "lose";
        }
    }
}

system hud_bar each HudHp, Ui order 50 {
    let f = clamp($hp / 100, 0, 1);
    Ui.fill = f;
    Ui.color[0] = 0.9;
    Ui.color[1] = 0.12 + 0.5 * f;
    Ui.color[2] = 0.16;
}

system hint_text each Hint, Text order 50 {
    Hint.t += dt;
    Text.visible = Hint.t < 9 && $paused == 0;
}

system banner_text each Banner, Text order 50 {
    Banner.t += dt;
    Text.visible = Banner.t < 3.2;
    Text.color[3] = clamp(3.2 - Banner.t, 0, 1);
}

/// A red flash over the picture when the hero is hurt, and a shake of the camera.
system hurt_flash each HurtFlash, Ui order 62 {
    $flash = max($flash - dt * 2.5, 0);
    Ui.color[3] = $flash * 0.35;
    Ui.visible = $flash > 0;
}

system cam_shake each ShakeCam, CameraRig order 63 {
    if $shake > 0 {
        CameraRig.shake = max(CameraRig.shake, $shake);
        $shake = 0;
    }
}

/// Sparkles at a pickup, a puff where a guard died: each is one cloud that is moved to the spot and fired again.
system fx_pool each FxPool, Particles, Transform order 61 {
    if FxPool.kind == 1 && $sgo > 0 {
        Transform.pos[0] = $sx;
        Transform.pos[1] = $sy;
        Particles.time = 0;
        Particles.playing = true;
        $sgo = 0;
    }
    if FxPool.kind == 2 && $dgo > 0 {
        Transform.pos[0] = $dx;
        Transform.pos[1] = $dy;
        Particles.time = 0;
        Particles.playing = true;
        $dgo = 0;
    }
}

/// The dungeon's own music takes over from the theme (two persistent players: one is faded out, the other in).
system music_theme_off each ThemeTrack, MusicPlayer order 80 {
    MusicPlayer.playing = false;
}

system music_depths_on each DepthsTrack, MusicPlayer order 80 {
    MusicPlayer.playing = true;
}
"""

DIALOGUE = """== start
Warden: {#npc.hello}
Warden: {#npc.door}
* [{#npc.q_advice}] -> advice
* [{#npc.q_fine}] -> bye

== advice
if $gift == 1 -> again
res gift = 1
res score += 20
Warden: {#npc.advice}
-> end

== again
Warden: {#npc.again}
-> end

== bye
Warden: {#npc.bye}
-> end
"""

FLAME = {
    "count": 70, "lifetime": 0.75, "looping": True, "prewarm": True, "shape": 1, "extent": [0.05, 0, 0], "direction": [0, 1, 0], "cone": 18, "speed_range": [0.45, 1.0],
    "gravity": [0, 0.7, 0], "size": [0.4, 0.1], "color": [1, 0.8, 0.35, 0.9], "color_end": [0.9, 0.2, 0.05, 0], "additive": True, "seed": 3,
}
FLAME_MODULES = "color 0:1,0.92,0.55,0 0.12:1,0.78,0.28,0.95 0.55:1,0.42,0.1,0.65 1:0.35,0.08,0.04,0\nsize 0:0.2 0.3:0.34 1:0.06\nnoise strength=0.45 scale=0.9 speed=0.8"
SPARK = {
    "count": 26, "lifetime": 0.7, "looping": False, "playing": False, "time": 99, "shape": 1, "extent": [0.08, 0, 0], "cone": 180, "speed_range": [1.2, 3.2], "gravity": [0, -1.5, 0], "drag": 1.6,
    "size": [0.2, 0.04], "color": [1, 0.9, 0.4, 1], "color_end": [1, 0.5, 0.1, 0], "additive": True, "fade_in": 0, "seed": 5,
}
SPARK_MODULES = "color 0:1,1,0.8,1 0.4:1,0.85,0.3,0.9 1:1,0.45,0.1,0\nsize 0:0.22 1:0.03\ntrail segments=4 time=0.18 width=0.5"
DEATH = {
    "count": 42, "lifetime": 0.8, "looping": False, "playing": False, "time": 99, "shape": 1, "extent": [0.1, 0, 0], "cone": 180, "speed_range": [1.0, 4.0], "gravity": [0, -2.5, 0], "drag": 1.4,
    "size": [0.3, 0.05], "color": [0.95, 0.2, 0.15, 1], "color_end": [0.4, 0.05, 0.1, 0], "additive": False, "fade_in": 0, "seed": 9,
}


def build_level(cfg):
    n, theme = cfg["n"], cfg["theme"]
    w, h = cfg["size"]
    enemy = ENEMIES[cfg["enemy"]]
    call("reset", {"seed": 1})
    define_components(["Hero", "Enemy", "Coin", "Key", "Heart", "Door", "Exit", "Wall", "Fader", "SwitchId", "PauseUi", "PauseButton", "HudHp", "HurtFlash", "SlashFx", "FxPool", "Hint", "Banner", "ShakeCam", "ThemeTrack", "DepthsTrack", "Npc", "TalkHint"])

    # ---- the level: first look at seeds until one has a chokepoint for a door, then make it for real, dress it and make it solid
    chosen = None
    for seed in cfg["seeds"]:
        preview = call("generate_map", {"mode": cfg["mode"], "seed": seed, "width": w, "height": h, "floor": 0, "wall": 1, "points": 26, "clear": 10, **cfg["extra"]})
        grid = parse_grid(preview["data"])
        floor = [[v == 0 for v in row] for row in grid]
        start, exit_cell = tuple(preview["start"]), tuple(preview["exit"])
        path, door, key = plan_level(floor, start, exit_cell)
        if door and 45 <= len(path) <= 260:
            chosen = (seed, preview, floor, path, door, key)
            break
    if not chosen:
        raise SystemExit(f"level {n}: no seed with a door")
    seed, preview, floor, path, door, key = chosen
    h, w = len(floor), len(floor[0])
    start, exit_cell = tuple(preview["start"]), tuple(preview["exit"])
    print(f"level {n} ({cfg['mode']}): seed {seed}, {w}x{h}, the way is {len(path)} cells, door at {door}, key at {key}")

    spawn(10, f"Level {n}", Transform={"pos": [0, 0, -0.05]}, Tilemap={"texture": f"{D}/tiles_{theme}.png", "sheet": [19, 1], "size": [w, h], "tile": 1.0})
    call("generate_map", {"uid": 10, "mode": cfg["mode"], "seed": seed, "width": w, "height": h, "floor": 0, "wall": 1, "points": 26, "clear": 10, **cfg["extra"]})
    call("autotile", {"uid": 10, "base": 3, "mode": "4", "solid": [1], "edge": "solid"})
    got = call("get", {"uid": 10, "components": ["Tilemap"]})
    rows = got["components"]["Tilemap"]["data"].strip().split("\n")
    varied = []
    for y, line in enumerate(rows):
        cells = line.split()
        for x, v in enumerate(cells):
            if v == "0":
                r = hash2(x, y, seed)
                cells[x] = "0" if r < 190 else "1" if r < 232 else "2"
        varied.append(" ".join(cells))
    call("set", {"uid": 10, "component": "Tilemap", "value": {"data": "\n".join(varied)}})
    walls = call("tilemap_colliders", {"uid": 10, "solid": [[3, 18]], "components": {"Wall": {}}})
    print(f"  {walls.get('boxes')} wall boxes for {walls.get('solid_cells')} wall cells")

    def at(cell, z=0.0):
        p = cell_world(cell, w, h)
        return [p[0], p[1], z]

    # ---- the world
    spawn(2, "Physics", PhysicsSettings={"gravity": [0, 0, 0]})
    sx, sy, _ = at(start)
    spawn(3, "Camera", Transform={"pos": [sx, sy, 10]}, Camera={"ortho": True, "ortho_height": 13, "clear": [0.02, 0.015, 0.03, 1]},
          CameraRig={"target": 1000, "follow_speed": 6, "offset": [0, 0, 10], "look_at": False}, ShakeCam={})
    spawn(5, "Look", Environment={"sky": False, "bloom": 0.45, "bloom_threshold": 0.7, "vignette": 0.25, "saturation": 1.08})
    spawn(6, "Localization", Localization={"language": LANG})
    input_map()
    fader(900, {1: level_file(n + 1) if n < 3 else f"{D}/win.lunaris.json", 2: f"{D}/gameover.lunaris.json", 3: f"{D}/menu.lunaris.json"})
    spawn(910, "Nav", UiNav={"current": 0})
    sounds()

    # ---- the hero and the lantern of darkness round it
    extra = {"Sensor": {"radius": 2.4, "fov": 360, "filter": "Npc", "los": False}} if cfg.get("npc") else {}  # the hero notices the warden when it is close
    spawn(1000, "Hero", Transform={"pos": [sx, sy, 0]}, Velocity={}, Hero={}, HitPoints={"value": 100, "max": 100, "regen": 1.5, "regen_delay": 6},
          RigidBody={"mass": 1, "restitution": 0, "friction": 0, "gravity_scale": 0}, Collider={"shape": 0, "size": [0.7, 0.7, 0.7]},
          Sprite={"size": [1, 1], "texture": f"{D}/hero.png", "sheet": [6, 1], "frame": 0}, Animator={"clip": "idle", "looping": True},
          SpriteAnim={"clips": "idle: 0-1 fps=2 loop\nwalk: 2-5 fps=10 loop step@0 step@2"},
          StateMachine={"machine": "state idle clip=idle\nstate walk clip=walk\nidle -> walk when $action.move_x != 0 or $action.move_y != 0\nwalk -> idle when $action.move_x == 0 and $action.move_y == 0"}, **extra)
    spawn(1001, "Lantern", Parent={"target": 1000}, LocalTransform={"pos": [0, 0, 0.4]}, Transform={"pos": [sx, sy, 0.5]},
          Sprite={"size": [44, 44], "texture": f"{D}/lantern.png", "color": [1, 1, 1, 1]})
    spawn(1002, "Swing", SlashFx={}, Transform={"pos": [sx, sy, 0.15]}, Sprite={"size": [1.3, 1.3], "texture": f"{D}/props.png", "sheet": [8, 1], "frame": 6, "visible": False})
    spawn(1003, "Sparkles", FxPool={"kind": 1}, Transform={"pos": [sx, sy, 0.3]}, Particles=SPARK, ParticleModules={"modules": SPARK_MODULES})
    spawn(1004, "Death puff", FxPool={"kind": 2}, Transform={"pos": [sx, sy, 0.3]}, Particles=DEATH)

    # ---- the exit, the key and the door
    ex, ey, _ = at(exit_cell)
    spawn(1010, "Exit", Exit={}, Transform={"pos": [ex, ey, -0.02]}, Collider={"shape": 0, "size": [0.9, 0.9, 0.9], "trigger": True},
          Sprite={"size": [1, 1], "texture": f"{D}/items.png", "sheet": [8, 1], "frame": 7})
    spawn(1011, "Exit glow", Transform={"pos": [ex, ey, 0.3]}, Particles={"count": 1, "lifetime": 100, "looping": True, "prewarm": True, "speed_range": [0, 0], "size": [3.2, 3.2], "color": [0.5, 0.6, 1, 0.3], "color_end": [0.5, 0.6, 1, 0.3], "additive": True, "texture": "soft"})
    dx, dy, _ = at(door)
    spawn(1020, "Door", Door={}, Transform={"pos": [dx, dy, 0]}, Collider={"shape": 1, "size": [1, 1, 1]},
          Sprite={"size": [1, 1], "texture": f"{D}/items.png", "sheet": [8, 1], "frame": 6})
    kx, ky, _ = at(key)
    spawn(1021, "Key", Key={}, Transform={"pos": [kx, ky, -0.02]}, Collider={"shape": 0, "size": [0.7, 0.7, 0.7], "trigger": True},
          Sprite={"size": [0.9, 0.9], "texture": f"{D}/items.png", "sheet": [8, 1], "frame": 4})
    spawn(1022, "Key glow", Transform={"pos": [kx, ky, 0.3]}, Particles={"count": 1, "lifetime": 100, "looping": True, "prewarm": True, "speed_range": [0, 0], "size": [2.2, 2.2], "color": [1, 0.85, 0.3, 0.3], "color_end": [1, 0.85, 0.3, 0.3], "additive": True, "texture": "soft"})

    # ---- who goes where: guards on the spread-out spawn points, hearts far away, coins along the way and on what is left
    dist, _ = bfs(floor, start)
    used = {start, exit_cell, door, key}
    spots = [tuple(s) for s in preview["spawns"] if tuple(s) not in used and tuple(s) in dist]
    spots.sort(key=lambda c: -dist[c])
    guard_spots = [s for s in spots if dist[s] >= 14][: cfg["enemies"]]
    used.update(guard_spots)
    rest = [s for s in spots if s not in used]
    heart_spots = rest[: cfg["hearts"]]
    used.update(heart_spots)
    # coins: along the shortest way every few cells, then on the remaining spawn points and in the dead ends the guards do not stand in
    coin_cells = []
    for i in range(6, len(path) - 3, 7):
        if path[i] not in used and abs(path[i][0] - door[0]) + abs(path[i][1] - door[1]) > 1:
            coin_cells.append(path[i])
    coin_cells += [s for s in rest if s not in heart_spots]
    dead_ends = [c for c in dist if sum(1 for q in neighbours(c, w, h) if floor[q[1]][q[0]]) == 1 and c not in used and dist[c] > 10]
    dead_ends.sort(key=lambda c: -dist[c])
    coin_cells += dead_ends
    coin_cells = [c for c in dict.fromkeys(coin_cells) if c not in guard_spots and c not in (key, door)][: cfg["coins"]]
    for i, c in enumerate(guard_spots):
        x, y, _ = at(c)
        e = enemy
        spawn(300 + i, f"{cfg['enemy'].title()} {i + 1}", Transform={"pos": [x, y, 0]}, Velocity={},
              Enemy={"damage": e["damage"], "chase": e["chase"], "idle": e["idle"], "wander": e["wander"], "worth": e["worth"]},
              HitPoints={"value": e["hp"], "max": e["hp"]}, Sensor={"radius": e["radius"], "fov": 360, "filter": "Hero", "los": e["los"], "eye": 0},
              Steer={"max_speed": e["idle"], "max_accel": 14, "plane": 1, "arrive": 0.2, "wander": e["wander"], "face": False},
              RigidBody={"mass": 1, "restitution": 0, "friction": 0, "gravity_scale": 0, "drag": 0.5}, Collider={"shape": 0, "size": [0.7, 0.7, 0.7]},
              Sprite={"size": [1, 1], "texture": f"{D}/{e['sheet']}", "sheet": [e["frames"], 1], "frame": 0}, Animator={"clip": "walk", "looping": True},
              SpriteAnim={"clips": f"walk: 0-{e['frames'] - 1} fps={8 if e['frames'] > 2 else 7} loop"})
    for i, c in enumerate(heart_spots):
        x, y, _ = at(c)
        spawn(2500 + i, f"Heart {i + 1}", Heart={}, Transform={"pos": [x, y, -0.02]}, Collider={"shape": 0, "size": [0.6, 0.6, 0.6], "trigger": True},
              Sprite={"size": [0.8, 0.8], "texture": f"{D}/items.png", "sheet": [8, 1], "frame": 5})
    for i, c in enumerate(coin_cells):
        x, y, _ = at(c)
        spawn(3000 + i, f"Coin {i + 1}", Coin={}, Transform={"pos": [x, y, -0.02]}, Collider={"shape": 0, "size": [0.6, 0.6, 0.6], "trigger": True},
              Sprite={"size": [0.8, 0.8], "texture": f"{D}/items.png", "sheet": [8, 1], "frame": 0}, Animator={"clip": "spin", "looping": True, "time": (i % 4) * 0.1},
              SpriteAnim={"clips": "spin: 0-3 fps=7 loop"})
    print(f"  {len(guard_spots)} guards, {len(heart_spots)} hearts, {len(coin_cells)} coins")

    # ---- torches on the walls that look at the floor, props on the floor
    torch_cells = []
    for y in range(1, h - 1):
        for x in range(w):
            if not floor[y][x] and floor[y + 1][x] and hash2(x, y, seed + 77) < 40:
                if all(abs(x - tx) + abs(y - ty) > 7 for tx, ty in torch_cells):
                    torch_cells.append((x, y))
    torch_cells = torch_cells[: cfg["torches"]]
    for i, c in enumerate(torch_cells):
        x, y, _ = at(c)
        spawn(4000 + i * 3, f"Torch {i + 1}", Transform={"pos": [x, y - 0.1, -0.04]}, Sprite={"size": [1, 1], "texture": f"{D}/props.png", "sheet": [8, 1], "frame": 0})
        spawn(4001 + i * 3, f"Flame {i + 1}", Transform={"pos": [x, y + 0.2, 0.3]}, Particles={**FLAME, "seed": 3 + i}, ParticleModules={"modules": FLAME_MODULES})
        spawn(4002 + i * 3, f"Glow {i + 1}", Transform={"pos": [x, y - 0.5, 0.3]}, Particles={"count": 1, "lifetime": 100, "looping": True, "prewarm": True, "speed_range": [0, 0], "size": [6, 6], "color": [1, 0.55, 0.18, 0.11], "color_end": [1, 0.55, 0.18, 0.11], "additive": True, "texture": "soft"})
    print(f"  {len(torch_cells)} torches")
    prop_frames = {"dungeon": [1, 2, 3, 4, 5], "cave": [3, 4, 5, 1], "maze": [3, 4, 5]}[theme]
    # nothing solid on the ways a player has to walk: start to key (round the door), key to stairs, and the door's own corridor
    to_key = path_to(bfs(floor, start, frozenset([door]))[1], key)
    to_exit = path_to(bfs(floor, key, frozenset())[1], exit_cell)
    on_path = set(path) | set(to_key) | set(to_exit)
    props = 0
    for y in range(1, h - 1):
        for x in range(1, w - 1):
            if props >= cfg["props"]:
                break
            c = (x, y)
            if not floor[y][x] or c in used or c in on_path or c in coin_cells or hash2(x, y, seed + 5) > 45:
                continue
            around = sum(1 for dx in (-1, 0, 1) for dy in (-1, 0, 1) if (dx or dy) and floor[y + dy][x + dx])
            nearwall = any(not floor[q[1]][q[0]] for q in neighbours(c, w, h))
            deadend = sum(1 for q in neighbours(c, w, h) if floor[q[1]][q[0]]) == 1
            if theme == "maze":
                if not deadend:  # a maze is all corridor: bones and skulls in the dead ends
                    continue
            elif around < 6 or not nearwall:
                continue
            if abs(x - start[0]) + abs(y - start[1]) < 5:
                continue
            frame = prop_frames[hash2(x, y, seed) % len(prop_frames)]
            px, py, _ = at(c)
            comps = {"Transform": {"pos": [px, py, 0.0 if frame in (1, 2) else -0.04]}, "Sprite": {"size": [1, 1], "texture": f"{D}/props.png", "sheet": [8, 1], "frame": frame}}
            if frame in (1, 2):  # barrels and crates are solid
                comps["Collider"] = {"shape": 1, "size": [0.8, 0.8, 1]}
            spawn(5000 + props, f"Prop {props + 1}", **comps)
            props += 1
    print(f"  {props} props")

    # ---- the interface
    panel(1100, "HUD panel", (0.012, 0.02), (0.27, 0.125), layer=1)
    icon(1101, "Heart icon", 5, 8, "items.png", (0.02, 0.034), (0.026, 0.046))
    spawn(1102, "Health bar", HudHp={}, Ui={"pos": [0.055, 0.038], "size": [0.215, 0.036], "color": [0.9, 0.6, 0.16, 1], "back_color": [0.18, 0.04, 0.06, 0.9], "fill": 1, "border": 0.002, "border_color": [1, 1, 1, 0.7], "text": "{hp}", "text_size": 0.026, "layer": 3},
          UiStyle={"radius": 0.008}, TextFont={"file": FONT})
    icon(1103, "Coin icon", 0, 8, "items.png", (0.02, 0.088), (0.026, 0.046))
    ui_text(1104, "Coins", "{coins}", (0.056, 0.093), 0.034, align=0)
    icon(1105, "Key icon", 4, 8, "items.png", (0.118, 0.088), (0.026, 0.046))
    ui_text(1106, "Keys", "{keys}", (0.154, 0.093), 0.034, align=0)
    ui_text(1107, "Level", "{#hud.level} {level}", (0.985, 0.03), 0.04, align=2)
    ui_text(1108, "Score", "{#hud.score} {score}", (0.985, 0.085), 0.034, color=(1, 0.9, 0.5, 1), align=2)
    ui_text(1109, "Banner", f"{{#level{n}.name}}", (0.5, 0.2), 0.075, color=(1, 0.95, 0.75, 1), Banner={})
    ui_text(1110, "Hint", "{#hud.hint}", (0.5, 0.945), 0.028, color=(1, 1, 1, 0.8), Hint={})
    spawn(1111, "Hurt flash", HurtFlash={}, Ui={"pos": [0, 0], "size": [1, 1], "color": [0.9, 0.05, 0.05, 0], "layer": 10, "visible": False})
    if cfg.get("npc"):
        # the old warden of the cellars, two steps from the start: E talks to him (Dialogue: typed lines, a choice, a reward once)
        ring = sorted((c for c in dist if 4 <= dist[c] <= 8 and c not in on_path and c not in used), key=lambda c: (dist[c], c))
        wx, wy, _ = at(ring[0] if ring else path[3])
        spawn(1400, "Warden", Npc={}, Transform={"pos": [wx, wy, -0.01]}, Sprite={"size": [1, 1], "texture": f"{D}/npc.png", "sheet": [2, 1], "frame": 0},
              Animator={"clip": "breathe", "looping": True}, SpriteAnim={"clips": "breathe: 0-1 fps=2 loop"},
              Dialogue={"script": DIALOGUE, "text_ui": 1401, "speaker_ui": 1402, "choice_1": 1403, "choice_2": 1404, "speed": 50})
        spawn(1401, "Dialogue text", Ui={"pos": [0.1, 0.7], "size": [0.8, 0.2], "color": [0.1, 0.12, 0.26, 0.95], "border": 0.004, "border_color": [0.8, 0.85, 1, 0.9], "text": "", "text_size": 0.036, "text_color": [1, 1, 1, 1], "button": True, "visible": False, "layer": 4},
              UiStyle={"radius": 0.03, "color2": [0.04, 0.05, 0.12, 0.95], "shadow_color": [0, 0, 0, 0.5], "shadow_offset": [0.006, 0.01]}, TextFont={"file": FONT, "wrap": 0.74, "line_spacing": 0.3})
        spawn(1402, "Dialogue speaker", Text={"value": "", "pos": [0.12, 0.655], "size": 0.04, "color": [1, 0.9, 0.4, 1], "shadow": True, "visible": False}, TextFont={"file": FONT})
        for k in range(2):
            button(1403 + k, f"Choice {k + 1}", "", (0.3, 0.45 + 0.09 * k), (0.4, 0.075), {}, layer=5, color=(0.2, 0.55, 0.35, 0.95), color2=(0.08, 0.3, 0.18, 0.95), text_size=0.03, visible=False)
        ui_text(1405, "Talk hint", "{#npc.talk}", (0.5, 0.86), 0.04, color=(1, 1, 1, 1), visible=False, TalkHint={})
    # the pause menu
    spawn(1200, "Pause dim", PauseUi={}, Ui={"pos": [0, 0], "size": [1, 1], "color": [0, 0, 0, 0.62], "layer": 20, "visible": False})
    ui_text(1210, "Paused title", "{#pause.title}", (0.5, 0.26), 0.09, color=(1, 0.95, 0.75, 1), PauseUi={})
    button(1201, "Resume", "{#pause.resume}", (0.36, 0.42), (0.28, 0.09), {"PauseUi": {}, "PauseButton": {"kind": 1}}, layer=21, visible=False)
    button(1202, "To menu", "{#pause.menu}", (0.36, 0.55), (0.28, 0.09), {"PauseUi": {}, "PauseButton": {"kind": 2}}, layer=21, visible=False)

    # ---- the rules
    call("define_script", {"source": fader_script(f"        $level = {n};\n") + LEVEL_SCRIPT})
    for name, value in (("level", n), ("hp", 100), ("coins", 0), ("keys", 0), ("score", 0), ("kills", 0), ("paused", 0), ("leave", 0), ("face.x", 0), ("face.y", -1), ("hero.x", sx), ("hero.y", sy), ("gift", 0), ("near", 0), ("talking", 0),
                        ("app.quit_on_escape", 0)):
        call("set_resource", {"name": name, "value": value})
    show_check(f"level {n}")
    call("save_scene", {"path": level_file(n)})
    return dict(seed=seed, start=start, exit=exit_cell, door=door, key=key, path_len=len(path))


EN = {
    "hud": {"level": "LEVEL", "score": "SCORE", "hint": "WASD: walk     SPACE: sword     E: talk     ESC: pause"},
    "level1": {"name": "THE CELLARS"}, "level2": {"name": "THE CAVES"}, "level3": {"name": "THE CATACOMBS"},
    "pause": {"title": "PAUSED", "resume": "Resume", "menu": "Main menu"},
    "npc": {"talk": "Press E to talk", "hello": "Careful, stranger. These cellars are full of grunts.", "door": "A locked door bars the way to the stairs. Find the key first.",
            "q_advice": "Any advice?", "q_fine": "I will manage", "advice": "Swing your sword with SPACE and keep moving. Torchlight is your friend. Take 20 gold for the road.",
            "again": "I have told you all I know. Go on.", "bye": "Then good luck, and mind the dark."},
    "menu": {"title": "DEPTHS", "subtitle": "a dungeon crawler made with Lunaris", "play": "Play", "continue": "Continue", "credits": "Credits", "quit": "Quit", "back": "Back",
             "nosave": "No saved game yet", "hint": "W S or arrows to choose, Enter to confirm, or use the mouse"},
    "credits": {"title": "CREDITS", "text": "Depths is a small game made to test the Lunaris engine: every level, picture and sound was made by scripts that call the engine's tools. No outside art.\n\nEngine: Lunaris\nDesign, art, sound and rules: written as data and Lunaris Script"},
    "over": {"title": "YOU DIED", "stats": "Level {level}      Score {score}", "retry": "Retry level", "menu": "Main menu"},
    "win": {"title": "YOU ESCAPED!", "score": "FINAL SCORE", "stats": "Coins {coins}      Guards slain {kills}", "again": "Play again", "menu": "Main menu",
            "rank_s": "RANK S  -  a legend of the depths", "rank_a": "RANK A  -  a hero", "rank_b": "RANK B  -  a survivor", "rank_c": "RANK C  -  lucky to be alive"},
}
DE = {
    "hud": {"level": "EBENE", "score": "PUNKTE", "hint": "WASD: gehen     LEERTASTE: Schwert     E: sprechen     ESC: Pause"},
    "level1": {"name": "DIE KELLER"}, "level2": {"name": "DIE HOEHLEN"}, "level3": {"name": "DIE KATAKOMBEN"},
    "pause": {"title": "PAUSE", "resume": "Weiter", "menu": "Hauptmenue"},
    "npc": {"talk": "E druecken zum Sprechen", "hello": "Vorsicht, Fremder. In diesen Kellern wimmelt es von Grunzern.", "door": "Eine verschlossene Tuer versperrt den Weg zur Treppe. Such zuerst den Schluessel.",
            "q_advice": "Ein Rat?", "q_fine": "Ich schaffe das", "advice": "Schwing dein Schwert mit LEERTASTE und bleib in Bewegung. Fackellicht ist dein Freund. Nimm 20 Gold mit.",
            "again": "Mehr weiss ich nicht. Geh weiter.", "bye": "Dann viel Glueck, und huete dich vor der Dunkelheit."},
    "menu": {"title": "DEPTHS", "subtitle": "ein Dungeon-Spiel mit Lunaris", "play": "Spielen", "continue": "Fortsetzen", "credits": "Mitwirkende", "quit": "Beenden", "back": "Zurueck",
             "nosave": "Noch kein Spielstand", "hint": "W S oder Pfeile waehlen, Enter bestaetigt, oder die Maus"},
    "credits": {"title": "MITWIRKENDE", "text": "Depths ist ein kleines Spiel zum Testen der Lunaris-Engine: jede Ebene, jedes Bild und jeder Klang entstand durch Skripte, die die Werkzeuge der Engine aufrufen. Keine fremde Grafik.\n\nEngine: Lunaris\nEntwurf, Grafik, Klang und Regeln: als Daten und Lunaris Script geschrieben"},
    "over": {"title": "DU BIST GESTORBEN", "stats": "Ebene {level}      Punkte {score}", "retry": "Ebene wiederholen", "menu": "Hauptmenue"},
    "win": {"title": "DU BIST ENTKOMMEN!", "score": "ENDSTAND", "stats": "Muenzen {coins}      Wachen besiegt {kills}", "again": "Nochmal spielen", "menu": "Hauptmenue",
            "rank_s": "RANG S  -  eine Legende der Tiefe", "rank_a": "RANG A  -  ein Held", "rank_b": "RANG B  -  ein Ueberlebender", "rank_c": "RANG C  -  mit Glueck davongekommen"},
}


def write_lang():
    os.makedirs(f"{D}/lang", exist_ok=True)
    for name, data in (("en", EN), ("de", DE)):
        with open(f"{D}/lang/{name}.json", "w", encoding="utf-8", newline="\n") as f:
            json.dump(data, f, indent=2, ensure_ascii=True)
            f.write("\n")


MIXER = "bus master volume=1\nbus music parent=master volume=0.55\nbus sfx parent=master volume=0.9\nroute examples/depths/music_* -> music\nroute * -> sfx"
THEME = f"{D}/music_theme.wav"
DEPTHS = f"{D}/music_depths.wav"


def build_boot():
    """The start scene: the music and the mixer (they `Persist`, so they play on through every scene change) and a switch to the menu that goes off at once."""
    call("reset", {"seed": 1})
    define_components(["ThemeTrack", "DepthsTrack"])
    spawn(9000, "Theme music", Persist={}, ThemeTrack={}, MusicPlayer={"playlist": THEME, "crossfade": 3.0, "volume": 0.8, "repeat": True, "playing": True})
    spawn(9002, "Depths music", Persist={}, DepthsTrack={}, MusicPlayer={"playlist": DEPTHS, "crossfade": 3.0, "volume": 0.8, "repeat": True, "playing": False})
    spawn(9001, "Mixer", Persist={}, AudioMixer={"config": MIXER})
    spawn(2, "Camera", Transform={"pos": [0, 0, 10]}, Camera={"ortho": True, "ortho_height": 10, "clear": [0.02, 0.015, 0.03, 1]})
    spawn(3, "To the menu", SceneSwitch={"path": f"{D}/menu.lunaris.json", "go": True})
    show_check("boot")
    call("save_scene", {"path": f"{D}/boot.lunaris.json"})


MENU_SCRIPT = """
system menu_nav each UiNav order 5 {
    if $credits == 1 {
        UiNav.current = 1305;
    } else {
        if UiNav.current == 0 || UiNav.current == 1305 {
            UiNav.current = 1301;
        }
    }
}

system main_show each MainUi, Ui order 6 {
    Ui.visible = $credits == 0;
}

system credits_show each CreditsUi, Ui order 6 {
    Ui.visible = $credits == 1;
}

system credits_show_text each CreditsUi, Text order 6 {
    Text.visible = $credits == 1;
}

system menu_buttons each MenuButton, Ui order 20 {
    if Ui.clicked && $leave == 0 {
        emit "select";
        if MenuButton.kind == 1 {
            $level = 1;
            $hp = 100;
            $coins = 0;
            $keys = 0;
            $score = 0;
            $kills = 0;
            $leave = 1;
        }
        if MenuButton.kind == 2 {
            $game.error = 0;
            $game.load = 1;
        }
        if MenuButton.kind == 3 {
            $credits = 1;
        }
        if MenuButton.kind == 4 {
            $app.quit = 1;
        }
        if MenuButton.kind == 5 {
            $credits = 0;
        }
    }
}

system continue_look each ContinueButton, Ui order 21 {
    Ui.color[3] = select($"game.has.1" > 0, 0.96, 0.45);
}

system nosave_text each NoSave, Text order 21 {
    Text.visible = $game.error > 0 && $credits == 0;
}

system music_theme_on each ThemeTrack, MusicPlayer order 80 {
    MusicPlayer.playing = true;
}

system music_depths_off each DepthsTrack, MusicPlayer order 80 {
    MusicPlayer.playing = false;
}
"""


def build_menu():
    call("reset", {"seed": 1})
    define_components(["Fader", "SwitchId", "MenuButton", "ContinueButton", "NoSave", "CreditsUi", "MainUi", "ThemeTrack", "DepthsTrack"])
    W, H = 100, 64
    spawn(10, "Backdrop", Transform={"pos": [0, 0, -0.05]}, Tilemap={"texture": f"{D}/tiles_dungeon.png", "sheet": [19, 1], "size": [W, H], "tile": 1.0})
    info = call("generate_map", {"uid": 10, "mode": "rooms", "seed": 31, "width": W, "height": H, "floor": 0, "wall": 1, "points": 4, "clear": 4, "loops": 3})
    call("autotile", {"uid": 10, "base": 3, "mode": "4", "solid": [1], "edge": "solid"})
    # the camera drifts in a loop through the rooms near the middle of the map (so that the edge of the map is never in the picture), taken in the order of the angle round the middle
    pts = []
    for r in info.get("rooms", []):
        x, y = r["x"] + r["w"] / 2 - W / 2, H / 2 - (r["y"] + r["h"] / 2)
        if abs(x) < 30 and abs(y) < 16:
            pts.append((x, y))
    pts.sort(key=lambda p: math.atan2(p[1], p[0]))
    pts = pts[:8]
    print(f"  menu backdrop: {len(pts)} rooms on the camera's way")
    pts_text = "; ".join(f"{x:.2f} {y:.2f} 0" for x, y in pts)
    spawn(3, "Camera", Transform={"pos": [pts[0][0], pts[0][1], 10]}, Camera={"ortho": True, "ortho_height": 13, "clear": [0.02, 0.015, 0.03, 1]}, CameraRig={"target": 40, "follow_speed": 2.5, "offset": [0, 0, 10], "look_at": False})
    spawn(5, "Look", Environment={"sky": False, "bloom": 0.5, "bloom_threshold": 0.7, "vignette": 0.3, "saturation": 1.08})
    spawn(6, "Localization", Localization={"language": LANG})
    input_map()
    fader(900, {1: level_file(1)})
    spawn(910, "Nav", UiNav={"current": 1301})
    sounds()
    spawn(39, "Camera path", Transform={"pos": [0, 0, 0]}, Spline={"points": pts_text, "smooth": True, "closed": True})
    spawn(40, "Camera drift", Transform={"pos": [pts[0][0], pts[0][1], 0]}, PathFollower={"path": 39, "speed": 1.8, "mode": 1})
    spawn(41, "Lantern", Parent={"target": 40}, LocalTransform={"pos": [0, 0, 0.5]}, Transform={"pos": [pts[0][0], pts[0][1], 0.5]}, Sprite={"size": [60, 60], "texture": f"{D}/lantern.png"})
    # torches along the camera's way
    for i, (x, y) in enumerate(pts):
        spawn(4000 + i * 2, f"Flame {i + 1}", Transform={"pos": [x + 1.5, y + 1.2, 0.3]}, Particles={**FLAME, "seed": 3 + i}, ParticleModules={"modules": FLAME_MODULES})
        spawn(4001 + i * 2, f"Glow {i + 1}", Transform={"pos": [x + 1.5, y + 0.8, 0.3]}, Particles={"count": 1, "lifetime": 100, "looping": True, "prewarm": True, "speed_range": [0, 0], "size": [6, 6], "color": [1, 0.55, 0.18, 0.22], "color_end": [1, 0.55, 0.18, 0.22], "additive": True, "texture": "soft"})
    ui_text(1290, "Title", "{#menu.title}", (0.5, 0.1), 0.17, color=(1, 0.82, 0.3, 1))
    ui_text(1291, "Subtitle", "{#menu.subtitle}", (0.5, 0.3), 0.034, color=(0.85, 0.85, 1, 0.9))
    labels = [("Play", "menu.play", 1301, 1, {}), ("Continue", "menu.continue", 1302, 2, {"ContinueButton": {}}), ("Credits", "menu.credits", 1303, 3, {}), ("Quit", "menu.quit", 1304, 4, {})]
    for i, (name, key, uid, kind, extra) in enumerate(labels):
        button(uid, f"{name} button", f"{{#{key}}}", (0.36, 0.43 + 0.115 * i), (0.28, 0.09), {"MenuButton": {"kind": kind}, "MainUi": {}, **extra}, layer=3)
    ui_text(1292, "No save", "{#menu.nosave}", (0.5, 0.905), 0.03, color=(1, 0.6, 0.5, 1), NoSave={})
    ui_text(1293, "Hint", "{#menu.hint}", (0.5, 0.955), 0.026, color=(1, 1, 1, 0.75))
    # the credits
    spawn(1310, "Credits dim", CreditsUi={}, Ui={"pos": [0, 0], "size": [1, 1], "color": [0, 0, 0, 0.78], "layer": 20, "visible": False})
    ui_text(1311, "Credits title", "{#credits.title}", (0.5, 0.14), 0.09, color=(1, 0.82, 0.3, 1), CreditsUi={})
    spawn(1312, "Credits text", CreditsUi={}, Text={"value": "{#credits.text}", "pos": [0.5, 0.3], "size": 0.036, "align": 1, "color": [1, 1, 1, 1], "shadow": True, "visible": False}, TextFont={"file": FONT, "wrap": 0.7, "line_spacing": 0.4})
    button(1305, "Back button", "{#menu.back}", (0.36, 0.78), (0.28, 0.09), {"MenuButton": {"kind": 5}, "CreditsUi": {}}, layer=21, visible=False)
    call("define_script", {"source": fader_script("        $credits = 0;\n        $game.error = 0;\n        $game.probe = 1;\n") + MENU_SCRIPT})
    for name, value in (("credits", 0), ("app.quit_on_escape", 0)):
        call("set_resource", {"name": name, "value": value})
    show_check("menu")
    call("save_scene", {"path": f"{D}/menu.lunaris.json"})


OVER_SCRIPT = """
system over_buttons each OverButton, Ui order 20 {
    if Ui.clicked && $leave == 0 {
        emit "select";
        if OverButton.kind == 1 {
            $hp = $s.hp;
            $coins = $s.coins;
            $keys = $s.keys;
            $score = $s.score;
            $leave = 10 + $level;
        } else {
            $leave = 3;
        }
    }
}

system over_nav each UiNav order 5 {
    if UiNav.current == 0 {
        UiNav.current = 1301;
    }
}

system music_theme_on each ThemeTrack, MusicPlayer order 80 {
    MusicPlayer.playing = true;
}

system music_depths_off each DepthsTrack, MusicPlayer order 80 {
    MusicPlayer.playing = false;
}
"""


def build_gameover():
    call("reset", {"seed": 1})
    define_components(["Fader", "SwitchId", "OverButton", "ThemeTrack", "DepthsTrack"])
    spawn(3, "Camera", Transform={"pos": [0, 0, 10]}, Camera={"ortho": True, "ortho_height": 10, "clear": [0.07, 0.01, 0.02, 1]})
    spawn(5, "Look", Environment={"sky": False, "bloom": 0.3, "vignette": 0.5, "saturation": 0.8})
    spawn(6, "Localization", Localization={"language": LANG})
    input_map()
    fader(900, {3: f"{D}/menu.lunaris.json", 11: level_file(1), 12: level_file(2), 13: level_file(3)})
    spawn(910, "Nav", UiNav={"current": 1301})
    sounds()
    ui_text(1290, "Game over title", "{#over.title}", (0.5, 0.17), 0.16, color=(0.9, 0.18, 0.2, 1))
    ui_text(1291, "Stats", "{#over.stats}", (0.5, 0.42), 0.05, color=(1, 0.95, 0.8, 1))
    button(1301, "Retry button", "{#over.retry}", (0.34, 0.58), (0.32, 0.1), {"OverButton": {"kind": 1}}, text_size=0.042)
    button(1302, "Menu button", "{#over.menu}", (0.34, 0.72), (0.32, 0.1), {"OverButton": {"kind": 2}}, text_size=0.042)
    call("define_script", {"source": fader_script('        emit "lose";\n') + OVER_SCRIPT})
    for name, value in (("level", 1), ("s.hp", 100), ("s.coins", 0), ("s.keys", 0), ("s.score", 0), ("app.quit_on_escape", 0)):
        call("set_resource", {"name": name, "value": value})
    show_check("gameover")
    call("save_scene", {"path": f"{D}/gameover.lunaris.json"})


WIN_SCRIPT = """
system win_buttons each WinButton, Ui order 20 {
    if Ui.clicked && $leave == 0 {
        emit "select";
        if WinButton.kind == 1 {
            $level = 1;
            $hp = 100;
            $coins = 0;
            $keys = 0;
            $score = 0;
            $kills = 0;
            $leave = 11;
        } else {
            $leave = 3;
        }
    }
}

system win_nav each UiNav order 5 {
    if UiNav.current == 0 {
        UiNav.current = 1301;
    }
}

system rank_show each Rank, Text order 20 {
    Text.visible = $score >= Rank.min && $score < Rank.max;
}

system music_theme_on each ThemeTrack, MusicPlayer order 80 {
    MusicPlayer.playing = true;
}

system music_depths_off each DepthsTrack, MusicPlayer order 80 {
    MusicPlayer.playing = false;
}
"""


def build_win():
    call("reset", {"seed": 1})
    define_components(["Fader", "SwitchId", "WinButton", "Rank", "ThemeTrack", "DepthsTrack"])
    spawn(3, "Camera", Transform={"pos": [0, 0, 10]}, Camera={"ortho": True, "ortho_height": 10, "clear": [0.02, 0.03, 0.06, 1]})
    spawn(5, "Look", Environment={"sky": False, "bloom": 0.6, "bloom_threshold": 0.6, "vignette": 0.35, "saturation": 1.15})
    spawn(6, "Localization", Localization={"language": LANG})
    input_map()
    fader(900, {3: f"{D}/menu.lunaris.json", 11: level_file(1)})
    spawn(910, "Nav", UiNav={"current": 1301})
    sounds()
    # a fountain of gold, because it is the end
    spawn(1280, "Gold", Transform={"pos": [0, -3.5, 0.3]}, Particles={"count": 400, "lifetime": 2.4, "looping": True, "prewarm": True, "shape": 4, "extent": [3.2, 0, 0], "direction": [0, 1, 0], "cone": 20,
                                                                      "speed_range": [2.5, 6.5], "gravity": [0, -4.2, 0], "size": [0.22, 0.05], "color": [1, 0.85, 0.3, 1], "color_end": [1, 0.5, 0.1, 0], "additive": True, "seed": 2},
          ParticleModules={"modules": "size 0:0.22 1:0.04\nnoise strength=0.3 scale=0.8 speed=0.6"})
    ui_text(1290, "Win title", "{#win.title}", (0.5, 0.1), 0.15, color=(1, 0.85, 0.3, 1))
    ui_text(1291, "Score label", "{#win.score}", (0.5, 0.32), 0.04, color=(0.8, 0.85, 1, 1))
    ui_text(1292, "Final score", "{score}", (0.5, 0.38), 0.13, color=(1, 1, 1, 1))
    ui_text(1293, "Stats", "{#win.stats}", (0.5, 0.58), 0.036, color=(1, 0.95, 0.8, 1))
    for i, (key, lo, hi) in enumerate([("rank_s", 1500, 1e9), ("rank_a", 1000, 1500), ("rank_b", 600, 1000), ("rank_c", -1e9, 600)]):
        ui_text(1294 + i, f"Rank {key[-1].upper()}", f"{{#win.{key}}}", (0.5, 0.66), 0.045, color=(1, 0.9, 0.5, 1), Rank={"min": lo, "max": hi})
    button(1301, "Again button", "{#win.again}", (0.3, 0.78), (0.19, 0.09), {"WinButton": {"kind": 1}}, text_size=0.035)
    button(1302, "Menu button", "{#win.menu}", (0.51, 0.78), (0.19, 0.09), {"WinButton": {"kind": 2}}, text_size=0.035)
    call("define_script", {"source": fader_script('        emit "win";\n') + WIN_SCRIPT})
    for name, value in (("app.quit_on_escape", 0),):
        call("set_resource", {"name": name, "value": value})
    show_check("win")
    call("save_scene", {"path": f"{D}/win.lunaris.json"})


def stub_scenes():
    """The scenes name each other (a level the next one, the game over screen every level), and `check` says a switch to a file that is not there is an error: until the
    real files exist a stub stands in for each, so that every scene is checked against a complete game."""
    for name in ("boot", "menu", "level1", "level2", "level3", "gameover", "win"):
        path = f"{D}/{name}.lunaris.json"
        if not os.path.exists(path):
            with open(path, "w", encoding="utf-8", newline="\n") as f:
                f.write('{"lunaris": 1, "entities": []}\n')


def main():
    only = sys.argv[1:]
    os.makedirs(D, exist_ok=True)
    stub_scenes()
    start()
    write_lang()
    info = {}
    for cfg in LEVELS:
        if not only or f"level{cfg['n']}" in only:
            info[cfg["n"]] = build_level(cfg)
    for name, fn in (("boot", build_boot), ("menu", build_menu), ("gameover", build_gameover), ("win", build_win)):
        if not only or name in only:
            fn()
    finish()
    missing = check_language_keys()
    print(f"done ({n_requests} requests, {failed} failed, {missing} text keys missing from a language)")
    return 1 if failed or missing else 0


def check_language_keys():
    """Every `{#key}` that a scene uses must be in both language files, or the player sees the key."""
    import re

    def flat(d, prefix=""):
        out = set()
        for k, v in d.items():
            out |= flat(v, prefix + k + ".") if isinstance(v, dict) else {prefix + k}
        return out

    have = {"en": flat(EN), "de": flat(DE)}
    used = set()
    for name in ("boot", "menu", "level1", "level2", "level3", "gameover", "win"):
        with open(f"{D}/{name}.lunaris.json", encoding="utf-8") as f:
            used |= set(re.findall(r"\{#([A-Za-z0-9_.]+)\}", f.read()))
    bad = 0
    for lang, keys in have.items():
        for k in sorted(used - keys):
            print(f"  language {lang} has no text for `{k}`")
            bad += 1
    return bad


if __name__ == "__main__":
    sys.exit(main())
