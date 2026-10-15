"""Writes two small glTF test models to examples/models/ with nothing but the standard library:

  crate.glb   a textured cube (embedded PNG checker) with a green pyramid on top (second material, child node)
  tower.gltf  stacked boxes with an external tower.bin and a base colour per box (no texture)

    python -I tools/make_models.py
"""
import json
import math
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "models")
os.makedirs(OUT, exist_ok=True)


def png(width, height, pixel):
    raw = b"".join(b"\x00" + b"".join(bytes(pixel(x, y)) for x in range(width)) for y in range(height))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def box(sx, sy, sz, cx=0.0, cy=0.0, cz=0.0):
    """24 vertices (positions, normals, uvs) and 36 indices of an axis-aligned box."""
    faces = [  # normal, u axis, v axis
        ((1, 0, 0), (0, 0, -1), (0, 1, 0)), ((-1, 0, 0), (0, 0, 1), (0, 1, 0)),
        ((0, 1, 0), (1, 0, 0), (0, 0, -1)), ((0, -1, 0), (1, 0, 0), (0, 0, 1)),
        ((0, 0, 1), (1, 0, 0), (0, 1, 0)), ((0, 0, -1), (-1, 0, 0), (0, 1, 0)),
    ]
    h = (sx / 2, sy / 2, sz / 2)
    pos, nrm, uv, idx = [], [], [], []
    for n, u, v in faces:
        base = len(pos)
        for a, b, t in ((-1, -1, (0, 0)), (1, -1, (1, 0)), (1, 1, (1, 1)), (-1, 1, (0, 1))):
            pos.append(tuple(cx_ + n[i] * h[i] + u[i] * a * h[i] + v[i] * b * h[i] for i, cx_ in enumerate((cx, cy, cz))))
            nrm.append(n)
            uv.append((t[0], 1.0 - t[1]))  # glTF v runs top to bottom
        idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    return pos, nrm, uv, idx


def pyramid(size, height, cx, cy, cz):
    h = size / 2
    apex = (cx, cy + height, cz)
    corners = [(cx - h, cy, cz - h), (cx + h, cy, cz - h), (cx + h, cy, cz + h), (cx - h, cy, cz + h)]
    pos, nrm, uv, idx = [], [], [], []
    for i in range(4):
        a, b = corners[i], corners[(i + 1) % 4]
        # counter-clockwise seen from outside: a, apex, b ... verify with the normal pointing away from the centre
        tri = (a, b, apex)
        e1 = tuple(tri[1][k] - tri[0][k] for k in range(3))
        e2 = tuple(tri[2][k] - tri[0][k] for k in range(3))
        n = (e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0])
        length = math.sqrt(sum(c * c for c in n)) or 1.0
        n = tuple(c / length for c in n)
        mid = tuple((tri[0][k] + tri[1][k] + tri[2][k]) / 3 - (cx, cy + height / 3, cz)[k] for k in range(3))
        if sum(n[k] * mid[k] for k in range(3)) < 0:  # wrong way round: swap
            tri = (tri[1], tri[0], tri[2])
            n = tuple(-c for c in n)
        base = len(pos)
        pos += list(tri)
        nrm += [n, n, n]
        uv += [(0, 0), (1, 0), (0.5, 1)]
        idx += [base, base + 1, base + 2]
    return pos, nrm, uv, idx


def pack(pos, nrm, uv, idx, blob):
    """Append one primitive's data to `blob` (a bytearray); returns (accessors, views) JSON pieces with offsets filled in."""
    def add(data, fmt):
        while len(blob) % 4:
            blob.append(0)
        off = len(blob)
        blob.extend(b"".join(struct.pack("<" + fmt, *d) if isinstance(d, tuple) else struct.pack("<" + fmt, d) for d in data))
        return off, len(blob) - off

    p, n, t, i = add(pos, "fff"), add(nrm, "fff"), add(uv, "ff"), add(idx, "H")
    lo = [min(c[k] for c in pos) for k in range(3)]
    hi = [max(c[k] for c in pos) for k in range(3)]
    return [(p, "VEC3", 5126, len(pos), lo, hi), (n, "VEC3", 5126, len(nrm), None, None), (t, "VEC2", 5126, len(uv), None, None), (i, "SCALAR", 5123, len(idx), None, None)]


def build(primitives, nodes, materials, images=None, textures=None, bin_uri=None):
    blob = bytearray()
    views, accessors, prims = [], [], []
    for pos, nrm, uv, idx, mat in primitives:
        acc_ids = []
        for (off, length), typ, comp, count, lo, hi in pack(pos, nrm, uv, idx, blob):
            views.append({"buffer": 0, "byteOffset": off, "byteLength": length})
            acc = {"bufferView": len(views) - 1, "componentType": comp, "count": count, "type": typ}
            if lo:
                acc["min"], acc["max"] = lo, hi
            accessors.append(acc)
            acc_ids.append(len(accessors) - 1)
        prims.append({"attributes": {"POSITION": acc_ids[0], "NORMAL": acc_ids[1], "TEXCOORD_0": acc_ids[2]}, "indices": acc_ids[3], "material": mat})
    return blob, views, accessors, prims


