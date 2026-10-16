"""The scenes of Caldera (see `tools/build_caldera.py`): the island, the title, the game over and win screens, the boot scene."""
import math
import os

from caldera_lib import *  # noqa: F401,F403
import caldera_lib as L
import caldera_scripts as S

HERO_MACHINE = "\n".join([
    "state idle clip=idle", "state walk clip=walk speed=1.5", "state run clip=run speed=1.35", "state air clip=air", "state attack_a clip=attack once", "state attack_b clip=attack once",
    "state roll_a clip=roll once", "state roll_b clip=roll once", "state swim clip=swim", "state die clip=die once",
    "* -> die when Hero.state == 3 fade 0.1",
    "* -> roll_a when Hero.state == 2 and Hero.combo == 0 fade 0.05", "* -> roll_b when Hero.state == 2 and Hero.combo == 1 fade 0.05",
    "* -> attack_a when Hero.state == 1 and Hero.combo == 0 fade 0.05", "* -> attack_b when Hero.state == 1 and Hero.combo == 1 fade 0.05",
    "* -> swim when Hero.swim == 1 and Hero.state == 0 fade 0.2",
    "* -> air when Hero.air > 0.14 and Hero.state == 0 and Hero.swim == 0 fade 0.1",
    "* -> run when Hero.sprint == 1 and Hero.state == 0 and Hero.swim == 0 and Hero.air < 0.14 fade 0.12",
    "* -> walk when Hero.moving == 1 and Hero.state == 0 and Hero.swim == 0 and Hero.air < 0.14 fade 0.12",
    "* -> idle when Hero.moving == 0 and Hero.state == 0 and Hero.swim == 0 and Hero.air < 0.14 fade 0.2",
])
# the win: the camera leaves the hero, goes round the lit beacon and rises, then the picture fades to the end screen
CUTSCENE = "\n".join([
    "at 0 set CameraRig.target = 520", "at 0 set CameraRig.follow_speed = 0", "at 0 set CameraRig.mode = 1", "at 0 set CameraRig.look_offset[1] = 7.5",
    "at 0 ramp CameraRig.distance 17 -> 30 over 12 ease 3", "at 0 ramp CameraRig.yaw 0.25 -> 2.3 over 12 ease 3", "at 0 ramp CameraRig.pitch 0.04 -> 0.34 over 12 ease 3",
    "at 0.3 event beacon", "at 0.4 set CameraRig.shake = 0.5", "at 2.0 set CameraRig.shake = 0.35", "at 5.0 event shard_appear", "at 11.3 res leave = 4", "at 12 end"])

HERO_EVENTS = "walk length=1.0: 0.22 step, 0.72 step\nrun length=0.66: 0.1 step, 0.43 step"

GOLEM_MACHINE = "\n".join([
    "state sleep clip=sleep", "state wake clip=wake once", "state idle clip=idle", "state walk clip=walk speed=1.1", "state swing clip=swing once", "state slam clip=slam once",
    "state stagger clip=stagger once", "state rest clip=sleep speed=0.5", "state die clip=die once",
    "* -> die when Golem.mode == 6 fade 0.1", "* -> wake when Golem.mode == 1 fade 0.2", "* -> swing when Golem.mode == 3 fade 0.1", "* -> slam when Golem.mode == 4 fade 0.1",
    "* -> stagger when Golem.mode == 5 fade 0.05", "* -> rest when Golem.mode == 7 fade 0.3", "* -> sleep when Golem.mode == 0 fade 0.4",
    "* -> idle when Golem.mode == 2 and Golem.dist < 3.4 fade 0.2", "* -> walk when Golem.mode == 2 or Golem.mode == 8 fade 0.2",
])
GOLEM_EVENTS = "walk length=1.3: 0.15 golem_step, 0.8 golem_step"

# the boss: two attacks (a slam when close, a charge when far) and a vulnerable rest after three of them
BOSS_TREE = """selector
  sequence
    cond Golem.boss == 1 and Golem.mode >= 2 and Golem.mode <= 8
    selector
      sequence
        cond Golem.combo >= 3 and Golem.mode == 2
        act set Golem.rest = 1, event boss_tired
        wait 5.0
        act set Golem.rest = 0, set Golem.combo = 0
      sequence
        cond Golem.dist < 6 and Golem.mode == 2
        act set Golem.attack = 1
        wait 1.7
        act set Golem.attack = 0, set Golem.combo += 1
        wait 0.5
      sequence
        cond Golem.dist >= 6 and Golem.dist < 20 and Golem.mode == 2
        act set Golem.attack = 2
        wait 1.9
        act set Golem.attack = 0, set Golem.combo += 1
        wait 0.5
"""

DIALOGUE = """== start
{#npc.name}: {#npc.hello}
if $g.dep >= 6 -> beacon
if $g.shards > 0 -> bring
-> menu

== menu
* [{#npc.q_quest}] -> quest
* [{#npc.q_where}] -> where
* [{#npc.q_night}] -> night
* [{#npc.q_bye}] -> end

== quest
{#npc.name}: {#npc.quest1}
{#npc.name}: {#npc.quest2}
-> menu

== where
if $g.s1 == 0 -> h1
if $g.s2 == 0 -> h2
if $g.s3 == 0 -> h3
if $g.s4 == 0 -> h4
if $g.s5 == 0 -> h5
if $g.s6 == 0 -> h6
{#npc.name}: {#npc.h_all}
-> menu

== h1
{#npc.name}: {#npc.h1}
-> menu

== h2
{#npc.name}: {#npc.h2}
-> menu

== h3
{#npc.name}: {#npc.h3}
-> menu

== h4
{#npc.name}: {#npc.h4}
-> menu

== h5
{#npc.name}: {#npc.h5}
-> menu

== h6
{#npc.name}: {#npc.h6}
-> menu

== night
{#npc.name}: {#npc.night1}
{#npc.name}: {#npc.night2}
-> menu

== bring
{#npc.name}: {#npc.bring}
-> menu

== beacon
{#npc.name}: {#npc.beacon}
-> end
"""


