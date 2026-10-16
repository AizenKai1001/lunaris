"""A small reader and writer of binary glTF (.glb) files for rigged models, standard library only: the nodes and skins, the animation tracks, and a function that writes a new file with a model and its (new) clips. It is what
`tools/glb_retarget.py` is made from; models of other makers (Quaternius's CC0 libraries, for one) come in with different skeletons and are made one.

Quaternions are (x, y, z, w) as in glTF. Matrices are not needed: the skeleton is worked through its local transforms.
"""
import json
import math
import struct

COMPONENT = {5120: ("b", 1), 5121: ("B", 1), 5122: ("h", 2), 5123: ("H", 2), 5125: ("I", 4), 5126: ("f", 4)}
NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


# ---------------------------------------------------------------- quaternions
def qmul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz)


def qinv(q):
    n = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]
    return (-q[0] / n, -q[1] / n, -q[2] / n, q[3] / n)


def qnorm(q):
    n = math.sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]) or 1.0
    return (q[0] / n, q[1] / n, q[2] / n, q[3] / n)


def qrot(q, v):
    x, y, z = v
    qx, qy, qz, qw = q
    tx, ty, tz = 2 * (qy * z - qz * y), 2 * (qz * x - qx * z), 2 * (qx * y - qy * x)
    return (x + qw * tx + qy * tz - qz * ty, y + qw * ty + qz * tx - qx * tz, z + qw * tz + qx * ty - qy * tx)


def qslerp(a, b, t):
    d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
    if d < 0:
        b, d = (-b[0], -b[1], -b[2], -b[3]), -d
    if d > 0.9995:
        return qnorm(tuple(a[i] + (b[i] - a[i]) * t for i in range(4)))
    th = math.acos(max(-1.0, min(1.0, d)))
    s = math.sin(th)
    wa, wb = math.sin((1 - t) * th) / s, math.sin(t * th) / s
    return tuple(a[i] * wa + b[i] * wb for i in range(4))


# ---------------------------------------------------------------- reading
class Glb:
    def __init__(self, path):
        data = open(path, "rb").read()
        magic, version, _length = struct.unpack_from("<4sII", data, 0)
        assert magic == b"glTF" and version == 2, path
        pos = 12
        self.json, self.bin = None, b""
        while pos < len(data):
            n, kind = struct.unpack_from("<I4s", data, pos)
            chunk = data[pos + 8:pos + 8 + n]
            if kind == b"JSON":
                self.json = json.loads(chunk.decode("utf-8"))
            elif kind == b"BIN\x00":
                self.bin = chunk
            pos += 8 + n
        self.nodes = self.json["nodes"]
        self.parent = {}
        for i, n in enumerate(self.nodes):
            for c in n.get("children", []):
                self.parent[c] = i

    def accessor(self, index):
        """The values of an accessor as a list of tuples (or numbers for SCALAR)."""
        a = self.json["accessors"][index]
        bv = self.json["bufferViews"][a["bufferView"]]
        fmt, size = COMPONENT[a["componentType"]]
        n = NCOMP[a["type"]]
        stride = bv.get("byteStride", size * n)
        base = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
        out = []
        norm = a.get("normalized", False)
        scale = {5120: 127.0, 5121: 255.0, 5122: 32767.0, 5123: 65535.0}.get(a["componentType"], 1.0)
        for i in range(a["count"]):
            vals = struct.unpack_from("<" + fmt * n, self.bin, base + i * stride)
            if norm:
                vals = tuple(max(v / scale, -1.0) for v in vals)
            out.append(vals[0] if n == 1 else vals)
        return out

    def skin_joints(self, skin=0):
        return self.json["skins"][skin]["joints"]

    def name(self, i):
        return self.nodes[i].get("name", f"node{i}")

    def rest(self, i):
        n = self.nodes[i]
        return tuple(n.get("translation", (0, 0, 0))), tuple(n.get("rotation", (0, 0, 0, 1))), tuple(n.get("scale", (1, 1, 1)))

    def animations(self):
        """{clip name: (duration, {node index: {path: (times, values)}})}"""
        out = {}
        for an in self.json.get("animations", []):
            tracks, dur = {}, 0.0
            for ch in an["channels"]:
                s = an["samplers"][ch["sampler"]]
                times = self.accessor(s["input"])
                vals = self.accessor(s["output"])
                node, path = ch["target"].get("node"), ch["target"]["path"]
                if node is None or path not in ("translation", "rotation", "scale"):
                    continue
                if s.get("interpolation", "LINEAR") == "CUBICSPLINE":
                    vals = vals[1::3]  # the values between the tangents
                tracks.setdefault(node, {})[path] = (times, vals)
                dur = max(dur, times[-1] if times else 0.0)
            out[an.get("name", f"clip{len(out)}")] = (dur, tracks)
        return out


