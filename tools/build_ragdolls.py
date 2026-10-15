"""Write examples/ragdolls.lunaris.json and the playtest examples/tests/ragdolls.test.json: three crash-test dummies made of boxes, a ball and capsules on ball-and-socket
joints (`Joint` kind 3, docs/PHYSICS.md) are dropped onto a floor with a step and a low wall. Nothing drives them: they go limp.

    python -I tools/build_ragdolls.py
"""
import json
import math
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


def qz(a):
    return [0, 0, round(math.sin(a / 2), 6), round(math.cos(a / 2), 6)]


def r3(v):
    return [round(x, 4) for x in v]


add("Environment", Environment={"sky": True, "sky_top": [0.2, 0.42, 0.84], "sky_horizon": [0.82, 0.88, 0.96], "ground": [0.3, 0.3, 0.28], "clouds": 0.25, "fog": 0.003, "fog_color": [0.82, 0.88, 0.96], "bloom": 0.1})
add("Sun", Transform={"rot": [-0.5, 0.35, 0.15, 0.77]}, Light={"kind": 0, "color": [1.0, 0.94, 0.84], "intensity": 2.4})
add("Floor", Transform={"pos": [0, -0.5, 0], "scale": [30, 1, 16]}, MeshRenderer={"mesh": "cube", "texture": "stone", "uv_scale": [12, 6], "color": [0.8, 0.78, 0.74, 1], "roughness": 0.8},
    Collider={"shape": 1, "size": [1, 1, 1]}, RigidBody={"friction": 0.9, "restitution": 0.02})
add("Step", Transform={"pos": [0.2, 0.2, 0], "scale": [2.4, 0.4, 3.0]}, MeshRenderer={"mesh": "cube", "color": [0.55, 0.5, 0.45, 1], "roughness": 0.7}, Collider={"shape": 1, "size": [1, 1, 1]}, RigidBody={"friction": 0.9, "restitution": 0.02})
add("Wall", Transform={"pos": [4.2, 0.35, 0], "scale": [0.3, 0.7, 3.0]}, MeshRenderer={"mesh": "cube", "color": [0.62, 0.38, 0.3, 1], "roughness": 0.7}, Collider={"shape": 1, "size": [1, 1, 1]}, RigidBody={"friction": 0.9, "restitution": 0.02})

PARTS = []  # names of every body, for the test


def body(name, pos, scale, mesh, shape, mass, color, joint=None, rot=None, drag=0.0):
    comps = {
        "Transform": {"pos": r3(pos), "scale": scale, **({"rot": rot} if rot else {})},
        "Velocity": {},
        "AngularVelocity": {"damping": 0.35},
        "Collider": {"shape": shape, "size": [1, 1, 1]},
        "RigidBody": {"mass": mass, "friction": 0.7, "restitution": 0.05, "drag": drag},
        "MeshRenderer": {"mesh": mesh, "color": color + [1], "roughness": 0.55},
    }
    if joint:
        comps["Joint"] = joint
    uid = add(name, **comps)
    PARTS.append(name)
    return uid