def dialogue_texts():
    """Maren's words (the keys of `DIALOGUE`), in English and German."""
    for key, en, de in [
        ("npc.hello", "You came ashore at dawn. Few do, these days. I am Maren, I keep the fire.", "Du bist im Morgengrauen gelandet. Das tun nur wenige. Ich bin Maren, ich huete das Feuer."),
        ("npc.q_quest", "What must I do?", "Was soll ich tun?"), ("npc.q_where", "Where are the shards?", "Wo sind die Splitter?"), ("npc.q_night", "What about the night?", "Und die Nacht?"), ("npc.q_bye", "Farewell.", "Lebe wohl."),
        ("npc.quest1", "Six ember shards fell from the mountain's mouth when it last woke. Together they lit the beacon that kept the dark away.", "Sechs Glutsplitter fielen aus dem Maul des Berges, als er zuletzt erwachte. Zusammen entzuendeten sie das Leuchtfeuer, das die Dunkelheit fernhielt."),
        ("npc.quest2", "Bring them to the altar here, one by one or all together, and light the beacon. Take care: stone golems sleep over the shards.", "Bring sie zum Altar hier, einzeln oder alle zusammen, und entzuende das Leuchtfeuer. Hab acht: Steingolems schlafen ueber den Splittern."),
        ("npc.h1", "One lies in the wreck on the east beach. Walk along the shore.", "Einer liegt im Wrack am Oststrand. Geh am Ufer entlang."),
        ("npc.h2", "Another rests among the whale bones on the south shore.", "Ein anderer ruht zwischen den Walknochen am Suedufer."),
        ("npc.h3", "Look for the old ruin in the forest. A golem sleeps nearby.", "Suche die alte Ruine im Wald. In der Naehe schlaeft ein Golem."),
        ("npc.h4", "A shard lies on the little island in the lake. You will have to swim.", "Ein Splitter liegt auf der kleinen Insel im See. Du musst schwimmen."),
        ("npc.h5", "The standing stones in the highlands hold one, and a golem guards it.", "Die Steine im Hochland bergen einen, und ein Golem bewacht ihn."),
        ("npc.h6", "The last is on the rim of the caldera, and the Warden will not give it up. Strike it when it kneels.", "Der letzte liegt am Rand der Caldera, und der Waechter gibt ihn nicht her. Schlag zu, wenn er kniet."),
        ("npc.h_all", "You have them all. Put them on the altar and light the beacon.", "Du hast sie alle. Leg sie auf den Altar und entzuende das Leuchtfeuer."),
        ("npc.night1", "At night wraiths rise. Only fire shows them to you, and your lantern when it burns bright.", "Nachts erheben sich Geister. Nur Feuer zeigt sie dir, und deine Laterne, wenn sie hell brennt."),
        ("npc.night2", "Stay near a fire and they will not come. Rest here, and I will keep your place in the book.", "Bleib in der Naehe eines Feuers, dann kommen sie nicht. Rast hier, und ich halte deinen Platz im Buch fest."),
        ("npc.bring", "You carry embers! Put them on the altar, there by the big stone.", "Du traegst Glut! Leg sie auf den Altar, dort beim grossen Stein."),
        ("npc.beacon", "All six! Go to the beacon and light it. The island has waited long.", "Alle sechs! Geh zum Leuchtfeuer und entzuende es. Die Insel hat lange gewartet."),
    ]:
        tr(key, en, de)


def check_language_keys():
    """Every `{#key}` that a scene or a script uses must be in both language files, or the player sees the key."""
    import re
    used = set()
    for name in ("boot", "title", "island", "gameover", "win"):
        path = f"{D}/{name}.lunaris.json"
        if os.path.exists(path):
            with open(path, encoding="utf-8") as f:
                used |= set(re.findall(r"\{#([A-Za-z0-9_.]+)\}", f.read()))
    missing = sorted(used - set(TEXTS))
    for k in missing:
        print(f"  no text for `{k}` in the language files")
    return len(missing)


# ------------------------------------------------------------------------------------------------ pieces shared by several scenes
def settings_panel(pos0=0):
    """The settings screen: mouse sensitivity, music and effects volume, the language, back. `SettingsUi` shows it while `$settings` is on."""
    spawn(2300, "Settings dim", SettingsUi={}, Ui={"pos": [0, 0], "size": [1, 1], "color": [0, 0, 0, 0.68], "layer": 20, "visible": False})
    panel(2301, "Settings panel", (0.27, 0.14), (0.46, 0.72), color=(0.07, 0.06, 0.08, 0.96), radius=0.02, layer=21, border=0.003, SettingsUi={}, **{})
    ui_text(2310, "Settings title", tr("settings.title", "SETTINGS", "EINSTELLUNGEN"), (0.5, 0.17), 0.065, color=(1, 0.9, 0.6, 1), SettingsUi={})
    rows = [(0.30, tr("settings.sens", "Mouse sensitivity", "Mausempfindlichkeit"), 1, 1.0, 0.3, 2.5), (0.43, tr("settings.music", "Music", "Musik"), 2, 0.7, 0.0, 1.0),
            (0.56, tr("settings.sfx", "Effects", "Effekte"), 3, 0.9, 0.0, 1.0)]
    for i, (y, label, kind, val, lo, hi) in enumerate(rows):
        ui_text(2311 + i, f"Setting label {i + 1}", label, (0.3, y - 0.035), 0.032, color=(1, 0.95, 0.85, 1), align=0, SettingsUi={})
        slider(2321 + i, f"Setting slider {i + 1}", (0.3, y), (0.4, 0.03), val, lo, hi, kind, layer=22, visible=False, SettingsUi={})
    ui_text(2314, "Setting label 4", tr("settings.language", "Language", "Sprache"), (0.3, 0.605), 0.032, color=(1, 0.95, 0.85, 1), align=0, SettingsUi={})
    button(2331, "Language button", "{#language.name}", (0.3, 0.645), (0.4, 0.075), {"SettingsUi": {}, "SettingBtn": {"kind": 2}}, layer=22, visible=False, text_size=0.034)
    button(2332, "Back button", tr("settings.back", "Back", "Zurueck"), (0.38, 0.75), (0.24, 0.08), {"SettingsUi": {}, "SettingBtn": {"kind": 1}}, layer=22, visible=False)
    TEXTS["language.name"] = ("English", "Deutsch")


def common_scene_bits(language=True):
    spawn(6, "Localization", Localization={"language": L.LANG}, LanguageList={"languages": L.LANGS})
    input_map()
    spawn(14, "Nav", UiNav={"current": 0}, UiNavAuto={"enabled": True})
    spawn(15, "Interface shape", UiScale={"aspect": 1.7778})
    sounds()


def set_defaults(extra=()):
    for name, value in (("opt.sens", 1.0), ("bus.music.volume", 0.7), ("bus.sfx.volume", 0.9), ("app.quit_on_escape", 0), ("language.index", 0), ("language.count", 2)) + tuple(extra):
        call("set_resource", {"name": name, "value": value})


