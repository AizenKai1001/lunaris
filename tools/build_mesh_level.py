"""Write examples/models/mesh_level.glb, examples/mesh_level.lunaris.json and the playtest examples/tests/mesh_level.test.json: a small courtyard made
of triangles (a model file, built here with nothing but the standard library) that things can roll, tumble and walk on, because the
entity that shows it also carries a `MeshCollider` (docs/PHYSICS.md, "Mesh colliders").

The courtyard (24 x 16 m): a stone floor with a pit in it (2 m deep), a raised platform in the west with a ramp down to the floor, a staircase of
four 0.25 m steps in the south-east that climbs to a ledge, a low wall, and walls round the edge. A ball rolls down the ramp and into the low
wall, another ball is dropped into the pit, a stack of crates stands on the platform, a box slides off the ledge and tumbles down the steps, a
capsule falls over, three rocks (`ConvexCollider`: the convex hull of examples/models/rock.glb) tumble to rest on the floor, a wedge (the hull of
examples/models/wedge.glb) is held by friction on the ramp, a guard walks by itself (`NavAgent`) from the floor up the ramp to the platform, and
the hero walks where WASD says (Space jumps) over all of it. The script also writes the two small models of the convex pieces.

    python -I tools/build_mesh_level.py
"""
import json
import math
import os
import struct

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
MODEL = "examples/models/mesh_level.glb"

# ------------------------------------------------------------------------------------------------ the geometry
HX, HZ = 12.0, 8.0  # half sizes of the courtyard
PIT = (5.0, 8.0, -2.0, 2.0)  # x0, x1, z0, z1 of the pit
PIT_DEPTH = 2.0
PLATFORM = (-12.0, -8.0, -6.0, 0.0, 1.2)  # x0, x1, z0, z1, top height (the ramp starts at its east edge)
RAMP_RUN = 4.8
STAIRS_X = (0.0, 3.0)
STEP_RISE, STEP_RUN, STEPS, STAIRS_Z0 = 0.25, 0.6, 4, 2.0
LOW_WALL = (3.0, 3.4, -8.0, -3.0, 1.0)

materials = {
    "stone": [0.4, 0.4, 0.38],
    "wood": [0.46, 0.3, 0.17],
    "brick": [0.5, 0.2, 0.15],
    "dark": [0.14, 0.14, 0.17],
    "tan": [0.58, 0.5, 0.36],
}
tris = {name: [] for name in materials}


def sub(a, b):
    return [a[i] - b[i] for i in range(3)]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def dot(a, b):
    return sum(a[i] * b[i] for i in range(3))


def quad(mat, a, b, c, d, want):
    """Two triangles for the four corners (in order round the piece), wound so that the front faces the way `want` points."""
    n = cross(sub(b, a), sub(c, a))
    if dot(n, want) >= 0:
        tris[mat] += [(a, b, c), (a, c, d)]
    else:
        tris[mat] += [(a, c, b), (a, d, c)]


def floor(mat, x0, x1, z0, z1, y):
    quad(mat, [x0, y, z0], [x0, y, z1], [x1, y, z1], [x1, y, z0], [0, 1, 0])


def wall_x(mat, x, z0, z1, y0, y1, face):
    quad(mat, [x, y0, z0], [x, y1, z0], [x, y1, z1], [x, y0, z1], [face, 0, 0])


def wall_z(mat, z, x0, x1, y0, y1, face):
    quad(mat, [x0, y0, z], [x0, y1, z], [x1, y1, z], [x1, y0, z], [0, 0, face])


def block(mat, x0, x1, y0, y1, z0, z1, top=None):
    """A closed block without a bottom (it stands on the floor)."""
    floor(top or mat, x0, x1, z0, z1, y1)
    wall_x(mat, x0, z0, z1, y0, y1, -1)
    wall_x(mat, x1, z0, z1, y0, y1, 1)
    wall_z(mat, z0, x0, x1, y0, y1, -1)
    wall_z(mat, z1, x0, x1, y0, y1, 1)


