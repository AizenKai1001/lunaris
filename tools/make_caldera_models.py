"""Writes the models of Caldera to examples/caldera/models/ with nothing but the standard library (glTF 2.0 binary, flat shaded, one vertex colour per part):

  hero.glb       a low-poly wanderer with a sword on an 18-joint skeleton; clips idle, walk, run, attack, roll, swim, air, die
  golem.glb      a stone golem, 2.6 m tall (the boss is the same model scaled up); clips sleep, wake, idle, walk, swing, slam, stagger, die
  wraith.glb     a hooded ghost that floats; clips float, lunge
  crab.glb, gull.glb   wildlife; clips scuttle and flap
  pine.glb, broadleaf.glb, palm.glb, deadtree.glb, rock1.glb, rock2.glb, rock3.glb, shard.glb   scenery and the ember shard (no skeleton)

    python -I tools/make_caldera_models.py          (a few seconds; the same bytes every time)

A part is a box, a prism, a bipyramid or a bumpy ball placed in the model's own coordinates (metres, feet on y = 0, the front towards -z, the hero's right hand at +x) and
fixed to one joint; the clips are functions of the phase of a loop sampled at 30 frames a second into rotation (and, for the hips, translation) tracks.
Colours are written in the linear space glTF uses (the display colours below are squared).
"""
import json
import math
import os
import struct

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "caldera", "models")
os.makedirs(OUT, exist_ok=True)
TAU = 2 * math.pi
FPS = 30


def lin(c):
    return tuple(max(0.0, min(1.0, v)) ** 2.2 for v in c)


# ------------------------------------------------------------------------------------------------ vector and quaternion maths
def qmul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz)


def quat_euler(rx, ry, rz, order="xyz"):
    """Degrees about the axes in `order` (the first letter first), all about the fixed axes of the parent."""
    def q(axis, deg):
        h = math.radians(deg) / 2
        s, c = math.sin(h), math.cos(h)
        return tuple(s if i == axis else 0.0 for i in range(3)) + (c,)
    deg = {"x": rx, "y": ry, "z": rz}
    out = (0.0, 0.0, 0.0, 1.0)
    for a in order:
        out = qmul(q("xyz".index(a), deg[a]), out)
    return out


def rot_point(p, e, order="xyz"):
    """Rotate point p by the Euler angles e (degrees, as quat_euler)."""
    qx, qy, qz, qw = quat_euler(*e, order=order)
    x, y, z = p
    tx, ty, tz = 2 * (qy * z - qz * y), 2 * (qz * x - qx * z), 2 * (qx * y - qy * x)
    return (x + qw * tx + qy * tz - qz * ty, y + qw * ty + qz * tx - qx * tz, z + qw * tz + qx * ty - qy * tx)


