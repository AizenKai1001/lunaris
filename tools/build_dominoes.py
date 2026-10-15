"""Write examples/dominoes.lunaris.json and the playtest examples/tests/dominoes.test.json: a playground for rotating bodies (docs/PHYSICS.md).
A ball rolls down a ramp (a rotated box) onto the floor and knocks over a winding row of dominoes (boxes that tip and topple, each leaning on the
next as it falls); the last one pushes a hinged door open (`Joint` kind 4, with angle limits), and a short chain of capsules on ball-and-socket
joints (kind 3) swings from a gantry at the back.

    python -I tools/build_dominoes.py
"""
import json
import math
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


def qy(a):
    return [0, round(math.sin(a / 2), 6), 0, round(math.cos(a / 2), 6)]


def qz(a):
    return [0, 0, round(math.sin(a / 2), 6), round(math.cos(a / 2), 6)]


def r3(v):
    return [round(x, 4) for x in v]


# ---- the set
add("Environment", Environment={"sky": True, "sky_top": [0.18, 0.4, 0.82], "sky_horizon": [0.8, 0.86, 0.95], "ground": [0.3, 0.28, 0.25], "clouds": 0.3, "fog": 0.004, "fog_color": [0.8, 0.86, 0.95], "bloom": 0.12})
add("Sun", Transform={"rot": [-0.52, 0.3, 0.14, 0.78]}, Light={"kind": 0, "color": [1.0, 0.93, 0.82], "intensity": 2.6})
add("Floor", Transform={"pos": [0, -0.5, 0], "scale": [24, 1, 14]}, MeshRenderer={"mesh": "cube", "texture": "wood", "uv_scale": [10, 6], "color": [0.78, 0.74, 0.7, 1], "roughness": 0.7},
    Collider={"shape": 1, "size": [1, 1, 1]}, RigidBody={"friction": 0.8, "restitution": 0.05})

# ---- the ramp: a box turned 15 degrees, its lower end level with the floor (and sunk a little into it, so the ball rolls on without a bump)
TILT = math.radians(15)
RAMP_L, RAMP_HALF_T, RAMP_W = 2.2, 0.1, 1.0  # half length, half thickness, width
RAMP_END_X = -1.0  # where the top surface meets the floor
cx = RAMP_END_X - RAMP_L * math.cos(TILT) - RAMP_HALF_T * math.sin(TILT)
cy = RAMP_L * math.sin(TILT) - RAMP_HALF_T * math.cos(TILT)
add("Ramp", Transform={"pos": [round(cx, 4), round(cy, 4), 0], "rot": qz(-TILT), "scale": [2 * RAMP_L, 2 * RAMP_HALF_T, RAMP_W]}, MeshRenderer={"mesh": "cube", "color": [0.55, 0.42, 0.3, 1], "texture": "wood", "uv_scale": [4, 1], "roughness": 0.6},
    Collider={"shape": 1, "size": [1, 1, 1]}, RigidBody={"friction": 0.9, "restitution": 0})

# ---- the ball, near the top of the ramp (its centre one radius above the surface)
BALL_R = 0.2
along = -RAMP_L + 0.35
bx = cx + along * math.cos(TILT) + (RAMP_HALF_T + BALL_R + 0.003) * math.sin(TILT)
by = cy - along * math.sin(TILT) + (RAMP_HALF_T + BALL_R + 0.003) * math.cos(TILT)
ball = add("Ball", Transform={"pos": [round(bx, 4), round(by, 4), 0], "scale": [2 * BALL_R] * 3}, Velocity={}, AngularVelocity={"damping": 0.02}, Collider={"shape": 0, "size": [1, 1, 1]},
           RigidBody={"mass": 2.0, "friction": 0.7, "restitution": 0.05}, MeshRenderer={"mesh": "sphere", "color": [0.85, 0.15, 0.12, 1], "metallic": 0.4, "roughness": 0.25})

