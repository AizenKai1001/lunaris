"""Look inside a PLATEAU 3D Tiles set (what the tileset says, and what one tile holds): python3 plateau_inspect.py TILESET_FOLDER"""
import json
import os
import struct
import sys


def tiles(node, depth=0, out=None):
    out = [] if out is None else out
    content = (node.get("content") or {}).get("uri") or (node.get("content") or {}).get("url")
    if content:
        out.append((depth, content, node.get("boundingVolume"), node.get("transform")))
    for c in node.get("children") or []:
        tiles(c, depth + 1, out)
    return out


def b3dm(path):
    d = open(path, "rb").read()
    magic, ver, total, ftj, ftb, btj, btb = struct.unpack_from("<4sIIIIII", d, 0)
    off = 28
    ft = json.loads(d[off:off + ftj]) if ftj else {}
    off += ftj + ftb
    bt = json.loads(d[off:off + btj]) if btj else {}
    off += btj + btb
    return magic, ver, ft, bt, d[off:]


def main():
    folder = sys.argv[1]
    ts = json.load(open(os.path.join(folder, "tileset.json")))
    root = ts["root"]
    print("asset", ts.get("asset"), "geometricError", ts.get("geometricError"))
    print("root transform", root.get("transform"))
    print("root boundingVolume", root.get("boundingVolume"))
    leaves = tiles(root)
    print(len(leaves), "tiles with content; depths", sorted({l[0] for l in leaves}))
    for l in leaves[:3]:
        print("  ", l)
    first = os.path.join(folder, leaves[0][1])
    magic, ver, ft, bt, glb = b3dm(first)
    print("b3dm", magic, ver, "feature table", ft, "batch table keys", list(bt)[:12], "glb bytes", len(glb))
    gm, gv, gl = struct.unpack_from("<4sII", glb, 0)
    jl, jt = struct.unpack_from("<II", glb, 12)
    gj = json.loads(glb[20:20 + jl])
    print("glb", gm, gv, gl, "nodes", len(gj.get("nodes", [])), "meshes", len(gj.get("meshes", [])), "materials", len(gj.get("materials", [])), "images", len(gj.get("images", [])), "textures", len(gj.get("textures", [])))
    print("extensions", gj.get("extensionsUsed"), "asset", gj.get("asset"))
    print("nodes", json.dumps(gj.get("nodes", [])[:3])[:400])
    print("accessors", json.dumps(gj["accessors"][:3])[:500])
    print("materials", json.dumps(gj["materials"][:2])[:400])
    print("images", json.dumps(gj.get("images", [])[:3]))


main()
