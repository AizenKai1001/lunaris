r"""PLATEAU 3D Tiles (Japan's official open 3D city models, MLIT) as glTF files the engine reads, in a local frame.

    python plateau_tiles.py TILESET_FOLDER OUT_FOLDER LAT0 LON0 [H0]

TILESET_FOLDER is one unpacked tileset (it has `tileset.json` and the tiles, `.b3dm` or `.glb`). Every tile becomes OUT_FOLDER/<name>.gltf (+ .bin and the pictures as JPEG): y up, x east, z south, the point
(LAT0, LON0, H0 above the ellipsoid) at the origin, in metres. `OUT_FOLDER/index.json` lists the tiles with their middle, radius and triangle count (the world builder places them from it).

What is in a tile: Draco-compressed triangles (KHR_draco_mesh_compression), WebP pictures (EXT_texture_webp), the position given relative to a centre in Earth-centred coordinates (CESIUM_RTC) with the glTF axes
of Cesium (y up -> z up). A surface with no picture gets a colour of its own: roofs (the normal points up) and walls differ, and each building (the batch id of its vertices) gets its own shade, so that a city
of untextured LoD2 buildings is not one white mass. The data is (c) MLIT Japan, Project PLATEAU: 出典：国土交通省 PLATEAUウェブサイト, processed here (converted to glTF, local frame, colours added).
"""
import json
import math
import os
import struct
import sys
import time
import zlib

import numpy as np
from PIL import Image

import DracoPy

A = 6378137.0
F = 1 / 298.257223563
E2 = F * (2 - F)


def geodetic_to_ecef(lat, lon, h):
    la, lo = math.radians(lat), math.radians(lon)
    n = A / math.sqrt(1 - E2 * math.sin(la) ** 2)
    return np.array([(n + h) * math.cos(la) * math.cos(lo), (n + h) * math.cos(la) * math.sin(lo), (n * (1 - E2) + h) * math.sin(la)])


def enu_matrix(lat, lon):
    """Rows east, north, up (in Earth-centred axes) at a place."""
    la, lo = math.radians(lat), math.radians(lon)
    e = np.array([-math.sin(lo), math.cos(lo), 0.0])
    n = np.array([-math.sin(la) * math.cos(lo), -math.sin(la) * math.sin(lo), math.cos(la)])
    u = np.array([math.cos(la) * math.cos(lo), math.cos(la) * math.sin(lo), math.sin(la)])
    return np.stack([e, n, u])


class Frame:
    def __init__(self, lat0, lon0, h0):
        self.origin = geodetic_to_ecef(lat0, lon0, h0)
        self.r = enu_matrix(lat0, lon0)

    def to_local(self, ecef):
        """Earth-centred points (N x 3) to the engine's frame: x east, y up, z south."""
        enu = (ecef - self.origin) @ self.r.T
        return np.stack([enu[:, 0], enu[:, 2], -enu[:, 1]], axis=1)

    def rot_local(self, v_ecef):
        enu = v_ecef @ self.r.T
        return np.stack([enu[:, 0], enu[:, 2], -enu[:, 1]], axis=1)


def read_container(path):
    """(glTF json, binary chunk, batch table) of a .b3dm or .glb file."""
    d = open(path, "rb").read()
    batch = {}
    if d[:4] == b"b3dm":
        _, _, _, ftj, ftb, btj, btb = struct.unpack_from("<4sIIIIII", d, 0)
        off = 28 + ftj + ftb
        if btj:
            try:
                batch = json.loads(d[off:off + btj])
            except ValueError:
                batch = {}
        off += btj + btb
        d = d[off:]
    jl, _ = struct.unpack_from("<II", d, 12)
    gj = json.loads(d[20:20 + jl])
    bo = 20 + jl
    binchunk = b""
    if bo + 8 <= len(d):
        bl, _ = struct.unpack_from("<II", d, bo)
        binchunk = d[bo + 8:bo + 8 + bl]
    return gj, binchunk, batch


COMP = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


