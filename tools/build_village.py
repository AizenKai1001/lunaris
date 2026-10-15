"""Write examples/village.lunaris.json, the language files lang/village_en.json and lang/village_de.json, and the playtest examples/tests/village.test.json: a small village
that uses many of the game blocks together (and so finds where they do not fit): `CharacterBody` + `InputMap` + `CameraRig` to walk, a `Sensor` to notice the elder,
a `Dialogue` with a quest (typed out, translated with `Localization`, drawn with a TrueType font on rounded `Ui` panels), a name tag pinned by `UiAnchor`, a minimap from a
`CameraView`, a campfire of `Particles` with a `Light`, a pickup by contact, an `EventWatch` that sets off a celebration of particles when the quest is done, and music.

    python3 tools/build_village.py     (it needs the `lunaris` binary: target/release/lunaris)
"""
import json
import os
import subprocess
import sys

BIN = "target/release/lunaris"
FONT = "examples/fonts/pixel.ttf"
reqs = []


def call(name, args):
    reqs.append({"name": name, "arguments": args})


def spawn(uid, name, **components):
    call("spawn", {"uid": uid, "components": {"Name": {"value": name}, **components}})


for tag in ("Hero", "Npc", "Bucket", "HintText", "QuestFetch", "QuestBack", "Celebration"):
    call("define_component", {"name": tag, "fields": []})

DIALOGUE = """# the elder's talk
== start
if $quest == 1 -> reminder
if $quest == 2 -> water_back
if $quest == 3 -> after
Elder: {#elder.hello}
Elder: {#elder.need}
* [{#choice.help}] -> accept
* [{#choice.later}] -> end

== accept
res quest = 1
Elder: {#elder.accept}
-> end

== reminder
Elder: {#elder.reminder}
-> end

== water_back
Elder: {#elder.water}
* [{#choice.give}] -> thanks
* [{#choice.later}] -> end

== thanks
res quest = 3
res coins += 10
event quest_done
Elder: {#elder.thanks}
-> end

== after
Elder: {#elder.after}
-> end
"""

# ---- the world
spawn(1, "Environment", Environment={"sky": True, "clouds": 0.4, "fog": 0.003, "bloom": 0.3, "ssao": 1.0})
spawn(2, "Sun", Transform={"rot": [-0.5, 0.3, 0.1, 0.8]}, Light={"kind": 0, "intensity": 2.8}, TimeOfDay={"time": 16.5, "day_seconds": 1200, "azimuth": 0.9})
spawn(3, "Ground", Transform={"pos": [0, -0.5, 0], "scale": [60, 1, 60]}, MeshRenderer={"mesh": "cube", "texture": "grass", "uv_scale": [30, 30], "color": [0.7, 0.85, 0.7, 1], "roughness": 0.95}, Collider={"shape": 1, "size": [1, 1, 1]})
uid = 4
for i, (x, z, w, d) in enumerate([(-9, -2, 5, 4), (9, -9, 4, 5), (-8, 10, 6, 4), (12, 8, 4, 4)]):
    spawn(uid, f"House {i + 1}", Transform={"pos": [x, 1.5, z], "scale": [w, 3, d]}, MeshRenderer={"mesh": "cube", "texture": "bricks", "uv_scale": [2, 1], "color": [0.9, 0.78, 0.7, 1]}, Collider={"shape": 1, "size": [1, 1, 1]})
    spawn(uid + 1, f"Roof {i + 1}", Transform={"pos": [x, 3.3, z], "scale": [w + 0.6, 0.7, d + 0.6]}, MeshRenderer={"mesh": "cube", "color": [0.55, 0.22, 0.18, 1], "roughness": 0.8})
    uid += 2
spawn(uid, "Well", Transform={"pos": [8, 0.5, -2.5], "scale": [1.6, 1.0, 1.6]}, MeshRenderer={"mesh": "cylinder", "texture": "stone", "color": [0.7, 0.7, 0.72, 1]}, Collider={"shape": 1, "size": [1, 1, 1]})
well = uid
uid = 100
for i, (x, z) in enumerate([(-14, 4), (-3, 14), (4, 16), (16, 0), (-16, -9), (5, -14), (-2, -12), (14, 14)]):
    spawn(uid, f"Trunk {i + 1}", Transform={"pos": [x, 1.2, z], "scale": [0.5, 2.4, 0.5]}, MeshRenderer={"mesh": "cylinder", "color": [0.4, 0.27, 0.16, 1]}, Collider={"shape": 1, "size": [1, 1, 1]})
    spawn(uid + 1, f"Crown {i + 1}", Transform={"pos": [x, 3.2, z], "scale": [2.2, 2.2, 2.2]}, MeshRenderer={"mesh": "sphere", "color": [0.22, 0.55, 0.25, 1], "roughness": 0.9}, Sway={"amount": 0.15, "extent": 3.0})
    uid += 2