# ---- the dominoes: a straight run, a bend to the left (away from the camera), a short run to the door
DOM_H, DOM_T, DOM_W, DOM_STEP = 0.62, 0.1, 0.32, 0.3
x, z, heading = 0.4, 0.0, 0.0
path = []
turns = [0.0] * 6 + [0.13] * 8 + [0.0] * 3
for k, turn in enumerate(turns):
    path.append((x, z, heading))
    heading += turn
    x += DOM_STEP * math.cos(heading)
    z -= DOM_STEP * math.sin(heading)
first_domino = None
for k, (px, pz, hd) in enumerate(path):
    t = k / (len(path) - 1)
    color = [0.25 + 0.6 * t, 0.55 - 0.25 * t, 0.9 - 0.65 * t, 1]
    uid = add(f"Domino {k + 1}", Transform={"pos": [round(px, 4), DOM_H / 2, round(pz, 4)], "rot": qy(hd), "scale": [DOM_T, DOM_H, DOM_W]}, Velocity={}, AngularVelocity={"damping": 0.05},
              Collider={"shape": 1, "size": [1, 1, 1]}, RigidBody={"mass": 0.25, "friction": 0.5, "restitution": 0.05},
              MeshRenderer={"mesh": "cube", "color": [round(c, 3) for c in color], "roughness": 0.4})
    first_domino = first_domino or uid
last_x, last_z, last_h = path[-1]

# ---- the door: a panel across the path, hinged on a post about the vertical axis; it can only swing forward, up to 1.5 radians
dx, dz = math.cos(last_h), -math.sin(last_h)  # the way the row runs
pz_x, pz_z = math.sin(last_h), math.cos(last_h)  # across it
DOOR_W, DOOR_H, DOOR_T = 0.8, 0.7, 0.05
gap = 0.3
door_c = [last_x + dx * gap, DOOR_H / 2 + 0.03, last_z + dz * gap]
hinge = [door_c[0] - pz_x * DOOR_W / 2, door_c[1], door_c[2] - pz_z * DOOR_W / 2]
post = add("Hinge post", Transform={"pos": [round(hinge[0] - pz_x * 0.07, 4), 0.45, round(hinge[2] - pz_z * 0.07, 4)], "rot": qy(last_h), "scale": [0.08, 0.9, 0.08]},
           MeshRenderer={"mesh": "cube", "color": [0.35, 0.3, 0.28, 1], "roughness": 0.5}, Collider={"shape": 1, "size": [1, 1, 1]})
door = add("Door", Transform={"pos": r3(door_c), "rot": qy(last_h), "scale": [DOOR_T, DOOR_H, DOOR_W]}, Velocity={}, AngularVelocity={"damping": 0.15}, Collider={"shape": 1, "size": [1, 1, 1]},
           RigidBody={"mass": 0.25, "friction": 0.4, "restitution": 0.1}, MeshRenderer={"mesh": "cube", "color": [0.7, 0.5, 0.3, 1], "texture": "wood", "roughness": 0.55},
           Joint={"kind": 4, "target": post, "anchor": r3(hinge), "axis": [0, 1, 0], "limit": [0.0, 1.5]})

# ---- a gantry with a chain of four capsules on ball-and-socket joints, swinging at the back
GX, GY, GZ = -1.6, 2.3, -1.6
add("Gantry beam", Transform={"pos": [GX, GY + 0.04, GZ], "scale": [1.0, 0.08, 0.08]}, MeshRenderer={"mesh": "cube", "color": [0.3, 0.3, 0.34, 1], "metallic": 0.6, "roughness": 0.4})
add("Gantry post", Transform={"pos": [GX - 0.5, GY / 2, GZ], "scale": [0.08, GY, 0.08]}, MeshRenderer={"mesh": "cube", "color": [0.3, 0.3, 0.34, 1], "metallic": 0.6, "roughness": 0.4})
LINK = 0.4
pivot = [GX, GY, GZ]
prev = 0
for k, a in enumerate([0.9, 0.5, 0.25, 0.0]):
    d = [math.sin(a), -math.cos(a), 0]
    c = [pivot[0] + d[0] * LINK / 2, pivot[1] + d[1] * LINK / 2, pivot[2]]
    prev = add(f"Chain {k + 1}", Transform={"pos": r3(c), "rot": qz(a), "scale": [0.12, LINK, 0.12]}, Velocity={}, AngularVelocity={"damping": 0.1}, Collider={"shape": 2, "size": [1, 1, 1]},
               RigidBody={"mass": 0.4, "friction": 0.3, "restitution": 0.1}, MeshRenderer={"mesh": "cylinder", "color": [0.85, 0.65, 0.2, 1], "metallic": 0.8, "roughness": 0.3},
               Joint={"kind": 3, "target": prev, "anchor": r3(pivot)})
    pivot = [pivot[0] + d[0] * LINK, pivot[1] + d[1] * LINK, pivot[2]]