# the floor, with a hole for the pit
px0, px1, pz0, pz1 = PIT
floor("stone", -HX, px0, -HZ, HZ, 0.0)
floor("stone", px1, HX, -HZ, HZ, 0.0)
floor("stone", px0, px1, pz1, HZ, 0.0)
floor("stone", px0, px1, -HZ, pz0, 0.0)
# the pit: its floor and four walls (facing in)
floor("dark", px0, px1, pz0, pz1, -PIT_DEPTH)
wall_x("stone", px0, pz0, pz1, -PIT_DEPTH, 0.0, 1)
wall_x("stone", px1, pz0, pz1, -PIT_DEPTH, 0.0, -1)
wall_z("stone", pz0, px0, px1, -PIT_DEPTH, 0.0, 1)
wall_z("stone", pz1, px0, px1, -PIT_DEPTH, 0.0, -1)

# the platform and the ramp down from its east edge
plx0, plx1, plz0, plz1, plh = PLATFORM
block("tan", plx0, plx1, 0.0, plh, plz0, plz1)
rx1 = plx1 + RAMP_RUN
quad("wood", [plx1, plh, plz0], [plx1, plh, plz1], [rx1, 0.0, plz1], [rx1, 0.0, plz0], [0, 1, 0])
for z, face in ((plz0, -1), (plz1, 1)):  # the two triangular sides of the ramp
    a, b, c = [plx1, 0.0, z], [plx1, plh, z], [rx1, 0.0, z]
    tris["tan"].append((a, b, c) if cross(sub(b, a), sub(c, a))[2] * face >= 0 else (a, c, b))

# the staircase (treads and risers, along +z) and the ledge it leads to
sx0, sx1 = STAIRS_X
for k in range(STEPS):
    z0 = STAIRS_Z0 + STEP_RUN * k
    z1 = HZ if k == STEPS - 1 else z0 + STEP_RUN
    floor("wood", sx0, sx1, z0, z1, STEP_RISE * (k + 1))
    wall_z("wood", z0, sx0, sx1, STEP_RISE * k, STEP_RISE * (k + 1), -1)
    for x, face in ((sx0, -1), (sx1, 1)):
        wall_x("tan", x, z0, z1, 0.0, STEP_RISE * (k + 1), face)

# a low wall for the ball to run into
lx0, lx1, lz0, lz1, lh = LOW_WALL
block("brick", lx0, lx1, 0.0, lh, lz0, lz1)

# the walls round the courtyard (3 m, facing in)
wall_x("brick", -HX, -HZ, HZ, 0.0, 3.0, 1)
wall_x("brick", HX, -HZ, HZ, 0.0, 3.0, -1)
wall_z("brick", -HZ, -HX, HX, 0.0, 3.0, 1)
wall_z("brick", HZ, -HX, HX, 0.0, 3.0, -1)


def glb_bytes(tri_sets, node):
    """One mesh, one primitive for each material, flat shaded (every triangle has its own three vertices and a normal)."""
    blob = bytearray()
    views, accessors, prims, mats = [], [], [], []
    for name, rgb in materials.items():
        t = tri_sets.get(name)
        if not t:
            continue
        pos, nrm = [], []
        for a, b, c in t:
            n = cross(sub(b, a), sub(c, a))
            ln = math.sqrt(dot(n, n)) or 1.0
            n = [x / ln for x in n]
            for v in (a, b, c):
                pos.append(v)
                nrm.append(n)
        ids = []
        for data in (pos, nrm):
            off = len(blob)
            for v in data:
                blob += struct.pack("<3f", *v)
            views.append({"buffer": 0, "byteOffset": off, "byteLength": len(data) * 12})
            acc = {"bufferView": len(views) - 1, "componentType": 5126, "count": len(data), "type": "VEC3"}
            if data is pos:
                acc["min"] = [min(p[k] for p in pos) for k in range(3)]
                acc["max"] = [max(p[k] for p in pos) for k in range(3)]
            accessors.append(acc)
            ids.append(len(accessors) - 1)
        mats.append({"name": name, "pbrMetallicRoughness": {"baseColorFactor": rgb + [1.0], "metallicFactor": 0.0, "roughnessFactor": 0.85}})
        prims.append({"attributes": {"POSITION": ids[0], "NORMAL": ids[1]}, "material": len(mats) - 1})
    doc = {
        "asset": {"version": "2.0", "generator": "lunaris tools/build_mesh_level.py"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"name": node, "mesh": 0}],
        "meshes": [{"name": node, "primitives": prims}], "materials": mats,
        "buffers": [{"byteLength": len(blob)}], "bufferViews": views, "accessors": accessors,
    }
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    body = bytes(blob)
    body += b"\0" * (-len(body) % 4)
    total = 12 + 8 + len(js) + 8 + len(body)
    return b"glTF" + struct.pack("<II", 2, total) + struct.pack("<I", len(js)) + b"JSON" + js + struct.pack("<I", len(body)) + b"BIN\0" + body