def accessor(gj, binchunk, idx):
    a = gj["accessors"][idx]
    bv = gj["bufferViews"][a["bufferView"]]
    dt = np.dtype(COMP[a["componentType"]])
    n = NCOMP[a["type"]]
    off = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    stride = bv.get("byteStride") or dt.itemsize * n
    if stride == dt.itemsize * n:
        arr = np.frombuffer(binchunk, dtype=dt, count=a["count"] * n, offset=off).reshape(a["count"], n)
    else:
        arr = np.stack([np.frombuffer(binchunk, dtype=dt, count=n, offset=off + i * stride) for i in range(a["count"])])
    return arr.astype(np.float64) if dt == np.float32 else arr


def node_matrix(node):
    if "matrix" in node:
        return np.array(node["matrix"], dtype=np.float64).reshape(4, 4).T
    m = np.eye(4)
    s = node.get("scale", [1, 1, 1])
    q = node.get("rotation", [0, 0, 0, 1])
    t = node.get("translation", [0, 0, 0])
    x, y, z, w = q
    r = np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)], [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)], [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])
    m[:3, :3] = r * np.array(s)
    m[:3, 3] = t
    return m


def walk(gj, node_idx, parent, out):
    node = gj["nodes"][node_idx]
    m = parent @ node_matrix(node)
    if "mesh" in node:
        out.append((node["mesh"], m))
    for c in node.get("children", []):
        walk(gj, c, m, out)


def instances(gj):
    scene = gj["scenes"][gj.get("scene", 0)] if gj.get("scenes") else {"nodes": list(range(len(gj["nodes"])))}
    out = []
    for n in scene["nodes"]:
        walk(gj, n, np.eye(4), out)
    return out


def image_bytes(gj, binchunk, tex_index):
    t = gj["textures"][tex_index]
    src = t.get("source")
    if src is None:
        for ext in (t.get("extensions") or {}).values():
            if "source" in ext:
                src = ext["source"]
                break
    img = gj["images"][src]
    bv = gj["bufferViews"][img["bufferView"]]
    off = bv.get("byteOffset", 0)
    return binchunk[off:off + bv["byteLength"]]


# colours for what has no picture (linear, display-ish values the engine squares): roofs and walls of a city
ROOFS = np.array([[0.42, 0.43, 0.45], [0.36, 0.37, 0.40], [0.48, 0.45, 0.42], [0.33, 0.36, 0.36], [0.52, 0.50, 0.47], [0.38, 0.30, 0.27]])
WALLS = np.array([[0.80, 0.79, 0.76], [0.72, 0.71, 0.68], [0.84, 0.82, 0.77], [0.66, 0.68, 0.70], [0.78, 0.74, 0.68], [0.60, 0.61, 0.62], [0.86, 0.86, 0.85], [0.70, 0.66, 0.60]])


class Ortho:
    """The aerial photographs of the ground cells (plateau_dem.py / plateau_ortho.py: DEM_FOLDER/cell_<i>_<j>/photo.jpg or terrain.jpg) sampled at a place in the local frame: the colour of a roof seen from above."""

    def __init__(self, folder):
        self.folder = folder
        self.cache = {}

    def cell(self, i, j):
        key = (i, j)
        if key not in self.cache:
            d = os.path.join(self.folder, f"cell_{i}_{j}")
            path = next((os.path.join(d, n) for n in ("photo.jpg", "terrain.jpg") if os.path.exists(os.path.join(d, n))), None)
            self.cache[key] = None if path is None else np.asarray(Image.open(path).convert("RGB"), dtype=np.uint8)
        return self.cache[key]

    def sample(self, x, z):
        out = np.full((len(x), 3), np.nan, dtype=np.float32)
        ci, cj = np.floor((x + 500) / 1000).astype(int), np.floor((z + 500) / 1000).astype(int)
        for key in set(zip(ci.tolist(), cj.tolist())):
            im = self.cell(*key)
            if im is None:
                continue
            m = (ci == key[0]) & (cj == key[1])
            n = im.shape[0]
            u = np.clip((x[m] - (key[0] * 1000 - 500)) / 1000.0 * n - 0.5, 0, n - 1.001)
            v = np.clip((z[m] - (key[1] * 1000 - 500)) / 1000.0 * n - 0.5, 0, n - 1.001)
            u0, v0 = u.astype(int), v.astype(int)
            fu, fv = (u - u0)[:, None], (v - v0)[:, None]
            px = lambda a, b: im[a, b].astype(np.float32) / 255.0  # noqa: E731
            out[m] = px(v0, u0) * (1 - fu) * (1 - fv) + px(v0, u0 + 1) * fu * (1 - fv) + px(v0 + 1, u0) * (1 - fu) * fv + px(v0 + 1, u0 + 1) * fu * fv
        return out