uid = 40  # interface and gameplay entities from here on, whatever the scenery used

# ---- the campfire
spawn(uid, "Campfire light", Transform={"pos": [-4, 0.9, 5]}, Light={"kind": 1, "color": [1.0, 0.6, 0.25], "intensity": 3.0, "range": 8})
spawn(uid + 1, "Fire", Transform={"pos": [-4, 0.2, 5]}, Particles={"count": 500, "lifetime": 1.1, "shape": 3, "extent": [0.25, 0, 0], "cone": 14, "speed_range": [0.8, 1.6], "gravity": [0, 1.2, 0], "turbulence": 0.15,
    "size": [0.5, 0.1], "color": [1.0, 0.65, 0.18, 0.9], "color_end": [0.9, 0.12, 0.02, 0.0], "additive": True, "seed": 11})
spawn(uid + 2, "Smoke", Transform={"pos": [-4, 1.0, 5]}, Particles={"count": 120, "lifetime": 3.5, "shape": 3, "extent": [0.15, 0, 0], "cone": 18, "speed_range": [0.5, 0.9], "gravity": [0.3, 0.5, 0], "drag": 0.25, "turbulence": 0.4,
    "size": [0.3, 1.2], "color": [0.35, 0.35, 0.38, 0.45], "color_end": [0.2, 0.2, 0.22, 0.0], "additive": False, "fade_in": 0.2, "seed": 5})
for k, (dx, ry) in enumerate([(-0.3, 0.4), (0.3, -0.4)]):
    spawn(uid + 3 + k, f"Log {k + 1}", Transform={"pos": [-4 + dx, 0.12, 5], "rot": [0, ry * 0.5, 0, 0.9], "scale": [0.9, 0.2, 0.2]}, MeshRenderer={"mesh": "cube", "color": [0.3, 0.18, 0.1, 1]})

# ---- the elder, who talks
ELDER = 30
spawn(ELDER, "Elder", Transform={"pos": [0, 0, -4]}, Npc={}, Dialogue={"script": DIALOGUE, "text_ui": 60, "speaker_ui": 61, "choice_1": 62, "choice_2": 63, "choice_3": 64, "speed": 40})
spawn(31, "Elder body", Parent={"target": ELDER}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.6, 0.9, 0.6]}, Transform={"pos": [0, 0.9, -4]}, MeshRenderer={"mesh": "cylinder", "color": [0.55, 0.3, 0.6, 1], "roughness": 0.5})
spawn(32, "Elder head", Parent={"target": ELDER}, LocalTransform={"pos": [0, 1.95, 0], "scale": [0.55, 0.55, 0.55]}, Transform={"pos": [0, 1.95, -4]}, MeshRenderer={"mesh": "sphere", "color": [0.95, 0.8, 0.68, 1], "roughness": 0.45})
spawn(33, "Elder hair", Parent={"target": ELDER}, LocalTransform={"pos": [0, 2.15, 0], "scale": [0.6, 0.3, 0.6]}, Transform={"pos": [0, 2.15, -4]}, MeshRenderer={"mesh": "sphere", "color": [0.85, 0.85, 0.88, 1]})
spawn(34, "Celebration", Celebration={}, Transform={"pos": [0, 2.0, -4]}, Particles={"count": 400, "lifetime": 1.8, "looping": False, "playing": False, "time": 99, "shape": 1, "extent": [0.2, 0, 0], "cone": 180,
    "speed_range": [2.0, 5.0], "gravity": [0, -3, 0], "drag": 1.2, "size": [0.16, 0.04], "color": [1.0, 0.9, 0.4, 1], "color_end": [1.0, 0.4, 0.1, 0], "additive": True, "fade_in": 0.0, "seed": 9})