def solid(points, material):
    """The triangles of the convex solid whose corners are `points`: every three corners with all the others on one side of them, wound outwards."""
    centre = [sum(p[k] for p in points) / len(points) for k in range(3)]
    out = []
    n = len(points)
    for i in range(n):
        for j in range(i + 1, n):
            for k in range(j + 1, n):
                nrm = cross(sub(points[j], points[i]), sub(points[k], points[i]))
                if dot(nrm, nrm) < 1e-12:
                    continue
                sides = [dot(nrm, sub(p, points[i])) for m, p in enumerate(points) if m not in (i, j, k)]
                if any(s > 1e-9 for s in sides) and any(s < -1e-9 for s in sides):
                    continue
                if dot(nrm, sub(points[i], centre)) >= 0:
                    out.append((points[i], points[j], points[k]))
                else:
                    out.append((points[i], points[k], points[j]))
    return {material: out}


# the convex pieces: a rock (a stretched icosahedron) and a wedge (a triangular prism), models that a `ConvexCollider` takes the hull of
PHI = (1 + math.sqrt(5)) / 2
rock_points = []
for a, b in ((-1, PHI), (1, PHI), (-1, -PHI), (1, -PHI)):
    rock_points += [[a, b, 0], [0, a, b], [b, 0, a]]
rock_points = [[0.25 * p[0], 0.2 * p[1], 0.3 * p[2]] for p in rock_points]
A0, A1, A2, B0, B1, B2 = [-0.3, 0.0, -0.3], [0.3, 0.0, -0.3], [0.0, 0.35, -0.3], [-0.3, 0.0, 0.3], [0.3, 0.0, 0.3], [0.0, 0.35, 0.3]
wedge_points = [A0, A1, A2, B0, B1, B2]
wedge_centre = [sum(p[k] for p in wedge_points) / 6 for k in range(3)]
wedge_tris = []
for a, b, c in ((A0, A1, A2), (B0, B1, B2), (A0, A1, B1), (A0, B1, B0), (A0, A2, B2), (A0, B2, B0), (A1, A2, B2), (A1, B2, B1)):
    outward = dot(cross(sub(b, a), sub(c, a)), sub(a, wedge_centre)) >= 0
    wedge_tris.append((a, b, c) if outward else (a, c, b))
ROCK, WEDGE = "examples/models/rock.glb", "examples/models/wedge.glb"
for path, tri_sets, node in ((ROCK, solid(rock_points, "stone"), "rock"), (WEDGE, {"wood": wedge_tris}, "wedge")):
    with open(os.path.join(ROOT, path), "wb") as f:
        f.write(glb_bytes(tri_sets, node))

model_path = os.path.join(ROOT, MODEL)
with open(model_path, "wb") as f:
    f.write(glb_bytes(tris, "courtyard"))
count = sum(len(v) for v in tris.values())

# ------------------------------------------------------------------------------------------------ the scene
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


def qz(a):
    return [0, 0, round(math.sin(a / 2), 6), round(math.cos(a / 2), 6)]


def r3(v):
    return [round(x, 4) for x in v]


add("Environment", Environment={"sky": True, "sky_top": [0.2, 0.42, 0.84], "sky_horizon": [0.82, 0.88, 0.96], "ground": [0.3, 0.28, 0.25], "clouds": 0.25, "fog": 0.003, "fog_color": [0.82, 0.88, 0.96], "bloom": 0.1})
add("Sun", Transform={"rot": [-0.55, 0.3, 0.16, 0.76]}, Light={"kind": 0, "color": [1.0, 0.94, 0.84], "intensity": 2.6})
add("Courtyard", Transform={}, MeshRenderer={"mesh": MODEL, "color": [1, 1, 1, 1], "roughness": 0.85}, MeshCollider={"mesh": MODEL, "friction": 0.7, "restitution": 0.1})