# ------------------------------------------------------------------------------------------------ the island
GOLEMS = [  # name, x, z, awake radius (the places are moved to gentle ground by `good_spot`)
    ("Golem forest", -62.0, -14.0, 13.0), ("Golem pass", -30.0, -20.0, 14.0), ("Golem highland 1", -94.0, -34.0, 14.0),
    ("Golem highland 2", -108.0, -54.0, 14.0), ("Golem highland 3", -70.0, -40.0, 14.0),
]


def build_island():
    call("reset", {"seed": 1})
    define_components(list(COMPONENTS))
    u = Uids(5000)
    place = dress_world(u)
    sx, sz = WORLD["start"]["x"], WORLD["start"]["z"]
    sy = th(sx, sz)
    common_scene_bits()
    fader(900, {2: (f"{D}/gameover.lunaris.json", CLEAN), 3: (f"{D}/title.lunaris.json", NEW_RUN), 4: (f"{D}/win.lunaris.json", CLEAN)})

    dialogue_texts()
    # ---- the hero, its camera and its lantern
    spawn(7, "Camera", Transform={"pos": [sx, sy + 3.5, sz + 6.5]}, Camera={"fov_y": 62, "near": 0.15, "far": 1100},
          CameraRig={"target": 100, "mode": 1, "look_at": True, "look_offset": [0, 1.55, 0], "follow_speed": 16, "yaw": 0.0, "pitch": 0.3, "distance": 6.6, "shake_decay": 1.8, "shake_amount": 0.5},
          CameraCollide={"radius": 0.4, "min_distance": 1.3}, Timeline={"script": CUTSCENE, "playing": False})
    spawn(100, "Hero", Hero={"stamina": 100, "fx": 0, "fz": -1}, Transform={"pos": [sx, sy + 0.05, sz]}, CharacterBody={"radius": 0.38, "height": 1.8, "step_height": 0.55, "max_slope": 52},
          HitPoints={"value": 100, "max": 100, "regen": 0.8, "regen_delay": 9}, MeshRenderer={"mesh": f"{M}/hero.glb", "roughness": 0.8}, Animator={"clip": "idle", "looping": True},
          Sensor={"radius": 3.3, "fov": 110, "filter": "Foe", "los": False, "eye": 1.0}, StateMachine={"machine": HERO_MACHINE}, AnimEvents={"events": HERO_EVENTS})
    spawn(101, "Lantern", HeroLight={}, Parent={"target": 100}, LocalTransform={"pos": [0.4, 1.7, -0.3]}, Transform={"pos": [sx + 0.4, sy + 1.7, sz - 0.3]}, Light={"kind": 1, "color": [1.0, 0.72, 0.38], "intensity": 0.4, "range": 6})

    # ---- the keeper of the fire
    cx, cz = WORLD["camp"]["x"], WORLD["camp"]["z"]
    nx, nz = cx - 5.5, cz + 4.5
    spawn(550, "Maren", Npc={}, Transform={"pos": [nx, th(nx, nz), nz], "rot": quat_y(270)}, MeshRenderer={"mesh": f"{M}/hero.glb", "color": [0.8, 0.55, 0.9, 1], "roughness": 0.9}, Animator={"clip": "idle", "looping": True},
          Collider={"shape": 1, "size": [0.8, 1.8, 0.8]}, Dialogue={"script": DIALOGUE, "text_ui": 2100, "speaker_ui": 2101, "choice_1": 2102, "choice_2": 2103, "choice_3": 2104, "choice_4": 2105, "speed": 0})
    spawn(551, "Maren's name", Text={"value": tr("npc.name", "Maren", "Maren"), "pos": [0.5, 0.4], "size": 0.034, "align": 1, "color": [1, 0.9, 0.6, 1], "shadow": True}, TextFont={"file": FONT}, NpcLabel={},
          UiAnchor={"entity": 550, "world": [0, 2.35, 0]})

    # ---- the golems, the boss and the wraiths
    def golem(uid, name, x, z, wake, boss=False):
        y = th(x, z)
        s = 1.8 if boss else 1.0
        hp = 420 if boss else 100
        spawn(uid, name, Foe={}, Golem={"mode": 0, "wake_r": wake, "home_x": x, "home_z": z, "leash": 80 if boss else 42, "boss": 1 if boss else 0, "dmg": 34 if boss else 20, "fx": 0, "fz": 1},
              Transform={"pos": [x, y, z], "scale": [s, s, s]}, CharacterBody={"radius": 0.8 * s, "height": 2.7 * s, "step_height": 0.8, "max_slope": 50},
              NavAgent={"speed": 4.6 if boss else 3.4, "radius": 0.95 * s, "height": 2.7 * s, "step_height": 0.8, "arrive": 1.2},
              HitPoints={"value": hp, "max": hp}, Sensor={"radius": wake, "fov": 360, "filter": "Hero", "los": True, "eye": 1.5},
              MeshRenderer={"mesh": f"{M}/golem.glb", "roughness": 0.95, "color": [1.0, 0.82, 0.74, 1] if boss else [1, 1, 1, 1]},
              Animator={"clip": "sleep", "looping": True, "time": (uid % 7) * 0.4}, StateMachine={"machine": GOLEM_MACHINE}, AnimEvents={"events": GOLEM_EVENTS},
              Light={"kind": 1, "color": [1.0, 0.55, 0.2], "intensity": 0, "range": 7 if boss else 5}, **({"BehaviorTree": {"script": BOSS_TREE}} if boss else {}))

    bx, bz = place["arena"][0], place["arena"][1] + 3.0
    for k in range(6):
        spawn(600 + k, f"Wraith {k + 1}", Foe={}, Wraith={"slot": k}, Transform={"pos": [0, -200, 0], "scale": [1.2, 1.2, 1.2]}, Velocity={},
              Steer={"max_speed": 3.2, "max_accel": 7, "plane": 0, "seek": 100, "seek_weight": 1.0, "arrive": 0.4, "wander": 0.0, "face": True, "active": False},
              HitPoints={"value": 40, "max": 40}, MeshRenderer={"mesh": f"{M}/wraith.glb", "color": [1, 1, 1, 0], "visible": False, "emissive": 0.35, "roughness": 0.6}, Animator={"clip": "float", "looping": True, "time": k * 0.31})

    # ---- wildlife: crabs on the beaches, gulls over them
    rng = random.Random(5)
    n_crab = 0
    for k in range(900):
        if n_crab >= 16:
            break
        ang = rng.uniform(0, 2 * math.pi)
        r = rng.uniform(100, 150)
        x, z = math.cos(ang) * r, math.sin(ang) * r
        if th(x, z) < 5.9 or th(x, z) > 10.0:
            continue
        if layer_at(x, z) != "sand":
            continue
        spawn(700 + n_crab, f"Crab {n_crab + 1}", Transform={"pos": [x, th(x, z) + 0.1, z]}, Velocity={}, RigidBody={"mass": 1, "friction": 0.0, "restitution": 0}, Collider={"shape": 0, "size": [0.06, 0.06, 0.06]},
              Steer={"max_speed": 1.6, "max_accel": 6, "plane": 0, "wander": 0.9, "group": "crab", "radius": 5, "personal": 0.7, "separation": 1.2, "alignment": 0.4, "cohesion": 0.2, "flee": 100, "flee_radius": 5, "flee_weight": 2.0, "face": True},
              MeshRenderer={"mesh": f"{M}/crab.glb", "roughness": 0.7}, Animator={"clip": "scuttle", "looping": True, "time": (k % 5) * 0.1, "speed": 1.3})
        n_crab += 1
    for k in range(10):
        ang = math.radians(60 + k * 8)
        r = 120 + (k % 3) * 10
        spawn(720 + k, f"Gull {k + 1}", Transform={"pos": [math.cos(ang) * r, 30 + (k % 4) * 4, math.sin(ang) * r], "scale": [1.5, 1.5, 1.5]}, Velocity={"lin": [2.0, 0, 0.5]},
              Steer={"max_speed": 7, "max_accel": 6, "plane": 0, "wander": 1.5, "group": "gull", "radius": 40, "personal": 6, "separation": 1.5, "alignment": 1.0, "cohesion": 0.8, "face": True, "bounds": [-100, 20, 150, 170], "edge": 30},
              MeshRenderer={"mesh": f"{M}/gull.glb", "roughness": 0.8}, Animator={"clip": "flap", "looping": True, "time": (k % 7) * 0.1, "speed": 0.85 + 0.05 * (k % 4)})
    print(f"  {n_crab} crabs, 10 gulls")

    # ---- effects, pooled: one cloud each, moved to the spot and fired again
    SPARK = {"count": 40, "lifetime": 0.55, "looping": False, "playing": False, "time": 99, "visible": False, "shape": 1, "extent": [0.08, 0, 0], "cone": 180, "speed_range": [2.0, 6.0], "gravity": [0, -6, 0], "drag": 1.4,
             "size": [0.26, 0.04], "color": [1, 0.9, 0.5, 1], "color_end": [1, 0.45, 0.1, 0], "additive": True, "fade_in": 0, "seed": 5, "stretch": 0.05}
    DEBRIS = {"count": 70, "lifetime": 1.2, "looping": False, "playing": False, "time": 99, "visible": False, "shape": 1, "extent": [0.5, 0, 0], "cone": 180, "speed_range": [2.0, 7.0], "gravity": [0, -9, 0], "drag": 0.3,
              "size": [0.4, 0.2], "color": [0.5, 0.5, 0.5, 1], "color_end": [0.3, 0.3, 0.3, 0], "additive": False, "fade_in": 0, "seed": 9}
    DUST = {"count": 90, "lifetime": 1.4, "looping": False, "playing": False, "time": 99, "visible": False, "shape": 4, "extent": [0.8, 0, 0], "direction": [0, 0.15, 0], "cone": 60, "speed_range": [3.0, 6.0], "gravity": [0, 0.3, 0],
            "drag": 1.6, "size": [0.9, 2.4], "color": [0.55, 0.5, 0.45, 0.6], "color_end": [0.4, 0.38, 0.35, 0], "additive": False, "fade_in": 0, "seed": 11}
    WRAITH_HIT = {**SPARK, "color": [0.7, 0.9, 1, 1], "color_end": [0.3, 0.4, 1, 0], "seed": 13}
    SMOKE = {"count": 60, "lifetime": 1.5, "looping": False, "playing": False, "time": 99, "visible": False, "shape": 1, "extent": [0.4, 0, 0], "cone": 180, "speed_range": [0.4, 1.6], "gravity": [0, 0.8, 0], "drag": 1.0,
             "size": [0.6, 1.8], "color": [0.35, 0.45, 0.7, 0.55], "color_end": [0.2, 0.25, 0.45, 0], "additive": False, "fade_in": 0, "seed": 15}
    GLINT = {"count": 90, "lifetime": 1.3, "looping": False, "playing": False, "time": 99, "visible": False, "shape": 1, "extent": [0.2, 0, 0], "direction": [0, 1, 0], "cone": 70, "speed_range": [1.5, 4.5], "gravity": [0, 1.0, 0], "drag": 0.8,
             "size": [0.3, 0.04], "color": [1, 0.75, 0.3, 1], "color_end": [1, 0.4, 0.1, 0], "additive": True, "fade_in": 0, "seed": 17, "turbulence": 0.4}
    for k, (kind, cloud, mods) in enumerate([(1, SPARK, "color 0:1,1,0.8,1 0.4:1,0.85,0.3,0.9 1:1,0.45,0.1,0\nsize 0:0.26 1:0.03\ntrail segments=4 time=0.15 width=0.5"), (2, DEBRIS, "mesh cube\nsize 0:0.4 1:0.1"), (3, DUST, ""),
                                             (4, WRAITH_HIT, ""), (5, SMOKE, ""), (6, GLINT, "")]):
        comps = {"FxPool": {"kind": kind}, "Transform": {"pos": [0, -50, 0]}, "Particles": cloud}
        if mods:
            comps["ParticleModules"] = {"modules": mods}
        spawn(800 + k, f"Effect {kind}", **comps)
    spawn(810, "Splash", SplashFx={}, Transform={"pos": [0, -50, 0]}, Particles={"count": 60, "lifetime": 0.9, "looping": False, "playing": False, "time": 99, "shape": 1, "extent": [0.3, 0, 0], "direction": [0, 1, 0], "cone": 45,
                                                                                "speed_range": [2.0, 4.5], "gravity": [0, -9, 0], "size": [0.22, 0.05], "color": [0.8, 0.92, 1, 0.9], "color_end": [0.7, 0.85, 1, 0], "additive": False, "fade_in": 0, "seed": 19})

    # ---- the interface
    build_hud()
    call("define_script", {"source": S.fader_script("        $music = 2;\n") + S.CORE + S.SETTINGS + S.HERO + S.GOLEM + S.WRAITH + S.WORLD + S.NPC + S.HUD + S.MUSIC + S.MUSIC_ISLAND})
    for name, value in (("cam.yaw", 0.0), ("music", 2), ("cursor.lock", 1), ("g.dep", 0), ("g.shards", 0), ("g.kills", 0), ("g.lit", 0), ("camp.x", cx), ("camp.z", cz), ("dist.fire", 99), ("dist.altar", 99), ("dist.npc", 99), ("near.shard", 999), ("boss.hp", 420), ("hp", 100), ("stamina", 100), ("language.index", 0), ("language.count", 2)):
        call("set_resource", {"name": name, "value": value})
    set_defaults()

    # ---- forests and rocks, the last thing made (scatter gives the entities the numbers after the last one); the places of the game are cleared of them
    scatter_trees()
    clear_around([(cx, cz, 17), (sx, sz, 8), (nx, nz, 3)] + [(x, z, 7.5) for (x, z) in place["sites"].values()] + [(place["altar"][0], place["altar"][1], 7), (bx, bz, 14)])
    # (shard 4 is on an island in the lake: it is reached by swimming; shard 6 stands on a pedestal, so the place next to it is asked)
    src = (cx - 3.0, cz + 6.0)
    wanted = [(f"shard {n}", x + (2.5 if n == 6 else 0.0), z) for n, (x, z) in place["sites"].items() if n != 4] + [("the Caldera Warden", bx, bz), ("the start", sx, sz)]
    shut = ensure_reachable(src, wanted)
    # the golems stand on gentle ground the hero can walk to: the first such place at or near where each is wanted
    for i, (name, x, z, wake) in enumerate(GOLEMS):
        chosen = None
        for dx, dz in ((0, 0), (8, 0), (-8, 0), (0, 8), (0, -8), (14, 0), (-14, 0), (0, 14), (0, -14)):
            gx, gz = good_spot(x + dx, z + dz)
            clear_around([(gx, gz, 6)])
            if reachable(src, gx, gz):
                chosen = (gx, gz)
                break
        if not chosen:
            shut.append(name)
            chosen = good_spot(x, z)
        print(f"  {name}: ({chosen[0]:.0f}, {chosen[1]:.0f})")
        golem(200 + i, name, chosen[0], chosen[1], wake)
    golem(300, "Caldera Warden", bx, bz, 22.0, boss=True)
    if shut:
        print("  SHUT PLACES:", shut)
    chk = show_check("island")
    call("save_scene", {"path": f"{D}/island.lunaris.json"})
    return chk