spawn(35, "Watch", EventWatch={"names": "quest_done"})
spawn(36, "Elder name", Text={"value": "{#name.elder}", "pos": [0, 0], "size": 0.04, "align": 1, "color": [1, 0.95, 0.6, 1], "shadow": True}, TextFont={"file": FONT}, UiAnchor={"entity": ELDER, "world": [0, 2.7, 0], "offset": [0, -0.03]})

# ---- the hero, the camera, the keys
spawn(20, "Hero", Transform={"pos": [0, 0, 9]}, Hero={}, CharacterBody={"radius": 0.4, "height": 1.8}, Sensor={"radius": 3.5, "fov": 360, "filter": "Npc"}, Collider={"shape": 0, "size": [1.8, 1.8, 1.8], "trigger": True})
spawn(21, "Hero body", Parent={"target": 20}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.5, 0.9, 0.5]}, Transform={"pos": [0, 0.9, 9]}, MeshRenderer={"mesh": "cylinder", "color": [0.2, 0.55, 0.95, 1], "roughness": 0.4})
spawn(22, "Hero head", Parent={"target": 20}, LocalTransform={"pos": [0, 1.9, 0], "scale": [0.55, 0.55, 0.55]}, Transform={"pos": [0, 1.9, 9]}, MeshRenderer={"mesh": "sphere", "color": [1, 0.85, 0.7, 1], "roughness": 0.45})
spawn(23, "Camera", Transform={"pos": [0, 8, 17]}, Camera={"fov_y": 50, "clear": [0.5, 0.65, 0.85, 1]}, CameraRig={"target": 20, "follow_speed": 4, "offset": [0, 8, 8], "look_offset": [0, 0.8, 0]})
spawn(24, "Input", InputMap={"bindings": "move_x = d, a:-1\nmove_y = s, w:-1\ninteract = e\ndialogue_next = space, enter\ndialogue_choice_1 = 1\ndialogue_choice_2 = 2\ndialogue_choice_3 = 3"})
spawn(25, "Bucket", Bucket={}, Transform={"pos": [7, 0.4, -1], "scale": [0.45, 0.5, 0.45]}, MeshRenderer={"mesh": "cylinder", "color": [0.6, 0.45, 0.25, 1]}, Collider={"shape": 0, "size": [1.6, 1.6, 1.6], "trigger": True})
spawn(26, "Music", MusicPlayer={"playlist": "examples/sounds/radio.wav volume=0.3", "crossfade": 1.0})

# ---- the interface
PANEL = {"radius": 0.035, "color2": [0.04, 0.05, 0.12, 0.94], "shadow_color": [0, 0, 0, 0.5], "shadow_offset": [0.006, 0.01]}
spawn(60, "Dialogue text", Ui={"pos": [0.08, 0.7], "size": [0.84, 0.22], "color": [0.12, 0.16, 0.3, 0.94], "border": 0.004, "border_color": [0.8, 0.85, 1, 0.9], "text": "", "text_size": 0.04, "text_color": [1, 1, 1, 1], "button": True, "visible": False, "layer": 3},
      UiStyle=PANEL, TextFont={"file": FONT, "wrap": 0.74, "line_spacing": 0.3})
spawn(61, "Dialogue speaker", Text={"value": "", "pos": [0.1, 0.645], "size": 0.045, "color": [1, 0.9, 0.4, 1], "shadow": True, "visible": False}, TextFont={"file": FONT})
for k in range(3):
    spawn(62 + k, f"Choice {k + 1}", Ui={"pos": [0.5, 0.38 + 0.085 * k], "size": [0.42, 0.07], "color": [0.2, 0.55, 0.35, 0.95], "button": True, "visible": False, "text": "", "text_size": 0.032, "text_color": [1, 1, 1, 1], "layer": 4},
          UiStyle={"radius": 0.03, "color2": [0.08, 0.3, 0.18, 0.95], "shadow_color": [0, 0, 0, 0.45], "shadow_offset": [0.004, 0.008]}, TextFont={"file": FONT})