# the ball: starts on the platform and rolls east off the ramp into the low wall
BALL_R = 0.3
add("Ball", Transform={"pos": [-9.5, plh + BALL_R + 0.003, -4.5], "scale": [2 * BALL_R] * 3}, Velocity={"lin": [1.5, 0, 0]}, AngularVelocity={"damping": 0.3}, Collider={"shape": 0, "size": [1, 1, 1]},
    RigidBody={"mass": 1.0, "friction": 0.6, "restitution": 0.15}, MeshRenderer={"mesh": "sphere", "color": [0.88, 0.16, 0.12, 1], "metallic": 0.3, "roughness": 0.3})
# a second ball dropped into the pit
add("Pit ball", Transform={"pos": [6.5, 3.0, 0.2], "scale": [2 * BALL_R] * 3}, Velocity={}, AngularVelocity={"damping": 0.3}, Collider={"shape": 0, "size": [1, 1, 1]},
    RigidBody={"mass": 1.0, "friction": 0.6, "restitution": 0.2}, MeshRenderer={"mesh": "sphere", "color": [0.95, 0.8, 0.2, 1], "metallic": 0.3, "roughness": 0.3})
# three crates stacked on the platform
for k in range(3):
    add(f"Crate {k + 1}", Transform={"pos": [-10.5, plh + 0.3 + 0.6 * k + 0.001 * k, -1.2], "scale": [0.6, 0.6, 0.6]}, Velocity={}, AngularVelocity={"damping": 0.3}, Collider={"shape": 1, "size": [1, 1, 1]},
        RigidBody={"mass": 1.0, "friction": 0.7, "restitution": 0.0}, MeshRenderer={"mesh": "cube", "color": [0.7, 0.52, 0.3, 1], "texture": "wood", "roughness": 0.6})
# a box on the ledge at the top of the stairs, pushed off it: it tumbles down the steps
add("Tumbler", Transform={"pos": [1.5, STEP_RISE * STEPS + 0.25, 7.0], "scale": [0.5, 0.5, 0.5]}, Velocity={"lin": [0, 0, -8.0]}, AngularVelocity={"damping": 0.2}, Collider={"shape": 1, "size": [1, 1, 1]},
    RigidBody={"mass": 1.0, "friction": 0.5, "restitution": 0.1}, MeshRenderer={"mesh": "cube", "color": [0.3, 0.55, 0.85, 1], "roughness": 0.4})
# a capsule that falls over on the floor
add("Capsule", Transform={"pos": [9.5, 2.0, 5.0], "rot": qz(1.2), "scale": [0.4, 1.2, 0.4]}, Velocity={}, AngularVelocity={"damping": 0.3}, Collider={"shape": 2, "size": [1, 1, 1]},
    RigidBody={"mass": 1.0, "friction": 0.6, "restitution": 0.05}, MeshRenderer={"mesh": "cylinder", "color": [0.4, 0.8, 0.45, 1], "roughness": 0.4})

# convex pieces (`ConvexCollider`: the hull of a model, with the mass spread over its volume): three rocks dropped on the floor, which tumble
# and settle on whichever face they land on, and a wedge put on the ramp, which friction holds there
rocks = []
for k, (x, y, z, s, q) in enumerate([(-2.5, 4.0, 2.5, 1.0, [0.2, 0.1, 0.3, 0.92]), (-1.2, 6.0, 3.4, 1.4, [0.5, 0.0, 0.1, 0.86]), (0.2, 5.0, -1.0, 0.8, [0.0, 0.3, 0.5, 0.81])]):
    rocks.append((add(f"Rock {k + 1}", Transform={"pos": [x, y, z], "rot": q, "scale": [s] * 3}, Velocity={}, AngularVelocity={"damping": 0.3}, ConvexCollider={"mesh": ROCK},
                      RigidBody={"mass": 2.0 * s ** 3, "friction": 0.7, "restitution": 0.1}, MeshRenderer={"mesh": ROCK, "color": [0.75, 0.74, 0.72, 1], "roughness": 0.9}), s))
RAMP_TILT = math.atan2(plh, RAMP_RUN)
ramp_y = lambda x: plh * (1 - (x - plx1) / RAMP_RUN)
WEDGE_AT = [-6.0, ramp_y(-6.0) + 0.005, -0.8]
add("Wedge", Transform={"pos": WEDGE_AT, "rot": qz(-RAMP_TILT)}, Velocity={}, AngularVelocity={"damping": 0.3}, ConvexCollider={"mesh": WEDGE}, RigidBody={"mass": 1.0, "friction": 0.5, "restitution": 0.0},
    MeshRenderer={"mesh": WEDGE, "color": [0.9, 0.7, 0.3, 1], "roughness": 0.5})