def add(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


# ------------------------------------------------------------------------------------------------ the mesh builder
class Model:
    def __init__(self):
        self.pos, self.col, self.joint, self.idx = [], [], [], []

    def _quad(self, a, b, c, d, color, joint):
        base = len(self.pos)
        for p in (a, b, c, d):
            self.pos.append(p)
            self.col.append(lin(color))
            self.joint.append(joint)
        self.idx += [base, base + 1, base + 2, base, base + 2, base + 3]

    def _tri(self, a, b, c, color, joint):
        base = len(self.pos)
        for p in (a, b, c):
            self.pos.append(p)
            self.col.append(lin(color))
            self.joint.append(joint)
        self.idx += [base, base + 1, base + 2]

    def box(self, centre, size, color, joint=0, rot=(0, 0, 0), taper=1.0, order="xyz"):
        """An axis-aligned box about `centre` (turned about its own centre by `rot`); `taper` scales the top face (1 = straight)."""
        hx, hy, hz = size[0] / 2, size[1] / 2, size[2] / 2
        c = []
        for sy in (-1, 1):
            k = 1.0 if sy < 0 else taper
            for sx, sz in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                c.append(add(centre, rot_point((sx * hx * k, sy * hy, sz * hz * k), rot, order)))
        b, t = c[:4], c[4:]
        # counter-clockwise seen from outside
        self._quad(b[0], b[1], b[2], b[3], color, joint)  # bottom (faces -y)
        self._quad(t[0], t[3], t[2], t[1], color, joint)  # top
        self._quad(b[0], t[0], t[1], b[1], color, joint)  # front (-z)
        self._quad(b[2], t[2], t[3], b[3], color, joint)  # back (+z)
        self._quad(b[3], t[3], t[0], b[0], color, joint)  # left (-x)
        self._quad(b[1], t[1], t[2], b[2], color, joint)  # right (+x)

    def prism(self, centre, r0, r1, height, color, joint=0, sides=8, rot=(0, 0, 0), squash=1.0, order="xyz"):
        """A tapering prism along y: radius r0 at the bottom, r1 at the top (0 = a cone); `squash` scales z."""
        ring0, ring1 = [], []
        for i in range(sides):
            a = TAU * i / sides
            ring0.append(add(centre, rot_point((math.cos(a) * r0, -height / 2, math.sin(a) * r0 * squash), rot, order)))
            ring1.append(add(centre, rot_point((math.cos(a) * r1, height / 2, math.sin(a) * r1 * squash), rot, order)))
        for i in range(sides):
            j = (i + 1) % sides
            if r1 > 1e-6:
                self._quad(ring0[i], ring1[i], ring1[j], ring0[j], color, joint)
            else:
                self._tri(ring0[i], add(centre, rot_point((0, height / 2, 0), rot, order)), ring0[j], color, joint)
        bottom = add(centre, rot_point((0, -height / 2, 0), rot, order))
        for i in range(sides):
            self._tri(bottom, ring0[i], ring0[(i + 1) % sides], color, joint)
        if r1 > 1e-6:
            top = add(centre, rot_point((0, height / 2, 0), rot, order))
            for i in range(sides):
                self._tri(top, ring1[(i + 1) % sides], ring1[i], color, joint)

    def bipyramid(self, centre, radius, up, down, color, joint=0, sides=6, rot=(0, 0, 0)):
        ring = [add(centre, rot_point((math.cos(TAU * i / sides) * radius, 0, math.sin(TAU * i / sides) * radius), rot)) for i in range(sides)]
        top, bottom = add(centre, rot_point((0, up, 0), rot)), add(centre, rot_point((0, -down, 0), rot))
        for i in range(sides):
            j = (i + 1) % sides
            self._tri(ring[i], top, ring[j], color, joint)
            self._tri(ring[j], bottom, ring[i], color, joint)

    def blob(self, centre, radius, color, joint=0, seed=1, jitter=0.18, squash=(1.0, 1.0, 1.0), detail=1):
        """A bumpy ball: an icosahedron, split `detail` times, every corner pushed in or out a little."""
        t = (1 + math.sqrt(5)) / 2
        v = [(-1, t, 0), (1, t, 0), (-1, -t, 0), (1, -t, 0), (0, -1, t), (0, 1, t), (0, -1, -t), (0, 1, -t), (t, 0, -1), (t, 0, 1), (-t, 0, -1), (-t, 0, 1)]
        v = [tuple(c / math.sqrt(1 + t * t) for c in p) for p in v]
        f = [(0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4), (11, 10, 2), (10, 7, 6), (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8), (3, 8, 9),
             (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1)]
        for _ in range(detail):
            cache, nf = {}, []

            def mid(a, b):
                key = (min(a, b), max(a, b))
                if key not in cache:
                    m = tuple((v[a][k] + v[b][k]) / 2 for k in range(3))
                    n = math.sqrt(sum(c * c for c in m))
                    v.append(tuple(c / n for c in m))
                    cache[key] = len(v) - 1
                return cache[key]
            for a, b, c in f:
                ab, bc, ca = mid(a, b), mid(b, c), mid(c, a)
                nf += [(a, ab, ca), (b, bc, ab), (c, ca, bc), (ab, bc, ca)]
            f = nf
        pts = []
        for i, p in enumerate(v):
            h = ((i * 2654435761 + seed * 40503) & 0xFFFF) / 65535.0
            k = 1.0 + (h - 0.5) * 2 * jitter
            pts.append(add(centre, (p[0] * radius * k * squash[0], p[1] * radius * k * squash[1], p[2] * radius * k * squash[2])))
        for a, b, c in f:
            self._tri(pts[a], pts[b], pts[c], color, joint)


# ------------------------------------------------------------------------------------------------ writing a .glb
def write_glb(name, model, joints=None, clips=None):
    """`joints`: list of (name, parent index or -1, local offset); `clips`: {name: (seconds, loop, pose function)} where pose(p) -> ({joint: (rx, ry, rz)}, {joint: (dx, dy, dz)})."""
    blob = bytearray()
    views, accessors = [], []

    def put(data, comp, typ, count, bounds=None, norm=False):
        while len(blob) % 4:
            blob.append(0)
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data)})
        blob.extend(data)
        acc = {"bufferView": len(views) - 1, "componentType": comp, "count": count, "type": typ}
        if bounds:
            acc["min"], acc["max"] = bounds
        if norm:
            acc["normalized"] = True
        accessors.append(acc)
        return len(accessors) - 1

    pos = model.pos
    attrs = {"POSITION": put(b"".join(struct.pack("<fff", *p) for p in pos), 5126, "VEC3", len(pos), ([min(p[k] for p in pos) for k in range(3)], [max(p[k] for p in pos) for k in range(3)]))}
    attrs["COLOR_0"] = put(b"".join(struct.pack("<fff", *c) for c in model.col), 5126, "VEC3", len(pos))
    nodes = []
    skins = []
    animations = []
    scene_nodes = [0]
    if joints:
        attrs["JOINTS_0"] = put(b"".join(struct.pack("<BBBB", j, 0, 0, 0) for j in model.joint), 5121, "VEC4", len(pos))
        attrs["WEIGHTS_0"] = put(b"".join(struct.pack("<ffff", 1.0, 0.0, 0.0, 0.0) for _ in pos), 5126, "VEC4", len(pos))
    idx = model.idx
    big = len(pos) > 65535
    ind = put(b"".join(struct.pack("<I" if big else "<H", i) for i in idx), 5125 if big else 5123, "SCALAR", len(idx))
    mesh_node = {"name": name, "mesh": 0}
    nodes.append(mesh_node)
    if joints:
        world = []
        for (jn, parent, off) in joints:
            world.append(add(world[parent], off) if parent >= 0 else off)
        first = len(nodes) + 1  # nodes: 0 mesh, 1 armature, then the joints
        armature = {"name": "armature", "children": [first]}
        nodes.append(armature)
        for k, (jn, parent, off) in enumerate(joints):
            nodes.append({"name": jn, "translation": list(off), "children": [first + c for c, (_, p, _) in enumerate(joints) if p == k]})
        for n in nodes:
            if "children" in n and not n["children"]:
                del n["children"]
        ibm = b"".join(struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -w[0], -w[1], -w[2], 1) for w in world)
        a_ibm = put(ibm, 5126, "MAT4", len(joints))
        skins.append({"joints": [first + k for k in range(len(joints))], "inverseBindMatrices": a_ibm, "skeleton": first})
        mesh_node["skin"] = 0
        scene_nodes = [1, 0]
        names = {jn: k for k, (jn, _, _) in enumerate(joints)}
        for cname, (secs, loop, fn) in (clips or {}).items():
            n = max(2, round(secs * FPS))
            times = [k / FPS for k in range(n + 1)]
            a_t = put(b"".join(struct.pack("<f", t) for t in times), 5126, "SCALAR", len(times), ([0.0], [times[-1]]))
            samplers, channels = [], []
            poses = []
            for k in range(n + 1):
                p = (k % n) / n if loop else k / n
                poses.append(fn(p))
            for jn, k in names.items():
                rots = [quat_euler(*poses[s][0].get(jn, (0, 0, 0))) for s in range(n + 1)]
                # keep neighbouring quaternions in the same hemisphere so the interpolation takes the short way
                for s in range(1, len(rots)):
                    if sum(a * b for a, b in zip(rots[s - 1], rots[s])) < 0:
                        rots[s] = tuple(-c for c in rots[s])
                a_r = put(b"".join(struct.pack("<ffff", *q) for q in rots), 5126, "VEC4", len(rots))
                samplers.append({"input": a_t, "output": a_r, "interpolation": "LINEAR"})
                channels.append({"sampler": len(samplers) - 1, "target": {"node": first + k, "path": "rotation"}})
                if any(jn in poses[s][1] for s in range(n + 1)):
                    off = joints[k][2]
                    trs = [add(off, poses[s][1].get(jn, (0, 0, 0))) for s in range(n + 1)]
                    a_p = put(b"".join(struct.pack("<fff", *t) for t in trs), 5126, "VEC3", len(trs))
                    samplers.append({"input": a_t, "output": a_p, "interpolation": "LINEAR"})
                    channels.append({"sampler": len(samplers) - 1, "target": {"node": first + k, "path": "translation"}})
            animations.append({"name": cname, "samplers": samplers, "channels": channels})
    doc = {
        "asset": {"version": "2.0", "generator": "lunaris tools/make_caldera_models.py"},
        "scene": 0, "scenes": [{"nodes": scene_nodes}], "nodes": nodes,
        "meshes": [{"name": name, "primitives": [{"attributes": attrs, "indices": ind, "material": 0}]}],
        "materials": [{"pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.0, "roughnessFactor": 0.85}}],
        "buffers": [{"byteLength": len(blob)}], "bufferViews": views, "accessors": accessors,
    }
    if skins:
        doc["skins"] = skins
    if animations:
        doc["animations"] = animations
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    glb = b"glTF" + struct.pack("<II", 2, 12 + 8 + len(js) + 8 + len(blob)) + struct.pack("<I", len(js)) + b"JSON" + js + struct.pack("<I", len(blob)) + b"BIN\x00" + bytes(blob)
    with open(os.path.join(OUT, name + ".glb"), "wb") as f:
        f.write(glb)
    print(f"  {name}.glb: {len(pos)} vertices, {len(idx) // 3} triangles" + (f", {len(joints)} joints, {len(animations)} clips" if joints else "") + f", {len(glb) // 1024} KB")


# ------------------------------------------------------------------------------------------------ animation helpers
def sm(p):
    return p * p * (3 - 2 * p)


def seg(p, a, b):
    """0 before a, 1 after b, smooth between."""
    return sm(min(1.0, max(0.0, (p - a) / (b - a)))) if b > a else (1.0 if p >= b else 0.0)


def keys(p, ks):
    """Piecewise smooth interpolation through (phase, value) pairs."""
    if p <= ks[0][0]:
        return ks[0][1]
    for (p0, v0), (p1, v1) in zip(ks, ks[1:]):
        if p <= p1:
            return v0 + (v1 - v0) * seg(p, p0, p1)
    return ks[-1][1]


def S(p, k=1):
    return math.sin(TAU * p * k)


def C(p, k=1):
    return math.cos(TAU * p * k)


# ------------------------------------------------------------------------------------------------ the hero
HERO_JOINTS = [
    ("root", -1, (0.0, 0.0, 0.0)),            # 0
    ("hips", 0, (0.0, 0.95, 0.0)),            # 1
    ("spine", 1, (0.0, 0.10, 0.0)),           # 2
    ("chest", 2, (0.0, 0.22, 0.0)),           # 3
    ("neck", 3, (0.0, 0.30, 0.0)),            # 4
    ("head", 4, (0.0, 0.08, 0.0)),            # 5
    ("upper_arm_L", 3, (-0.25, 0.20, 0.0)),   # 6
    ("lower_arm_L", 6, (0.0, -0.28, 0.0)),    # 7
    ("hand_L", 7, (0.0, -0.26, 0.0)),         # 8
    ("upper_arm_R", 3, (0.25, 0.20, 0.0)),    # 9
    ("lower_arm_R", 9, (0.0, -0.28, 0.0)),    # 10
    ("hand_R", 10, (0.0, -0.26, 0.0)),        # 11
    ("upper_leg_L", 1, (-0.11, -0.04, 0.0)),  # 12
    ("lower_leg_L", 12, (0.0, -0.44, 0.0)),   # 13
    ("foot_L", 13, (0.0, -0.43, 0.0)),        # 14
    ("upper_leg_R", 1, (0.11, -0.04, 0.0)),   # 15
    ("lower_leg_R", 15, (0.0, -0.44, 0.0)),   # 16
    ("foot_R", 16, (0.0, -0.43, 0.0)),        # 17
]


def world_of(joints):
    w = []
    for (_, parent, off) in joints:
        w.append(add(w[parent], off) if parent >= 0 else off)
    return {j[0]: w[i] for i, j in enumerate(joints)}


def hero_model():
    m = Model()
    W = world_of(HERO_JOINTS)
    J = {j[0]: i for i, j in enumerate(HERO_JOINTS)}
    skin, tunic, trousers, boots, hair, scarf, leather, steel, gold, pack = (0.88, 0.68, 0.54), (0.13, 0.46, 0.52), (0.30, 0.22, 0.16), (0.17, 0.11, 0.09), (0.12, 0.07, 0.05), (0.88, 0.32, 0.14), (0.38, 0.24, 0.12), (0.78, 0.8, 0.86), (0.85, 0.7, 0.22), (0.55, 0.4, 0.22)

    def at(joint, dx, dy, dz):
        w = W[joint]
        return (w[0] + dx, w[1] + dy, w[2] + dz)

    # hips and belt, the tunic's skirt
    m.box(at("hips", 0, 0.0, 0), (0.34, 0.20, 0.22), trousers, J["hips"])
    m.box(at("hips", 0, 0.12, 0), (0.36, 0.06, 0.24), leather, J["hips"])
    m.box(at("hips", 0, -0.10, 0), (0.38, 0.20, 0.26), tunic, J["hips"], taper=0.9)
    # torso
    m.box(at("spine", 0, 0.11, 0), (0.36, 0.28, 0.23), tunic, J["spine"], taper=1.1)
    m.box(at("chest", 0, 0.12, 0), (0.44, 0.34, 0.26), tunic, J["chest"], taper=1.05)
    m.box(at("chest", 0, 0.28, 0), (0.50, 0.08, 0.28), scarf, J["chest"])          # the scarf round the shoulders
    m.box(at("chest", 0.0, 0.14, 0.17), (0.34, 0.40, 0.10), pack, J["chest"])      # a pack on the back (+z)
    m.box(at("chest", 0.0, 0.0, 0.17), (0.38, 0.05, 0.11), leather, J["chest"])
    m.box(at("neck", 0, 0.03, 0.0), (0.12, 0.10, 0.12), skin, J["neck"])
    m.box(at("neck", 0, 0.00, 0.10), (0.16, 0.30, 0.06), scarf, J["neck"], rot=(-18, 0, 0))  # the scarf's tail
    # head
    m.box(at("head", 0, 0.12, 0), (0.21, 0.23, 0.22), skin, J["head"], taper=0.92)
    m.box(at("head", 0, 0.24, 0.02), (0.24, 0.09, 0.25), hair, J["head"])
    m.box(at("head", 0, 0.12, 0.12), (0.22, 0.22, 0.06), hair, J["head"])
    m.box(at("head", -0.055, 0.14, -0.115), (0.04, 0.04, 0.012), (0.08, 0.05, 0.04), J["head"])  # eyes
    m.box(at("head", 0.055, 0.14, -0.115), (0.04, 0.04, 0.012), (0.08, 0.05, 0.04), J["head"])
    # arms (the right one carries the sword)
    for s, side in ((-1, "L"), (1, "R")):
        ua, la, hd = f"upper_arm_{side}", f"lower_arm_{side}", f"hand_{side}"
        m.box(at(ua, 0, 0.04, 0), (0.15, 0.12, 0.15), tunic, J[ua])                      # shoulder pad
        m.box(at(ua, 0, -0.13, 0), (0.12, 0.30, 0.12), tunic, J[ua], taper=1.15)
        m.box(at(la, 0, -0.12, 0), (0.105, 0.26, 0.105), skin, J[la], taper=1.1)
        m.box(at(la, 0, -0.03, 0), (0.12, 0.09, 0.12), leather, J[la])                   # a bracer
        m.box(at(hd, 0, -0.05, 0), (0.09, 0.11, 0.09), skin, J[hd])
    # the sword: grip in the right fist, blade forward (-z), crossguard
    m.box(at("hand_R", 0, -0.04, -0.03), (0.04, 0.04, 0.17), leather, J["hand_R"])
    m.box(at("hand_R", 0, -0.04, -0.13), (0.045, 0.19, 0.035), gold, J["hand_R"])
    m.box(at("hand_R", 0, -0.04, -0.60), (0.016, 0.075, 0.95), steel, J["hand_R"])
    m.box(at("hand_R", 0, -0.04, -1.10), (0.016, 0.05, 0.06), steel, J["hand_R"], taper=0.3)
    # legs
    for s, side in ((-1, "L"), (1, "R")):
        ul, ll, ft = f"upper_leg_{side}", f"lower_leg_{side}", f"foot_{side}"
        m.box(at(ul, 0, -0.22, 0), (0.16, 0.46, 0.17), trousers, J[ul], taper=1.12)
        m.box(at(ll, 0, -0.22, 0), (0.13, 0.46, 0.14), trousers, J[ll], taper=1.2)
        m.box(at(ll, 0, -0.30, 0), (0.15, 0.24, 0.16), boots, J[ll])                      # the boot's shaft
        m.box(at(ft, 0, -0.01, -0.06), (0.15, 0.10, 0.29), boots, J[ft])
    return m


def hero_pose_fn(clip):
    def pose(p):
        R, T = {}, {}

        def r(j, x=0, y=0, z=0):
            a = R.get(j, (0, 0, 0))
            R[j] = (a[0] + x, a[1] + y, a[2] + z)

        # the sword hand rests forward in all clips unless the clip moves it
        if clip == "idle":
            s = S(p)
            r("chest", -1.2 * s)
            r("head", 1.5 * s)
            r("spine", 2)
            r("upper_arm_R", -26 + 1.5 * s, 0, 8)
            r("lower_arm_R", 50 + 2 * s)
            r("upper_arm_L", 4 - 1.5 * s, 0, -7)
            r("lower_arm_L", 12)
            r("upper_leg_L", 3)
            r("upper_leg_R", -3)
            r("lower_leg_L", -4)
            r("lower_leg_R", -4)
            T["hips"] = (0, 0.008 * C(p), 0)
        elif clip in ("walk", "run"):
            run = clip == "run"
            w = TAU * p
            amp = 52 if run else 28
            knee = 88 if run else 38
            arm = 44 if run else 22
            for side, ph in (("L", 0.0), ("R", math.pi)):
                sn = math.sin(w + ph)
                r(f"upper_leg_{side}", amp * sn - (6 if run else 2))
                r(f"lower_leg_{side}", -(8 + knee * max(0.0, math.sin(w + ph + 1.9))))
                r(f"foot_{side}", 8 * math.sin(w + ph + 0.6) + (6 if run else 2) )
            r("upper_arm_L", -arm * math.sin(w) + (4 if run else 2), 0, -8 if not run else -4)
            r("lower_arm_L", (95 if run else 28) + (10 if run else 6) * math.sin(w))
            r("upper_arm_R", arm * 0.4 * math.sin(w) + (-30 if run else -22), 0, 8)
            r("lower_arm_R", (70 if run else 52))
            r("spine", -(13 if run else 3), (7 if run else 3) * math.sin(w), 0)
            r("chest", 0, -(8 if run else 4) * math.sin(w), 2 * math.sin(2 * w))
            r("head", (10 if run else 2), (4 if run else 1.5) * math.sin(w), 0)
            T["hips"] = (0, (0.07 if run else 0.035) * math.cos(2 * w) - (0.04 if run else 0.0), 0)
            r("hips", 0, -(6 if run else 3) * math.sin(w), 0)
        elif clip == "attack":
            # windup (0 - 0.30): the sword goes up over the shoulder; slash (0.30 - 0.50): an overhead cut down to the front; recover (0.50 - 1)
            ua = keys(p, [(0, -26), (0.30, 165), (0.50, 70), (1.0, -26)])
            la = keys(p, [(0, 50), (0.30, 25), (0.50, 5), (1.0, 50)])
            r("hand_R", keys(p, [(0, 0), (0.30, -35), (0.50, -90), (1.0, 0)]))
            twist = keys(p, [(0, 0), (0.30, 24), (0.50, -34), (1.0, 0)])
            lunge = keys(p, [(0, 0), (0.30, 0), (0.45, 1), (0.75, 1), (1.0, 0)])
            r("upper_arm_R", ua, 0, 8)
            r("lower_arm_R", la)
            r("spine", keys(p, [(0, 0), (0.3, 4), (0.5, -10), (1, 0)]), twist * 0.5, 0)
            r("chest", 0, twist * 0.5, 0)
            r("head", 0, -twist * 0.4, 0)
            r("upper_arm_L", keys(p, [(0, 0), (0.3, 12), (0.5, 0), (1, 0)]), 0, keys(p, [(0, 0), (0.3, -22), (0.5, 0), (1, 0)]))
            r("lower_arm_L", keys(p, [(0, 0), (0.3, 40), (0.5, 10), (1, 0)]))
            r("upper_leg_R", 34 * lunge)
            r("lower_leg_R", -22 * lunge)
            r("upper_leg_L", -18 * lunge)
            r("lower_leg_L", -10 * lunge)
            T["hips"] = (0, -0.06 * lunge, -0.12 * lunge)
            r("hips", 0, keys(p, [(0, 0), (0.3, 8), (0.5, -6), (1, 0)]), 0)
        elif clip == "roll":
            tuck = seg(p, 0.0, 0.2) * (1 - seg(p, 0.8, 1.0))
            flip = seg(p, 0.08, 0.92)
            r("hips", -360 * flip)
            T["hips"] = (0, -0.42 * tuck, -0.05)
            r("spine", -28 * tuck)
            r("chest", -22 * tuck)
            r("head", -12 * tuck)
            r("upper_leg_L", 120 * tuck)
            r("upper_leg_R", 120 * tuck)
            r("lower_leg_L", -125 * tuck)
            r("lower_leg_R", -125 * tuck)
            r("upper_arm_L", -75 * tuck, 0, -12 * tuck)
            r("upper_arm_R", -75 * tuck, 0, 12 * tuck)
            r("lower_arm_L", 100 * tuck)
            r("lower_arm_R", 100 * tuck)
        elif clip == "swim":
            w = TAU * p
            r("hips", -68)
            T["hips"] = (0, -0.10, 0.0)
            r("spine", 12)
            r("head", 38 + 6 * math.sin(2 * w))
            for side, ph in (("L", 0.0), ("R", math.pi)):
                r(f"upper_arm_{side}", -125 + 70 * math.sin(w + ph + 1.57), 0, (-14 if side == "L" else 14))
                r(f"lower_arm_{side}", 30 + 25 * math.sin(w + ph))
                r(f"upper_leg_{side}", 16 * math.sin(2 * w + ph))
                r(f"lower_leg_{side}", -14 - 10 * math.sin(2 * w + ph))
        elif clip == "air":
            s = S(p)
            r("upper_leg_L", 34)
            r("lower_leg_L", -55)
            r("upper_leg_R", -12)
            r("lower_leg_R", -35)
            r("upper_arm_L", -50, 0, -34)
            r("upper_arm_R", -95, 0, 30)
            r("lower_arm_R", 30)
            r("spine", -6 + s)
            T["hips"] = (0, 0.02 * s, 0)
        elif clip == "die":
            f = seg(p, 0.0, 0.7)
            r("hips", 88 * f)
            T["hips"] = (0, -0.70 * f, 0.15 * f)
            r("head", -22 * f)
            r("upper_arm_L", -40 * f, 0, -50 * f)
            r("upper_arm_R", -20 * f, 0, 60 * f)
            r("lower_arm_R", 40 * f)
            r("upper_leg_L", -10 * f)
            r("upper_leg_R", 18 * f)
            r("lower_leg_R", -28 * f)
        return R, T
    return pose


def hero():
    clips = {"idle": (2.0, True, hero_pose_fn("idle")), "walk": (1.0, True, hero_pose_fn("walk")), "run": (0.66, True, hero_pose_fn("run")), "attack": (0.6, False, hero_pose_fn("attack")),
             "roll": (0.6, False, hero_pose_fn("roll")), "swim": (1.2, True, hero_pose_fn("swim")), "air": (0.8, True, hero_pose_fn("air")), "die": (1.2, False, hero_pose_fn("die"))}
    write_glb("hero", hero_model(), HERO_JOINTS, clips)


# ------------------------------------------------------------------------------------------------ the golem
GOLEM_JOINTS = [
    ("root", -1, (0.0, 0.0, 0.0)),            # 0
    ("hips", 0, (0.0, 1.25, 0.0)),            # 1
    ("torso", 1, (0.0, 0.30, 0.0)),           # 2
    ("head", 2, (0.0, 0.95, -0.05)),          # 3
    ("upper_arm_L", 2, (-0.82, 0.70, 0.0)),   # 4
    ("lower_arm_L", 4, (0.0, -0.72, 0.0)),    # 5
    ("upper_arm_R", 2, (0.82, 0.70, 0.0)),    # 6
    ("lower_arm_R", 6, (0.0, -0.72, 0.0)),    # 7
    ("upper_leg_L", 1, (-0.36, -0.15, 0.0)),  # 8
    ("lower_leg_L", 8, (0.0, -0.55, 0.0)),    # 9
    ("upper_leg_R", 1, (0.36, -0.15, 0.0)),   # 10
    ("lower_leg_R", 10, (0.0, -0.55, 0.0)),   # 11
]


def golem_model():
    m = Model()
    W = world_of(GOLEM_JOINTS)
    J = {j[0]: i for i, j in enumerate(GOLEM_JOINTS)}
    stone, dark, moss, ember, light = (0.42, 0.43, 0.45), (0.27, 0.28, 0.31), (0.30, 0.42, 0.22), (1.0, 0.55, 0.12), (0.55, 0.55, 0.56)

    def at(j, dx, dy, dz):
        w = W[j]
        return (w[0] + dx, w[1] + dy, w[2] + dz)

    m.box(at("hips", 0, 0.0, 0), (0.95, 0.55, 0.62), dark, J["hips"], taper=1.1)
    m.box(at("torso", 0, 0.42, 0), (1.35, 1.05, 0.85), stone, J["torso"], taper=1.18)
    m.box(at("torso", 0, 0.98, 0.0), (1.55, 0.34, 0.95), light, J["torso"])                  # shoulders' yoke
    m.blob(at("torso", 0, 0.3, 0.45), 0.42, moss, J["torso"], seed=3, jitter=0.25, squash=(1.2, 0.5, 0.5))   # moss on the back
    m.box(at("torso", 0, 0.45, -0.46), (0.5, 0.42, 0.1), (0.8, 0.38, 0.1), J["torso"])       # a glowing crack on the chest
    m.box(at("head", 0, 0.2, 0), (0.62, 0.52, 0.58), stone, J["head"], taper=0.9)
    m.box(at("head", -0.17, 0.26, -0.3), (0.16, 0.09, 0.05), ember, J["head"])               # eyes
    m.box(at("head", 0.17, 0.26, -0.3), (0.16, 0.09, 0.05), ember, J["head"])
    m.box(at("head", 0, 0.54, 0.04), (0.5, 0.16, 0.5), dark, J["head"])
    for s, side in ((-1, "L"), (1, "R")):
        ua, la = f"upper_arm_{side}", f"lower_arm_{side}"
        m.box(at(ua, 0, 0.1, 0), (0.58, 0.46, 0.58), light, J[ua])                              # shoulder boulder
        m.blob(at(ua, s * 0.1, 0.34, 0.0), 0.3, moss, J[ua], seed=7 + s, jitter=0.2, squash=(1.1, 0.5, 1.0))
        m.box(at(ua, 0, -0.36, 0), (0.46, 0.75, 0.46), stone, J[ua], taper=1.2)
        m.box(at(la, 0, -0.36, 0), (0.54, 0.8, 0.54), dark, J[la], taper=1.25)
        m.box(at(la, 0, -0.84, 0), (0.7, 0.45, 0.7), stone, J[la])                              # the fist
    for s, side in ((-1, "L"), (1, "R")):
        ul, ll = f"upper_leg_{side}", f"lower_leg_{side}"
        m.box(at(ul, 0, -0.3, 0), (0.55, 0.7, 0.55), stone, J[ul], taper=1.15)
        m.box(at(ll, 0, -0.35, 0), (0.5, 0.75, 0.5), dark, J[ll], taper=1.25)
        m.box(at(ll, 0, -0.78, -0.06), (0.68, 0.34, 0.9), stone, J[ll])                         # the foot
    return m


def golem_pose_fn(clip):
    def pose(p):
        R, T = {}, {}

        def r(j, x=0, y=0, z=0):
            a = R.get(j, (0, 0, 0))
            R[j] = (a[0] + x, a[1] + y, a[2] + z)

        sleep = clip in ("sleep",)
        if clip in ("sleep", "wake"):
            k = 1.0 if clip == "sleep" else 1.0 - seg(p, 0.05, 0.9)
            s = S(p) if clip == "sleep" else 0.0
            T["hips"] = (0, -0.62 * k, 0.1 * k)
            r("torso", 38 * k + 1.2 * s * k, 0, 0)
            r("head", 28 * k)
            r("upper_arm_L", -40 * k, 0, 18 * k)
            r("upper_arm_R", -40 * k, 0, -18 * k)
            r("lower_arm_L", 70 * k)
            r("lower_arm_R", 70 * k)
            r("upper_leg_L", 88 * k)
            r("upper_leg_R", 88 * k)
            r("lower_leg_L", -100 * k)
            r("lower_leg_R", -100 * k)
        elif clip == "idle":
            s = S(p)
            r("torso", -1.5 * s)
            r("head", 2 * s)
            r("upper_arm_L", 2 * s, 0, -6)
            r("upper_arm_R", -2 * s, 0, 6)
            r("lower_arm_L", 10)
            r("lower_arm_R", 10)
            T["hips"] = (0, 0.012 * C(p), 0)
        elif clip == "walk":
            w = TAU * p
            for side, ph in (("L", 0.0), ("R", math.pi)):
                sn = math.sin(w + ph)
                r(f"upper_leg_{side}", 24 * sn)
                r(f"lower_leg_{side}", -(4 + 26 * max(0.0, math.sin(w + ph + 1.9))))
            r("upper_arm_L", -20 * math.sin(w), 0, -8)
            r("upper_arm_R", 20 * math.sin(w), 0, 8)
            r("lower_arm_L", 16)
            r("lower_arm_R", 16)
            r("torso", 4, 6 * math.sin(w), 0)
            r("head", -3, -3 * math.sin(w), 0)
            T["hips"] = (0, 0.06 * math.cos(2 * w), 0)
            r("hips", 0, -5 * math.sin(w), 3 * math.sin(w))
        elif clip == "swing":
            up = seg(p, 0.0, 0.35)
            cut = seg(p, 0.35, 0.55)
            back = seg(p, 0.55, 1.0)
            r("upper_arm_R", (-35 * up + 125 * cut) * (1 - back) + 0, 0, 32 * up * (1 - cut))
            r("lower_arm_R", 20 * up * (1 - back))
            r("torso", 0, (20 * up - 55 * cut) * (1 - back), 0)
            r("head", 0, (-10 * up + 25 * cut) * (1 - back), 0)
            r("upper_arm_L", -30 * up * (1 - back))
            T["hips"] = (0, -0.1 * up * (1 - cut), 0)
        elif clip == "slam":
            up = seg(p, 0.0, 0.5)
            hit = seg(p, 0.5, 0.64)
            back = seg(p, 0.7, 1.0)
            a = (-170 * up + 195 * hit) * (1 - back)
            r("upper_arm_L", a, 0, -8)
            r("upper_arm_R", a, 0, 8)
            r("lower_arm_L", 18 * up * (1 - back))
            r("lower_arm_R", 18 * up * (1 - back))
            r("torso", (-18 * up + 52 * hit) * (1 - back))
            r("head", (-10 * up + 20 * hit) * (1 - back))
            T["hips"] = (0, (0.1 * up - 0.4 * hit) * (1 - back), -0.2 * hit * (1 - back))
            r("upper_leg_L", 24 * hit * (1 - back))
            r("upper_leg_R", 24 * hit * (1 - back))
            r("lower_leg_L", -40 * hit * (1 - back))
            r("lower_leg_R", -40 * hit * (1 - back))
        elif clip == "stagger":
            k = math.sin(math.pi * seg(p, 0.0, 1.0))
            r("torso", -24 * k)
            r("head", -16 * k)
            r("upper_arm_L", -30 * k, 0, -30 * k)
            r("upper_arm_R", -30 * k, 0, 30 * k)
            T["hips"] = (0, -0.1 * k, 0.15 * k)
            r("upper_leg_L", -10 * k)
            r("upper_leg_R", 14 * k)
        elif clip == "die":
            f = seg(p, 0.0, 0.8)
            r("torso", 70 * f)
            r("head", 30 * f)
            T["hips"] = (0, -0.85 * f, 0.4 * f)
            r("upper_arm_L", -20 * f, 0, -50 * f)
            r("upper_arm_R", -20 * f, 0, 50 * f)
            r("upper_leg_L", 70 * f)
            r("upper_leg_R", 60 * f)
            r("lower_leg_L", -60 * f)
            r("lower_leg_R", -80 * f)
        return R, T
    return pose


def golem():
    clips = {"sleep": (3.0, True, golem_pose_fn("sleep")), "wake": (1.4, False, golem_pose_fn("wake")), "idle": (2.4, True, golem_pose_fn("idle")), "walk": (1.3, True, golem_pose_fn("walk")),
             "swing": (1.0, False, golem_pose_fn("swing")), "slam": (1.5, False, golem_pose_fn("slam")), "stagger": (0.7, False, golem_pose_fn("stagger")), "die": (1.2, False, golem_pose_fn("die"))}
    write_glb("golem", golem_model(), GOLEM_JOINTS, clips)


# ------------------------------------------------------------------------------------------------ the wraith, a crab and a gull
WRAITH_JOINTS = [("root", -1, (0, 0, 0)), ("body", 0, (0, 1.0, 0)), ("head", 1, (0, 0.75, 0)), ("arm_L", 1, (-0.32, 0.55, 0)), ("arm_R", 1, (0.32, 0.55, 0))]


def wraith():
    m = Model()
    W = world_of(WRAITH_JOINTS)
    cloak, hood, eye = (0.22, 0.26, 0.42), (0.14, 0.16, 0.28), (0.6, 0.95, 1.0)
    m.prism((0, 0.85, 0), 0.62, 0.22, 1.45, cloak, 1, sides=7, squash=0.8)
    m.prism((0, 0.28, 0), 0.7, 0.55, 0.3, (0.18, 0.2, 0.36), 1, sides=7, squash=0.8)
    for k in range(5):  # ragged tatters at the hem
        a = TAU * k / 5
        m.prism((math.cos(a) * 0.5, 0.12, math.sin(a) * 0.42), 0.16, 0.0, 0.5, cloak, 1, sides=4)
    h = W["head"]
    m.blob((h[0], h[1] + 0.12, h[2]), 0.26, hood, 2, seed=5, jitter=0.08, squash=(0.9, 1.1, 1.0))
    m.box((h[0] - 0.08, h[1] + 0.1, h[2] - 0.22), (0.07, 0.05, 0.03), eye, 2)
    m.box((h[0] + 0.08, h[1] + 0.1, h[2] - 0.22), (0.07, 0.05, 0.03), eye, 2)
    for side, joint in ((-1, 3), (1, 4)):
        a = W["arm_L" if side < 0 else "arm_R"]
        m.prism((a[0] + side * 0.05, a[1] - 0.25, a[2] - 0.1), 0.1, 0.03, 0.6, cloak, joint, sides=5, rot=(-25, 0, side * 10))

    def pose(clip):
        def f(p):
            R, T = {}, {}
            if clip == "float":
                T["body"] = (0, 0.12 * S(p), 0)
                R["body"] = (4 * S(p, 1), 6 * S(p, 2), 3 * C(p))
                R["arm_L"] = (-20 + 8 * S(p), 0, -10 + 6 * C(p))
                R["arm_R"] = (-20 - 8 * S(p), 0, 10 - 6 * C(p))
                R["head"] = (3 * S(p, 2), 0, 0)
            else:
                k = math.sin(math.pi * p)
                R["body"] = (-30 * k, 0, 0)
                T["body"] = (0, 0, -0.3 * k)
                R["arm_L"] = (-100 * k, 0, -20)
                R["arm_R"] = (-100 * k, 0, 20)
            return R, T
        return f
    write_glb("wraith", m, WRAITH_JOINTS, {"float": (2.0, True, pose("float")), "lunge": (0.6, False, pose("lunge"))})


CRAB_JOINTS = [("root", -1, (0, 0, 0)), ("body", 0, (0, 0.14, 0)), ("claw_L", 1, (-0.22, 0.02, -0.16)), ("claw_R", 1, (0.22, 0.02, -0.16)), ("legs_L", 1, (-0.18, -0.04, 0.0)), ("legs_R", 1, (0.18, -0.04, 0.0))]


def crab():
    m = Model()
    W = world_of(CRAB_JOINTS)
    shell, claw = (0.85, 0.32, 0.2), (0.95, 0.4, 0.25)
    m.box((0, 0.15, 0), (0.34, 0.12, 0.26), shell, 1, taper=0.85)
    m.blob((0, 0.19, 0), 0.15, shell, 1, seed=9, jitter=0.1, squash=(1.3, 0.55, 1.0), detail=0)
    for side, jc, jl in ((-1, 2, 4), (1, 3, 5)):
        c = W["claw_L" if side < 0 else "claw_R"]
        m.box((c[0] + side * 0.04, c[1], c[2] - 0.1), (0.1, 0.07, 0.2), claw, jc)
        m.box((c[0] + side * 0.06, c[1] + 0.02, c[2] - 0.22), (0.14, 0.07, 0.1), claw, jc, taper=0.5)
        for k in range(3):
            lz = -0.08 + 0.09 * k
            m.box((side * 0.27, 0.07, lz), (0.2, 0.025, 0.025), (0.7, 0.28, 0.18), jl, rot=(0, 0, side * -20))
    m.box((-0.07, 0.24, -0.12), (0.025, 0.06, 0.025), (0.05, 0.05, 0.05), 1)
    m.box((0.07, 0.24, -0.12), (0.025, 0.06, 0.025), (0.05, 0.05, 0.05), 1)

    def scuttle(p):
        R, T = {}, {}
        w = TAU * p
        R["legs_L"] = (0, 0, 14 * math.sin(4 * w))
        R["legs_R"] = (0, 0, -14 * math.sin(4 * w))
        R["claw_L"] = (0, 24 * math.sin(2 * w), 0)
        R["claw_R"] = (0, -24 * math.sin(2 * w), 0)
        T["body"] = (0, 0.012 * math.sin(4 * w), 0)
        return R, T
    write_glb("crab", m, CRAB_JOINTS, {"scuttle": (0.5, True, scuttle)})


GULL_JOINTS = [("root", -1, (0, 0, 0)), ("body", 0, (0, 0, 0)), ("wing_L", 1, (-0.1, 0.04, 0)), ("wing_R", 1, (0.1, 0.04, 0))]


def gull():
    m = Model()
    white, grey, beak = (0.95, 0.95, 0.96), (0.62, 0.66, 0.72), (0.95, 0.7, 0.2)
    m.blob((0, 0, 0), 0.17, white, 1, seed=2, jitter=0.05, squash=(0.8, 0.7, 1.7), detail=1)
    m.blob((0, 0.06, -0.26), 0.09, white, 1, seed=3, jitter=0.05, detail=0)
    m.box((0, 0.05, -0.38), (0.03, 0.03, 0.12), beak, 1, taper=0.4)
    m.box((0, 0.02, 0.3), (0.12, 0.025, 0.2), grey, 1, taper=0.8)
    for side, j in ((-1, 2), (1, 3)):
        m.box((side * 0.45, 0.04, 0.0), (0.7, 0.02, 0.3), white, j, taper=0.5)
        m.box((side * 0.85, 0.04, 0.02), (0.14, 0.018, 0.2), grey, j, taper=0.5)

    def flap(p):
        R, T = {}, {}
        a = 38 * math.sin(TAU * p)
        R["wing_L"] = (0, 0, -a)
        R["wing_R"] = (0, 0, a)
        T["body"] = (0, 0.03 * math.cos(TAU * p), 0)
        return R, T
    write_glb("gull", m, GULL_JOINTS, {"flap": (0.7, True, flap)})


# ------------------------------------------------------------------------------------------------ scenery
def pine(seed=1):
    m = Model()
    bark = (0.30, 0.19, 0.11)
    m.prism((0, 1.3, 0), 0.30, 0.17, 2.6, bark, sides=7)
    greens = [(0.10, 0.30, 0.15), (0.12, 0.34, 0.17), (0.14, 0.38, 0.19), (0.16, 0.42, 0.20)]
    for k, (r, y, h) in enumerate([(2.1, 1.7, 2.3), (1.75, 2.9, 2.2), (1.35, 4.0, 2.0), (0.9, 5.0, 1.9)]):
        m.prism((0, y + h / 2, 0), r, 0.0, h, greens[k], sides=8, rot=(0, 20 * k, 0))
    write_glb("pine", m)


def broadleaf():
    m = Model()
    bark = (0.36, 0.25, 0.15)
    m.prism((0, 1.7, 0), 0.34, 0.22, 3.4, bark, sides=7)
    m.prism((0.5, 3.2, 0.1), 0.12, 0.07, 1.3, bark, sides=5, rot=(0, 0, -50))
    m.prism((-0.5, 3.3, -0.1), 0.12, 0.07, 1.2, bark, sides=5, rot=(0, 0, 50))
    for k, (x, y, z, r) in enumerate([(0.0, 4.6, 0.0, 2.1), (1.2, 4.0, 0.5, 1.5), (-1.3, 4.2, -0.4, 1.6), (0.3, 5.5, -0.6, 1.4), (-0.4, 3.9, 1.2, 1.3)]):
        m.blob((x, y, z), r, (0.20 + 0.03 * (k % 3), 0.42 + 0.04 * (k % 2), 0.16), 0, seed=10 + k, jitter=0.16, squash=(1.0, 0.85, 1.0))
    write_glb("broadleaf", m)


def palm():
    m = Model()
    bark = (0.55, 0.42, 0.25)
    x = 0.0
    for k in range(6):
        y = 0.55 + k * 1.0
        m.prism((x, y, 0), 0.20 - 0.015 * k, 0.19 - 0.015 * k, 1.1, bark if k % 2 else (0.5, 0.38, 0.22), sides=6, rot=(0, 0, -6))
        x += 0.1
    top = (x, 6.4, 0)
    for k in range(9):
        a = 40 * k
        ang = math.radians(a)
        droop = 28 + 14 * (k % 2)
        ln = 2.0 - 0.25 * (k % 2)
        d = (math.cos(ang), -math.sin(ang))  # yaw -a about y turns +x into (cos a, 0, -sin a)... the frond lies along +x after the tilt
        centre = (top[0] + d[0] * ln * 0.45, top[1] + 0.12 - math.sin(math.radians(droop)) * ln * 0.45, top[2] + d[1] * ln * 0.45)
        m.box(centre, (ln, 0.03, 0.5), (0.20 + 0.02 * (k % 3), 0.5 - 0.03 * (k % 2), 0.17), rot=(0, -a, -droop), taper=0.55, order="zyx")
    for k in range(3):
        a = TAU * k / 3 + 0.4
        m.blob((top[0] + math.cos(a) * 0.15, top[1] - 0.15, top[2] + math.sin(a) * 0.15), 0.13, (0.35, 0.22, 0.1), 0, seed=40 + k, jitter=0.1, detail=0)
    write_glb("palm", m)


def deadtree():
    m = Model()
    bark = (0.20, 0.17, 0.15)
    m.prism((0, 1.9, 0), 0.26, 0.10, 3.8, bark, sides=6, rot=(0, 0, 3))
    for (y, a, ln, tilt) in ((2.4, 0, 1.4, 52), (3.0, 140, 1.2, -48), (3.5, 260, 1.0, 40), (1.6, 70, 0.9, -55)):
        d = math.radians(a)
        m.prism((math.cos(d) * ln * 0.3, y + ln * 0.2, -math.sin(d) * ln * 0.3), 0.08, 0.02, ln, bark, sides=4, rot=(0, -a, tilt), order="zyx")
    write_glb("deadtree", m)


def rock(n):
    m = Model()
    grey = [(0.42, 0.41, 0.40), (0.36, 0.36, 0.38), (0.46, 0.43, 0.38)][n - 1]
    m.blob((0, 0.55, 0), 1.0, grey, 0, seed=100 + n * 7, jitter=0.3, squash=(1.0 + 0.1 * n, 0.62, 0.9 + 0.1 * n))
    if n == 2:
        m.blob((0.9, 0.3, 0.4), 0.55, (0.38, 0.38, 0.4), 0, seed=222, jitter=0.25, squash=(1, 0.7, 1))
    if n == 3:
        m.blob((-0.8, 0.3, -0.5), 0.5, (0.5, 0.46, 0.4), 0, seed=333, jitter=0.25, squash=(1, 0.7, 1))
    write_glb(f"rock{n}", m)


def tent():
    m = Model()
    cloth, dark, rope = (0.80, 0.62, 0.36), (0.10, 0.08, 0.06), (0.4, 0.28, 0.16)
    hw, hl, h = 1.6, 1.9, 2.3
    A, B, C = (-hw, 0, -hl), (hw, 0, -hl), (0, h, -hl)
    D, E, F = (-hw, 0, hl), (hw, 0, hl), (0, h, hl)
    m._quad(A, D, F, C, cloth, 0)
    m._quad(B, C, F, E, (0.74, 0.57, 0.33), 0)
    m._tri(A, C, B, cloth, 0)
    m._tri(D, E, F, cloth, 0)
    d = hl + 0.02
    m._tri((-0.55, 0, -d), (0, 1.55, -d), (0.55, 0, -d), dark, 0)
    m.box((0, h + 0.05, 0), (0.12, 0.12, 2 * hl + 0.3), rope, 0)
    m.box((-hw - 0.15, 0.05, -hl - 0.4), (0.1, 0.1, 0.1), rope, 0)
    write_glb("tent", m)


def beacon():
    m = Model()
    stone, dark, glow = (0.52, 0.5, 0.46), (0.30, 0.28, 0.26), (0.9, 0.5, 0.15)
    m.box((0, 0.35, 0), (5.0, 0.7, 5.0), dark)
    m.box((0, 1.0, 0), (3.8, 0.6, 3.8), stone)
    m.prism((0, 4.6, 0), 1.45, 0.95, 7.0, stone, sides=4, rot=(0, 45, 0))
    for k in range(4):  # runes that glow once the fire is lit (the colour is only a hint: they are lit by the beacon's light)
        a = k * math.pi / 2
        m.box((math.cos(a) * 1.22, 3.2, math.sin(a) * 1.22), (0.1, 0.9, 0.1) if k % 2 == 0 else (0.1, 0.9, 0.1), glow, 0, rot=(0, -math.degrees(a), 0))
    m.box((0, 8.3, 0), (3.0, 0.5, 3.0), dark)
    for k in range(8):
        a = k * math.pi / 4
        m.box((math.cos(a) * 1.35, 8.95, math.sin(a) * 1.35), (0.5, 0.8, 0.5), stone, 0, rot=(0, -math.degrees(a), 0), taper=1.2)
    write_glb("beacon", m)


def altar():
    m = Model()
    stone, dark = (0.55, 0.53, 0.5), (0.34, 0.32, 0.3)
    m.box((0, 0.15, 0), (3.2, 0.3, 3.2), dark)
    m.box((0, 0.75, 0), (2.2, 0.9, 2.2), stone, taper=0.92)
    m.box((0, 1.33, 0), (2.7, 0.26, 2.7), stone)
    for k in range(6):
        a = k * math.pi / 3
        m.box((math.cos(a) * 0.95, 1.5, math.sin(a) * 0.95), (0.34, 0.1, 0.34), dark, 0, rot=(0, -math.degrees(a), 0))
    write_glb("altar", m)


def shard():
    m = Model()
    m.bipyramid((0, 0.55, 0), 0.20, 0.55, 0.35, (1.0, 0.62, 0.2), sides=6)
    m.bipyramid((0.3, 0.3, 0.05), 0.09, 0.22, 0.12, (1.0, 0.5, 0.15), sides=5, rot=(0, 0, -25))
    m.bipyramid((-0.26, 0.28, -0.08), 0.08, 0.2, 0.1, (1.0, 0.78, 0.35), sides=5, rot=(0, 0, 28))
    write_glb("shard", m)


def main():
    print("models ->", os.path.relpath(OUT))
    hero()
    golem()
    wraith()
    crab()
    gull()
    pine()
    broadleaf()
    palm()
    deadtree()
    for n in (1, 2, 3):
        rock(n)
    shard()
    tent()
    beacon()
    altar()


if __name__ == "__main__":
    main()
