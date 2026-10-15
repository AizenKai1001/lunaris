"""Write examples/driving.lunaris.json, its heightmap examples/terrain/driving.png and the playtest examples/tests/driving.test.json: a raycast car (`Vehicle`,
docs/PHYSICS.md) on a proving ground. A flat meadow in a ring of hills (`Terrain`), a road of two gates (triggers on their own collision layer, so the
cones and crates do not set them off), rumble strips, a slalom of cones, a wall of crates, and a jump (a box turned 14 degrees). W / arrow up drives, S / arrow
down brakes and then reverses, A and D steer, Space is the handbrake, R puts the car back; a pad's right trigger, left trigger and left stick do the same.

    python3 tools/build_driving.py
    (it needs the `lunaris` binary: target/release/lunaris)

The layers: 1 the world (terrain, ramp, strips: wants the car and the props), 2 the car (wants the world, the gates and the props), 3 the gates (triggers: want
the car only), 4 the props (cones and crates: want the world, the car and each other).
"""
import json
import math
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import make_heightmap as mh  # noqa: E402

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
BIN = "target/release/lunaris"

# ---------------------------------------------------------------------------------------------------------------------------------- the ground
N, SIZE, RELIEF = 129, 256.0, 16.0  # samples a side, metres across, metres from black to white
CELL = SIZE / (N - 1)
FLAT, RISE = 100.0, 28.0  # the meadow is flat out to this radius; then the ground climbs over this distance


def smooth(t):
    t = max(0.0, min(1.0, t))
    return t * t * (3.0 - 2.0 * t)


rows = []
for j in range(N):
    row = []
    for i in range(N):
        x, z = -SIZE / 2 + i * CELL, -SIZE / 2 + j * CELL
        r = math.hypot(x, z)
        hills = 0.55 + 0.45 * mh.fbm(x / 40.0 + 11.0, z / 40.0 + 5.0, 3, 4)
        row.append(int(round(65535 * smooth((r - FLAT) / RISE) * min(1.0, hills))))
    rows.append(row)
os.makedirs(os.path.join(ROOT, "examples", "terrain"), exist_ok=True)
mh.write_png16(os.path.join(ROOT, "examples", "terrain", "driving.png"), rows)

# ---------------------------------------------------------------------------------------------------------------------------------- the calls
reqs = []


def call(name, args):
    reqs.append({"name": name, "arguments": args})


def spawn(uid, name, **components):
    call("spawn", {"uid": uid, "components": {"Name": {"value": name}, **components}})


def layer(n, mask):
    return {"CollisionLayer": {"layer": n, "mask": mask}}


WORLD, CAR, GATE, PROP = 1, 2, 3, 4
bit = lambda *ls: sum(1 << l for l in ls)  # noqa: E731
M_WORLD, M_CAR, M_GATE, M_PROP = bit(CAR, PROP), bit(WORLD, GATE, PROP), bit(CAR), bit(WORLD, CAR, PROP)

call("define_component", {"name": "Car", "fields": []})
call("define_component", {"name": "Gate", "fields": []})
call("define_component", {"name": "Prop", "fields": []})

# the world
spawn(1, "Environment", Environment={"sky": True, "clouds": 0.4, "fog": 0.0015, "bloom": 0.3, "ssao": 1.0})
spawn(2, "Sun", Transform={"rot": [-0.45, 0.35, 0.12, 0.8]}, Light={"kind": 0, "intensity": 3.0})
spawn(3, "Hills", Transform={"pos": [0, 0, 0]}, Terrain={"heightmap": "examples/terrain/driving.png", "size": [SIZE, SIZE], "height": RELIEF, "color": [0.40, 0.55, 0.32, 1], "texture": "grass", "uv_scale": [90, 90], "friction": 0.9}, **layer(WORLD, M_WORLD))
spawn(4, "Road", Transform={"pos": [0, 0.012, -30], "scale": [12, 0.02, 190]}, MeshRenderer={"mesh": "cube", "texture": "concrete", "uv_scale": [3, 60], "color": [0.34, 0.35, 0.38, 1], "roughness": 0.85})
for k in range(18):  # dashes down the middle
    spawn(200 + k, f"Dash {k + 1}", Transform={"pos": [0, 0.03, 40 - k * 9.5], "scale": [0.25, 0.01, 4]}, MeshRenderer={"mesh": "cube", "color": [0.92, 0.9, 0.8, 1], "roughness": 0.6})