def build_hud():
    # health, stamina, shards
    panel(1000, "HUD panel", (0.012, 0.02), (0.27, 0.155), layer=1, HudItem={})
    ui_text(1001, "HP label", tr("hud.hp", "HP", "LP"), (0.022, 0.034), 0.026, align=0, color=(1, 0.8, 0.75, 1), HudItem={})
    spawn(1002, "Health bar", HudHp={}, HudItem={}, Ui={"pos": [0.065, 0.036], "size": [0.205, 0.03], "color": [0.86, 0.62, 0.16, 1], "back_color": [0.2, 0.05, 0.06, 0.9], "fill": 1, "border": 0.002, "border_color": [1, 0.9, 0.8, 0.7],
                                                         "text": "{hp}", "text_size": 0.024, "layer": 3}, UiStyle={"radius": 0.008}, TextFont={"file": FONT})
    ui_text(1003, "Stamina label", tr("hud.stamina", "ST", "AU"), (0.022, 0.078), 0.024, align=0, color=(0.8, 1, 0.8, 1), HudItem={})
    spawn(1004, "Stamina bar", HudSt={}, HudItem={}, Ui={"pos": [0.065, 0.078], "size": [0.205, 0.018], "color": [0.3, 0.78, 0.35, 1], "back_color": [0.05, 0.12, 0.06, 0.9], "fill": 1, "border": 0.002, "border_color": [0.9, 1, 0.9, 0.6], "layer": 3},
          UiStyle={"radius": 0.006})
    for k in range(6):
        spawn(1010 + k, f"Shard icon {k + 1}", HudShard={"n": k + 1}, HudItem={}, Ui={"pos": [0.03 + 0.033 * k, 0.118], "size": [0.022, 0.039], "color": [0.16, 0.15, 0.2, 0.55], "layer": 3}, UiStyle={"radius": 0.009})
    ui_text(1017, "Shard count", "{g.dep}/6", (0.278, 0.124), 0.03, align=2, color=(1, 0.85, 0.5, 1), HudItem={})
    # the compass
    panel(1020, "Compass strip", (0.31, 0.022), (0.38, 0.05), color=(0.03, 0.04, 0.07, 0.5), layer=1, HudItem={})
    for k, (key, en, de) in enumerate([("hud.n", "N", "N"), ("hud.e", "E", "O"), ("hud.s", "S", "S"), ("hud.w", "W", "W")]):
        ui_text(1021 + k, f"Compass {en}", tr(key, en, de), (0.5, 0.03), 0.034, color=(1, 0.93, 0.7, 1), CompassMark={"kind": 0, "n": k})
    ui_text(1025, "Compass camp", tr("hud.camp", "CAMP", "LAGER"), (0.5, 0.034), 0.024, color=(1, 0.6, 0.3, 1), CompassMark={"kind": 1, "n": 0})
    spawn(1026, "Compass centre", HudItem={}, Ui={"pos": [0.4985, 0.022], "size": [0.003, 0.012], "color": [1, 1, 1, 0.9], "layer": 4})
    # the minimap: a second camera high above the hero, shown by a CameraView on a Ui
    spawn(1090, "Map camera", MapCam={}, Transform={"pos": [0, 300, 0], "rot": [-0.7071, 0, 0, 0.7071]}, Camera={"active": False, "ortho": True, "ortho_height": 452, "far": 700})
    panel(1030, "Map frame", (0.817, 0.017), (0.166, 0.296), color=(0.03, 0.03, 0.05, 0.8), radius=0.012, layer=1, HudItem={}, **{})
    spawn(1031, "Minimap", HudItem={}, Ui={"pos": [0.822, 0.026], "size": [0.156, 0.277], "color": [1, 1, 1, 1], "layer": 2}, UiStyle={"radius": 0.01}, CameraView={"camera": 1090, "width": 256, "height": 256, "every": 600})
    spawn(1032, "Map blip", MapBlip={}, Ui={"pos": [0.896, 0.1555], "size": [0.009, 0.016], "color": [1.0, 0.85, 0.2, 1], "layer": 5}, UiStyle={"radius": 0.01})
    # prompts, toasts, hints
    for k, (key, en, de) in enumerate([("prompt.talk", "E   Talk", "E   Sprechen"), ("prompt.rest", "E   Rest by the fire and save", "E   Am Feuer rasten und speichern"),
                                       ("prompt.altar", "E   Place the ember shards", "E   Die Glutsplitter ablegen"), ("prompt.beacon", "E   Light the beacon", "E   Das Leuchtfeuer entzuenden")]):
        ui_text(1040 + k, f"Prompt {k + 1}", tr(key, en, de), (0.5, 0.8), 0.04, color=(1, 0.96, 0.8, 1), visible=False, PromptText={"n": k + 1})
    for k, (key, en, de) in enumerate([("toast.shard", "An ember shard!", "Ein Glutsplitter!"), ("toast.saved", "Game saved", "Spiel gespeichert"), ("toast.placed", "The shards glow on the altar", "Die Splitter gluehen auf dem Altar"),
                                       ("toast.night", "Night falls: stay near the fire", "Die Nacht bricht an: bleib am Feuer")]):
        ui_text(1050 + k, f"Toast {k + 1}", tr(key, en, de), (0.5, 0.2), 0.05, color=(1, 0.88, 0.55, 1), visible=False, ToastText={"n": k + 1})
    ui_text(1060, "Key hint", tr("hint.keys", "WASD walk    mouse look    Shift run    Space jump    click attack    right click roll    E use    L quest log    Esc pause",
                                 "WASD gehen    Maus schauen    Shift rennen    Leertaste springen    Klick Angriff    Rechtsklick rollen    E benutzen    L Aufgaben    Esc Pause"), (0.5, 0.945), 0.022, color=(1, 1, 1, 0.85), Hint={}, wrap=0.92)
    # the boss' bar
    panel(1065, "Boss frame", (0.27, 0.905), (0.46, 0.05), color=(0.03, 0.02, 0.03, 0.7), layer=1, HudItem={}, BossBar={}, **{})
    spawn(1066, "Boss bar", BossBar={}, Ui={"pos": [0.276, 0.915], "size": [0.448, 0.026], "color": [0.9, 0.18, 0.12, 1], "back_color": [0.15, 0.04, 0.04, 0.9], "fill": 1, "layer": 3, "visible": False}, UiStyle={"radius": 0.006})
    ui_text(1067, "Boss name", tr("boss.name", "THE CALDERA WARDEN", "DER WAECHTER DER CALDERA"), (0.5, 0.874), 0.032, color=(1, 0.7, 0.55, 1), BossBarText={})
    # the screen when hurt and when dead
    spawn(1070, "Hurt flash", HurtFlash={}, Ui={"pos": [0, 0], "size": [1, 1], "color": [0.9, 0.05, 0.05, 0], "layer": 10, "visible": False}, UiEdge={"screen": True})
    spawn(1071, "Dead overlay", DeadUi={}, Ui={"pos": [0, 0], "size": [1, 1], "color": [0.3, 0.0, 0.02, 0], "layer": 11, "visible": False}, UiEdge={"screen": True})
    # the quest log
    panel(1080, "Quest panel", (0.3, 0.18), (0.4, 0.58), color=(0.06, 0.05, 0.07, 0.94), radius=0.02, layer=14, border=0.003, QuestUi={}, **{})
    ui_text(1081, "Quest title", tr("quest.title", "QUEST LOG", "AUFGABEN"), (0.5, 0.2), 0.05, color=(1, 0.9, 0.6, 1), QuestUi={}, visible=False)
    ui_text(1082, "Quest lead", tr("quest.lead", "Find the six ember shards, carry them to the altar, light the beacon.", "Finde die sechs Glutsplitter, bring sie zum Altar, entzuende das Leuchtfeuer."), (0.5, 0.265), 0.022, color=(1, 1, 1, 0.9), QuestUi={}, visible=False,
            wrap=0.36)
    rows = [("quest.s1", "The wreck on the east beach", "Das Wrack am Oststrand"), ("quest.s2", "The whale bones on the south shore", "Die Walknochen am Suedufer"), ("quest.s3", "The ruin in the forest", "Die Ruine im Wald"),
            ("quest.s4", "The island in the lake", "Die Insel im See"), ("quest.s5", "The standing stones of the highlands", "Die Steine im Hochland"), ("quest.s6", "The rim of the caldera", "Der Rand der Caldera")]
    for k, (key, en, de) in enumerate(rows):
        spawn(1090 + 10 + k, f"Quest mark {k + 1}", QuestRow={"n": k + 1}, Ui={"pos": [0.32, 0.36 + 0.06 * k], "size": [0.018, 0.032], "color": [0.18, 0.16, 0.2, 1], "layer": 15, "visible": False}, UiStyle={"radius": 0.008})
        ui_text(1110 + k, f"Quest row {k + 1}", tr(key, en, de), (0.35, 0.364 + 0.06 * k), 0.028, color=(1, 0.95, 0.85, 1), align=0, visible=False, QuestRowText={"n": k + 1})
    ui_text(1120, "Quest close", tr("quest.close", "L closes", "L schliesst"), (0.5, 0.72), 0.02, color=(1, 1, 1, 0.7), QuestUi={}, visible=False)
    # the conversation
    spawn(2100, "Dialogue text", Ui={"pos": [0.12, 0.68], "size": [0.76, 0.2], "color": [0.08, 0.07, 0.1, 0.95], "border": 0.004, "border_color": [1, 0.82, 0.5, 0.9], "text": "", "text_size": 0.032, "text_color": [1, 1, 1, 1], "button": True,
                                     "visible": False, "layer": 6}, UiStyle={"radius": 0.025, "color2": [0.04, 0.03, 0.05, 0.95], "shadow_color": [0, 0, 0, 0.5], "shadow_offset": [0.006, 0.01]}, TextFont={"file": FONT, "wrap": 0.7, "line_spacing": 0.3})
    spawn(2101, "Dialogue speaker", Text={"value": "", "pos": [0.14, 0.64], "size": 0.036, "color": [1, 0.85, 0.4, 1], "shadow": True, "visible": False}, TextFont={"file": FONT})
    for k in range(4):
        button(2102 + k, f"Choice {k + 1}", "", (0.32, 0.36 + 0.075 * k), (0.36, 0.062), {}, layer=7, color=(0.55, 0.32, 0.14, 0.96), color2=(0.3, 0.16, 0.08, 0.96), text_size=0.026, visible=False)
    # the pause menu and the settings
    spawn(2200, "Pause dim", PauseUi={}, Ui={"pos": [0, 0], "size": [1, 1], "color": [0, 0, 0, 0.62], "layer": 20, "visible": False}, UiEdge={"screen": True})
    ui_text(2210, "Paused title", tr("pause.title", "PAUSED", "PAUSE"), (0.5, 0.2), 0.09, color=(1, 0.92, 0.65, 1), PauseUi={}, visible=False)
    button(2201, "Resume", tr("pause.resume", "Resume", "Weiter"), (0.34, 0.38), (0.32, 0.085), {"PauseUi": {}, "PauseBtn": {"kind": 1}}, layer=21, visible=False)
    button(2202, "Settings", tr("pause.settings", "Settings", "Einstellungen"), (0.34, 0.5), (0.32, 0.085), {"PauseUi": {}, "PauseBtn": {"kind": 2}}, layer=21, visible=False)
    button(2203, "To the title", tr("pause.title_screen", "Quit to the title", "Zum Titel"), (0.34, 0.62), (0.32, 0.085), {"PauseUi": {}, "PauseBtn": {"kind": 3}}, layer=21, visible=False)
    settings_panel()
    ui_text(2400, "Dead text", tr("hud.fallen", "YOU FELL", "DU BIST GEFALLEN"), (0.5, 0.4), 0.12, color=(1, 0.5, 0.45, 1), visible=False, DeadUi={})


