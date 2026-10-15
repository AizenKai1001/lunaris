"""Build examples/clockwork.lunaris.json entirely through engine tool calls (the way an AI agent would), then save it.

A small solar system made of nothing but attached entities: a pivot that spins, a planet attached to it at some distance, a
pivot attached to the planet that spins faster, a moon attached to that. There is no orbit code anywhere: spinning a parent
carries its children around (see `Parent` / `LocalTransform` in docs/AI_GUIDE.md).

    python -I tools/build_clockwork.py
"""
import math
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "clients", "python"))
from lunaris import Engine  # noqa: E402

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")


def quat_x(angle):
    return [math.sin(angle / 2), 0, 0, math.cos(angle / 2)]


with Engine(root=ROOT) as eng:
    eng.reset(seed=1)
    made = {}

    def add(name, parent=None, **components):
        comps = {"Name": {"value": name}, **components}
        if parent is not None:
            comps["Parent"] = {"target": made[parent]}
        made[name] = eng.spawn(components=comps)["uid"]
        return made[name]

    def body(name, parent, at, size, color, shininess=32, emissive=False):
        mesh = {"mesh": "sphere", "color": color, "shininess": shininess}
        if emissive:
            mesh["emissive"] = 0.8
        return add(name, parent, Transform={}, LocalTransform={"pos": at, "scale": [size] * 3}, MeshRenderer=mesh)

    def pivot(name, parent, at, spin_y, tilt_x=0.0):
        comps = {"Transform": {}, "Velocity": {"ang": [0, spin_y, 0]}}
        if parent is not None:
            comps["LocalTransform"] = {"pos": at, "rot": quat_x(tilt_x)}
        else:
            comps["Transform"] = {"pos": at, "rot": quat_x(tilt_x)}
        return add(name, parent, **comps)

    # the sun, spinning in place, with a light inside it
    add("Sun", None, Transform={"scale": [3.2, 3.2, 3.2]}, Velocity={"ang": [0, 0.3, 0]}, MeshRenderer={"mesh": "sphere", "color": [1.0, 0.82, 0.25, 1], "shininess": 4, "emissive": 1.0})
    add("Sunlight", None, Transform={}, Light={"kind": 1, "color": [1.0, 0.93, 0.8], "intensity": 3.2, "range": 60})

    # planet 1: one pivot, one planet, one fast moon
    pivot("Pivot 1", None, [0, 0, 0], 0.9)
    body("Mercury", "Pivot 1", [5.5, 0, 0], 0.7, [0.75, 0.7, 0.65, 1])

    # planet 2: a planet with a moon whose pivot is attached to the planet
    pivot("Pivot 2", None, [0, 0, 0], 0.55, tilt_x=0.12)
    body("Earth", "Pivot 2", [9.0, 0, 0], 1.3, [0.2, 0.45, 0.95, 1], shininess=64)
    pivot("Earth moon pivot", "Earth", [0, 0, 0], 3.0)
    body("Moon", "Earth moon pivot", [1.5, 0, 0], 0.4, [0.8, 0.8, 0.82, 1])

    # planet 3: bigger, with two moons turning opposite ways and a moon of a moon
    pivot("Pivot 3", None, [0, 0, 0], 0.32, tilt_x=-0.1)
    body("Giant", "Pivot 3", [13.5, 0, 0], 2.2, [0.85, 0.55, 0.3, 1])
    pivot("Giant pivot a", "Giant", [0, 0, 0], 2.0)
    body("Giant moon a", "Giant pivot a", [2.0, 0, 0], 0.35, [0.9, 0.9, 0.6, 1])
    pivot("Giant pivot b", "Giant", [0, 0, 0], -1.4, tilt_x=0.5)
    body("Giant moon b", "Giant pivot b", [2.6, 0, 0], 0.45, [0.6, 0.85, 0.7, 1])
    pivot("Moon b pivot", "Giant moon b", [0, 0, 0], 4.5)
    body("Moon b moon", "Moon b pivot", [0.9, 0, 0], 0.18, [1.0, 0.5, 0.6, 1])

    # a camera above the plane, looking down at the sun
    tilt = math.atan2(20, 17)
    add("Camera", None, Transform={"pos": [0, 20, 17], "rot": quat_x(-tilt)}, Camera={"fov_y": 52, "clear": [0.02, 0.02, 0.05, 1]})

    problems = eng.check()
    assert problems["ok"], problems
    eng.save_scene(path="examples/clockwork.lunaris.json")
    eng.step(ticks=600)
    frame = eng.render(width=640, height=360)
    open(os.path.join(ROOT, "docs", "gallery", "clockwork.png"), "wb").write(frame["png"])
    print("saved examples/clockwork.lunaris.json;", len(made), "entities; hash after 10 s:", eng.hash()["hash"])