spawn(70, "Coins", Text={"value": "{#hud.coins} {coins}", "pos": [0.03, 0.04], "size": 0.05, "color": [1, 0.9, 0.4, 1], "shadow": True}, TextFont={"file": FONT})
spawn(71, "Hint", HintText={}, Text={"value": "{#hint.talk}", "pos": [0.5, 0.88], "size": 0.045, "align": 1, "color": [1, 1, 1, 1], "shadow": True, "visible": False}, TextFont={"file": FONT})
spawn(72, "Quest fetch", QuestFetch={}, Text={"value": "{#quest.fetch}", "pos": [0.03, 0.11], "size": 0.035, "color": [0.8, 1, 0.8, 1], "shadow": True, "visible": False}, TextFont={"file": FONT})
spawn(73, "Quest back", QuestBack={}, Text={"value": "{#quest.back}", "pos": [0.03, 0.11], "size": 0.035, "color": [0.8, 1, 0.8, 1], "shadow": True, "visible": False}, TextFont={"file": FONT})
spawn(74, "Map camera", Transform={"pos": [0, 60, 0], "rot": [-0.7071068, 0, 0, 0.7071068]}, Camera={"active": False, "ortho": True, "ortho_height": 40, "clear": [0.15, 0.4, 0.2, 1]})
spawn(75, "Minimap", Ui={"pos": [0.8, 0.03], "size": [0.17, 0.3], "color": [0, 0, 0, 1], "border": 0.004, "border_color": [1, 1, 1, 0.9], "layer": 2}, CameraView={"camera": 74, "width": 128, "height": 128, "every": 6}, UiStyle={"radius": 0.03})
spawn(76, "Localization", Localization={"language": "village_en"})

call("set_resource", {"name": "quest", "value": 0})
call("set_resource", {"name": "coins", "value": 0})
call("set_resource", {"name": "near", "value": 0})
call("set_resource", {"name": "talking", "value": 0})

SCRIPT = """
/// Walking: the keys move the hero, except while the elder is talking.
system hero_walk each Hero, CharacterBody order 10 {
    CharacterBody.walk[0] = $action.move_x * 5 * (1 - $talking);
    CharacterBody.walk[1] = $action.move_y * 5 * (1 - $talking);
}

/// Does the hero's sensor see the elder?
system senses each Hero, Sensor order 11 {
    if Sensor.count > 0 {
        $near = 1;
    } else {
        $near = 0;
    }
}

system is_talking each Npc, Dialogue order 12 {
    if Dialogue.active {
        $talking = 1;
    } else {
        $talking = 0;
    }
}

/// E starts the elder's talk when the hero is close.
system interact each Npc, Dialogue order 14 {
    if $action.interact.pressed > 0.5 && $near > 0.5 && $talking < 0.5 {
        Dialogue.active = true;
    }
}

/// The bucket by the well: picking it up completes the first half of the quest.
system bucket_pickup on_contact a(Hero) b(Bucket) order 30 {
    if $quest == 1 {
        $quest = 2;
        emit "pickup";
        despawn b;
    }
}

system hint each HintText, Text order 50 {
    Text.visible = $near > 0.5 && $talking < 0.5;
}

system quest_fetch_text each QuestFetch, Text order 50 {
    Text.visible = $quest == 1;
}

system quest_back_text each QuestBack, Text order 50 {
    Text.visible = $quest == 2;
}

/// When the quest is done the elder's celebration goes off again (an `EventWatch` turns the event into the resource).
system celebrate each Celebration, Particles order 60 {
    if $event.quest_done > 0 {
        Particles.time = 0;
        Particles.playing = true;
    }
}
"""
call("define_script", {"source": SCRIPT})
call("check", {})
call("save_scene", {"path": "examples/village.lunaris.json"})