# ------------------------------------------------------------------------------------------------ the title
def build_title():
    call("reset", {"seed": 1})
    define_components(list(COMPONENTS))
    u = Uids(5000)
    dress_world(u, time=17.3, day_seconds=0)
    common_scene_bits()
    fader(900, {1: (f"{D}/island.lunaris.json", NEW_RUN)})
    spawn(38, "Island centre", Transform={"pos": [0, 24, 0]})
    spawn(7, "Camera", Transform={"pos": [0, 60, 200]}, Camera={"fov_y": 52, "near": 0.3, "far": 1800},
          CameraRig={"target": 38, "mode": 1, "look_at": True, "look_offset": [0, 0, 0], "follow_speed": 0, "yaw": 0.55, "pitch": 0.2, "distance": 215})
    ui_text(1290, "Title", tr("menu.title", "CALDERA", "CALDERA"), (0.5, 0.09), 0.17, color=(1, 0.84, 0.45, 1), MenuUi={})
    ui_text(1291, "Subtitle", tr("menu.subtitle", "a island adventure made with Lunaris", "ein Inselabenteuer, gemacht mit Lunaris"), (0.5, 0.3), 0.032, color=(1, 0.95, 0.85, 0.95), MenuUi={})
    TEXTS["menu.subtitle"] = ("an island adventure made with Lunaris", "ein Inselabenteuer, gemacht mit Lunaris")
    labels = [("Play", "menu.play", ("Play", "Spielen"), 1, {}), ("Continue", "menu.continue", ("Continue", "Fortsetzen"), 2, {"ContinueBtn": {}}),
              ("Settings", "menu.settings", ("Settings", "Einstellungen"), 3, {}), ("Quit", "menu.quit", ("Quit", "Beenden"), 4, {})]
    for i, (name, key, (en, de), kind, extra) in enumerate(labels):
        button(1301 + i, f"{name} button", tr(key, en, de), (0.36, 0.42 + 0.115 * i), (0.28, 0.09), {"MenuBtn": {"kind": kind}, "MenuUi": {}, **extra}, layer=3)
    ui_text(1292, "No save", tr("menu.nosave", "No saved game yet", "Noch kein Spielstand"), (0.5, 0.89), 0.03, color=(1, 0.6, 0.5, 1), NoSaveText={})
    ui_text(1293, "Hint", tr("menu.hint", "W S or arrows to choose, Enter to confirm, or use the mouse", "W S oder Pfeile waehlen, Enter bestaetigt, oder die Maus"), (0.5, 0.95), 0.024, color=(1, 1, 1, 0.8), MenuUi={})
    settings_panel()
    call("define_script", {"source": S.fader_script("        $game.error = 0;\n        $game.probe = 1;\n        $music = 1;\n        $settings = 0;\n") + S.TITLE + S.SETTINGS + S.MUSIC})
    set_defaults()
    call("set_resource", {"name": "music", "value": 1})
    scatter_trees()
    chk = show_check("title")
    call("save_scene", {"path": f"{D}/title.lunaris.json"})
    return chk


