"""The code from docs/TUTORIAL.md, runnable: builds a small game (Coin Chase) by calling Lunaris tools, checks it, plays it, saves it.

    python -I tools/tutorial_coin_chase.py

Needs the built engine (cargo build --release). Writes examples/coin_chase.lunaris.json.
"""
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "clients", "python"))
from lunaris import Engine  # noqa: E402

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
COINS = 10

with Engine(root=ROOT) as eng:
    eng.reset(seed=1)

    # --- 1. a component of our own: plain data with named, typed fields
    eng.define_component(name="Hero", doc="The thing the player moves.", fields=[{"name": "speed", "type": "f32", "default": 6}])
    eng.define_component(name="Coin", doc="Something to collect.", fields=[])

    # --- 2. the world: everything is an entity made of components
    def spawn(name, **components):
        return eng.spawn(components={"Name": {"value": name}, **components})["uid"]

    spawn("Floor", Transform={"scale": [40, 1, 40]}, MeshRenderer={"mesh": "plane", "color": [0.25, 0.4, 0.3, 1]})
    spawn("Sun", Transform={"rot": [-0.5, 0.2, 0, 0.84]}, Light={"kind": 0, "intensity": 1.1})
    hero = spawn("Hero", Transform={"pos": [0, 0.5, 0]}, Velocity={}, Hero={"speed": 6},
                 Collider={"shape": 0}, RigidBody={"gravity_scale": 0},
                 MeshRenderer={"mesh": "sphere", "color": [0.3, 0.55, 1.0, 1], "shininess": 80})
    # the camera is attached to the hero: it follows without any code (Parent + LocalTransform)
    spawn("Camera", Transform={}, Parent={"target": hero}, LocalTransform={"pos": [0, 11, 9], "rot": [-0.4226, 0, 0, 0.9063]}, Camera={"fov_y": 55})
    for i in range(COINS):
        a = i * 2 * math.pi / COINS
        spawn(f"Coin {i + 1}", Transform={"pos": [round(4 * math.cos(a), 2), 0.6, round(4 * math.sin(a), 2)], "scale": [0.5, 0.5, 0.5]},
              Coin={}, Velocity={"ang": [0, 3, 0]},  # the engine turns angular velocity into rotation: spinning needs no system
              Collider={"shape": 0, "trigger": True}, MeshRenderer={"mesh": "sphere", "color": [1.0, 0.8, 0.2, 1], "emissive": 0.4})
    spawn("Sfx coin", Sound={"name": "coin", "wave": 1, "freq": 880, "freq_end": 1320, "duration": 0.12, "volume": 0.2})
    spawn("Score", Text={"value": f"Coins {{score}} / {COINS}", "pos": [0.02, 0.03], "size": 0.06})
    eng.set_resource(name="score", value=0)

    # --- 3. the rules: graph systems, JSON that runs every tick
    def axis(plus, minus):
        held = lambda keys: {"max": [{"res": f"input.{k}"} for k in keys]}
        return {"sub": [held(plus), held(minus)]}

    eng.define_system(name="hero_move", each=["Hero", "Velocity"], doc="Keys become velocity.", do=[
        {"set": "Velocity.lin[0]", "to": {"mul": [{"get": "Hero.speed"}, axis(["d", "arrowright"], ["a", "arrowleft"])]}},
        {"set": "Velocity.lin[2]", "to": {"mul": [{"get": "Hero.speed"}, axis(["s", "arrowdown"], ["w", "arrowup"])]}},
    ])
    eng.define_system(name="coin_pickup", on_contact={"a": ["Hero"], "b": ["Coin"]}, doc="Touching a coin scores and removes it.", do=[
        {"res_add": "score", "by": 1},
        {"emit": "coin"},
        {"despawn": "b"},
    ])

    # --- 4. ask the engine what it thinks is wrong, look at it, and save it (everything above is in the file, as text)
    report = eng.check()
    print("check:", report["errors"], "errors,", report["warnings"], "warnings")
    assert report["ok"], report
    frame = eng.render(width=480, height=270)
    with open(os.path.join(ROOT, "target", "coin-chase.png"), "wb") as f:
        f.write(frame["png"])
    eng.save_scene(path="examples/coin_chase.lunaris.json")

    # --- 5. play it through the same tools a person's keyboard uses
    eng.input(values={"d": True})
    eng.step(ticks=60)
    eng.input(values={"d": False})
    score = eng.resources()["resources"]["score"]
    print("after holding D for one second the score is", score)
    assert score >= 1