# the guard walks by itself: from the floor, up the ramp, to a spot on the platform
GUARD_TARGET = [-10.5, plh, -3.5]
guard = add("Guard", Transform={"pos": [-1.0, 0.0, -1.5]}, CharacterBody={"radius": 0.35, "height": 1.8, "step_height": 0.4}, NavAgent={"target": GUARD_TARGET, "active": True, "speed": 2.5, "radius": 0.4, "height": 1.8})
add("Guard body", Transform={"pos": [-1.0, 0.9, -1.5], "scale": [0.7, 1.8, 0.7]}, Parent={"target": guard}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.7, 1.8, 0.7]}, MeshRenderer={"mesh": "cylinder", "color": [0.3, 0.35, 0.85, 1], "roughness": 0.5})
# the hero: WASD walks, Space jumps
hero = add("Hero", Transform={"pos": [9.5, 0.0, -5.5]}, CharacterBody={"radius": 0.35, "height": 1.8, "step_height": 0.4})
add("Hero body", Transform={"pos": [9.5, 0.9, -5.5], "scale": [0.7, 1.8, 0.7]}, Parent={"target": hero}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.7, 1.8, 0.7]}, MeshRenderer={"mesh": "cylinder", "color": [0.95, 0.5, 0.25, 1], "roughness": 0.5})
add("Hero head", Transform={"pos": [9.5, 2.0, -5.5], "scale": [0.55, 0.55, 0.55]}, Parent={"target": hero}, LocalTransform={"pos": [0, 1.95, 0], "scale": [0.55, 0.55, 0.55]}, MeshRenderer={"mesh": "sphere", "color": [1.0, 0.85, 0.7, 1], "roughness": 0.45})

# the camera: above and in front of the middle of the courtyard
look, cam = [0.0, 0.3, -0.5], [0.0, 13.0, 15.0]
pitch = math.atan2(cam[1] - look[1], cam[2] - look[2])
add("Camera", Transform={"pos": cam, "rot": [round(-math.sin(pitch / 2), 5), 0, 0, round(math.cos(pitch / 2), 5)]}, Camera={"fov_y": 52, "clear": [0.5, 0.65, 0.85, 1]})
add("Hint", Text={"value": "WASD walk   Space jump", "pos": [0.5, 0.04], "size": 0.04, "align": 1, "color": [1, 1, 1, 1], "shadow": True})