ORTHO = None  # set by main() when ORTHO_DEM names a folder of ground cells
LOW_RISE = 30.0  # metres: a roof seen from above is where it seems to be only for the lower buildings (the photographs are not true orthophotographs)


def roof_colours(col, pos, nrm, bid, batch):
    """Untextured low buildings: roofs take the colour of the aerial photograph (shadows lifted, cast calmed), walls a little of it."""
    if ORTHO is None or bid is None:
        return col
    heights = batch.get("bldg:measuredHeight") or []
    hv = np.array([float(h) if isinstance(h, (int, float)) else 99.0 for h in heights] or [99.0])
    low = hv[np.clip(bid.astype(np.int64), 0, len(hv) - 1)] < LOW_RISE
    roof = (nrm[:, 1] > 0.5) & low
    if not roof.any():
        return col
    photo = ORTHO.sample(pos[roof, 0].astype(np.float64), pos[roof, 2].astype(np.float64))
    ok = np.isfinite(photo).all(axis=1)
    lift = np.clip(photo, 0, 1) ** 0.72
    grey = (lift @ np.array([0.299, 0.587, 0.114], dtype=np.float32))[:, None]
    lift = grey + (lift - grey) * 0.8
    idx = np.nonzero(roof)[0][ok]
    col = col.copy()
    col[idx] = np.clip(0.15 * col[idx] + 0.85 * lift[ok], 0, 1)
    # the walls of the same building: a quarter of the building's average roof colour
    n_b = int(bid.max()) + 1
    sums = np.zeros((n_b, 3), dtype=np.float64)
    cnt = np.zeros(n_b, dtype=np.float64)
    np.add.at(sums, bid[idx].astype(np.int64), lift[ok])
    np.add.at(cnt, bid[idx].astype(np.int64), 1.0)
    avg = (sums / np.maximum(cnt, 1.0)[:, None]).astype(np.float32)
    has = cnt[bid.astype(np.int64)] > 0
    wall = ~(nrm[:, 1] > 0.5) & low & has
    col[wall] = np.clip(0.75 * col[wall] + 0.25 * np.clip(avg[bid[wall].astype(np.int64)] * 1.15, 0, 1), 0, 1)
    return col


def shade(batch_ids, normals):
    h = (batch_ids.astype(np.uint64) * np.uint64(2654435761)) & np.uint64(0xFFFFFFFF)
    roof = normals[:, 1] > 0.5
    col = np.empty((len(normals), 3), dtype=np.float32)
    col[roof] = ROOFS[(h[roof] >> np.uint64(7)) % len(ROOFS)]
    col[~roof] = WALLS[(h[~roof] >> np.uint64(5)) % len(WALLS)]
    # a little variation inside a shade, from the building, so that neighbours with the same palette entry still differ
    col *= (0.93 + 0.14 * ((h >> np.uint64(11)) & np.uint64(255)).astype(np.float32)[:, None] / 255.0)
    return np.clip(col, 0.0, 1.0)


def road_colours(bid, batch, normals):
    """Colours for road tiles from what the batch table says each surface is: the roadway (TrafficArea) is asphalt, the pavement and verge (AuxiliaryTrafficArea) concrete; other things keep the city palette."""
    kinds = batch.get("feature_type") or []
    ids = np.clip(bid.astype(np.int64), 0, max(len(kinds) - 1, 0))
    aux = np.array([("Auxiliary" in str(kinds[i])) if kinds else False for i in range(len(kinds))] or [False])[ids]
    h = (bid.astype(np.uint64) * np.uint64(2654435761)) & np.uint64(0xFFFFFFFF)
    jitter = (0.92 + 0.16 * ((h >> np.uint64(9)) & np.uint64(255)).astype(np.float32) / 255.0)[:, None]
    asphalt = np.array([0.20, 0.20, 0.21], dtype=np.float32)
    concrete = np.array([0.52, 0.51, 0.49], dtype=np.float32)
    col = np.where(aux[:, None], concrete, asphalt) * jitter
    return np.clip(col, 0.0, 1.0).astype(np.float32)