# the track, from the start at z = +40 toward -z
for k in range(6):  # rumble strips: low bumps across the road
    spawn(230 + k, f"Rumble {k + 1}", Transform={"pos": [0, 0.035, -12 - k * 1.3], "scale": [12, 0.07, 0.4]}, Collider={"shape": 1, "size": [1, 1, 1]},
          MeshRenderer={"mesh": "cube", "color": [0.9, 0.9, 0.9, 1], "roughness": 0.7}, **layer(WORLD, M_WORLD))
for k, (x, z) in enumerate([(-2.5, -24), (2.5, -28), (-2.5, -32), (2.5, -36), (-2.5, -40), (2.5, -44)]):  # slalom cones: light, they topple
    spawn(240 + k, f"Cone {k + 1}", Prop={}, Transform={"pos": [x, 0.36, z], "scale": [0.45, 0.72, 0.45]}, Velocity={}, AngularVelocity={"damping": 0.3}, Collider={"shape": 1, "size": [1, 1, 1]},
          RigidBody={"mass": 4, "friction": 0.5, "restitution": 0.2}, MeshRenderer={"mesh": "cylinder", "color": [1.0, 0.45, 0.1, 1], "roughness": 0.5}, **layer(PROP, M_PROP))
crate = 260
for ix in range(3):  # a wall of crates beside the road
    for iy in range(3):
        spawn(crate, f"Crate {crate - 259}", Prop={}, Transform={"pos": [-9 + ix * 1.02, 0.5 + iy * 1.01, -30]}, Velocity={}, AngularVelocity={"damping": 0.3}, Collider={"shape": 1, "size": [1, 1, 1]},
              RigidBody={"mass": 18, "friction": 0.6, "restitution": 0.05}, MeshRenderer={"mesh": "cube", "texture": "wood", "color": [0.8, 0.65, 0.45, 1]}, **layer(PROP, M_PROP))
        crate += 1