# ------------------------------------------------------------------------------------------------ the game over screen and the win screen
def embers(uid, y=0.0, color=(1, 0.5, 0.15, 0.9), count=160, spread=8):
    spawn(uid, "Embers", Transform={"pos": [0, y, 0]}, Particles={"count": count, "lifetime": 4.5, "looping": True, "prewarm": True, "shape": 3, "extent": [spread, 0, 0], "direction": [0, 1, 0], "cone": 30, "speed_range": [0.6, 1.8],
                                                                  "gravity": [0, 0.35, 0], "turbulence": 0.6, "size": [0.12, 0.02], "color": list(color), "color_end": [color[0], color[1] * 0.4, 0.02, 0], "additive": True, "seed": 3})


def build_gameover():
    call("reset", {"seed": 1})
    define_components(list(COMPONENTS))
    common_scene_bits()
    fader(900, {3: (f"{D}/title.lunaris.json", NEW_RUN), 11: (f"{D}/island.lunaris.json", NEW_RUN)})
    spawn(1, "Look", Environment={"sky": True, "sky_top": [0.03, 0.02, 0.05], "sky_horizon": [0.14, 0.05, 0.05], "ground": [0.03, 0.02, 0.02], "clouds": 0.0, "bloom": 0.8, "vignette": 0.55, "saturation": 0.8, "ambient": 0.2})
    spawn(2, "Sun", Transform={"rot": [-0.3, 0.3, 0.0, 0.9]}, Light={"kind": 0, "color": [0.6, 0.2, 0.15], "intensity": 0.4})
    spawn(7, "Camera", Transform={"pos": [0, 2.0, 9]}, Camera={"fov_y": 50})
    embers(8, -2.0)
    ui_text(1290, "Game over title", tr("over.title", "YOU FELL", "DU BIST GEFALLEN"), (0.5, 0.16), 0.15, color=(0.95, 0.3, 0.25, 1))
    ui_text(1291, "Stats", tr("over.stats", "Shards placed {g.dep} of 6      Foes defeated {g.kills}", "Splitter abgelegt {g.dep} von 6      Gegner besiegt {g.kills}"), (0.5, 0.42), 0.04, color=(1, 0.93, 0.78, 1))
    button(1301, "Retry button", tr("over.retry", "Back to the last fire", "Zurueck zum letzten Feuer"), (0.3, 0.58), (0.4, 0.1), {"OverBtn": {"kind": 1}}, text_size=0.036)
    button(1302, "Title button", tr("over.title_screen", "Title screen", "Titelbild"), (0.3, 0.72), (0.4, 0.1), {"OverBtn": {"kind": 2}}, text_size=0.036)
    call("define_script", {"source": S.fader_script('        emit "lose";\n        $game.probe = 1;\n        $music = 1;\n') + S.OVER + S.MUSIC})
    set_defaults()
    chk = show_check("gameover")
    call("save_scene", {"path": f"{D}/gameover.lunaris.json"})
    return chk