def decode_tile(path, frame, kind=""):
    """The primitives of one tile in the engine's frame: a list of dicts (pos, nrm, uv, col, idx, texture bytes or None, factor)."""
    gj, binchunk, batch = read_container(path)
    rtc = np.array((gj.get("extensions") or {}).get("CESIUM_RTC", {}).get("center", [0.0, 0.0, 0.0]))
    prims = []
    for mesh_idx, m in instances(gj):
        for p in gj["meshes"][mesh_idx]["primitives"]:
            attrs = p.get("attributes", {})
            dr = (p.get("extensions") or {}).get("KHR_draco_mesh_compression")
            if dr:
                bv = gj["bufferViews"][dr["bufferView"]]
                blob = binchunk[bv.get("byteOffset", 0):bv.get("byteOffset", 0) + bv["byteLength"]]
                mesh = DracoPy.decode(blob)
                pos = np.asarray(mesh.points, dtype=np.float64)
                nrm = np.asarray(mesh.normals, dtype=np.float64) if mesh.normals is not None and len(mesh.normals) else None
                uv = np.asarray(mesh.tex_coord, dtype=np.float64) if mesh.tex_coord is not None and len(mesh.tex_coord) else None
                idx = np.asarray(mesh.faces, dtype=np.uint32).reshape(-1)
                bid = None
                aid = dr["attributes"].get("_BATCHID")
                if aid is not None:
                    try:
                        bid = np.asarray(mesh.get_attribute_by_unique_id(aid)["data"]).reshape(-1)
                    except Exception:  # noqa: BLE001
                        bid = None
                if uv is None and "TEXCOORD_0" in dr["attributes"]:
                    try:
                        uv = np.asarray(mesh.get_attribute_by_unique_id(dr["attributes"]["TEXCOORD_0"])["data"], dtype=np.float64).reshape(len(pos), 2)
                    except Exception:  # noqa: BLE001
                        uv = None
            else:
                pos = accessor(gj, binchunk, attrs["POSITION"])
                nrm = accessor(gj, binchunk, attrs["NORMAL"]) if "NORMAL" in attrs else None
                uv = accessor(gj, binchunk, attrs["TEXCOORD_0"]) if "TEXCOORD_0" in attrs else None
                bid = accessor(gj, binchunk, attrs["_BATCHID"]).reshape(-1) if "_BATCHID" in attrs else None
                idx = accessor(gj, binchunk, p["indices"]).reshape(-1).astype(np.uint32) if "indices" in p else np.arange(len(pos), dtype=np.uint32)
            if len(pos) == 0 or len(idx) < 3:
                continue
            # the node's matrix, then Cesium's y-up -> z-up, then the centre: Earth-centred metres
            h = np.concatenate([pos, np.ones((len(pos), 1))], axis=1) @ m.T
            q = h[:, :3]
            ecef = np.stack([q[:, 0], -q[:, 2], q[:, 1]], axis=1) + rtc
            local = frame.to_local(ecef)
            if nrm is None:
                nl = np.zeros_like(local)
                nl[:, 1] = 1.0
            else:
                nr = nrm @ m[:3, :3].T
                ne = np.stack([nr[:, 0], -nr[:, 2], nr[:, 1]], axis=1)
                nl = frame.rot_local(ne)
                nl /= np.maximum(np.linalg.norm(nl, axis=1, keepdims=True), 1e-9)
            mat = gj["materials"][p["material"]] if "material" in p and gj.get("materials") else {}
            pbr = mat.get("pbrMetallicRoughness", {})
            tex = None
            if "baseColorTexture" in pbr and uv is not None:
                tex = image_bytes(gj, binchunk, pbr["baseColorTexture"]["index"])
            factor = pbr.get("baseColorFactor", [1.0, 1.0, 1.0, 1.0])
            col = None
            if tex is None and kind == "tran" and bid is not None:
                col = road_colours(bid, batch, nl)
            elif tex is None:
                col = shade(bid if bid is not None else np.zeros(len(local), dtype=np.int64), nl)
                if kind == "bldg":
                    col = roof_colours(col, local, nl, bid, batch)
                if factor[:3] != [1.0, 1.0, 1.0]:
                    col = np.clip(col * np.array(factor[:3], dtype=np.float32), 0, 1)
            prims.append({"pos": local.astype(np.float32), "nrm": nl.astype(np.float32), "uv": None if uv is None else uv.astype(np.float32), "col": col, "idx": idx, "tex": tex, "bid": bid, "batch": batch})
    return prims