for k, z in enumerate([-8, -52]):  # two gates: a translucent wall across the road that a car (and only a car) sets off
    spawn(280 + k, f"Gate {k + 1}", Gate={}, Transform={"pos": [0, 2, z], "scale": [12, 4, 0.3]}, Collider={"shape": 1, "size": [1, 1, 1], "trigger": True},
          MeshRenderer={"mesh": "cube", "color": [1.0, 0.85, 0.2, 0.22], "emissive": 0.6}, **layer(GATE, M_GATE))
    for side in (-1, 1):
        spawn(282 + 2 * k + (side + 1) // 2, f"Gate post {k + 1}", Transform={"pos": [side * 6.2, 2, z], "scale": [0.35, 4, 0.35]}, MeshRenderer={"mesh": "cylinder", "color": [0.95, 0.3, 0.2, 1], "emissive": 0.4})
th = math.radians(14.0)  # the jump: a box 10 m long turned 14 degrees about x, its low end in the ground at z = -58
h, length = 0.4, 10.0
cy = -(h / 2) * math.cos(th) + (length / 2) * math.sin(th)
cz = -58 - (h / 2) * math.sin(th) - (length / 2) * math.cos(th)
spawn(290, "Jump", Transform={"pos": [0, cy, cz], "rot": [math.sin(th / 2), 0, 0, math.cos(th / 2)], "scale": [8, h, length]}, Collider={"shape": 1, "size": [1, 1, 1]},
      MeshRenderer={"mesh": "cube", "texture": "concrete", "uv_scale": [2, 4], "color": [0.55, 0.5, 0.45, 1]}, **layer(WORLD, M_WORLD))

# the car: 1,200 kg, rear-wheel drive, a box of 1.8 x 0.7 x 4.2 for a chassis (its Transform has no scale, so the models can be its children)
WHEELS = "\n".join([
    "# a family car: wheelbase 2.6 m, track 1.6 m",
    "defaults: radius=0.35 rest=0.3 travel=0.2 spring=35000 damper=3200 grip=1.2",
    "fl: -0.8 -0.15 -1.3 steer=1 brake=1 entity=21",
    "fr: 0.8 -0.15 -1.3 steer=1 brake=1 entity=22",
    "rl: -0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1 entity=23",
    "rr: 0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1 entity=24",
])
REST = 0.35 + (0.3 - 1200 * 9.8 / 4 / 35000) + 0.15  # the height of the chassis's centre over flat ground at rest
spawn(10, "Car", Car={}, Transform={"pos": [0, round(REST + 0.02, 3), 40]}, Velocity={}, AngularVelocity={"damping": 0.05}, Collider={"shape": 1, "size": [1.8, 0.7, 4.2]},
      RigidBody={"mass": 1200, "friction": 0.3, "restitution": 0.0}, Vehicle={"wheels": WHEELS, "engine_force": 9000, "power": 90000, "top_speed": 55, "brake_force": 16000}, **layer(CAR, M_CAR))
spawn(11, "Body", Parent={"target": 10}, Transform={}, LocalTransform={"pos": [0, 0.02, 0], "scale": [1.8, 0.5, 4.2]}, MeshRenderer={"mesh": "cube", "color": [0.75, 0.12, 0.1, 1], "metallic": 0.5, "roughness": 0.3})
spawn(12, "Cabin", Parent={"target": 10}, Transform={}, LocalTransform={"pos": [0, 0.42, 0.25], "scale": [1.5, 0.42, 2.1]}, MeshRenderer={"mesh": "cube", "color": [0.2, 0.28, 0.36, 1], "metallic": 0.2, "roughness": 0.1})
for k, x in enumerate((-0.6, 0.6)):
    spawn(13 + k, f"Headlight {k + 1}", Parent={"target": 10}, Transform={}, LocalTransform={"pos": [x, 0.08, -2.12], "scale": [0.32, 0.14, 0.06]}, MeshRenderer={"mesh": "cube", "color": [1, 0.97, 0.8, 1], "emissive": 2.5})
    spawn(15 + k, f"Tail light {k + 1}", Parent={"target": 10}, Transform={}, LocalTransform={"pos": [x, 0.12, 2.12], "scale": [0.34, 0.1, 0.05]}, MeshRenderer={"mesh": "cube", "color": [1, 0.1, 0.1, 1], "emissive": 1.5})
# the wheels: a pivot the engine places (strut length, steering, spin), with the tyre (a cylinder turned onto its side) and a bar across it so that the turning shows
for k, (name, x, z) in enumerate([("front left", -0.8, -1.3), ("front right", 0.8, -1.3), ("rear left", -0.8, 1.3), ("rear right", 0.8, 1.3)]):
    spawn(21 + k, f"Wheel {name}", Parent={"target": 10}, Transform={}, LocalTransform={"pos": [x, -0.4, z]})
    spawn(25 + k, f"Tyre {name}", Parent={"target": 21 + k}, Transform={}, LocalTransform={"rot": [0, 0, 0.70710678, 0.70710678], "scale": [0.7, 0.26, 0.7]}, MeshRenderer={"mesh": "cylinder", "color": [0.06, 0.06, 0.07, 1], "roughness": 0.9})
    spawn(29 + k, f"Hub {name}", Parent={"target": 21 + k}, Transform={}, LocalTransform={"scale": [0.3, 0.52, 0.07]}, MeshRenderer={"mesh": "cube", "color": [0.8, 0.8, 0.85, 1], "metallic": 0.9, "roughness": 0.3})

# controls, the camera and the HUD
spawn(100, "Input", InputMap={"bindings": "throttle = w, arrowup, gamepad.rt\nreverse = s, arrowdown, gamepad.lt\nsteer = d, arrowright, a:-1, arrowleft:-1, gamepad.lx\nhandbrake = space, gamepad.a\nreset = r, gamepad.y"})
spawn(101, "Camera", Transform={"pos": [0, 4, 52]}, Camera={"fov_y": 62, "clear": [0.55, 0.7, 0.88, 1]}, CameraRig={"target": 10, "mode": 1, "distance": 9.5, "pitch": 0.3, "yaw": 0.0, "follow_speed": 7, "look_at": True, "look_offset": [0, 0.9, 0]})
spawn(102, "Speed", Text={"value": "{car.kmh} km/h     gates {gates} of 2", "pos": [0.03, 0.04], "size": 0.055, "align": 0, "color": [1, 1, 1, 1], "shadow": True})
spawn(103, "Hint", Text={"value": "W drive   S brake, then reverse   A D steer   Space handbrake   R put the car back", "pos": [0.5, 0.94], "size": 0.028, "align": 1, "color": [1, 1, 1, 0.85], "shadow": True})

SCRIPT = """
/// The pedals and the wheel: the actions of the `InputMap` become the car's controls.
system drive each Vehicle order 10 {
    Vehicle.throttle = $action.throttle - $action.reverse;
    Vehicle.steer = $action.steer;
    Vehicle.handbrake = $action.handbrake;
}

/// R puts the car back on the start line, upright and still.
system reset each Car, Transform, Velocity, AngularVelocity order 11 {
    if $action.reset > 0.5 {
        Transform.pos[0] = 0;
        Transform.pos[1] = 0.8;
        Transform.pos[2] = 40;
        Transform.rot[0] = 0;
        Transform.rot[1] = 0;
        Transform.rot[2] = 0;
        Transform.rot[3] = 1;
        Velocity.lin[0] = 0;
        Velocity.lin[1] = 0;
        Velocity.lin[2] = 0;
        AngularVelocity.w[0] = 0;
        AngularVelocity.w[1] = 0;
        AngularVelocity.w[2] = 0;
    }
}

/// What the HUD and the camera need to know about the car.
system car_status each Car, Vehicle, Transform order 20 {
    $car.kmh = floor(abs(Vehicle.speed) * 3.6);
    $car.x = Transform.pos[0];
    $car.z = Transform.pos[2];
    // the way the car points (its local -z), in the ground plane
    let fx = -(2 * (Transform.rot[0] * Transform.rot[2] + Transform.rot[3] * Transform.rot[1]));
    let fz = -(1 - 2 * (Transform.rot[0] * Transform.rot[0] + Transform.rot[1] * Transform.rot[1]));
    $car.heading = atan2(fx, -fz);
    $car.yaw = atan2(-fx, -fz);
}

/// The camera swings round behind the car, the short way.
system chase each CameraRig order 25 {
    let turn = $car.yaw - CameraRig.yaw;
    let short = turn - 6.2831853 * round(turn / 6.2831853);
    CameraRig.yaw += short * (1 - exp(-3.5 * dt));
}

/// Driving through a gate counts it and takes the gate down. Cones and crates are on another layer: they never set a gate off.
system gate_passed on_contact a(Gate) b(Car) order 30 {
    $gates += 1;
    emit "gate";
    despawn a;
}
"""
call("define_script", {"source": SCRIPT})
call("set_resource", {"name": "gates", "value": 0})
call("check", {})
call("save_scene", {"path": "examples/driving.lunaris.json"})

lines = [json.dumps({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": r}) for i, r in enumerate(reqs)]
p = subprocess.run([BIN, "serve", "--root", ROOT, "--no-gpu"], input="\n".join(lines) + "\n", capture_output=True, text=True, timeout=300)
bad = 0
for line in p.stdout.splitlines():
    try:
        m = json.loads(line)
    except ValueError:
        continue
    res = m.get("result") or {}
    if "error" in m or res.get("isError"):
        bad += 1
        print("request", m.get("id"), reqs[m["id"] - 1]["name"], "failed:", json.dumps(m.get("error") or res)[:400])
    elif reqs[m["id"] - 1]["name"] == "check":
        for c in res.get("content") or []:
            if c.get("type") == "text":
                print("check:", c["text"][:900])
print(f"wrote examples/driving.lunaris.json ({len(reqs)} requests, {bad} failed)")
if bad:
    sys.exit(1)

# ---------------------------------------------------------------------------------------------------------------------------------- the playtest
steps = [
    {"expect": {"check": "errors", "eq": 0}},
    {"step": 90},
    {"expect": {"name": "Car", "field": "Vehicle.on_ground", "eq": 4, "message": "all four wheels stand on the meadow"}},
    {"expect": {"name": "Car", "field": "Transform.pos[1]", "approx": [round(REST, 3), 0.02], "message": "the springs hold the chassis at its ride height over the grass"}},
    {"expect": {"resource": "car.kmh", "eq": 0}},
    {"input": {"w": True}},
    {"step": 240},
    {"expect": {"resource": "car.kmh", "ge": 60, "message": "four seconds of full throttle: well over 60 km/h"}},
    {"expect": {"resource": "gates", "ge": 1, "message": "the first gate (across the road at z = -8) was driven through"}},
    {"expect": {"name": "Car", "field": "Transform.pos[0]", "between": [-2.0, 2.0], "message": "driving straight"}},
    {"input": {"w": False, "s": True}},
    {"until": {"resource": "car.kmh", "le": 2, "max_ticks": 400}},
    {"input": {"s": False}},
    {"step": 30},
    {"expect": {"resource": "car.kmh", "le": 2, "message": "stopped by the brakes"}},
    {"expect": {"name": "Car", "field": "Vehicle.on_ground", "eq": 4}},
    {"input": {"w": True, "d": True}},
    {"step": 90},
    {"expect": {"name": "Car", "field": "Vehicle.steering", "ge": 12, "message": "the front wheels are turned to the right"}},
    {"expect": {"resource": "car.heading", "between": [0.5, 2.8], "message": "and the car has turned right (its heading, in radians, grows to the right)"}},
    {"input": {"w": False, "d": False, "s": True}},
    {"until": {"resource": "car.kmh", "le": 2, "max_ticks": 400}},
    {"input": {"s": False}},
    # the jump: put the car on the road 14 m before the ramp, standing, and drive at it
    {"call": {"tool": "set", "args": {"uid": 10, "component": "Transform", "value": {"pos": [0, 0.8, -44], "rot": [0, 0, 0, 1]}}}},
    {"call": {"tool": "set", "args": {"uid": 10, "component": "Velocity", "value": {"lin": [0, 0, 0]}}}},
    {"call": {"tool": "set", "args": {"uid": 10, "component": "AngularVelocity", "value": {"w": [0, 0, 0]}}}},
    {"input": {"w": True}},
    {"until": {"name": "Car", "field": "Vehicle.on_ground", "eq": 0, "max_ticks": 400}},
    {"expect": {"resource": "gates", "ge": 2, "message": "the second gate, at z = -52, was driven through on the way to the ramp"}},
    {"until": {"name": "Car", "field": "Vehicle.on_ground", "eq": 4, "max_ticks": 300}},
    {"expect": {"name": "Car", "field": "Transform.pos[1]", "between": [0.55, 1.0], "message": "it jumped and landed on the meadow on all four wheels (the first time all four were down again, the springs still pressed in a little)"}},
    {"input": {"w": False}},
    {"step": 120},
    {"expect": {"name": "Car", "field": "Vehicle.on_ground", "eq": 4, "message": "and it is rolling out on four wheels"}},
]
spec = {"name": "Driving: the car settles on the springs, accelerates through the first gate, brakes to a stop, turns, and jumps the ramp through the second gate (Vehicle, Terrain, trigger gates on their own collision layer)",
        "scene": "examples/driving.lunaris.json", "seed": 1, "replay_check": True, "steps": steps}
os.makedirs(os.path.join(ROOT, "examples", "tests"), exist_ok=True)
with open(os.path.join(ROOT, "examples", "tests", "driving.test.json"), "w", encoding="utf-8", newline="\n") as f:
    json.dump(spec, f, indent=2)
    f.write("\n")
print("wrote examples/tests/driving.test.json")