def build_win():
    call("reset", {"seed": 1})
    define_components(list(COMPONENTS))
    common_scene_bits()
    fader(900, {3: (f"{D}/title.lunaris.json", NEW_RUN), 11: (f"{D}/island.lunaris.json", NEW_RUN)})
    spawn(1, "Look", Environment={"sky": True, "sky_top": [0.04, 0.05, 0.16], "sky_horizon": [0.45, 0.22, 0.14], "ground": [0.05, 0.04, 0.04], "clouds": 0.2, "bloom": 0.9, "vignette": 0.35, "saturation": 1.15, "ambient": 0.5})
    spawn(2, "Sun", Transform={"rot": [-0.2, 0.3, 0.0, 0.93]}, Light={"kind": 0, "color": [1.0, 0.6, 0.3], "intensity": 1.2})
    spawn(3, "Flame light", Transform={"pos": [0, 2.5, 3]}, Light={"kind": 1, "color": [1.0, 0.65, 0.3], "intensity": 7, "range": 40})
    spawn(7, "Camera", Transform={"pos": [0, 3.0, 12]}, Camera={"fov_y": 50})
    spawn(8, "Flames", Transform={"pos": [0, 0.0, 2]},
          Particles={"count": 900, "lifetime": 1.6, "looping": True, "prewarm": True, "shape": 3, "extent": [1.0, 0, 0], "direction": [0, 1, 0], "cone": 10, "speed_range": [2.5, 5.0], "gravity": [0, 2.4, 0], "turbulence": 0.4,
                     "size": [1.5, 0.2], "color": [1, 0.7, 0.2, 0.9], "color_end": [0.9, 0.12, 0.02, 0], "additive": True, "seed": 8},
          ParticleModules={"modules": "color 0:1,0.95,0.7,0 0.1:1,0.85,0.4,0.95 0.5:1,0.5,0.12,0.8 1:0.3,0.08,0.04,0\nsize 0:0.4 0.25:1.4 1:0.2\nnoise strength=0.7 scale=0.8 speed=0.9"})
    embers(9, 0.0, count=300, spread=5)
    ui_text(1290, "Win title", tr("win.title", "THE BEACON BURNS", "DAS LEUCHTFEUER BRENNT"), (0.5, 0.1), 0.12, color=(1, 0.86, 0.45, 1))
    ui_text(1291, "Win line", tr("win.line", "The ember shards are home, and the night is over.", "Die Glutsplitter sind zuhause, und die Nacht ist vorbei."), (0.5, 0.3), 0.034, color=(1, 0.95, 0.85, 1))
    ui_text(1292, "Win stats", tr("win.stats", "Foes defeated {g.kills}", "Gegner besiegt {g.kills}"), (0.5, 0.55), 0.04, color=(1, 0.9, 0.7, 1))
    button(1301, "Again button", tr("win.again", "Play again", "Nochmal spielen"), (0.28, 0.76), (0.22, 0.09), {"WinBtn": {"kind": 1}}, text_size=0.034)
    button(1302, "Title button", tr("win.title_screen", "Title screen", "Titelbild"), (0.52, 0.76), (0.2, 0.09), {"WinBtn": {"kind": 2}}, text_size=0.034)
    call("define_script", {"source": S.fader_script('        emit "win";\n        $music = 1;\n') + S.WIN + S.MUSIC})
    set_defaults()
    chk = show_check("win")
    call("save_scene", {"path": f"{D}/win.lunaris.json"})
    return chk