def write_gltf(out_base, prims, image_names):
    """One .gltf + .bin with a primitive for each of `prims` (pictures are files beside it)."""
    binary = bytearray()
    views, accs, primitives, mats, images, textures = [], [], [], [], [], []
    tex_of = {}

    def add(arr, target=None):
        while len(binary) % 4:
            binary.append(0)
        views.append({"buffer": 0, "byteOffset": len(binary), "byteLength": arr.nbytes, **({"target": target} if target else {})})
        binary.extend(arr.tobytes())
        return len(views) - 1

    for p in prims:
        vi = add(np.ascontiguousarray(p["pos"], dtype=np.float32), 34962)
        lo, hi = p["pos"].min(axis=0).tolist(), p["pos"].max(axis=0).tolist()
        accs.append({"bufferView": vi, "componentType": 5126, "count": len(p["pos"]), "type": "VEC3", "min": lo, "max": hi})
        a_pos = len(accs) - 1
        accs.append({"bufferView": add(np.ascontiguousarray(p["nrm"], dtype=np.float32), 34962), "componentType": 5126, "count": len(p["nrm"]), "type": "VEC3"})
        a_nrm = len(accs) - 1
        attributes = {"POSITION": a_pos, "NORMAL": a_nrm}
        if p["uv"] is not None and (p["tex"] is not None or p.get("tex_uri")):
            uv = np.ascontiguousarray(p["uv"], dtype=np.float32)
            accs.append({"bufferView": add(uv, 34962), "componentType": 5126, "count": len(uv), "type": "VEC2"})
            attributes["TEXCOORD_0"] = len(accs) - 1
        if p["col"] is not None and not p.get("tex_uri"):
            accs.append({"bufferView": add(np.ascontiguousarray(p["col"], dtype=np.float32), 34962), "componentType": 5126, "count": len(p["col"]), "type": "VEC3"})
            attributes["COLOR_0"] = len(accs) - 1
        accs.append({"bufferView": add(np.ascontiguousarray(p["idx"], dtype=np.uint32), 34963), "componentType": 5125, "count": len(p["idx"]), "type": "SCALAR"})
        a_idx = len(accs) - 1
        if p.get("tex_uri"):
            # a picture that lives in the folder and is shared by every tile (one file, one texture on the card)
            if p["tex_uri"] not in tex_of:
                images.append({"uri": p["tex_uri"]})
                textures.append({"source": len(images) - 1, "sampler": 0})
                tex_of[p["tex_uri"]] = len(textures) - 1
            mats.append({"pbrMetallicRoughness": {"baseColorTexture": {"index": tex_of[p["tex_uri"]]}, "metallicFactor": 0.0, "roughnessFactor": 0.9}})
        elif p["tex"] is not None:
            key = zlib.crc32(p["tex"])
            if key not in tex_of:
                name = f"{image_names}_{len(tex_of)}.jpg"
                from io import BytesIO

                im = Image.open(BytesIO(p["tex"]))
                im = im.convert("RGB")
                im.save(os.path.join(os.path.dirname(out_base), name), quality=88)
                images.append({"uri": name})
                textures.append({"source": len(images) - 1, "sampler": 0})
                tex_of[key] = len(textures) - 1
            mats.append({"pbrMetallicRoughness": {"baseColorTexture": {"index": tex_of[key]}, "metallicFactor": 0.0, "roughnessFactor": 0.85}})
        else:
            mats.append({"pbrMetallicRoughness": {"baseColorFactor": [1.0, 1.0, 1.0, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.85}})
        primitives.append({"attributes": attributes, "indices": a_idx, "material": len(mats) - 1})
    gltf = {"asset": {"version": "2.0", "generator": "lunaris realmap plateau_tiles (data: MLIT Project PLATEAU)", "extras": {"lunaris_lod": "whole"}}, "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}], "meshes": [{"primitives": primitives}], "materials": mats,
            "buffers": [{"uri": os.path.basename(out_base) + ".bin", "byteLength": len(binary)}], "bufferViews": views, "accessors": accs, "samplers": [{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]}
    if images:
        gltf["images"], gltf["textures"] = images, textures
    open(out_base + ".bin", "wb").write(bytes(binary))
    json.dump(gltf, open(out_base + ".gltf", "w"), separators=(",", ":"))


def tile_names(uris):
    """A name for every tile uri: its file name, or with its folders in front where the file names of the tileset are not unique (a big tileset has `data/0/data5.b3dm` and `data/1/data5.b3dm`)."""
    base = {u: os.path.splitext(os.path.basename(u))[0] for u in uris}
    count = {}
    for b in base.values():
        count[b] = count.get(b, 0) + 1
    return {u: (b if count[b] == 1 else os.path.splitext(u)[0].replace("/", "_")) for u, b in base.items()}


def nodes_with_content(node, out, depth=0, parent=None):
    """(depth, geometric error, is a leaf, the parent's name) of every tile that has content: the leaves are the detail, the inner tiles simplified versions of what is under them (refinement REPLACE)."""
    c = node.get("content") or {}
    uri = c.get("uri") or c.get("url")
    kids = node.get("children") or []
    me = parent
    if uri:
        out[uri] = (depth, float(node.get("geometricError") or 0.0), not kids, parent)
        me = os.path.splitext(os.path.basename(uri))[0]
    for ch in kids:
        nodes_with_content(ch, out, depth + 1, me)
    return out


def convert(job):
    """One tile: decode, write; returns its index entry (or None)."""
    folder, out, lat0, lon0, h0, uri, info, name = job
    path = os.path.join(folder, uri)
    if not os.path.exists(path):
        return None
    try:
        prims = decode_tile(path, Frame(lat0, lon0, h0), os.path.basename(os.path.normpath(out)).split("_")[0])
    except Exception as e:  # noqa: BLE001
        print("tile", uri, "failed:", repr(e)[:200], flush=True)
        return None
    if not prims:
        return None
    write_gltf(os.path.join(out, name), prims, name)
    allpos = np.concatenate([p["pos"] for p in prims])
    lo, hi = allpos.min(axis=0), allpos.max(axis=0)
    centre = ((lo + hi) / 2).tolist()
    radius = float(np.linalg.norm((hi - lo) / 2))
    return {"name": name, "file": name + ".gltf", "depth": info[0], "geometric_error": round(info[1], 2), "leaf": info[2], "parent": info[3], "centre": [round(c, 2) for c in centre], "radius": round(radius, 1), "min": [round(float(v), 2) for v in lo], "max": [round(float(v), 2) for v in hi], "triangles": int(sum(len(p["idx"]) // 3 for p in prims))}


def main():
    global ORTHO
    folder, out = sys.argv[1], sys.argv[2]
    if os.environ.get("ORTHO_DEM"):
        ORTHO = Ortho(os.environ["ORTHO_DEM"])
    lat0, lon0 = float(sys.argv[3]), float(sys.argv[4])
    h0 = float(sys.argv[5]) if len(sys.argv) > 5 else 0.0
    only = os.environ.get("ONLY")  # a comma list of tile numbers, for a try
    os.makedirs(out, exist_ok=True)
    ts = json.load(open(os.path.join(folder, "tileset.json")))
    nodes = nodes_with_content(ts["root"], {})
    uris = sorted(nodes)
    names = tile_names(uris)
    if os.environ.get("LEAVES") == "1":  # only the finest tiles (the kinds that are not a hierarchy in the world)
        uris = [u for u in uris if nodes[u][2]]
    if only:
        uris = [u for u in uris if any(f"data{n}." in u for n in only.split(","))]
    index = []
    t0 = time.time()
    jobs = [(folder, out, lat0, lon0, h0, u, nodes[u], names[u]) for u in uris]
    import multiprocessing

    with multiprocessing.Pool(int(os.environ.get("PROCS", "6"))) as pool:
        for k, entry in enumerate(pool.imap_unordered(convert, jobs, chunksize=2)):
            if entry:
                index.append(entry)
            if k % 50 == 0:
                print(f"{k + 1}/{len(jobs)}, {time.time() - t0:.0f} s", flush=True)
    index.sort(key=lambda t: t["name"])
    json.dump({"origin": {"lat": lat0, "lon": lon0, "h": h0}, "tiles": index}, open(os.path.join(out, "index.json"), "w"), separators=(",", ":"))
    print(len(index), "tiles,", sum(t["triangles"] for t in index), "triangles,", f"{time.time() - t0:.0f} s")


if __name__ == "__main__":
    main()
