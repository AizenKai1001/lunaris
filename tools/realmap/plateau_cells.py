r"""The buildings of a PLATEAU ward as streamable units with exclusive levels of detail.

    python plateau_cells.py OUT_FOLDER WARD_CODE TILES_FOLDER LAT0 LON0 H0

TILES_FOLDER is `<fetch>/<code>/tiles` (plateau_fetch.py): `fetched.json` next to it names the tilesets `bldg_lod1` and `bldg_lod2` (the LoD2 one may be missing). A unit is a leaf tile of the LoD2 tileset: its buildings and the
one picture (atlas) they share. For every unit two files are written, holding the SAME buildings:

  OUT_FOLDER/near/<code>_<tile>.gltf   LoD2, with the pictures of the data (shared jpg files in near/tex/);
  OUT_FOLDER/far/<code>_<tile>.gltf    the LoD1 blocks of these buildings (the LoD1 tiles are read once and each building goes to the unit its gml_id is in): vertex colours, no picture;
  OUT_FOLDER/rest/c<i>_<j>_<code>.gltf the LoD1 buildings no LoD2 tile has (all of them for a ward without LoD2), in 500 m cells of the local frame, vertex colours;
  OUT_FOLDER/units_<code>.json         the middle, extent, triangle counts and pictures of every unit and rest cell.

A streaming world loads the `near` or the `far` file of a unit by the distance of the unit's middle (the same middle: never both, never neither), and a rest cell by its own distance out to the far limit. A picture a unit needs is
loaded once however many units share it. Roofs of untextured low buildings take their colour from the ground photographs when ORTHO_DEM names a folder of ground cells (plateau_ortho.py); TEX_MAX (default 4096) caps the pictures.
The data is (c) MLIT Japan, Project PLATEAU: 出典：国土交通省 PLATEAUウェブサイト, processed here.
"""
import json
import multiprocessing
import os
import sys
import time
import zlib
from io import BytesIO

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import plateau_tiles as T  # noqa: E402

CELL = 500.0  # the grid of the rest cells (buildings that only have LoD1)
TEX_MAX = int(os.environ.get("TEX_MAX", "4096"))

OWNER = {}  # gml_id -> unit name, for the workers that cut the LoD1 tiles (set by init)


def init(owner):
    OWNER.update(owner)


def leaves_of(tileset_dir):
    """[(uri, name)] of the leaf tiles of a tileset (the name is unique in it)."""
    ts = json.load(open(os.path.join(tileset_dir, "tileset.json")))
    nodes = T.nodes_with_content(ts["root"], {})
    names = T.tile_names(sorted(nodes))
    return [(u, names[u]) for u, i in sorted(nodes.items()) if i[2]]


def save_picture(data, path):
    im = Image.open(BytesIO(data)).convert("RGB")
    if max(im.size) > TEX_MAX:
        s = TEX_MAX / max(im.size)
        im = im.resize((max(1, round(im.size[0] * s)), max(1, round(im.size[1] * s))), Image.LANCZOS)
    im.save(path, quality=92)


def merge(prims):
    """Prims with the same picture (none: those without) become one; returns the new list."""
    groups = {}
    for p in prims:
        groups.setdefault(p.get("tex_uri") or "", []).append(p)
    out = []
    for uri, items in groups.items():
        base = 0
        pos, nrm, uv, col, idx = [], [], [], [], []
        for p in items:
            pos.append(p["pos"])
            nrm.append(p["nrm"])
            if p["uv"] is not None:
                uv.append(p["uv"])
            if p["col"] is not None:
                col.append(p["col"])
            idx.append(p["idx"] + base)
            base += len(p["pos"])
        m = {"pos": np.concatenate(pos), "nrm": np.concatenate(nrm), "uv": np.concatenate(uv) if uv and len(uv) == len(items) else None, "col": np.concatenate(col) if col and len(col) == len(items) else None, "idx": np.concatenate(idx).astype(np.uint32), "tex": None, "tex_uri": uri or None}
        if uri and m["uv"] is None:
            m["tex_uri"] = None
        if not m["tex_uri"] and m["col"] is None:
            m["col"] = np.ones_like(m["pos"])
        out.append(m)
    return out


