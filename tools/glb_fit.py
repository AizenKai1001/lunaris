"""Put a .glb model where a game wants it: scaled to a height in metres, standing on y = 0, centred on x and z.

AI image-to-3D tools (Hunyuan3D, TripoSR, ...) and many downloaded models come out at an arbitrary size with the middle of the model at the
origin. This rewrites the POSITION data of every mesh in place (a uniform scale and a shift, so the normals stay right), keeping the file
otherwise as it is. A model with a skin, morph targets or turned / scaled nodes is refused: fit those in a modelling program.

    python tools/glb_fit.py model.glb --height 1.2 [--out fitted.glb] [--origin center|feet]

`--origin feet` (default) puts the lowest point at y = 0 and the middle of the base at x = z = 0; `center` leaves y centred.
"""
import argparse
import json
import struct
import sys

COMPONENT = {5126: ("f", 4)}  # only float32 positions exist in practice


def read_glb(path):
    data = open(path, "rb").read()
    magic, version, _length = struct.unpack_from("<4sII", data, 0)
    if magic != b"glTF" or version != 2:
        sys.exit("not a binary glTF 2 file: " + path)
    pos, doc, blob = 12, None, b""
    while pos < len(data):
        n, kind = struct.unpack_from("<I4s", data, pos)
        chunk = data[pos + 8:pos + 8 + n]
        if kind == b"JSON":
            doc = json.loads(chunk.decode("utf-8"))
        elif kind == b"BIN\x00":
            blob = bytearray(chunk)
        pos += 8 + n
    if doc is None or not blob:
        sys.exit("the file has no JSON or no binary chunk")
    return doc, blob


def write_glb(path, doc, blob):
    js = json.dumps(doc, separators=(",", ":")).encode("utf-8")
    js += b" " * (-len(js) % 4)
    blob = bytes(blob) + b"\x00" * (-len(blob) % 4)
    total = 12 + 8 + len(js) + 8 + len(blob)
    with open(path, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, total))
        f.write(struct.pack("<I4s", len(js), b"JSON") + js)
        f.write(struct.pack("<I4s", len(blob), b"BIN\x00") + blob)


def position_accessors(doc):
    seen, out = set(), []
    for mesh in doc.get("meshes", []):
        for prim in mesh.get("primitives", []):
            if prim.get("targets"):
                sys.exit("the model has morph targets: fit it in a modelling program")
            a = prim["attributes"]["POSITION"]
            if a not in seen:
                seen.add(a)
                out.append(a)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("model")
    ap.add_argument("--height", type=float, required=True, help="the height in metres the model gets (y extent)")
    ap.add_argument("--out", help="write here instead of over the input")
    ap.add_argument("--origin", choices=["feet", "center"], default="feet")
    args = ap.parse_args()
    doc, blob = read_glb(args.model)
    if doc.get("skins"):
        sys.exit("the model has a skin: fit it in a modelling program")
    for node in doc.get("nodes", []):
        if any(k in node for k in ("matrix", "rotation", "scale", "translation")):
            m = node.get("matrix")
            ident = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]
            if (m is not None and m != ident) or any(k in node for k in ("rotation", "scale", "translation")):
                sys.exit("a node of the model has a transform: fit it in a modelling program")
    accs = position_accessors(doc)
    pts = []  # (accessor index, offset of each position in the blob)
    lo, hi = [1e30] * 3, [-1e30] * 3
    for ai in accs:
        acc = doc["accessors"][ai]
        if acc["componentType"] != 5126 or acc["type"] != "VEC3":
            sys.exit("positions are not float32 vectors")
        bv = doc["bufferViews"][acc["bufferView"]]
        stride = bv.get("byteStride", 12)
        base = bv.get("byteOffset", 0) + acc.get("byteOffset", 0)
        offs = [base + i * stride for i in range(acc["count"])]
        pts.append((ai, offs))
        for o in offs:
            x, y, z = struct.unpack_from("<3f", blob, o)
            for k, v in enumerate((x, y, z)):
                lo[k] = min(lo[k], v)
                hi[k] = max(hi[k], v)
    extent = hi[1] - lo[1]
    if extent <= 0:
        sys.exit("the model has no height")
    s = args.height / extent
    cx, cz = (lo[0] + hi[0]) / 2, (lo[2] + hi[2]) / 2
    cy = lo[1] if args.origin == "feet" else (lo[1] + hi[1]) / 2
    for ai, offs in pts:
        mn, mx = [1e30] * 3, [-1e30] * 3
        for o in offs:
            x, y, z = struct.unpack_from("<3f", blob, o)
            p = ((x - cx) * s, (y - cy) * s, (z - cz) * s)
            struct.pack_into("<3f", blob, o, *p)
            for k in range(3):
                mn[k] = min(mn[k], p[k])
                mx[k] = max(mx[k], p[k])
        doc["accessors"][ai]["min"] = mn
        doc["accessors"][ai]["max"] = mx
    write_glb(args.out or args.model, doc, blob)
    size = [(hi[k] - lo[k]) * s for k in range(3)]
    print("fitted %s: scale %.4f, size %.3f x %.3f x %.3f m" % (args.out or args.model, s, size[0], size[1], size[2]))


if __name__ == "__main__":
    main()