def ragdoll(tag, origin, tilt, color):
    """A figure standing at `origin` (its feet), the whole thing turned `tilt` radians about z first."""
    ox, oy, oz = origin
    c, s = math.cos(tilt), math.sin(tilt)

    def at(x, y):
        """A point of the standing figure (x across, y up from the feet), turned about the origin."""
        return [ox + x * c - y * s, oy + x * s + y * c, oz]

    rot = qz(tilt)
    skin = [0.9, 0.72, 0.58]
    # pelvis and torso
    pelvis = body(f"{tag} pelvis", at(0, 0.98), [0.34, 0.2, 0.2], "cube", 1, 9.0, color, rot=rot)
    torso = body(f"{tag} torso", at(0, 1.33), [0.36, 0.5, 0.22], "cube", 1, 14.0, color, joint={"kind": 3, "target": pelvis, "anchor": r3(at(0, 1.1))}, rot=rot)
    body(f"{tag} head", at(0, 1.72), [0.24, 0.24, 0.24], "sphere", 0, 4.0, skin, joint={"kind": 3, "target": torso, "anchor": r3(at(0, 1.58))}, rot=rot)
    for side, sx in (("left", -1), ("right", 1)):
        # arms hang from the shoulders: an upper arm and a forearm
        upper = body(f"{tag} {side} upper arm", at(sx * 0.27, 1.38), [0.1, 0.3, 0.1], "cylinder", 2, 2.2, color, joint={"kind": 3, "target": torso, "anchor": r3(at(sx * 0.27, 1.52))}, rot=rot)
        body(f"{tag} {side} forearm", at(sx * 0.27, 1.1), [0.09, 0.28, 0.09], "cylinder", 2, 1.5, skin, joint={"kind": 3, "target": upper, "anchor": r3(at(sx * 0.27, 1.24))}, rot=rot)
        # legs: a thigh and a shin
        thigh = body(f"{tag} {side} thigh", at(sx * 0.1, 0.74), [0.15, 0.42, 0.15], "cylinder", 2, 6.0, color, joint={"kind": 3, "target": pelvis, "anchor": r3(at(sx * 0.1, 0.92))}, rot=rot)
        body(f"{tag} {side} shin", at(sx * 0.1, 0.32), [0.12, 0.42, 0.12], "cylinder", 2, 3.5, [0.3, 0.3, 0.38], joint={"kind": 3, "target": thigh, "anchor": r3(at(sx * 0.1, 0.53))}, rot=rot)


ragdoll("Red", [-3.0, 1.2, 0.0], 0.35, [0.85, 0.22, 0.2])
ragdoll("Blue", [0.2, 2.6, 0.0], -0.2, [0.22, 0.45, 0.9])
ragdoll("Green", [2.6, 1.6, 0.4], 0.6, [0.25, 0.7, 0.3])
add("Camera", Transform={"pos": [0.2, 3.2, 7.6], "rot": [-0.1494, 0, 0, 0.9888]}, Camera={"fov_y": 52, "clear": [0.5, 0.65, 0.85, 1]})

doc = {"lunaris": 1, "entities": entities}
path_out = os.path.join(ROOT, "examples", "ragdolls.lunaris.json")
with open(path_out, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")
print(f"wrote {path_out}: {len(entities)} entities, {len(PARTS)} bodies")

expects = [{"expect": {"check": "errors", "eq": 0}}, {"step": 420}]
for tag in ("Red", "Blue", "Green"):
    expects.append({"expect": {"name": f"{tag} pelvis", "field": "Transform.pos[1]", "between": [0.05, 0.9], "message": f"{tag}: the figure went limp and is low (not flung into the air, not through the floor)"}})
    expects.append({"expect": {"name": f"{tag} head", "field": "Transform.pos[1]", "between": [0.05, 1.7], "message": f"{tag}: the head is where a head can be"}})
    for part in ("left shin", "right forearm", "left thigh"):
        expects.append({"expect": {"name": f"{tag} {part}", "field": "Transform.pos[1]", "between": [0.03, 1.7], "message": f"{tag} {part}: above the floor, nowhere wild"}})
expects.append({"expect": {"name": "Red pelvis", "field": "Transform.pos[0]", "between": [-9, 9], "message": "the dummies stayed on the stage"}})
test = {
    "name": "Ragdolls: three dummies of boxes and capsules on ball-and-socket joints are dropped and go limp without exploding",
    "scene": "examples/ragdolls.lunaris.json",
    "seed": 1,
    "replay_check": True,
    "steps": expects,
}
tpath = os.path.join(ROOT, "examples", "tests", "ragdolls.test.json")
with open(tpath, "w", encoding="utf-8", newline="\n") as f:
    json.dump(test, f, indent=2)
    f.write("\n")
print(f"wrote {tpath}")