def write_unit(folder, name, prims):
    """Write the prims as folder/name.gltf; the index entry (None when there is nothing)."""
    prims = merge([p for p in prims if len(p["idx"]) >= 3])
    if not prims:
        return None
    T.write_gltf(os.path.join(folder, name), prims, name)
    allpos = np.concatenate([p["pos"] for p in prims])
    lo, hi = allpos.min(axis=0), allpos.max(axis=0)
    return {"file": name + ".gltf", "min": [round(float(v), 1) for v in lo], "max": [round(float(v), 1) for v in hi], "triangles": int(sum(len(p["idx"]) // 3 for p in prims)), "textures": sorted({p["tex_uri"] for p in prims if p["tex_uri"]})}


def pieces(p, keys):
    """The triangles of a prim cut by building: {key: prim dict}, `keys(b)` giving the key of the building with batch id b ("" for one to leave out)."""
    bid = p["bid"] if p["bid"] is not None else np.zeros(len(p["pos"]), dtype=np.int64)
    t = p["idx"].reshape(-1, 3)
    tb = bid[t[:, 0]].astype(np.int64)
    ub = np.unique(tb)
    kv = np.array([keys(int(b)) or "" for b in ub], dtype=object)
    out = {}
    for key in set(kv.tolist()) - {""}:
        sel = np.isin(tb, ub[kv == key])
        tt = t[sel]
        used, new_idx = np.unique(tt, return_inverse=True)
        out[key] = {"pos": p["pos"][used], "nrm": p["nrm"][used], "uv": None if p["uv"] is None else p["uv"][used], "col": None if p["col"] is None else p["col"][used], "idx": new_idx.reshape(-1).astype(np.uint32), "tex": None, "tex_uri": None}
    return out


def near_unit(job):
    """One LoD2 leaf tile -> near/<name>.gltf; returns (name, index entry, the gml ids of the buildings)."""
    path, name, near_dir, tex_dir, frame_args = job
    prims = T.decode_tile(path, T.Frame(*frame_args), "bldg")
    ids = []
    out = []
    for p in prims:
        gml = p["batch"].get("gml_id") or []
        if p["bid"] is not None:
            ids += [str(gml[b]) for b in np.unique(p["bid"]).tolist() if 0 <= b < len(gml) and gml[b]]
        tex_uri = None
        if p["tex"] is not None and p["uv"] is not None:
            pic = f"{name}_{zlib.crc32(p['tex']):08x}.jpg"
            fp = os.path.join(tex_dir, pic)
            if not os.path.exists(fp):
                save_picture(p["tex"], fp)
            tex_uri = "tex/" + pic
        out.append({"pos": p["pos"], "nrm": p["nrm"], "uv": p["uv"], "col": p["col"], "idx": p["idx"], "tex": None, "tex_uri": tex_uri})
    entry = write_unit(near_dir, name, out)
    return name, entry, ids


def far_pieces(job):
    """One LoD1 leaf tile cut for the units (OWNER) and the rest cells: ({unit name: [prims]}, {(i, j): [prims]})."""
    path, frame_args = job
    prims = T.decode_tile(path, T.Frame(*frame_args), "bldg")
    units, rest = {}, {}
    for p in prims:
        gml = p["batch"].get("gml_id") or []
        bid = p["bid"] if p["bid"] is not None else np.zeros(len(p["pos"]), dtype=np.int64)
        ub, inv = np.unique(bid, return_inverse=True)
        cnt = np.bincount(inv).astype(np.float64)
        cx = np.bincount(inv, weights=p["pos"][:, 0]) / cnt
        cz = np.bincount(inv, weights=p["pos"][:, 2]) / cnt
        cell = {int(b): (int(round(x / CELL)), int(round(z / CELL))) for b, x, z in zip(ub.tolist(), cx.tolist(), cz.tolist())}

        def key(b):
            g = str(gml[b]) if 0 <= b < len(gml) and gml[b] else None
            if g is not None and g in OWNER:
                return "u:" + OWNER[g]
            i, j = cell[b]
            return f"r:{i}:{j}"

        for k, piece in pieces(p, key).items():
            if k.startswith("u:"):
                units.setdefault(k[2:], []).append(piece)
            else:
                _, i, j = k.split(":")
                rest.setdefault((int(i), int(j)), []).append(piece)
    return units, rest


def main():
    out, code, tiles_dir = sys.argv[1], sys.argv[2], sys.argv[3]
    frame_args = (float(sys.argv[4]), float(sys.argv[5]), float(sys.argv[6]))
    meta = json.load(open(os.path.join(os.path.dirname(os.path.normpath(tiles_dir)), "fetched.json")))["tilesets"]
    if os.environ.get("ORTHO_DEM"):
        T.ORTHO = T.Ortho(os.environ["ORTHO_DEM"])
    near_dir, far_dir, rest_dir = (os.path.join(out, d) for d in ("near", "far", "rest"))
    tex_dir = os.path.join(near_dir, "tex")
    for d in (near_dir, far_dir, rest_dir, tex_dir):
        os.makedirs(d, exist_ok=True)
    t0 = time.time()
    procs = int(os.environ.get("PROCS", "6"))
    units = {}
    owner = {}
    if "bldg_lod2" in meta:
        d2 = os.path.join(tiles_dir, meta["bldg_lod2"])
        jobs = [(os.path.join(d2, u), f"{code}_{n}", near_dir, tex_dir, frame_args) for u, n in leaves_of(d2)]
        with multiprocessing.Pool(procs) as pool:
            for name, entry, ids in pool.imap_unordered(near_unit, jobs, chunksize=2):
                if entry is None:
                    continue
                units[name] = {"name": name, "near": entry}
                for g in ids:
                    owner.setdefault(g, name)
        print(code, f"lod2: {len(jobs)} tiles, {len(units)} units, {len(owner)} buildings, {time.time() - t0:.0f} s", flush=True)
    rest = {}
    far = {}
    if "bldg_lod1" in meta:
        d1 = os.path.join(tiles_dir, meta["bldg_lod1"])
        jobs = [(os.path.join(d1, u), frame_args) for u, _ in leaves_of(d1)]
        with multiprocessing.Pool(procs, initializer=init, initargs=(owner,)) as pool:
            for u_pieces, r_pieces in pool.imap_unordered(far_pieces, jobs):
                for k, ps in u_pieces.items():
                    far.setdefault(k, []).extend(ps)
                for k, ps in r_pieces.items():
                    rest.setdefault(k, []).extend(ps)
        print(code, f"lod1: {len(jobs)} tiles -> {len(far)} units, {len(rest)} rest cells, {time.time() - t0:.0f} s", flush=True)
    for name, ps in far.items():
        entry = write_unit(far_dir, name, ps)
        if entry is not None and name in units:
            units[name]["far"] = entry
    rest_cells = []
    for (i, j), ps in sorted(rest.items()):
        entry = write_unit(rest_dir, f"c{i}_{j}_{code}", ps)
        if entry is not None:
            rest_cells.append(entry)
    for u in units.values():
        lo, hi = np.array(u["near"]["min"]), np.array(u["near"]["max"])
        u["centre"] = [round(float(v), 1) for v in (lo + hi) / 2]
        u["radius"] = round(float(np.linalg.norm((hi - lo) / 2)), 1)
    for r in rest_cells:
        lo, hi = np.array(r["min"]), np.array(r["max"])
        r["centre"] = [round(float(v), 1) for v in (lo + hi) / 2]
        r["radius"] = round(float(np.linalg.norm((hi - lo) / 2)), 1)
    json.dump({"code": code, "units": sorted(units.values(), key=lambda u: u["name"]), "rest": rest_cells}, open(os.path.join(out, f"units_{code}.json"), "w"), separators=(",", ":"))
    tn = sum(u["near"]["triangles"] for u in units.values())
    tf = sum(u.get("far", {}).get("triangles", 0) for u in units.values())
    tr = sum(r["triangles"] for r in rest_cells)
    nofar = sum(1 for u in units.values() if "far" not in u)
    print(f"{code}: {len(units)} units ({nofar} without far), near {tn} triangles, far {tf}, rest {tr} in {len(rest_cells)} cells, {time.time() - t0:.0f} s")


if __name__ == "__main__":
    main()