doc = {"lunaris": 1, "entities": entities, "systems": [
    {"name": "walk_input", "doc": "Keys into the character controller: A/D and W/S set the walking speed (4 m/s), Space asks for a jump (6 m/s up) when on the ground. The guard is steered by its NavAgent after this system, which wins.",
     "each": ["CharacterBody"], "do": [
         {"set": "CharacterBody.walk[0]", "to": {"mul": [{"sub": [{"res": "input.d"}, {"res": "input.a"}]}, 4]}},
         {"set": "CharacterBody.walk[1]", "to": {"mul": [{"sub": [{"res": "input.s"}, {"res": "input.w"}]}, 4]}},
         {"if": {"gt": [{"res": "input.space"}, 0.5]}, "then": [{"set": "CharacterBody.jump", "to": 6}]},
     ]},
]}
scene_path = os.path.join(ROOT, "examples", "mesh_level.lunaris.json")
with open(scene_path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")

# ------------------------------------------------------------------------------------------------ the playtest
rest_y = plh + 0.3  # a crate's centre on the platform
test = {
    "name": "Mesh level: a ball rolls down the ramp of a model-file courtyard into a low wall, another comes to rest at the bottom of the pit, crates stay stacked on the platform, a box tumbles down the steps, three convex rocks settle on the floor and a wedge stays on the ramp, the guard walks up the ramp by itself, the hero stands on the floor and is stopped by the wall",
    "scene": "examples/mesh_level.lunaris.json",
    "seed": 1,
    "replay_check": True,
    "steps": [
        {"expect": {"check": "errors", "eq": 0}},
        {"step": 90},
        {"expect": {"name": "Ball", "field": "Transform.pos[0]", "between": [-8.0, -2.0], "message": "the ball is rolling down the ramp"}},
        {"expect": {"name": "Ball", "field": "Transform.pos[1]", "between": [0.28, plh + 0.4], "message": "on the surface of the ramp, not through it"}},
        {"step": 510},
        {"expect": {"name": "Ball", "field": "Transform.pos[0]", "between": [-1.0, 2.75], "message": "the ball ran across the floor and stopped at the low wall (x = 3)"}},
        {"expect": {"name": "Ball", "field": "Transform.pos[1]", "approx": [0.3, 0.03], "message": "and lies on the floor"}},
        {"expect": {"name": "Pit ball", "field": "Transform.pos[1]", "approx": [round(-PIT_DEPTH + BALL_R, 2), 0.03], "message": "the other ball fell into the pit and lies on its floor (2 m down)"}},
        {"expect": {"name": "Pit ball", "field": "Transform.pos[0]", "between": [5.3, 7.7], "message": "inside the pit"}},
        {"expect": {"name": "Crate 1", "field": "Transform.pos[1]", "approx": [round(rest_y, 3), 0.02], "message": "the bottom crate stands on the platform (1.2 m up)"}},
        {"expect": {"name": "Crate 3", "field": "Transform.pos[1]", "approx": [round(rest_y + 1.2, 3), 0.04], "message": "and the third is still on top of the other two"}},
        {"expect": {"name": "Tumbler", "field": "Transform.pos[2]", "lt": STAIRS_Z0 + 0.4, "message": "the box slid off the ledge and went down the steps"}},
        {"expect": {"name": "Tumbler", "field": "Transform.pos[1]", "lt": 0.6, "message": "and lies on the floor at the bottom"}},
        {"expect": {"name": "Capsule", "field": "Transform.pos[1]", "approx": [0.2, 0.03], "message": "the capsule fell over and lies on the floor (0.2 m radius)"}},
        *[{"expect": {"name": f"Rock {k + 1}", "field": "Transform.pos[1]", "between": [round(0.12 * s, 3), round(0.55 * s, 3)], "message": f"rock {k + 1} has tumbled and lies on the floor on one of its faces (its centre is between {0.12 * s:.2f} and {0.55 * s:.2f} m up, whichever face)"}} for k, (_, s) in enumerate(rocks)],
        *[{"expect": {"name": f"Rock {k + 1}", "field": "Velocity.lin[1]", "between": [-0.05, 0.05], "message": "and is not moving up or down"}} for k in range(len(rocks))],
        {"expect": {"name": "Wedge", "field": "Transform.pos[0]", "between": [round(WEDGE_AT[0] - 0.08, 2), round(WEDGE_AT[0] + 0.08, 2)], "message": "the wedge is still where it was put on the ramp: friction (0.5 against 0.7) is more than the slope's 14 degrees ask for"}},
        {"expect": {"name": "Wedge", "field": "Transform.pos[1]", "approx": [round(WEDGE_AT[1], 3), 0.04], "message": "and lies on the ramp's surface, not in it"}},
        {"expect": {"name": "Guard", "field": "NavAgent.status", "eq": 2, "message": "the guard arrived at its target on the platform"}},
        {"expect": {"name": "Guard", "field": "Transform.pos[1]", "approx": [plh, 0.03], "message": "walked up the ramp: its feet are on the platform"}},
        {"expect": {"name": "Guard", "field": "CharacterBody.grounded", "eq": True}},
        {"expect": {"name": "Hero", "field": "Transform.pos[1]", "approx": [0.0, 0.02], "message": "the hero stands on the floor"}},
        {"expect": {"name": "Hero", "field": "CharacterBody.grounded", "eq": True}},
        {"input": {"a": True}},
        {"step": 120},
        {"input": {"a": False}},
        {"expect": {"name": "Hero", "field": "Transform.pos[0]", "between": [3.6, 4.0], "message": "the hero walked west at 4 m/s and was stopped by the low wall (it ends at 3.4 m, its own radius 0.35 m away from it)"}},
        {"expect": {"name": "Hero", "field": "Transform.pos[1]", "approx": [0.0, 0.05], "message": "still on the floor: the wall is higher than the 0.4 m it can step up"}},
    ],
}
tpath = os.path.join(ROOT, "examples", "tests", "mesh_level.test.json")
with open(tpath, "w", encoding="utf-8", newline="\n") as f:
    json.dump(test, f, indent=2)
    f.write("\n")
print(f"wrote {model_path} ({count} triangles), {scene_path} ({len(entities)} entities), {tpath}; guard is uid {guard}, hero is uid {hero}")
