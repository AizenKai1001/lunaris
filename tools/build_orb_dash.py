"""Builds examples/orb_dash.lunaris.json by calling Lunaris tools, the way an AI agent would: no Rust, no editor.

    python -I tools/build_orb_dash.py

Orb Dash: roll the blue ball with WASD or the arrow keys, collect the 12 gold orbs before the clock runs out. The whole game
is data: five components, a handful of graph systems, and a few Text entities for the HUD.
"""
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "clients", "python"))
from lunaris import Engine  # noqa: E402

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
ORBS = 12
TIME = 45.0


def axis(pos_keys, neg_keys):
    """An input axis: +1 while any of pos_keys is held, -1 for neg_keys, 0 for both or neither."""
    held = lambda keys: {"max": [{"res": f"input.{k}"} for k in keys]}
    return {"sub": [held(pos_keys), held(neg_keys)]}


with Engine(root=ROOT) as eng:
    eng.reset(seed=7)
    eng.define_component(name="Player", doc="The ball the human or the bot controls.", fields=[{"name": "speed", "type": "f32", "default": 6}])
    eng.define_component(name="Orb", doc="A collectable. `phase` offsets its bobbing.", fields=[{"name": "phase", "type": "f32"}])
    eng.define_component(name="Timer", doc="The round clock.", fields=[{"name": "left", "type": "f32", "default": TIME}])
    eng.define_component(name="WinBanner", doc="Marks the text shown when every orb is collected.", fields=[{"name": "n", "type": "u32"}])
    eng.define_component(name="LoseBanner", doc="Marks the text shown when time runs out.", fields=[{"name": "n", "type": "u32"}])

    def named(name, **components):
        return eng.spawn(components={"Name": {"value": name}, **components})

    # camera looks down at the arena from behind and above (rotation: pitch -54.46 degrees about X)
    named("Camera", Transform={"pos": [0, 14, 10], "rot": [-0.4577, 0, 0, 0.8891]}, Camera={"fov_y": 55, "clear": [0.07, 0.09, 0.14, 1]})
    named("Sun", Transform={"pos": [0, 10, 0], "rot": [-0.5, 0.2, 0, 0.84]}, Light={"kind": 0, "intensity": 1.1})
    named("Floor", Transform={"pos": [0, 0, 0], "scale": [120, 1, 120]}, MeshRenderer={"mesh": "plane", "color": [0.22, 0.34, 0.3, 1]})
    named("Player", Transform={"pos": [0, 0.5, 0]}, Velocity={}, Player={"speed": 6}, Collider={"shape": 0, "size": [1, 1, 1]}, RigidBody={"gravity_scale": 0},
          MeshRenderer={"mesh": "sphere", "color": [0.25, 0.55, 1.0, 1], "shininess": 80})
    for i in range(ORBS):
        a, r = i * 2.399963, 3.0 + i * 1.15  # golden-angle spiral
        named(f"Orb {i + 1}", Transform={"pos": [round(r * math.cos(a), 2), 0.7, round(r * math.sin(a), 2)], "scale": [0.6, 0.6, 0.6]},
              Orb={"phase": round(i * 0.7, 2)}, Collider={"shape": 0, "size": [1, 1, 1], "trigger": True}, MeshRenderer={"mesh": "sphere", "color": [1.0, 0.8, 0.2, 1], "shininess": 120})
    named("Clock", Timer={"left": TIME})
    # sound effects: the events the systems emit below are played by name
    named("Sfx coin", Sound={"name": "coin", "wave": 1, "freq": 880, "freq_end": 1320, "duration": 0.12, "volume": 0.22})
    named("Sfx win", Sound={"name": "win", "wave": 3, "freq": 440, "freq_end": 880, "duration": 0.6, "volume": 0.3})
    named("Sfx lose", Sound={"name": "lose", "wave": 2, "freq": 330, "freq_end": 82, "duration": 0.8, "volume": 0.22})

    # HUD: Text with {resource} placeholders is redrawn from the live state every frame
    named("Score", Text={"value": f"Orbs {{score}} / {ORBS}", "pos": [0.02, 0.03], "size": 0.06})
    named("Time", Text={"value": "Time {time_left}", "pos": [0.98, 0.03], "size": 0.06, "align": 2})
    named("Hint", Text={"value": "WASD or arrows to move", "pos": [0.5, 0.93], "size": 0.04, "align": 1, "color": [0.8, 0.85, 0.95, 0.8]})
    named("Win", WinBanner={}, Text={"value": "YOU WIN!", "pos": [0.5, 0.4], "size": 0.16, "align": 1, "color": [0.4, 1, 0.5, 1], "visible": False})
    named("Lose", LoseBanner={}, Text={"value": "TIME UP", "pos": [0.5, 0.4], "size": 0.16, "align": 1, "color": [1, 0.4, 0.4, 1], "visible": False})
    eng.set_resource(name="score", value=0)
    eng.set_resource(name="time_left", value=TIME)

    won = {"ge": [{"res": "score"}, ORBS]}
    eng.define_system(name="timer", order=90, doc="Count the clock down until every orb is collected; publish it as `time_left`.", each=["Timer"], do=[
        {"if": {"lt": [{"res": "score"}, ORBS]}, "then": [{"set": "Timer.left", "to": {"max": [0, {"sub": [{"get": "Timer.left"}, {"dt": True}]}]}}]},
        {"res_set": "time_left", "to": {"get": "Timer.left"}},
        {"if": {"and": [won, {"eq": [{"res": "won_flag"}, 0]}]}, "then": [{"res_set": "won_flag", "to": 1}, {"emit": "win"}]},
        {"if": {"and": [{"le": [{"get": "Timer.left"}, 0]}, {"not": won}, {"eq": [{"res": "lost_flag"}, 0]}]}, "then": [{"res_set": "lost_flag", "to": 1}, {"emit": "lose"}]},
    ])
    eng.define_system(name="player_move", order=100, doc="Keys to velocity; publish the position for the other systems.", each=["Player", "Transform", "Velocity"], do=[
        {"let": "live", "be": {"select": [{"and": [{"gt": [{"res": "time_left"}, 0]}, {"not": won}]}, 1, 0]}},
        {"set": "Velocity.lin[0]", "to": {"mul": [{"get": "Player.speed"}, {"var": "live"}, axis(["d", "arrowright"], ["a", "arrowleft"])]}},
        {"set": "Velocity.lin[2]", "to": {"mul": [{"get": "Player.speed"}, {"var": "live"}, axis(["s", "arrowdown"], ["w", "arrowup"])]}},
        {"set": "Transform.pos[0]", "to": {"clamp": [{"get": "Transform.pos[0]"}, -18, 18]}},
        {"set": "Transform.pos[2]", "to": {"clamp": [{"get": "Transform.pos[2]"}, -18, 18]}},
        {"res_set": "player.x", "to": {"get": "Transform.pos[0]"}},
        {"res_set": "player.z", "to": {"get": "Transform.pos[2]"}},
    ])
    eng.define_system(name="orb_pickup", order=110, doc="When the player touches an orb it scores a point and disappears.", on_contact={"a": ["Player"], "b": ["Orb"]}, do=[
        {"res_add": "score", "by": 1},
        {"emit": "coin"},
        {"despawn": "b"},
    ])
    eng.define_system(name="orb_bob", order=120, doc="Orbs bob up and down.", each=["Orb", "Transform"], do=[
        {"set": "Transform.pos[1]", "to": {"add": [0.7, {"mul": [0.15, {"sin": {"add": [{"mul": [{"tick": True}, 0.08]}, {"get": "Orb.phase"}]}}]}]}},
    ])
    eng.define_system(name="camera_follow", order=130, doc="The camera eases towards a point above and behind the player.", each=["Camera", "Transform"], do=[
        {"set": "Transform.pos[0]", "to": {"lerp": [{"get": "Transform.pos[0]"}, {"res": "player.x"}, 0.12]}},
        {"set": "Transform.pos[2]", "to": {"lerp": [{"get": "Transform.pos[2]"}, {"add": [{"res": "player.z"}, 10]}, 0.12]}},
    ])
    eng.define_system(name="win_banner", order=140, each=["WinBanner", "Text"], do=[{"set": "Text.visible", "to": won}])
    eng.define_system(name="lose_banner", order=140, each=["LoseBanner", "Text"], do=[
        {"set": "Text.visible", "to": {"and": [{"le": [{"res": "time_left"}, 0]}, {"not": won}]}},
    ])

    out = eng.save_scene(path="examples/orb_dash.lunaris.json")
    print("saved examples/orb_dash.lunaris.json -", eng.describe()["entities"], "entities,", len(eng.systems()["graph"]), "graph systems")