# ---- crate.glb: textured cube + green pyramid on top
tex = png(64, 64, lambda x, y: (200, 150, 90) if ((x // 8) + (y // 8)) % 2 == 0 else (120, 80, 40))
blob, views, accessors, prims = build(
    [(*box(1, 1, 1), 0), (*pyramid(0.8, 0.7, 0, 0.5, 0), 1)],
    None, None,
)
while len(blob) % 4:
    blob.append(0)
tex_off = len(blob)
blob.extend(tex)
views.append({"buffer": 0, "byteOffset": tex_off, "byteLength": len(tex)})
doc = {
    "asset": {"version": "2.0", "generator": "lunaris tools/make_models.py"},
    "scene": 0, "scenes": [{"nodes": [0]}],
    "nodes": [{"name": "crate", "mesh": 0, "children": [1]}, {"name": "gem", "mesh": 1, "translation": [0, 0.0, 0]}],
    "meshes": [{"name": "crate", "primitives": [prims[0]]}, {"name": "gem", "primitives": [prims[1]]}],
    "materials": [{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "baseColorFactor": [1, 1, 1, 1]}}, {"pbrMetallicRoughness": {"baseColorFactor": [0.1, 0.8, 0.35, 1]}}],
    "textures": [{"source": 0}], "images": [{"bufferView": len(views) - 1, "mimeType": "image/png"}],
    "buffers": [{"byteLength": len(blob)}], "bufferViews": views, "accessors": accessors,
}
js = json.dumps(doc, separators=(",", ":")).encode()
js += b" " * (-len(js) % 4)
glb = b"glTF" + struct.pack("<II", 2, 12 + 8 + len(js) + 8 + len(blob)) + struct.pack("<I", len(js)) + b"JSON" + js + struct.pack("<I", len(blob)) + b"BIN\x00" + bytes(blob)
open(os.path.join(OUT, "crate.glb"), "wb").write(glb)

# ---- tower.gltf + tower.bin: stacked boxes, one colour each, external buffer
colors = [[0.8, 0.2, 0.2, 1], [0.9, 0.7, 0.2, 1], [0.2, 0.6, 0.9, 1]]
prim_in = []
for k in range(3):
    s = 1.2 - 0.3 * k
    prim_in.append((*box(s, 0.5, s, 0, 0.25 + 0.5 * k, 0), k))
blob, views, accessors, prims = build(prim_in, None, None)
doc = {
    "asset": {"version": "2.0"},
    "scene": 0, "scenes": [{"nodes": [0]}],
    "nodes": [{"name": "tower", "mesh": 0}],
    "meshes": [{"primitives": prims}],
    "materials": [{"pbrMetallicRoughness": {"baseColorFactor": c}} for c in colors],
    "buffers": [{"byteLength": len(blob), "uri": "tower.bin"}], "bufferViews": views, "accessors": accessors,
}
open(os.path.join(OUT, "tower.bin"), "wb").write(bytes(blob))
open(os.path.join(OUT, "tower.gltf"), "w").write(json.dumps(doc, indent=1))
print("wrote", sorted(os.listdir(OUT)))


# ---- swayer.glb: a tapering four-sided column on a skeleton of four joints, with two animation clips ("sway" and "nod")
def swayer():
    blob = bytearray()
    views, accessors = [], []

    def add(data, comp, typ, count, norm=False, bounds=None):
        while len(blob) % 4:
            blob.append(0)
        off = len(blob)
        blob.extend(data)
        views.append({"buffer": 0, "byteOffset": off, "byteLength": len(data)})
        acc = {"bufferView": len(views) - 1, "componentType": comp, "count": count, "type": typ}
        if norm:
            acc["normalized"] = True
        if bounds:
            acc["min"], acc["max"] = bounds
        accessors.append(acc)
        return len(accessors) - 1

    rings, height = 16, 3.2
    joint_y = [0.0, 0.8, 1.6, 2.4]
    pos, nrm, jnt, wgt, idx = [], [], [], [], []
    for f in range(4):
        a0, a1 = math.pi / 2 * f, math.pi / 2 * (f + 1)
        mid = (a0 + a1) / 2
        normal = (math.cos(mid), 0.0, math.sin(mid))
        for k in range(rings + 1):
            y = height * k / rings
            r = 0.30 - 0.18 * (y / height)  # radius of the (square) cross-section's corner circle
            seg = min(int(y / 0.8), 2)
            t = (y - joint_y[seg]) / 0.8
            for a in (a0, a1):
                pos.append((r * math.cos(a), y, r * math.sin(a)))
                nrm.append(normal)
                jnt.append((seg, seg + 1, 0, 0))
                wgt.append((1.0 - t, t, 0.0, 0.0))
        base = f * (rings + 1) * 2
        for k in range(rings):
            i = base + k * 2
            # counter-clockwise seen from outside: (i, i+2, i+3) and (i, i+3, i+1) for this winding of a0 -> a1 going up
            idx += [i, i + 3, i + 2, i, i + 1, i + 3]
    # the top and bottom caps
    for cap_y, ny, order in ((0.0, -1.0, (0, 1, 2)), (height, 1.0, (0, 2, 1))):
        r = 0.30 - 0.18 * (cap_y / height)
        base = len(pos)
        seg = 0 if cap_y == 0 else 2
        jj, ww = ((0, 1, 0, 0), (1.0, 0.0, 0.0, 0.0)) if cap_y == 0 else ((2, 3, 0, 0), (0.0, 1.0, 0.0, 0.0))
        pos.append((0.0, cap_y, 0.0))
        for a in (0, math.pi / 2, math.pi, 3 * math.pi / 2):
            pos.append((r * math.cos(a), cap_y, r * math.sin(a)))
        for _ in range(5):
            nrm.append((0.0, ny, 0.0))
            jnt.append(jj)
            wgt.append(ww)
        for c in range(4):
            tri = (base, base + 1 + c, base + 1 + (c + 1) % 4)
            idx += [tri[order[0]], tri[order[1]], tri[order[2]]]
    lo = [min(p[k] for p in pos) for k in range(3)]
    hi = [max(p[k] for p in pos) for k in range(3)]
    a_pos = add(b"".join(struct.pack("<fff", *p) for p in pos), 5126, "VEC3", len(pos), bounds=(lo, hi))
    a_nrm = add(b"".join(struct.pack("<fff", *n) for n in nrm), 5126, "VEC3", len(nrm))
    a_jnt = add(b"".join(struct.pack("<BBBB", *j) for j in jnt), 5121, "VEC4", len(jnt))
    a_wgt = add(b"".join(struct.pack("<ffff", *w) for w in wgt), 5126, "VEC4", len(wgt))
    a_idx = add(b"".join(struct.pack("<H", i) for i in idx), 5123, "SCALAR", len(idx))
    ibm = b"".join(struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, -y, 0, 1) for y in joint_y)
    a_ibm = add(ibm, 5126, "MAT4", len(joint_y))

    samplers, channels, animations = [], [], []
    for name, duration, axis, amp, phase in (("sway", 2.0, (0, 0, 1), 0.45, 0.9), ("nod", 1.2, (1, 0, 0), 0.35, 0.6)):
        keys = 12
        times = [duration * k / keys for k in range(keys + 1)]
        a_t = add(b"".join(struct.pack("<f", t) for t in times), 5126, "SCALAR", len(times), bounds=([0.0], [duration]))
        samplers, channels = [], []
        for j in (1, 2, 3):
            q = []
            for t in times:
                ang = amp * math.sin(2 * math.pi * t / duration - phase * (j - 1))
                s = math.sin(ang / 2)
                q.append((axis[0] * s, axis[1] * s, axis[2] * s, math.cos(ang / 2)))
            a_q = add(b"".join(struct.pack("<ffff", *v) for v in q), 5126, "VEC4", len(q))
            samplers.append({"input": a_t, "output": a_q, "interpolation": "LINEAR"})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": 2 + j, "path": "rotation"}})
        animations.append({"name": name, "samplers": samplers, "channels": channels})

    nodes = [
        {"name": "swayer", "mesh": 0, "skin": 0},
        {"name": "armature", "children": [2]},
        {"name": "base", "translation": [0, 0, 0], "children": [3]},
        {"name": "lower", "translation": [0, 0.8, 0], "children": [4]},
        {"name": "middle", "translation": [0, 0.8, 0], "children": [5]},
        {"name": "top", "translation": [0, 0.8, 0]},
    ]
    doc = {
        "asset": {"version": "2.0", "generator": "lunaris tools/make_models.py"},
        "scene": 0, "scenes": [{"nodes": [1, 0]}],
        "nodes": nodes,
        "meshes": [{"name": "swayer", "primitives": [{"attributes": {"POSITION": a_pos, "NORMAL": a_nrm, "JOINTS_0": a_jnt, "WEIGHTS_0": a_wgt}, "indices": a_idx, "material": 0}]}],
        "materials": [{"pbrMetallicRoughness": {"baseColorFactor": [0.95, 0.5, 0.2, 1], "metallicFactor": 0.0, "roughnessFactor": 0.6}}],
        "skins": [{"joints": [2, 3, 4, 5], "inverseBindMatrices": a_ibm, "skeleton": 2}],
        "animations": animations,
        "buffers": [{"byteLength": len(blob)}], "bufferViews": views, "accessors": accessors,
    }
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    glb = b"glTF" + struct.pack("<II", 2, 12 + 8 + len(js) + 8 + len(blob)) + struct.pack("<I", len(js)) + b"JSON" + js + struct.pack("<I", len(blob)) + b"BIN\x00" + bytes(blob)
    open(os.path.join(OUT, "swayer.glb"), "wb").write(glb)


swayer()
print("wrote swayer.glb")
