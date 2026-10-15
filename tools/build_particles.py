"""Build examples/particles.lunaris.json through tool calls: a fountain, a campfire and fireworks, all made of `Emitter` entities.

    python -I tools/build_particles.py

The fireworks show how data and rules meet: a tiny graph system sets the emitter's `burst` every two seconds; the engine does the rest.
"""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "clients", "python"))
from lunaris import Engine  # noqa: E402

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")

with Engine(root=ROOT) as eng:
    eng.reset(seed=3)
    eng.define_component(name="Burster", doc="Fires the emitter's burst every `every` ticks.", fields=[{"name": "every", "type": "u32", "default": 120}, {"name": "power", "type": "u32", "default": 120}])

    def spawn(name, **components):
        return eng.spawn(components={"Name": {"value": name}, **components})["uid"]

    spawn("Camera", Transform={"pos": [0, 1, 10]}, Camera={"ortho": True, "ortho_height": 14, "clear": [0.03, 0.03, 0.08, 1]})
    # a dim floor so the effects have something to fall on
    spawn("Ground", Transform={"pos": [0, -6.2, 0], "scale": [40, 1.2, 1]}, Sprite={"color": [0.12, 0.14, 0.2, 1]})
    spawn("Fountain", Transform={"pos": [-6, -5.6, 0]},
          Emitter={"rate": 140, "life": 1.7, "life_var": 0.15, "speed": 8, "speed_var": 0.12, "dir": [0, 1, 0], "spread": 0.05, "gravity": [0, -9.5, 0],
                   "size0": 0.3, "size1": 0.08, "color0": [0.7, 0.88, 1, 1], "color1": [0.2, 0.45, 1, 0], "texture": "circle"})
    spawn("Campfire", Transform={"pos": [0, -5.6, 0]},
          Emitter={"rate": 110, "life": 1.1, "life_var": 0.3, "speed": 2.6, "speed_var": 0.3, "dir": [0, 1, 0], "spread": 0.1, "gravity": [0, 1.4, 0],
                   "size0": 0.55, "size1": 0.05, "color0": [1, 0.88, 0.3, 1], "color1": [0.9, 0.12, 0.02, 0], "texture": "circle"})
    spawn("Fireworks", Transform={"pos": [6, 1.5, 0]}, Burster={"every": 120, "power": 140},
          Emitter={"rate": 0, "life": 1.8, "life_var": 0.25, "speed": 5.5, "speed_var": 0.45, "spread": 1, "gravity": [0, -3, 0],
                   "size0": 0.26, "size1": 0, "color0": [1, 0.92, 0.45, 1], "color1": [1, 0.25, 0.1, 0], "texture": "circle", "max": 600})
    eng.define_system(name="launch", each=["Burster", "Emitter"], doc="Every `every` ticks, tell the emitter to throw a burst.", do=[
        {"if": {"eq": [{"mod": [{"tick": True}, {"get": "Burster.every"}]}, 0]}, "then": [{"set": "Emitter.burst", "to": {"get": "Burster.power"}}]},
    ])
    problems = eng.check()
    assert problems["ok"], problems
    eng.save_scene(path="examples/particles.lunaris.json")
    eng.step(ticks=150)
    frame = eng.render(width=640, height=360)
    open(os.path.join(ROOT, "docs", "gallery", "particles.png"), "wb").write(frame["png"])
    print("saved examples/particles.lunaris.json;", eng.query(with_=["Particle"], limit=1)["total"], "particles alive after 2.5 s")