# ------------------------------------------------------------------------------------------------ the first scene: the music, the mixer, then the title
MIXER = "bus master volume=1\nbus music parent=master volume=0.8\nbus sfx parent=master volume=0.9\nroute examples/caldera/music_* -> music\nroute * -> sfx"
TRACKS = [(1, "music_title.wav"), (2, "music_day.wav"), (3, "music_night.wav"), (4, "music_boss.wav")]


def build_boot():
    call("reset", {"seed": 1})
    define_components(list(COMPONENTS))
    for i, (tid, f) in enumerate(TRACKS):
        spawn(9000 + i, f"Music {tid}", Persist={}, MusicTrack={"id": tid}, MusicPlayer={"playlist": f"{D}/{f}", "crossfade": 4.0, "volume": 0.9, "repeat": True, "playing": tid == 1})
    spawn(9010, "Mixer", Persist={}, AudioMixer={"config": MIXER})
    spawn(2, "Camera", Transform={"pos": [0, 2, 8]}, Camera={"fov_y": 50})
    spawn(3, "To the title", SceneSwitch={"path": f"{D}/title.lunaris.json", "go": True})
    call("define_script", {"source": S.MUSIC})
    call("set_resource", {"name": "music", "value": 1})
    chk = show_check("boot")
    call("save_scene", {"path": f"{D}/boot.lunaris.json"})
    return chk


def stub_scenes():
    """The scenes name each other in a circle, and `check` says a switch to a file that is not there yet is a warning: a stub stands in for each until the real one is built."""
    os.makedirs(f"{D}/lang", exist_ok=True)
    for lang in ("en", "de"):
        if not os.path.exists(f"{D}/lang/{lang}.json"):
            with open(f"{D}/lang/{lang}.json", "w", encoding="utf-8", newline="\n") as f:
                f.write("{}\n")
    for name in ("boot", "title", "island", "gameover", "win"):
        path = f"{D}/{name}.lunaris.json"
        if not os.path.exists(path):
            with open(path, "w", encoding="utf-8", newline="\n") as f:
                f.write('{"lunaris": 1, "entities": []}\n')