lines = [json.dumps({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": r}) for i, r in enumerate(reqs)]
p = subprocess.run([BIN, "serve", "--root", ".", "--no-gpu"], input="\n".join(lines) + "\n", capture_output=True, text=True, timeout=240)
bad = 0
for line in p.stdout.splitlines():
    try:
        m = json.loads(line)
    except ValueError:
        continue
    res = m.get("result") or {}
    if "error" in m or res.get("isError"):
        bad += 1
        print("request", m.get("id"), reqs[m["id"] - 1]["name"], "failed:", json.dumps(m.get("error") or res)[:500])
    elif reqs[m["id"] - 1]["name"] == "check":
        for c in res.get("content") or []:
            if c.get("type") == "text":
                print("check:", c["text"][:900])
print(f"wrote examples/village.lunaris.json ({len(reqs)} requests, {bad} failed)")

# ---- the language files
EN = {"hud": {"coins": "Coins"}, "hint": {"talk": "Press E to talk"}, "quest": {"fetch": "Quest: bring water from the well", "back": "Quest: take the water back to the elder"}, "name": {"elder": "Elder Maren"},
      "choice": {"help": "I will help", "later": "Maybe later", "give": "Here is your water"},
      "elder": {"hello": "Welcome, traveller.", "need": "Our bucket was left at the well. Could you fetch some water for me?", "accept": "Thank you! The well is to the east.", "reminder": "The well is to the east. Please hurry.",
                "water": "You have the water!", "thanks": "Wonderful. Take these coins for your trouble.", "after": "Thank you again, friend."}}
DE = {"hud": {"coins": "Muenzen"}, "hint": {"talk": "E druecken zum Sprechen"}, "quest": {"fetch": "Aufgabe: Wasser vom Brunnen holen", "back": "Aufgabe: Das Wasser zur Aeltesten bringen"}, "name": {"elder": "Aelteste Maren"},
      "choice": {"help": "Ich helfe", "later": "Vielleicht spaeter", "give": "Hier ist dein Wasser"},
      "elder": {"hello": "Willkommen, Reisender.", "need": "Unser Eimer steht am Brunnen. Kannst du mir Wasser holen?", "accept": "Danke! Der Brunnen ist im Osten.", "reminder": "Der Brunnen ist im Osten. Beeil dich bitte.",
                "water": "Du hast das Wasser!", "thanks": "Wunderbar. Nimm diese Muenzen als Dank.", "after": "Nochmals danke, Freund."}}
os.makedirs("lang", exist_ok=True)
for name, data in (("village_en", EN), ("village_de", DE)):
    with open(f"lang/{name}.json", "w", encoding="utf-8", newline="\n") as f:
        json.dump(data, f, indent=2, ensure_ascii=True)
        f.write("\n")


# ---- the playtest
def press(key):
    return [{"input": {key: True}}, {"step": 2}, {"input": {key: False}}, {"step": 2}]


def say():
    """One line of the talk: wait for it to type out, then continue."""
    return [{"step": 30}] + press("space")


steps = [
    {"expect": {"check": "errors", "eq": 0}},
    {"input": {"w": True}},
    {"step": 120},
    {"input": {"w": False}},
    {"step": 5},
    {"expect": {"resource": "near", "eq": 1, "message": "the hero walked up to the elder and its sensor sees them"}},
    {"expect": {"resource": "talking", "eq": 0}},
    *press("e"),
    {"step": 3},
    {"expect": {"resource": "talking", "eq": 1, "message": "E started the talk"}},
    *say(), *say(),  # hello, the need
    {"step": 30},
    *press("1"),     # I will help
    {"step": 3},
    *say(),          # the elder's thanks for the offer
    {"step": 3},
    {"expect": {"resource": "talking", "eq": 0, "message": "the talk is over"}},
    {"expect": {"resource": "quest", "eq": 1, "message": "and the quest has begun"}},
    {"input": {"d": True}},
    {"step": 80},
    {"input": {"d": False}},
    {"step": 3},
    {"expect": {"resource": "quest", "eq": 2, "message": "the hero picked the bucket up by walking into it"}},
    {"input": {"a": True}},
    {"step": 95},
    {"input": {"a": False}},
    {"step": 5},
    {"expect": {"resource": "near", "eq": 1, "message": "back at the elder"}},
    *press("e"),
    {"step": 3},
    *say(),          # you have the water
    {"step": 30},
    *press("1"),     # here is your water
    {"step": 4},
    {"expect": {"resource": "coins", "eq": 10, "message": "the elder paid"}},
    {"expect": {"resource": "quest", "eq": 3}},
    *say(),
    {"step": 3},
    {"expect": {"name": "Celebration", "field": "Particles.time", "between": [0, 2.5], "message": "the event `quest_done` restarted the celebration of particles (its clock was at 99)"}},
    {"expect": {"resource": "talking", "eq": 0}},
]
test = {"name": "Village: walk to the elder, take the quest, fetch the bucket from the well, bring it back and get paid (dialogue, sensor, contacts, events, particles)", "scene": "examples/village.lunaris.json", "seed": 1, "replay_check": True, "steps": steps}
with open("examples/tests/village.test.json", "w", encoding="utf-8", newline="\n") as f:
    json.dump(test, f, indent=2)
    f.write("\n")
print("wrote lang/village_en.json, lang/village_de.json and examples/tests/village.test.json")
sys.exit(1 if bad else 0)
