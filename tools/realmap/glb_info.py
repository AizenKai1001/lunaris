"""What a house model of the library looks like to the fitting step: its bounds (after the node transforms, in the file's own axes), triangles, materials and pictures.

    python tools/realmap/glb_info.py FILE.glb [...]

The bounds are what `fit_models.py` uses to scale a model to a real building's footprint and height.
"""
import json
import struct
import sys

import numpy as np


def load_glb(path):
    b = open(path, "rb").read()
    if b[:4] == b"glTF":
        n = struct.unpack("<I", b[12:16])[0]
        j = json.loads(b[20 : 20 + n])
        off = 20 + n
        binary = None
        while off + 8 <= len(b):
            ln, kind = struct.unpack("<I4s", b[off : off + 8])
            if kind == b"BIN\x00":
                binary = b[off + 8 : off + 8 + ln]
            off += 8 + ln
        return j, binary
    return json.loads(b), None


def node_matrix(n):
    if "matrix" in n:
        return np.array(n["matrix"], dtype=np.float64).reshape(4, 4).T
    t = np.array(n.get("translation", [0, 0, 0]), dtype=np.float64)
    q = n.get("rotation", [0, 0, 0, 1])
    s = np.array(n.get("scale", [1, 1, 1]), dtype=np.float64)
    x, y, z, w = q
    r = np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)], [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)], [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])
    m = np.eye(4)
    m[:3, :3] = r * s
    m[:3, 3] = t
    return m


def bounds(j):
    """(min, max, triangles) of the scene in the file's axes, node transforms applied (the corners of each primitive's box: exact for boxes, a little generous for rotated meshes)."""
    nodes = j["nodes"]
    lo, hi, tris = np.full(3, 1e18), np.full(3, -1e18), 0

    def walk(i, parent):
        nonlocal lo, hi, tris
        n = nodes[i]
        m = parent @ node_matrix(n)
        if "mesh" in n:
            for p in j["meshes"][n["mesh"]]["primitives"]:
                a = j["accessors"][p["attributes"]["POSITION"]]
                c = np.array([[x, y, z, 1.0] for x in (a["min"][0], a["max"][0]) for y in (a["min"][1], a["max"][1]) for z in (a["min"][2], a["max"][2])])
                w = (m @ c.T).T[:, :3]
                lo, hi = np.minimum(lo, w.min(axis=0)), np.maximum(hi, w.max(axis=0))
                if "indices" in p:
                    tris += j["accessors"][p["indices"]]["count"] // 3
        for c in n.get("children", []):
            walk(c, m)

    roots = j["scenes"][j.get("scene", 0)]["nodes"] if "scenes" in j else [0]
    for r in roots:
        walk(r, np.eye(4))
    return lo, hi, tris


if __name__ == "__main__":
    for f in sys.argv[1:]:
        j, _ = load_glb(f)
        lo, hi, tris = bounds(j)
        size = hi - lo
        print(f"{f}: {len(j.get('meshes', []))} meshes, {tris} triangles, {len(j.get('materials', []))} materials, {len(j.get('images', []))} pictures; bounds {np.round(lo, 2).tolist()} .. {np.round(hi, 2).tolist()}; size {np.round(size, 2).tolist()} (x, y up, z)")