def sample(track, t, path):
    times, vals = track
    if not times:
        return None
    if t <= times[0]:
        return vals[0]
    if t >= times[-1]:
        return vals[-1]
    lo, hi = 0, len(times) - 1
    while hi - lo > 1:
        mid = (lo + hi) // 2
        if times[mid] <= t:
            lo = mid
        else:
            hi = mid
    k = (t - times[lo]) / max(1e-9, times[hi] - times[lo])
    a, b = vals[lo], vals[hi]
    if path == "rotation":
        return qslerp(a, b, k)
    return tuple(a[i] + (b[i] - a[i]) * k for i in range(3))


# ---------------------------------------------------------------- writing
def write_glb(path, doc, blob):
    js = json.dumps(doc, separators=(",", ":")).encode("utf-8")
    js += b" " * (-len(js) % 4)
    blob = bytes(blob) + b"\x00" * (-len(blob) % 4)
    total = 12 + 8 + len(js) + 8 + len(blob)
    with open(path, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, total))
        f.write(struct.pack("<I4s", len(js), b"JSON") + js)
        f.write(struct.pack("<I4s", len(blob), b"BIN\x00") + blob)


class Appender:
    """Appends accessors to a glb's buffer (for new animation data)."""

    def __init__(self, glb):
        self.doc = glb.json
        self.blob = bytearray(glb.bin)

    def add(self, values, type_, component=5126, with_minmax=False):
        fmt, size = COMPONENT[component]
        n = NCOMP[type_]
        while len(self.blob) % 4:
            self.blob.append(0)
        start = len(self.blob)
        for v in values:
            self.blob += struct.pack("<" + fmt * n, *((v,) if n == 1 else v))
        self.doc.setdefault("bufferViews", []).append({"buffer": 0, "byteOffset": start, "byteLength": len(self.blob) - start})
        acc = {"bufferView": len(self.doc["bufferViews"]) - 1, "componentType": component, "count": len(values), "type": type_}
        if with_minmax:
            acc["min"], acc["max"] = [min(values)], [max(values)]
        self.doc.setdefault("accessors", []).append(acc)
        return len(self.doc["accessors"]) - 1

    def finish(self):
        self.doc["buffers"][0]["byteLength"] = len(self.blob)
        return bytes(self.blob)


# ---------------------------------------------------------------- tidying
def used_accessors(doc):
    used = set()
    for m in doc.get("meshes", []):
        for p in m["primitives"]:
            used.update(p["attributes"].values())
            if "indices" in p:
                used.add(p["indices"])
            for t in p.get("targets", []):
                used.update(t.values())
    for s in doc.get("skins", []):
        if "inverseBindMatrices" in s:
            used.add(s["inverseBindMatrices"])
    for a in doc.get("animations", []):
        for s in a["samplers"]:
            used.update((s["input"], s["output"]))
    return used


def compact(glb):
    """Rebuild the binary chunk so that only what the meshes, skins and animations use is in it (after clips were taken out), and renumber the accessors. Returns (doc, blob)."""
    doc = glb.json
    used = sorted(used_accessors(doc))
    remap = {old: new for new, old in enumerate(used)}
    blob = bytearray()
    views, accessors = [], []
    for old in used:
        a = dict(doc["accessors"][old])
        bv = doc["bufferViews"][a["bufferView"]]
        fmt, size = COMPONENT[a["componentType"]]
        n = NCOMP[a["type"]]
        stride = bv.get("byteStride", size * n)
        base = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
        while len(blob) % 4:
            blob.append(0)
        start = len(blob)
        packed = size * n
        for i in range(a["count"]):
            blob += glb.bin[base + i * stride: base + i * stride + packed]
        v = {"buffer": 0, "byteOffset": start, "byteLength": len(blob) - start}
        if bv.get("target") in (34962, 34963):
            v["target"] = bv["target"]
        views.append(v)
        a["bufferView"] = len(views) - 1
        a.pop("byteOffset", None)
        accessors.append(a)
    for m in doc.get("meshes", []):
        for p in m["primitives"]:
            p["attributes"] = {k: remap[v] for k, v in p["attributes"].items()}
            if "indices" in p:
                p["indices"] = remap[p["indices"]]
            for t in p.get("targets", []):
                for k in t:
                    t[k] = remap[t[k]]
    for s in doc.get("skins", []):
        if "inverseBindMatrices" in s:
            s["inverseBindMatrices"] = remap[s["inverseBindMatrices"]]
    for an in doc.get("animations", []):
        for s in an["samplers"]:
            s["input"], s["output"] = remap[s["input"]], remap[s["output"]]
    doc["bufferViews"], doc["accessors"] = views, accessors
    doc["buffers"] = [{"byteLength": len(blob)}]
    return doc, bytes(blob)