# ---- the camera: in front, a little above, looking at the middle of the row
mid_x = (-RAMP_L * 2 + last_x) / 2 + 0.2
look = [mid_x, 0.3, -0.4]
cam = [mid_x, 3.1, 7.4]
pitch = math.atan2(cam[1] - look[1], cam[2] - look[2])
add("Camera", Transform={"pos": cam, "rot": [round(-math.sin(pitch / 2), 5), 0, 0, round(math.cos(pitch / 2), 5)]}, Camera={"fov_y": 55, "clear": [0.5, 0.65, 0.85, 1]})

doc = {"lunaris": 1, "entities": entities}
path_out = os.path.join(ROOT, "examples", "dominoes.lunaris.json")
with open(path_out, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")
print(f"wrote {path_out}: {len(entities)} entities, {len(path)} dominoes from uid {first_domino}, door uid {door}, ball uid {ball}")

test = {
    "name": "Dominoes: a ball rolls down a ramp and knocks over a winding row of dominoes, the last one pushes a hinged door open to its limit, a chain of capsules swings on ball-and-socket joints",
    "scene": "examples/dominoes.lunaris.json",
    "seed": 1,
    "replay_check": True,
    "steps": [
        {"expect": {"check": "errors", "eq": 0}},
        {"step": 60},
        {"expect": {"name": "Ball", "field": "Transform.pos[0]", "between": [-4.4, -3.6], "message": "the ball is rolling down the ramp"}},
        {"expect": {"name": "Ball", "field": "AngularVelocity.w[2]", "lt": -5, "message": "and spinning clockwise about z, as a ball rolling toward +x does (a rolling ball turns at speed / radius)"}},
        {"expect": {"name": "Domino 17", "field": "Transform.pos[1]", "approx": [0.31, 0.01], "message": "the dominoes stand"}},
        {"step": 90},
        {"expect": {"name": "Ball", "field": "Transform.pos[0]", "gt": 0.0, "message": "the ball has reached the first domino"}},
        {"expect": {"name": "Domino 17", "field": "Transform.pos[1]", "approx": [0.31, 0.01], "message": "the last domino still stands: the wave has not got there yet"}},
        {"expect": {"name": "Door", "field": "Transform.rot[1]", "approx": [round(math.sin(last_h / 2), 3), 0.01], "message": "and the door is shut"}},
        {"step": 200},
    ]
    + [{"expect": {"name": f"Domino {k}", "field": "Transform.pos[1]", "lt": 0.26, "message": "this domino has fallen (lying or leaning on the next: its centre is lower than 0.26 m)"}} for k in (1, 5, 9, 13, 17)]
    + [
        {"expect": {"name": "Domino 17", "field": "Transform.pos[1]", "lt": 0.1, "message": "the last one has nothing to lean on: flat on the floor"}},
        {"expect": {"name": "Door", "field": "Transform.rot[1]", "approx": [round(math.sin((last_h + 1.5) / 2), 3), 0.02], "message": "the door was pushed open to its limit of 1.5 radians and not past it"}},
        {"expect": {"name": "Chain 1", "field": "Transform.pos[1]", "approx": [2.1, 0.05], "message": "the chain hangs from the gantry"}},
        {"expect": {"name": "Chain 4", "field": "Transform.pos[1]", "between": [0.5, 1.5], "message": "and its end swings free below it"}},
    ],
}
tpath = os.path.join(ROOT, "examples", "tests", "dominoes.test.json")
with open(tpath, "w", encoding="utf-8", newline="\n") as f:
    json.dump(test, f, indent=2)
    f.write("\n")
print(f"wrote {tpath}")
