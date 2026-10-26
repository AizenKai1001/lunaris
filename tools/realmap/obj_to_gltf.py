"""A photogrammetry tile (Wavefront OBJ + MTL + JPEG textures, as the City of Melbourne publishes them) as a glTF the engine reads: one primitive and one material for each `usemtl`
(each material is a JPEG patch of the mesh; mesh.gltf, or mesh_0.gltf, mesh_1.gltf ... when the model is bigger than the engine's loader takes), y up, x east, z south, the tile's centre at the origin; smooth normals from the faces.

    python tools/realmap/obj_to_gltf.py TILE_FOLDER OBJ_NAME OUT_FOLDER [maxfaces]

The textures are used where they are (the .gltf refers to them by relative path), copy them next to the output or give the OUT_FOLDER the same place.
"""
import json
import os
import sys
import time

import numpy as np


def main():
    folder, objname, out = sys.argv[1], sys.argv[2], sys.argv[3]
    maxfaces = int(sys.argv[4]) if len(sys.argv) > 4 else 10**9
    os.makedirs(out, exist_ok=True)
    t0 = time.time()
    # the .mtl: material name -> texture file
    tex = {}
    cur = None
    for line in open(os.path.join(folder, objname.replace(".obj", ".mtl")), "r"):
        p = line.split()
        if not p:
            continue
        if p[0] == "newmtl":
            cur = p[1]
        elif p[0] == "map_Kd" and cur:
            tex[cur] = p[-1]
    # TEXTURE_SCALE=0.5 makes the photo patches half as wide and high (a level of detail of the texture set: this tile has 2 billion texels, 1.6 cm each, 8 GB even as bytes) into OUT/tex_50/ and refers to those
    scale = float(os.environ.get("TEXTURE_SCALE", "1"))
    if scale < 1.0:
        from PIL import Image

        sub = os.path.join(out, f"tex_{round(scale * 100)}")
        os.makedirs(sub, exist_ok=True)
        t1 = time.time()
        for mname, fname in list(tex.items()):
            dst = os.path.join(sub, os.path.basename(fname))
            if not os.path.exists(dst):
                im = Image.open(os.path.join(folder, fname)).convert("RGB")
                im.resize((max(4, round(im.width * scale)), max(4, round(im.height * scale))), Image.LANCZOS).save(dst, quality=92)
            tex[mname] = os.path.relpath(dst, out).replace(os.sep, "/")
        print(f"scaled {len(tex)} patches by {scale} in {time.time() - t1:.0f} s", flush=True)
    v, vt = [], []
    faces = {}  # material -> list of (v,vt) triples as flat int lists
    mat = None
    nf = 0
    with open(os.path.join(folder, objname), "r") as fh:
        for line in fh:
            if line.startswith("v "):
                p = line.split()
                v.append((float(p[1]), float(p[2]), float(p[3])))
            elif line.startswith("vt "):
                p = line.split()
                vt.append((float(p[1]), float(p[2])))
            elif line.startswith("f "):
                if nf >= maxfaces:
                    continue
                p = line.split()[1:]
                lst = faces.setdefault(mat, [])
                for tok in p[:3]:
                    a = tok.split("/")
                    lst.append(int(a[0]) - 1)
                    lst.append(int(a[1]) - 1 if len(a) > 1 and a[1] else 0)
                nf += 1
            elif line.startswith("usemtl"):
                mat = line.split()[1]
    print(f"read {len(v)} vertices, {len(vt)} uvs, {nf} faces, {len(faces)} materials in {time.time() - t0:.0f} s", flush=True)
    v = np.array(v, dtype=np.float64)
    vt = np.array(vt, dtype=np.float32)
    cx, cy = (v[:, 0].min() + v[:, 0].max()) / 2, (v[:, 1].min() + v[:, 1].max()) / 2
    pos_all = np.stack([v[:, 0] - cx, v[:, 2], -(v[:, 1] - cy)], axis=1).astype(np.float32)
    # smooth normals from all faces
    allf = np.concatenate([np.array(l, dtype=np.int64).reshape(-1, 6)[:, 0::2] for l in faces.values()])
    nrm = np.zeros_like(pos_all, dtype=np.float64)
    a, b, c = pos_all[allf[:, 0]].astype(np.float64), pos_all[allf[:, 1]].astype(np.float64), pos_all[allf[:, 2]].astype(np.float64)
    fn = np.cross(b - a, c - a)
    for k in range(3):
        np.add.at(nrm, allf[:, k], fn)
    ln = np.linalg.norm(nrm, axis=1, keepdims=True)
    nrm = (nrm / np.maximum(ln, 1e-12)).astype(np.float32)
    nrm[ln[:, 0] < 1e-12] = (0, 1, 0)
    # the engine reads a model of at most 4 million vertices and 128 MB: the primitives (one per photo patch) go into as many .gltf files as that takes (each a chunk of at most `budget` vertices);
    # the chunks share the JPEG patches and the frame, and a scene lists one MeshRenderer for each
    budget = int(os.environ.get("CHUNK_VERTICES", "1500000"))
    chunks, cur, cur_v = [], [], 0
    for name, lst in faces.items():
        nv = len(lst) // 2
        if cur and cur_v + nv > budget:
            chunks.append(cur)
            cur, cur_v = [], 0
        cur.append((name, lst))
        cur_v += nv
    if cur:
        chunks.append(cur)
    total_prims, total_bytes = 0, 0
    for k, items in enumerate(chunks):
        binary = bytearray()
        views, accs, prims, mats, images, textures = [], [], [], [], [], []

        def add(arr, target, comp, kind, count, mn=None, mx=None):
            while len(binary) % 4:
                binary.append(0)
            off = len(binary)
            raw = arr.tobytes()
            binary.extend(raw)
            views.append({"buffer": 0, "byteOffset": off, "byteLength": len(raw), "target": target})
            acc = {"bufferView": len(views) - 1, "componentType": comp, "count": count, "type": kind}
            if mn is not None:
                acc["min"], acc["max"] = mn, mx
            accs.append(acc)
            return len(accs) - 1

        for name, lst in items:
            arr = np.array(lst, dtype=np.int64).reshape(-1, 3, 2)
            flat = arr.reshape(-1, 2)
            key = flat[:, 0] * (len(vt) + 1) + flat[:, 1]
            uniq, inv = np.unique(key, return_inverse=True)
            vi, ti = uniq // (len(vt) + 1), uniq % (len(vt) + 1)
            p = pos_all[vi]
            n = nrm[vi]
            uv = vt[np.minimum(ti, len(vt) - 1)].copy()
            uv[:, 1] = 1.0 - uv[:, 1]  # OBJ's v runs up, glTF's down
            idx = inv.astype(np.uint32).reshape(-1, 3)  # (x, y, z) -> (x, z, -y) is a rotation, so the OBJ's counter-clockwise faces stay counter-clockwise
            pa = add(p, 34962, 5126, "VEC3", len(p), p.min(axis=0).tolist(), p.max(axis=0).tolist())
            na = add(n, 34962, 5126, "VEC3", len(n))
            ua = add(uv, 34962, 5126, "VEC2", len(uv))
            ia = add(idx.reshape(-1), 34963, 5125, "SCALAR", idx.size)
            m = {"name": name, "pbrMetallicRoughness": {"metallicFactor": 0.0, "roughnessFactor": 0.9}}
            if name in tex:
                images.append({"uri": tex[name]})
                textures.append({"source": len(images) - 1})
                m["pbrMetallicRoughness"]["baseColorTexture"] = {"index": len(textures) - 1}
            mats.append(m)
            prims.append({"attributes": {"POSITION": pa, "NORMAL": na, "TEXCOORD_0": ua}, "indices": ia, "material": len(mats) - 1})
        base = os.environ.get("STEM", "mesh")
        stem = base if len(chunks) == 1 else f"{base}_{k}"
        gltf = {"asset": {"version": "2.0", "generator": "lunaris realmap obj_to_gltf"}, "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}], "meshes": [{"primitives": prims}], "materials": mats, "images": images, "textures": textures, "buffers": [{"uri": stem + ".bin", "byteLength": len(binary)}], "bufferViews": views, "accessors": accs, "samplers": [{"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}]}
        for t in textures:
            t["sampler"] = 0
        open(os.path.join(out, stem + ".bin"), "wb").write(bytes(binary))
        json.dump(gltf, open(os.path.join(out, stem + ".gltf"), "w"))
        total_prims += len(prims)
        total_bytes += len(binary)
    print(f"wrote {total_prims} primitives in {len(chunks)} file(s), {total_bytes / 1e6:.0f} MB of buffers in {time.time() - t0:.0f} s; centre E {cx:.1f} N {cy:.1f}", flush=True)


if __name__ == "__main__":
    main()
