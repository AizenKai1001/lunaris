"""The shared parts of `tools/build_caldera.py`: talking to the engine over stdin/stdout, the language registry (`tr`), the data types of the game, small interface helpers, the world (sky, terrain, water, camp, ruins, trees).
"""
import collections
import json
import math
import os
import random
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import caldera_scripts as S  # noqa: E402

BIN = "target/release/lunaris"
D = "examples/caldera"
W = f"{D}/world"
M = f"{D}/models"
FONT = "examples/fonts/pixel.ttf"
LANG = f"{D}/lang/en"
LANGS = f"{D}/lang/en, {D}/lang/de"
SEA, LAKE = 5.0, 9.5

# ------------------------------------------------------------------------------------------------ the engine, spoken to over stdin/stdout
proc = None
n_requests = 0
failed = 0


def start():
    global proc, n_requests, failed
    proc = subprocess.Popen([BIN, "serve", "--root", ".", "--no-gpu"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    n_requests = failed = 0


def call(name, args=None, quiet=False):
    """One tool call; returns the tool's own JSON answer (an empty dict, and a line on stdout, when it failed; `quiet` for a call that may fail: no line, not counted)."""
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
        if quiet:
            return {}
        failed += 1
        print("request", n_requests, name, "failed:", json.dumps(m.get("error") or res).replace('\n', chr(10))[:3500])
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
    proc.wait(timeout=120)


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
    return chk


# ------------------------------------------------------------------------------------------------ the texts: written where they are used, collected into the language files
TEXTS = collections.OrderedDict()


def tr(key, en, de):
    """`{#key}` for a Text or Ui; the English and the German are remembered for the language files."""
    TEXTS[key] = (en, de)
    return "{#" + key + "}"


def write_lang():
    """The language files: what is in them already, with the texts of the scenes built this time put in (so that building one scene does not lose the texts of the others)."""
    os.makedirs(f"{D}/lang", exist_ok=True)
    for idx, name in ((0, "en"), (1, "de")):
        path = f"{D}/lang/{name}.json"
        try:
            with open(path, encoding="utf-8") as f:
                tree = json.load(f)
        except (OSError, ValueError):
            tree = {}
        for key, pair in TEXTS.items():
            node = tree
            parts = key.split(".")
            for part in parts[:-1]:
                node = node.setdefault(part, {})
                if not isinstance(node, dict):
                    raise SystemExit(f"text key `{key}` clashes with another key")
            node[parts[-1]] = pair[idx]
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            json.dump(tree, f, indent=2, ensure_ascii=True)
            f.write("\n")


# ------------------------------------------------------------------------------------------------ the data types of the game
F = "f32"
COMPONENTS = {
    "Hero": [("state", F), ("t", F), ("cd", F), ("stamina", F), ("exhaust", F), ("regen", F), ("fx", F), ("fz", F), ("rx", F), ("rz", F), ("swim", F), ("moving", F), ("sprint", F), ("combo", F), ("air", F)],
    "Foe": [], "Golem": [("mode", F), ("t", F), ("cd", F), ("repath", F), ("wake_r", F), ("dist", F), ("home_x", F), ("home_z", F), ("leash", F), ("boss", F), ("attack", F), ("combo", F), ("rest", F),
                         ("hitdone", F), ("dirx", F), ("dirz", F), ("fx", F), ("fz", F), ("tx", F), ("tz", F), ("dmg", F)],
    "Wraith": [("slot", F), ("state", F), ("t", F), ("cd", F), ("dist", F)],
    "Shard": [("n", F), ("t", F), ("base_y", F), ("locked", F)],
    "Campfire": [], "Altar": [], "Beacon": [], "BeaconFire": [], "BeaconFlame": [], "Socket": [("n", F)], "Npc": [], "HeroLight": [],
    "Fader": [("t", F), ("out", F), ("leaving", F), ("target", F), ("fired", F)], "SwitchId": [("n", F)],
    "PauseUi": [], "PauseBtn": [("kind", F)], "SettingsUi": [], "SettingBtn": [("kind", F)], "SettingSlider": [("kind", F)],
    "HudHp": [], "HudSt": [], "HudShard": [("n", F)], "HudItem": [], "PromptText": [("n", F)], "ToastText": [("n", F)], "CompassMark": [("kind", F), ("n", F)],
    "MapCam": [], "NpcLabel": [], "MapBlip": [], "DeadUi": [], "HurtFlash": [], "QuestUi": [], "QuestRow": [("n", F)], "QuestRowText": [("n", F)],
    "FxPool": [("kind", F)], "SplashFx": [], "MusicTrack": [("id", F)], "MenuBtn": [("kind", F)], "MenuUi": [], "ContinueBtn": [], "NoSaveText": [], "Spin": [("speed", F)],
    "BossBar": [], "BossBarText": [], "Hint": [("t", F)], "OverBtn": [("kind", F)], "WinBtn": [("kind", F)], "Orbit": [("speed", F)], "Tree": [], "Prop": [], "CreditsUi": [],
}


def define_components(names):
    for n in names:
        call("define_component", {"name": n, "fields": [{"name": f, "type": t, "default": 0} for f, t in COMPONENTS[n]]})


# ------------------------------------------------------------------------------------------------ small helpers for the interface
def ui_text(uid, name, text, pos, size, color=(1, 1, 1, 1), align=1, visible=True, shadow=True, wrap=0.0, **extra):
    font = {"file": FONT, "wrap": wrap} if wrap else {"file": FONT}
    spawn(uid, name, Text={"value": text, "pos": list(pos), "size": size, "align": align, "color": list(color), "shadow": shadow, "visible": visible}, TextFont=font, **extra)


def panel(uid, name, pos, size, color=(0.04, 0.05, 0.08, 0.62), radius=0.012, layer=1, border=0.0, **extra):
    ui = {"pos": list(pos), "size": list(size), "color": list(color), "layer": layer}
    if border:
        ui.update({"border": border, "border_color": [0.95, 0.8, 0.45, 0.8]})
    comps = {"Ui": ui, "UiStyle": {"radius": radius}}
    comps.update(extra)
    spawn(uid, name, **comps)


def button(uid, name, text, pos, size, tags, layer=3, color=(0.62, 0.34, 0.14, 0.96), color2=(0.34, 0.16, 0.08, 0.96), text_size=0.04, visible=True):
    comps = {"Ui": {"pos": list(pos), "size": list(size), "color": list(color), "text": text, "text_size": text_size, "text_color": [1, 0.97, 0.88, 1], "button": True, "layer": layer, "visible": visible,
                    "border": 0.003, "border_color": [1.0, 0.82, 0.5, 0.9]},
             "UiStyle": {"radius": 0.018, "color2": list(color2), "shadow_color": [0, 0, 0, 0.5], "shadow_offset": [0.004, 0.008]},
             "TextFont": {"file": FONT}}
    comps.update(tags)
    spawn(uid, name, **comps)


def slider(uid, name, pos, size, value, lo, hi, kind, layer=3, visible=True, **extra):
    spawn(uid, name, Ui={"pos": list(pos), "size": list(size), "color": [0.95, 0.66, 0.22, 1], "back_color": [0.1, 0.07, 0.05, 0.9], "button": True, "fill": (value - lo) / (hi - lo), "layer": layer,
                         "visible": visible, "border": 0.002, "border_color": [1, 0.85, 0.55, 0.9]},
          UiStyle={"radius": 0.01}, UiSlider={"value": value, "min": lo, "max": hi, "step": 0}, SettingSlider={"kind": kind}, **extra)


SOUNDS = [  # name, wave (0 sine 1 square 2 saw 3 triangle 4 noise), freq, freq_end, seconds, volume
    ("swing", 4, 700, 200, 0.16, 0.16), ("golem_hit", 2, 260, 70, 0.18, 0.3), ("wraith_hit", 3, 900, 300, 0.2, 0.22), ("golem_die", 2, 200, 40, 0.5, 0.28), ("golem_break", 4, 300, 30, 0.7, 0.36),
    ("golem_wake", 2, 70, 140, 0.9, 0.3), ("golem_swing", 4, 220, 90, 0.3, 0.14), ("golem_thud", 2, 120, 40, 0.3, 0.38), ("boss_slam", 2, 90, 30, 0.6, 0.45), ("golem_roar", 2, 120, 220, 0.8, 0.3),
    ("boss_tired", 3, 300, 90, 0.8, 0.22), ("hurt", 2, 240, 80, 0.25, 0.3), ("roll", 4, 500, 150, 0.26, 0.1), ("jump", 3, 280, 520, 0.12, 0.1), ("splash", 4, 900, 200, 0.4, 0.22),
    ("shard", 3, 660, 1580, 0.5, 0.3), ("shard_appear", 0, 880, 1320, 0.6, 0.24), ("deposit", 3, 440, 880, 0.7, 0.28), ("save", 0, 523, 784, 0.5, 0.22), ("beacon", 3, 220, 880, 2.4, 0.36),
    ("select", 1, 520, 780, 0.08, 0.12), ("win", 3, 523, 1046, 1.2, 0.3), ("lose", 2, 220, 55, 1.2, 0.3), ("wraith_rise", 0, 180, 90, 0.7, 0.14), ("wraith_bite", 2, 400, 120, 0.2, 0.2),
    ("step", 4, 240, 120, 0.05, 0.05), ("golem_step", 2, 90, 40, 0.12, 0.18), ("dialogue_start", 0, 620, 760, 0.1, 0.12),
]


def sounds():
    for i, (name, wave, f0, f1, dur, vol) in enumerate(SOUNDS):
        spawn(3000 + i, f"Sound {name}", Sound={"name": name, "wave": wave, "freq": f0, "freq_end": f1, "duration": dur, "volume": vol})


INPUT_MAP = "\n".join([
    "move_x = d, a:-1, gamepad.lx, arrowright, arrowleft:-1", "move_y = w, s:-1, gamepad.ly, arrowup, arrowdown:-1", "look_x = gamepad.rx", "look_y = gamepad.ry",
    "attack = mouse_left, j, gamepad.x, gamepad.rt", "roll = mouse_right, q, gamepad.b, gamepad.lt", "jump = space, gamepad.a", "sprint = shift, gamepad.ls, gamepad.lb",
    "interact = e, gamepad.y", "quest = l, gamepad.back", "pause = escape, gamepad.start",
    "dialogue_next = enter, space, gamepad.a", "dialogue_choice_1 = 1", "dialogue_choice_2 = 2, gamepad.b", "dialogue_choice_3 = 3, gamepad.x", "dialogue_choice_4 = 4, gamepad.y",
    "ui_up = w, arrowup, gamepad.dpad_up", "ui_down = s, arrowdown, gamepad.dpad_down", "ui_left = a, arrowleft, gamepad.dpad_left", "ui_right = d, arrowright, gamepad.dpad_right",
    "ui_accept = enter, gamepad.a", "deadzone = 0.15"])


def input_map(uid=5):
    spawn(uid, "Input", InputMap={"bindings": INPUT_MAP})


def fader(uid, switches):
    """The fade in at the start of a scene, the fade out before leaving it, and the switches (`SceneSwitch`) the scene can throw: `switches` maps a number to (scene file, resources to clear)."""
    spawn(uid, "Fader", Fader={}, ColorGrade={"fade": [0, 0, 0, 1]})
    for i, (n, (path, reset)) in enumerate(switches.items()):
        comps = {"SwitchId": {"n": n}, "SceneSwitch": {"path": path}}
        if reset:
            comps["SceneReset"] = {"resources": reset}
        spawn(uid + 1 + i, f"Switch {n}", **comps)


# what a scene entered by a switch starts afresh: the state of a run (`g.*`), and the traffic of the last scene
CLEAN = "paused leave fire time.scale cutscene talking settings questlog dead dead.t hitstop swing dmg shake flash fxgo splash prompt toast toast.k toast.t tgt tgt.d wraith.next wraith.timer lit.t boss.mode kills"
NEW_RUN = CLEAN + " g.*"

FOREST_BARK = [0.3, 0.2, 0.12, 1]


# ------------------------------------------------------------------------------------------------ the world: terrain, water, sky, trees, rocks, the camp and the places of the shards
WORLD = json.load(open(f"{W}/world.json")) if os.path.exists(f"{W}/world.json") else {}
LAYERS = (f"grass grass uv_scale=340 bump=0.02\nrock dirt uv_scale=260 tint=0.8,0.76,0.72 bump=0.06\nsand sand uv_scale=340 bump=0.01\nash {W}/ash.png uv_scale=180")


def th(x, z):
    """The height of the ground, as the engine says."""
    return call("terrain_height", {"x": x, "z": z}).get("height", 0.0)


def layer_at(x, z):
    r = call("terrain_layer", {"x": x, "z": z})
    return r.get("dominant_name") or r.get("dominant")


class Uids:
    def __init__(self, first):
        self.n = first

    def __call__(self):
        self.n += 1
        return self.n - 1


def prop_box(u, name, pos, size, color, texture="", rot=None, uv=None, collide=True, rough=0.9, emissive=0.0, metallic=0.0, shadow=True):
    comps = {"Prop": {}, "Transform": {"pos": list(pos), "scale": list(size)}, "MeshRenderer": {"mesh": "cube", "color": list(color) + [1], "roughness": rough, "emissive": emissive, "metallic": metallic, "cast_shadow": shadow}}
    if texture:
        comps["MeshRenderer"]["texture"] = texture
        comps["MeshRenderer"]["uv_scale"] = uv or [max(1, round(size[0])), max(1, round(size[1]))]
    if rot:
        comps["Transform"]["rot"] = list(rot)
    if collide:
        comps["Collider"] = {"shape": 1, "size": list(size)}
    spawn(u(), name, **comps)


def quat_y(deg):
    h = math.radians(deg) / 2
    return [0, math.sin(h), 0, math.cos(h)]


def ruin(u, cx, cz, yaw, name, size=8.0, tall=3.4, gaps=(1, 4)):
    """A broken square of walls and columns standing on the ground: four walls with gaps, columns at the corners, a floor of flagstones. Walls follow the ground at their middle."""
    stone = [0.52, 0.5, 0.46]
    cy, sy_ = math.cos(math.radians(yaw)), math.sin(math.radians(yaw))

    def at(lx, lz):
        return cx + lx * cy + lz * sy_, cz - lx * sy_ + lz * cy

    gy = th(cx, cz)
    prop_box(u, f"{name} floor", [cx, gy + 0.02, cz], [size, 0.25, size], [0.42, 0.41, 0.4], texture="stone", rot=quat_y(yaw), uv=[3, 3], collide=False)
    half = size / 2
    for k, (lx, lz, w, d) in enumerate([(0, -half, size, 0.6), (half, 0, 0.6, size), (0, half, size, 0.6), (-half, 0, 0.6, size)]):
        if k in gaps:
            # a wall with a doorway: two stumps
            for sgn in (-1, 1):
                stump = (size - 4.0) / 2  # (a doorway four metres wide: the path planner works on cells of a metre over a long way, and a gap of two and a half closes in them)
                off = 2.0 + stump / 2
                ox, oz = (sgn * off, 0) if w > d else (0, sgn * off)
                px, pz = at(lx + ox, lz + oz)
                sw, sd = (stump, 0.6) if w > d else (0.6, stump)
                prop_box(u, f"{name} wall {k}", [px, th(px, pz) + tall * 0.3, pz], [sw, tall * 0.6, sd], stone, texture="bricks", rot=quat_y(yaw), uv=[2, 2])
        else:
            px, pz = at(lx, lz)
            prop_box(u, f"{name} wall {k}", [px, th(px, pz) + tall * 0.42, pz], [w, tall * 0.84, d], stone, texture="bricks", rot=quat_y(yaw), uv=[3, 2])
    for k, (lx, lz) in enumerate([(-half, -half), (half, -half), (half, half), (-half, half)]):
        px, pz = at(lx, lz)
        h = tall * (1.15 if k % 2 == 0 else 0.7)
        spawn(u(), f"{name} column {k}", Prop={}, Transform={"pos": [px, th(px, pz) + h / 2, pz], "scale": [0.9, h, 0.9]}, MeshRenderer={"mesh": "cylinder", "texture": "stone", "uv_scale": [2, 3], "color": [0.62, 0.6, 0.56, 1], "roughness": 0.85},
              Collider={"shape": 1, "size": [0.9, h, 0.9]})


def pedestal(u, x, z, name, h=1.1):
    gy = th(x, z)
    prop_box(u, f"{name} base", [x, gy + 0.2, z], [1.6, 0.4, 1.6], [0.5, 0.48, 0.45], texture="stone", uv=[1, 1])
    prop_box(u, f"{name} pillar", [x, gy + 0.4 + h / 2, z], [0.9, h, 0.9], [0.56, 0.54, 0.5], texture="bricks", uv=[1, 2])
    return gy + 0.4 + h


def dress_world(u, time=6.5, day_seconds=540):
    """Everything that is only scenery and the places of the game: sky, sun, terrain, water, the lava of the crater, barriers at the sea, ruins, the camp, the wreck, the bones, trees and rocks."""
    spawn(1, "Environment", Environment={"sky": True, "sky_top": [0.17, 0.4, 0.82], "sky_horizon": [0.8, 0.86, 0.95], "ground": [0.3, 0.32, 0.3], "clouds": 0.5, "fog": 0.0008, "fog_color": [0.78, 0.84, 0.92],
                                         "bloom": 0.3, "bloom_threshold": 1.0, "ssao": 1.0, "exposure": 1.25, "vignette": 0.16, "shadows": True, "shadow_distance": 130, "saturation": 1.15, "ambient": 1.0})
    spawn(2, "Sun", Transform={"rot": [-0.5, 0.35, 0.15, 0.78]}, Light={"kind": 0, "color": [1.0, 0.94, 0.84], "intensity": 3.0})
    spawn(3, "Time of day", TimeOfDay={"time": time, "day_seconds": day_seconds, "azimuth": 1.5, "sun_intensity": 3.1, "night_intensity": 0.25})
    spawn(8, "Haze", HeightFog={"density": 0.008, "falloff": 0.06, "height": 14, "color": [0.8, 0.85, 0.92]})
    spawn(9, "Picture effects", PostFx={"shafts": 0.6, "contrast": 1.08})
    spawn(11, "Eyes", AutoExposure={"min": 0.6, "max": 1.45, "speed": 1.6, "target": 0.3})
    spawn(12, "Reflections", Ssr={"strength": 0.8, "max_distance": 36, "thickness": 0.6})
    spawn(13, "Lens", LensFlare={"intensity": 0.32, "ghosts": 3, "halo": 0.25})
    spawn(20, "Island", Transform={"pos": [0, 0, 0]}, Terrain={"heightmap": f"{W}/ground.png", "size": [448, 448], "height": 90, "step": 1, "color": [1, 1, 1, 1], "friction": 0.9},
          TerrainLayers={"splat": f"{W}/splat.png", "layers": LAYERS})
    spawn(21, "Water", Transform={"pos": [0, 0, 0]}, WaterBody={"heightmap": f"{W}/water.png", "size": [448, 448], "relief": 90, "color": [0.04, 0.3, 0.4, 0.86], "wave_height": 0.32, "wave_length": 10, "speed": 1.0})
    spawn(22, "Open sea", Transform={"pos": [0, SEA - 0.06, 0]}, Water={"size": [2600, 2600], "color": [0.04, 0.28, 0.4, 1.0], "wave_height": 0.3, "wave_length": 14, "speed": 0.9})
    # invisible walls far out at sea: the sea is too rough to swim further
    for i, (px, pz, sx, sz) in enumerate([(0, -205, 440, 4), (0, 205, 440, 4), (-205, 0, 4, 440), (205, 0, 4, 440)]):
        spawn(34 + i, f"Rough sea {i + 1}", Transform={"pos": [px, 20, pz], "scale": [sx, 60, sz]}, Collider={"shape": 1, "size": [sx, 60, sz]})
    # the lava lake of the crater
    vx, vz = WORLD["volcano"]["x"], WORLD["volcano"]["z"]
    lava_y = th(vx, vz) + 0.5
    spawn(23, "Lava", Prop={}, Transform={"pos": [vx, lava_y, vz], "scale": [16, 0.1, 16]}, MeshRenderer={"mesh": "cylinder", "texture": f"{W}/lava.png", "uv_scale": [3, 3], "emissive": 0.85, "color": [1.0, 0.8, 0.6, 1], "roughness": 0.6, "cast_shadow": False})
    spawn(27, "Lava light", Transform={"pos": [vx, lava_y + 2.5, vz]}, Light={"kind": 1, "color": [1.0, 0.45, 0.12], "intensity": 4.5, "range": 38})
    spawn(28, "Lava smoke", Transform={"pos": [vx, lava_y + 1.0, vz]}, Particles={"count": 300, "lifetime": 5, "looping": True, "prewarm": True, "shape": 3, "extent": [6, 0, 0], "direction": [0, 1, 0], "cone": 14, "speed_range": [1.2, 2.6],
                                                                                    "gravity": [0, 0.4, 0], "size": [2.0, 5.5], "color": [0.2, 0.18, 0.18, 0.5], "color_end": [0.12, 0.12, 0.12, 0], "additive": False, "turbulence": 0.6, "seed": 5})

    # the sounds of the place: surf on the beaches, the crackle of the fire, the rumble of the crater
    spawn(29, "Lava rumble", Transform={"pos": [vx, lava_y + 2, vz]}, AudioSource={"file": f"{D}/amb_lava.wav", "looped": True, "volume": 0.7, "spatial": True, "range": 70, "near": 8})
    for k, key in enumerate(("start", "wreck", "bones")):
        spawn(30 + k, f"Surf {k + 1}", Transform={"pos": [WORLD[key]["x"] * 1.12, SEA + 1, WORLD[key]["z"] * 1.12]}, AudioSource={"file": f"{D}/amb_waves.wav", "looped": True, "volume": 0.55, "spatial": True, "range": 60, "near": 6})
    # ---- where things are
    camp = WORLD["camp"]
    place = {"camp": (camp["x"], camp["z"])}
    # ---- the camp: a fire, tents, crates, the altar with its six sockets and the beacon
    cx, cz = camp["x"], camp["z"]
    fire_y = th(cx, cz)
    spawn(500, "Campfire", Campfire={}, Transform={"pos": [cx, fire_y + 0.9, cz]}, Light={"kind": 1, "color": [1.0, 0.62, 0.28], "intensity": 3.0, "range": 26})
    spawn(499, "Fire sound", Transform={"pos": [cx, fire_y + 0.5, cz]}, AudioSource={"file": f"{D}/amb_fire.wav", "looped": True, "volume": 0.8, "spatial": True, "range": 34, "near": 3})
    spawn(501, "Fire flames", Transform={"pos": [cx, fire_y + 0.25, cz]},
          Particles={"count": 420, "lifetime": 0.9, "looping": True, "prewarm": True, "shape": 3, "extent": [0.32, 0, 0], "direction": [0, 1, 0], "cone": 12, "speed_range": [0.7, 1.7], "gravity": [0, 1.3, 0], "turbulence": 0.2, "size": [0.62, 0.1],
                     "color": [1, 0.7, 0.2, 0.9], "color_end": [0.9, 0.12, 0.02, 0], "additive": True, "seed": 3},
          ParticleModules={"modules": "color 0:1,0.95,0.7,0 0.12:1,0.8,0.3,0.95 0.45:1,0.45,0.1,0.8 1:0.2,0.05,0.02,0\nsize 0:0.2 0.25:0.62 1:0.08\nnoise strength=0.5 scale=0.9 speed=0.8"})
    spawn(502, "Fire smoke", Transform={"pos": [cx, fire_y + 1.2, cz]},
          Particles={"count": 140, "lifetime": 4.5, "looping": True, "prewarm": True, "shape": 3, "extent": [0.25, 0, 0], "direction": [0, 1, 0], "cone": 8, "speed_range": [0.5, 1.0], "gravity": [0, 0.5, 0], "turbulence": 0.5, "size": [0.5, 2.2],
                     "color": [0.35, 0.33, 0.32, 0.38], "color_end": [0.25, 0.25, 0.25, 0], "additive": False, "seed": 4},
          ParticleModules={"modules": "wind 0.6,0,0.2 gust=0.7 freq=0.5\nsize 0:0.4 1:2.4"})
    spawn(503, "Fire sparks", Transform={"pos": [cx, fire_y + 0.6, cz]},
          Particles={"count": 50, "lifetime": 2.2, "looping": True, "prewarm": True, "shape": 3, "extent": [0.3, 0, 0], "direction": [0, 1, 0], "cone": 22, "speed_range": [1.0, 2.6], "gravity": [0, 0.6, 0], "turbulence": 0.7, "size": [0.08, 0.02],
                     "color": [1, 0.75, 0.3, 1], "color_end": [1, 0.3, 0.05, 0], "additive": True, "seed": 6, "stretch": 0.08})
    for k in range(6):  # logs round the fire, stones round the logs
        a = k * math.pi / 3
        spawn(504 + k, f"Fire stone {k + 1}", Prop={}, Transform={"pos": [cx + math.cos(a) * 1.15, fire_y + 0.12, cz + math.sin(a) * 1.15], "scale": [0.5, 0.36, 0.5]},
              MeshRenderer={"mesh": f"{M}/rock1.glb", "roughness": 0.95})
    prop_box(u, "Fire log 1", [cx, fire_y + 0.2, cz], [1.4, 0.28, 0.28], FOREST_BARK[:3], texture="wood", rot=quat_y(30), collide=False, uv=[2, 1])
    prop_box(u, "Fire log 2", [cx, fire_y + 0.32, cz], [1.4, 0.28, 0.28], FOREST_BARK[:3], texture="wood", rot=quat_y(-40), collide=False, uv=[2, 1])
    # the keeper's tents and the supplies
    for k, (dx, dz, col) in enumerate([(-7.5, -6.0, [1.0, 1.0, 1.0, 1]), (-9.5, 5.5, [0.78, 0.9, 1.1, 1])]):
        tx, tz = cx + dx, cz + dz
        ty = th(tx, tz)
        spawn(u(), f"Tent {k + 1}", Prop={}, Transform={"pos": [tx, ty, tz], "rot": quat_y([90, 270][k])}, MeshRenderer={"mesh": f"{M}/tent.glb", "color": col, "roughness": 0.95}, Collider={"shape": 1, "size": [3.3, 2.3, 3.3] if k == 0 else [3.3, 2.3, 3.3]})
    for k, (dx, dz) in enumerate([(4.5, 4.5), (5.4, 3.8), (4.9, 5.6)]):
        px, pz = cx + dx, cz + dz
        prop_box(u, f"Crate {k + 1}", [px, th(px, pz) + 0.5 + 0.55 * (k == 2), pz], [1.0, 1.0, 1.0], [0.6, 0.45, 0.28], texture="wood", rot=quat_y(15 * k), uv=[1, 1])
    # the altar: a stone table with six sockets, and the beacon behind it
    ax, az = cx + 9.0, cz - 7.0
    ay = th(ax, az)
    spawn(510, "Altar", Altar={}, Transform={"pos": [ax, ay, az]}, MeshRenderer={"mesh": f"{M}/altar.glb", "roughness": 0.85}, Collider={"shape": 1, "size": [2.4, 1.0, 2.4]})
    for k in range(6):
        sx, sz = ax + (k - 2.5) * 0.0, az
        a = k * math.pi / 3
        spawn(511 + k, f"Altar socket {k + 1}", Socket={"n": k + 1}, Transform={"pos": [ax + math.cos(a) * 0.95, ay + 1.62, az + math.sin(a) * 0.95], "scale": [0.22, 0.22, 0.22]},
              MeshRenderer={"mesh": "sphere", "color": [0.25, 0.22, 0.2, 1], "roughness": 0.5}, Light={"kind": 1, "color": [1.0, 0.62, 0.2], "intensity": 0, "range": 3.5})
    bx, bz = ax + 2.5, az - 4.0
    by = th(bx, bz)
    spawn(u(), "Beacon tower", Prop={}, Transform={"pos": [bx, by, bz]}, MeshRenderer={"mesh": f"{M}/beacon.glb", "roughness": 0.9}, Collider={"shape": 1, "size": [3.6, 7.0, 3.6]})
    spawn(520, "Beacon", Beacon={}, Transform={"pos": [bx, by + 9.2, bz]})
    spawn(521, "Beacon fire", BeaconFire={}, Transform={"pos": [bx, by + 10.2, bz]}, Light={"kind": 1, "color": [1.0, 0.7, 0.35], "intensity": 0, "range": 90})
    spawn(522, "Beacon flames", BeaconFlame={}, Transform={"pos": [bx, by + 9.4, bz]},
          Particles={"count": 1200, "lifetime": 1.6, "looping": True, "prewarm": True, "playing": False, "visible": False, "shape": 3, "extent": [0.9, 0, 0], "direction": [0, 1, 0], "cone": 10, "speed_range": [2.2, 4.8], "gravity": [0, 2.4, 0],
                     "turbulence": 0.4, "size": [1.4, 0.2], "color": [1, 0.7, 0.2, 0.9], "color_end": [0.9, 0.12, 0.02, 0], "additive": True, "seed": 8},
          ParticleModules={"modules": "color 0:1,0.95,0.7,0 0.1:1,0.85,0.4,0.95 0.5:1,0.5,0.12,0.8 1:0.3,0.08,0.04,0\nsize 0:0.4 0.25:1.3 1:0.2\nnoise strength=0.7 scale=0.8 speed=0.9"})
    place["altar"] = (ax, az)
    place["beacon"] = (bx, bz)
    # ---- the places of the shards
    sites = {}
    # 1: a wreck on the east beach
    wx, wz = WORLD["wreck"]["x"], WORLD["wreck"]["z"]
    wy = th(wx, wz)
    prop_box(u, "Wreck keel", [wx, wy + 0.35, wz], [5.2, 0.5, 1.4], [0.4, 0.28, 0.17], texture="wood", uv=[3, 1])
    prop_box(u, "Wreck hull port", [wx, wy + 0.8, wz - 0.7], [4.4, 1.1, 0.18], [0.45, 0.3, 0.18], texture="wood", uv=[3, 1], collide=False)
    prop_box(u, "Wreck mast", [wx + 1.5, wy + 2.0, wz - 0.6], [0.25, 4.2, 0.25], [0.36, 0.25, 0.15], texture="wood", rot=[0, 0, 0.2, 0.98], uv=[1, 3], collide=False)
    sites[1] = (wx + 2.2, wz + 1.0)
    # 2: the bones of a whale on the south-west beach
    bnx, bnz = WORLD["bones"]["x"], WORLD["bones"]["z"]
    for k in range(7):
        rx_ = bnx + (k - 3) * 1.3
        ry_ = th(rx_, bnz)
        for sgn in (-1, 1):
            prop_box(u, f"Whale rib {k + 1}", [rx_, ry_ + 1.3, bnz + sgn * 1.6], [0.28, 2.6, 0.28], [0.9, 0.86, 0.76], rot=[0.0, 0, -0.18 * sgn, 0.98], collide=False, rough=0.7)
    prop_box(u, "Whale spine", [bnx, th(bnx, bnz) + 0.35, bnz], [10, 0.5, 0.6], [0.88, 0.84, 0.74], collide=False, rough=0.7)
    sites[2] = (bnx + 0.0, bnz)
    # 3: a ruin in the forest, 4: the island in the lake, 5: a stone circle in the highlands, 6: the crater rim
    ruin(u, -52, -6, 0, "Forest ruin")  # (boxes that are turned count as their axis-aligned box for characters and for the path planner, so the walls stand square)
    sites[3] = (-52, -6)
    lk = WORLD["lake_at"]
    sites[4] = (lk["x"] + 3.0, lk["z"] - 2.0)
    hl = WORLD["highland"]
    for k in range(7):
        a = k * 2 * math.pi / 7
        px, pz = hl["x"] + math.cos(a) * 6.5, hl["z"] + math.sin(a) * 6.5
        h = 3.6 - 0.5 * (k % 3)
        prop_box(u, f"Standing stone {k + 1}", [px, th(px, pz) + h / 2 - 0.2, pz], [1.3, h, 0.9], [0.5, 0.5, 0.52], texture="stone", uv=[1, 2])
    sites[5] = (hl["x"], hl["z"])
    arena = WORLD["arena"]
    sites[6] = (arena["x"], arena["z"] - 4.0)
    pedestal(u, sites[6][0], sites[6][1], "Rim altar", 0.9)
    for k in range(10):  # a ring of broken pillars round the arena
        a = k * 2 * math.pi / 10 + 0.3
        px, pz = arena["x"] + math.cos(a) * 12.5, arena["z"] + math.sin(a) * 12.5
        if k % 3 == 2:
            continue
        h = 3.0 + 1.6 * ((k * 7) % 3)
        prop_box(u, f"Arena pillar {k + 1}", [px, th(px, pz) + h / 2 - 0.3, pz], [1.2, h, 1.2], [0.2, 0.19, 0.2], texture="stone", uv=[1, 2])
    for n, (sx, sz) in sites.items():
        sy = th(sx, sz)
        base = sy + (1.4 if n == 6 else 1.15 if n in (3, 5) else 0.9)
        spawn(400 + n, f"Shard {n}", Shard={"n": n, "base_y": base, "locked": 1 if n == 6 else 0}, Transform={"pos": [sx, base, sz]},
              MeshRenderer={"mesh": f"{M}/shard.glb", "emissive": 0.75, "roughness": 0.2, "metallic": 0.1}, Light={"kind": 1, "color": [1.0, 0.6, 0.2], "intensity": 1.6, "range": 9})
    place["sites"] = sites
    place["arena"] = (arena["x"], arena["z"])
    return place


def scatter_trees(prefix=""):
    """Forests and rocks by the `scatter` tool, each kind on the ground layer it likes (the splat map says where), kept off the places of the game afterwards."""
    pine = {"Tree": {}, "MeshRenderer": {"mesh": f"{M}/pine.glb", "roughness": 0.9}, "Collider": {"shape": 1, "size": [0.7, 3.4, 0.7]}, "Sway": {"amount": 0.12, "extent": 6, "speed": 0.7, "offset": 0.6}}
    broad = {"Tree": {}, "MeshRenderer": {"mesh": f"{M}/broadleaf.glb", "roughness": 0.9}, "Collider": {"shape": 1, "size": [0.8, 3.4, 0.8]}, "Sway": {"amount": 0.2, "extent": 6, "speed": 0.55, "offset": 2.1}}
    palm = {"Tree": {}, "MeshRenderer": {"mesh": f"{M}/palm.glb", "roughness": 0.85}, "Collider": {"shape": 1, "size": [0.5, 3.0, 0.5]}, "Sway": {"amount": 0.35, "extent": 6, "speed": 0.8, "offset": 4.0}}
    dead = {"Tree": {}, "MeshRenderer": {"mesh": f"{M}/deadtree.glb", "roughness": 0.95}, "Collider": {"shape": 1, "size": [0.5, 3.0, 0.5]}}
    calls = [
        (pine, dict(count=420, layer="grass", layer_min=0.8, slope=[0, 26], height=[14, 46], min_distance=5.2, scale=[0.85, 1.4], seed=11)),
        (broad, dict(count=300, layer="grass", layer_min=0.85, slope=[0, 20], height=[10, 22], min_distance=6.0, scale=[0.8, 1.25], seed=12)),
        (palm, dict(count=70, layer="sand", layer_min=0.75, slope=[0, 24], height=[6.4, 10.5], min_distance=7.0, scale=[0.85, 1.3], seed=13)),
        (dead, dict(count=36, layer="ash", layer_min=0.7, slope=[0, 35], height=[44, 70], min_distance=9.0, scale=[0.8, 1.3], seed=14)),
    ]
    for n in (1, 2, 3):
        calls.append(({"Tree": {}, "MeshRenderer": {"mesh": f"{M}/rock{n}.glb", "roughness": 0.95}, "Collider": {"shape": 1, "size": [1.8, 1.4, 1.8]}},
                      dict(count=[70, 60, 50][n - 1], layer="rock", layer_min=0.7, slope=[0, 50], min_distance=4.0, scale=[0.7, 2.6], seed=20 + n, yaw=True, sink=0.1)))
    calls.append(({"Tree": {}, "MeshRenderer": {"mesh": f"{M}/rock2.glb", "roughness": 0.95}, "Collider": {"shape": 1, "size": [1.8, 1.4, 1.8]}},
                  dict(count=40, layer="sand", layer_min=0.8, slope=[0, 40], height=[5.6, 10], min_distance=9.0, scale=[0.5, 1.2], seed=30, yaw=True, sink=0.2)))
    placed = 0
    for comps, args in calls:
        r = call("scatter", {"components": comps, **args})
        placed += r.get("placed", 0)
    print(f"  scatter: {placed} trees and rocks")
    return placed


def clear_around(spots, _unused=0):
    """Remove the scattered trees and rocks standing on top of the places of the game (camp, shard sites, the start, the golems' ground). `spots`: (x, z, radius)."""
    gone = []
    offset = 0
    while True:
        q = call("query", {"with": ["Tree"], "select": ["Transform"], "limit": 500, "offset": offset})
        ents = q.get("entities") or []
        for e in ents:
            comps = e.get("components") or e
            pos = (comps.get("Transform") or {}).get("pos")
            if not pos:
                continue
            for (sx, sz, r) in spots:
                if (pos[0] - sx) ** 2 + (pos[2] - sz) ** 2 < r * r:
                    gone.append(e["uid"])
                    break
        offset += len(ents)
        if len(ents) < 500:
            break
    for uid in gone:
        call("despawn", {"uid": uid})
    print(f"  {len(gone)} trees and rocks removed from the places of the game")
    return len(gone)


def good_spot(x, z, max_slope_deg=26.0, rmax=26):
    """The nearest place to (x, z) where the ground is gentle enough to stand a golem on (the engine's own ground normal says how steep it is)."""
    limit = math.cos(math.radians(max_slope_deg))
    for r in range(0, rmax + 1, 2):
        for k in range(0, 1 if r == 0 else 16):
            a = 2 * math.pi * k / 16
            px, pz = x + r * math.cos(a), z + r * math.sin(a)
            h = call("terrain_height", {"x": px, "z": pz}, quiet=True)
            n = h.get("normal")
            if n and n[1] >= limit and h.get("height", 0) > SEA + 2.5:
                return px, pz
    return x, z


def reachable(src, x, z):
    """Is there a walkable way (the engine's `path`) from `src` (x, z) to (x, z) for a character the size of the hero?"""
    y0, y1 = th(src[0], src[1]), th(x, z)
    r = call("path", {"from": [src[0], y0 + 0.1, src[1]], "to": [x, y1 + 0.2, z], "radius": 0.45, "height": 1.8, "step_height": 0.55}, quiet=True)
    return bool(r.get("points")) and r.get("exact", True)


def ensure_reachable(src, places, radii=(0, 9, 14, 20)):
    """Every place of the game must be walkable to from the camp: scattered trees and rocks that wall one in are taken away, a wider and wider circle at a time. Returns the places that stayed shut."""
    shut = []
    for name, x, z in places:
        ok = False
        for r in radii:
            if r:
                clear_around([(x, z, r)])
            if reachable(src, x, z):
                ok = True
                break
        print(f"  {'reachable' if ok else 'SHUT'}: {name} ({x:.0f}, {z:.0f}) after clearing {r} m")
        if not ok:
            shut.append(name)
    return shut
