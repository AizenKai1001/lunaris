r"""The ground of a PLATEAU city done properly: roads and pavements are the ground, not a mesh laid over a smooth relief.

    python plateau_ground.py DEM_FOLDER TRAN_TILESET BLDG_TILESET TRAN_ELEV_OUT LAT0 LON0 [H0]

What the data gives: a relief from a 5 m grid (smooth: a road lies 0.2 to 0.8 m under it, a building's base up to two metres), road surfaces of LoD3 (roadway, intersection, pavement, traffic island, with each triangle's height),
LoD2 buildings (their ground faces say where each stands and at what height), and an aerial photograph with baked shadows, a blue cast and buildings leaning over the street. This script, for every ground cell of plateau_dem.py:

1. burns the heights into the heightmap: where the road surfaces lie at ground level (within 2 m of the relief) the ground takes their height, under every building the height of its base, elsewhere the relief lowered by LOWER (0.3 m: the plots lie a
   little under the pavement); roads that are far from the relief (a motorway on stilts, a tunnel) are left alone;
2. rasterizes what lies where (roadway, pavement, island, building) at 0.5 m and makes the cell's *macro picture* from it (terrain.jpg, _l1, _l2): each class has its own colour, white paint (crossings, lane lines) is found in the photograph as what is
   much brighter than the asphalt around it, and ground that is none of these (parks, rail yards, plazas) keeps the photograph, its shadows lifted and its cast calmed. The photograph itself is kept as photo.jpg and the first relief as heightmap_dem.png;
3. writes splat.png (asphalt, pavement, plaza and building ground, grass: the weights of the engine's four terrain layers: real asphalt and paving textures give the detail up close, the macro picture the colour and the markings);
4. writes the road surfaces that are NOT at ground level (stilts, overpasses) as glTF tiles in TRAN_ELEV_OUT with planar texture coordinates and shared asphalt and pavement pictures (asphalt.jpg and pavement.jpg must be in that folder).

The data is (c) MLIT Japan, Project PLATEAU (processed here).
"""
import json
import math
import os
import shutil
import sys
import time

import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import binary_erosion, gaussian_filter, uniform_filter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from plateau_tiles import Frame, decode_tile, nodes_with_content, tile_names, write_gltf  # noqa: E402

N = 2048
LOWER = float(os.environ.get("LOWER", "0.3"))
GROUND_TOL = float(os.environ.get("GROUND_TOL", "2.0"))
# display colours (sRGB 0..1) of the classes in the macro picture
COLOUR = {1: (0.30, 0.30, 0.315), 2: (0.56, 0.54, 0.51), 3: (0.49, 0.48, 0.45), 5: (0.52, 0.52, 0.50)}


def load_dem(folder):
    meta = json.load(open(os.path.join(folder, "cells.json")))
    cells = {}
    for c in meta["cells"]:
        d = os.path.join(folder, f"cell_{c['i']}_{c['j']}")
        orig = os.path.join(d, "heightmap_dem.png")
        if not os.path.exists(orig):
            shutil.copy(os.path.join(d, "heightmap.png"), orig)
        # the relief of the first run is what the numbers y0/relief of that run mean: they are kept in dem_meta.json
        cells[(c["i"], c["j"])] = c
    return meta, cells


def dem_arrays(folder, meta_first):
    out = {}
    for (i, j), c in meta_first.items():
        im = np.asarray(Image.open(os.path.join(folder, f"cell_{i}_{j}", "heightmap_dem.png")), dtype=np.float32) / 65535.0
        out[(i, j)] = c["y0"] + im * c["relief"]
    return out


def dem_at(arrs, x, z):
    i, j = np.floor((x + 500) / 1000).astype(int), np.floor((z + 500) / 1000).astype(int)
    out = np.full(len(x), np.nan)
    for key in set(zip(i.tolist(), j.tolist())):
        if key not in arrs:
            continue
        m = (i == key[0]) & (j == key[1])
        u = np.clip(x[m] - (key[0] * 1000 - 500), 0, 1000)
        v = np.clip(z[m] - (key[1] * 1000 - 500), 0, 1000)
        u0, v0 = np.minimum(u.astype(int), 999), np.minimum(v.astype(int), 999)
        fu, fv = u - u0, v - v0
        h = arrs[key]
        out[m] = h[v0, u0] * (1 - fu) * (1 - fv) + h[v0, u0 + 1] * fu * (1 - fv) + h[v0 + 1, u0] * (1 - fu) * fv + h[v0 + 1, u0 + 1] * fu * fv
    return out


def planes(tri):
    """h = a x + b z + c for triangles (T, 3, 3) of (x, y, z); a flag for the usable ones."""
    p0, p1, p2 = tri[:, 0], tri[:, 1], tri[:, 2]
    n = np.cross(p1 - p0, p2 - p0)
    ok = np.abs(n[:, 1]) > 1e-6
    ny = np.where(ok, n[:, 1], 1.0)
    a = -n[:, 0] / ny
    b = -n[:, 2] / ny
    c = p0[:, 1] - a * p0[:, 0] - b * p0[:, 2]
    return np.stack([a, b, c], axis=1), ok, n


def road_categories(batch, bid):
    """Per vertex: 1 roadway or intersection, 2 pavement, 3 island, and whether it is a motorway (tran:Road_tran:function)."""
    fn = batch.get("tran:function") or []
    top = batch.get("tran:Road_tran:function") or []
    cat = np.array([2 if "歩道" in str(s) else 3 if "島" in str(s) else 1 for s in fn] or [1], dtype=np.uint8)
    hw = np.array([("高速" in str(s)) for s in top] or [False])
    k = np.clip(bid.astype(np.int64), 0, len(cat) - 1)
    kh = np.clip(bid.astype(np.int64), 0, len(hw) - 1)
    return cat[k], hw[kh]


def collect_roads(tileset_dir, frame, dem):
    ts = json.load(open(os.path.join(tileset_dir, "tileset.json")))
    nodes = nodes_with_content(ts["root"], {})
    names = tile_names(sorted(nodes))
    tris, pl, cats, ground, refs = [], [], [], [], []
    for uri, info in sorted(nodes.items()):
        if not info[2]:
            continue
        path = os.path.join(tileset_dir, uri)
        name = names[uri]
        for pi, p in enumerate(decode_tile(path, frame, "tran")):
            t = p["idx"].reshape(-1, 3)
            tri = p["pos"][t].astype(np.float64)
            plane, ok, _ = planes(tri)
            vcat, vhw = road_categories(p["batch"], p["bid"] if p["bid"] is not None else np.zeros(len(p["pos"]), dtype=np.int64))
            cat = vcat[t[:, 0]]
            hw = vhw[t[:, 0]]
            cx, cz = tri[:, :, 0].mean(axis=1), tri[:, :, 2].mean(axis=1)
            near = np.abs(tri[:, :, 1].mean(axis=1) - dem(cx, cz)) <= GROUND_TOL
            at_ground = ok & near & ~hw
            tris.append(tri[:, :, [0, 2]])
            pl.append(plane)
            cats.append(cat)
            ground.append(at_ground)
            refs.append((name, pi, len(t)))
    return np.concatenate(tris), np.concatenate(pl), np.concatenate(cats), np.concatenate(ground), refs


def collect_building_bases(tileset_dir, frame, dem):
    ts = json.load(open(os.path.join(tileset_dir, "tileset.json")))
    nodes = nodes_with_content(ts["root"], {})
    tris, pl = [], []
    for uri, info in sorted(nodes.items()):
        if not info[2]:
            continue
        for p in decode_tile(os.path.join(tileset_dir, uri), frame, "bldg"):
            t = p["idx"].reshape(-1, 3)
            tri = p["pos"][t].astype(np.float64)
            plane, ok, n = planes(tri)
            ln = np.linalg.norm(n, axis=1)
            down = ok & (n[:, 1] / np.maximum(ln, 1e-12) < -0.8)
            if down.any():
                # only the faces at the ground: the undersides of overhangs, of stilted buildings and of upper floors are not where it stands
                cxz = tri[down][:, :, [0, 2]].mean(axis=1)
                near = np.abs(tri[down][:, :, 1].mean(axis=1) - dem(cxz[:, 0], cxz[:, 1])) <= float(os.environ.get("BASE_TOL", "3.5"))
                idx = np.nonzero(down)[0][near]
                down = np.zeros(len(tri), dtype=bool)
                down[idx] = True
            if down.any():
                tris.append(tri[down][:, :, [0, 2]])
                pl.append(plane[down])
    return np.concatenate(tris), np.concatenate(pl)


def raster(cx, cz, layers):
    """Paint triangles into an N x N picture of the cell, in the order given (later over earlier): `layers` is a list of (triangles in x, z, planes, class). Returns the class picture and the height picture."""
    img = Image.new("I", (N, N), 0)
    draw = ImageDraw.Draw(img)
    classes = [0]
    abc = [(0.0, 0.0, 0.0)]
    for tri, pl, cls in layers:
        sel = (tri[:, :, 0].max(axis=1) >= cx - 500) & (tri[:, :, 0].min(axis=1) <= cx + 500) & (tri[:, :, 1].max(axis=1) >= cz - 500) & (tri[:, :, 1].min(axis=1) <= cz + 500)
        px = (tri[sel][:, :, 0] - (cx - 500)) / 1000.0 * N
        pz = (tri[sel][:, :, 1] - (cz - 500)) / 1000.0 * N
        pls = pl[sel]
        cl = cls[sel] if isinstance(cls, np.ndarray) else np.full(len(px), cls, dtype=np.uint8)
        base = len(classes)
        classes.extend(cl.tolist())
        abc.extend(map(tuple, pls.tolist()))
        for k in range(len(px)):
            draw.polygon([(px[k, 0], pz[k, 0]), (px[k, 1], pz[k, 1]), (px[k, 2], pz[k, 2])], fill=base + k)
    ids = np.asarray(img)
    cls_img = np.array(classes, dtype=np.uint8)[ids]
    abc_a = np.array(abc, dtype=np.float64)
    gx = (cx - 500) + (np.arange(N) + 0.5) * 1000.0 / N
    gz = (cz - 500) + (np.arange(N) + 0.5) * 1000.0 / N
    X, Z = np.meshgrid(gx, gz)
    h = abc_a[ids, 0] * X + abc_a[ids, 1] * Z + abc_a[ids, 2]
    h[ids == 0] = np.nan
    return cls_img, h.astype(np.float32)


def smoothstep(a, b, x):
    t = np.clip((x - a) / (b - a), 0, 1)
    return t * t * (3 - 2 * t)


def macro_and_splat(cls, photo):
    """The macro picture (display colours) and the four terrain-layer weights of one cell from its classes and photograph."""
    lum = photo @ np.array([0.299, 0.587, 0.114], dtype=np.float32)
    road = cls == 1
    # white paint: much brighter than the asphalt around it (a ratio, so that it is found in shadow too), well inside the road
    w = uniform_filter(road.astype(np.float32), 17)
    ref = uniform_filter(lum * road, 17) / np.maximum(w, 1e-3)
    ratio = lum / np.maximum(ref, 0.05)
    inner = binary_erosion(road, iterations=2)
    mark = np.clip((ratio - 1.4) / 0.6, 0, 1) * inner
    mark = gaussian_filter(mark, 0.5)
    # the ground that is none of the classes: the photograph with its shadows lifted and its cast calmed
    lift = np.clip(photo, 0, 1) ** 0.72
    grey = (lift @ np.array([0.299, 0.587, 0.114], dtype=np.float32))[..., None]
    other = grey + (lift - grey) * 0.75
    out = other.copy()
    for k, col in COLOUR.items():
        out[cls == k] = col
    out = out * (1 - mark[..., None]) + np.array([0.86, 0.86, 0.83], dtype=np.float32) * mark[..., None]
    out = gaussian_filter(out, (0.6, 0.6, 0))
    exg = 2 * photo[..., 1] - photo[..., 0] - photo[..., 2]
    veg = smoothstep(0.015, 0.09, exg) * (cls == 0)
    asphalt = (cls == 1).astype(np.float32)
    pave = ((cls == 2) | (cls == 3)).astype(np.float32)
    grass = veg.astype(np.float32)
    plaza = np.clip(1.0 - asphalt - pave - grass, 0, 1)
    wts = np.stack([asphalt, pave, plaza, grass], axis=2)
    wts = gaussian_filter(wts, (0.8, 0.8, 0))
    wts = wts / np.maximum(wts.sum(axis=2, keepdims=True), 1e-6)
    return np.clip(out, 0, 1), wts


def write_elevated(tileset_dir, out_dir, frame, ground_flags, refs):
    """The road surfaces that are not at ground level as glTF tiles with planar coordinates and shared pictures."""
    os.makedirs(out_dir, exist_ok=True)
    ts = json.load(open(os.path.join(tileset_dir, "tileset.json")))
    nodes = nodes_with_content(ts["root"], {})
    names = tile_names(sorted(nodes))
    pos_in = 0
    keep_by = {}
    for name, pi, n in refs:
        keep_by[(name, pi)] = ~ground_flags[pos_in:pos_in + n]
        pos_in += n
    index = []
    for uri, info in sorted(nodes.items()):
        if not info[2]:
            continue
        name = names[uri]
        out_prims = []
        for pi, p in enumerate(decode_tile(os.path.join(tileset_dir, uri), frame, "tran")):
            keep = keep_by.get((name, pi))
            if keep is None or not keep.any():
                continue
            t = p["idx"].reshape(-1, 3)[keep]
            vcat, _ = road_categories(p["batch"], p["bid"] if p["bid"] is not None else np.zeros(len(p["pos"]), dtype=np.int64))
            tcat = vcat[t[:, 0]]
            for cat, uri_name in ((1, "asphalt.jpg"), (2, "pavement.jpg")):
                sel = (tcat == cat) if cat == 2 else (tcat != 2)
                if not sel.any():
                    continue
                tt = t[sel]
                used, inv = np.unique(tt, return_inverse=True)
                pos = p["pos"][used]
                nrm = p["nrm"][used]
                scale = 3.0 if cat == 1 else 2.0
                uv = np.stack([pos[:, 0] / scale, pos[:, 2] / scale], axis=1).astype(np.float32)
                out_prims.append({"pos": pos, "nrm": nrm, "uv": uv, "col": None, "idx": inv.astype(np.uint32), "tex": None, "tex_uri": uri_name})
        if not out_prims:
            continue
        write_gltf(os.path.join(out_dir, name), out_prims, name)
        allpos = np.concatenate([q["pos"] for q in out_prims])
        lo, hi = allpos.min(axis=0), allpos.max(axis=0)
        index.append({"name": name, "file": name + ".gltf", "depth": 0, "geometric_error": 0.0, "leaf": True, "parent": None, "centre": [round(float(v), 2) for v in (lo + hi) / 2], "radius": round(float(np.linalg.norm((hi - lo) / 2)), 1), "min": [round(float(v), 2) for v in lo], "max": [round(float(v), 2) for v in hi], "triangles": int(sum(len(q["idx"]) // 3 for q in out_prims))})
    json.dump({"origin": {}, "tiles": index}, open(os.path.join(out_dir, "index.json"), "w"), separators=(",", ":"))
    return len(index)


def prepare(folder):
    """The first run keeps the relief as heightmap_dem.png / cells_dem.json and the photograph as photo.jpg (later runs start from those); returns (meta of the relief, {(i, j): cell})."""
    meta_path = os.path.join(folder, "cells_dem.json")
    if not os.path.exists(meta_path):
        shutil.copy(os.path.join(folder, "cells.json"), meta_path)
    meta = json.load(open(meta_path))
    first = {(c["i"], c["j"]): c for c in meta["cells"]}
    for (i, j) in first:
        d = os.path.join(folder, f"cell_{i}_{j}")
        if not os.path.exists(os.path.join(d, "heightmap_dem.png")):
            shutil.copy(os.path.join(d, "heightmap.png"), os.path.join(d, "heightmap_dem.png"))
        if not os.path.exists(os.path.join(d, "photo.jpg")) and os.path.exists(os.path.join(d, "terrain.jpg")):
            shutil.copy(os.path.join(d, "terrain.jpg"), os.path.join(d, "photo.jpg"))
    return meta, first


def burn_cell(folder, c, dem_cell, cls, hr, pix):
    """One cell: the heights burned in (heightmap.png), the macro picture (terrain*.jpg) and the layer weights (splat.png) from its classes `cls` and heights `hr` (raster()); returns the cell's new entry."""
    i, j = c["i"], c["j"]
    d = os.path.join(folder, f"cell_{i}_{j}")
    H = dem_cell - LOWER
    samp_c = cls[np.ix_(pix, pix)]
    samp_h = hr[np.ix_(pix, pix)]
    road_like = np.isin(samp_c, (1, 2, 3)) & np.isfinite(samp_h)
    bld = (samp_c == 5) & np.isfinite(samp_h)
    H = np.where(road_like, samp_h, H)
    H = np.where(bld, samp_h - 0.05, H)
    lo, hi = float(H.min()), float(H.max())
    y0 = math.floor(lo) - 0.5
    relief = max(math.ceil(hi - y0) + 0.5, 1.0)
    Image.fromarray(np.clip((H - y0) / relief * 65535.0 + 0.5, 0, 65535).astype("<u2")).save(os.path.join(d, "heightmap.png"))
    photo_path = os.path.join(d, "photo.jpg")
    photo = np.asarray(Image.open(photo_path).convert("RGB"), dtype=np.float32) / 255.0 if os.path.exists(photo_path) else np.full((N, N, 3), 0.4, dtype=np.float32)
    macro, wts = macro_and_splat(cls, photo)
    im = Image.fromarray((macro * 255 + 0.5).astype(np.uint8))
    im.save(os.path.join(d, "terrain.jpg"), quality=93)
    im.resize((1024, 1024), Image.LANCZOS).save(os.path.join(d, "terrain_l1.jpg"), quality=92)
    im.resize((512, 512), Image.LANCZOS).save(os.path.join(d, "terrain_l2.jpg"), quality=92)
    Image.fromarray((wts * 255 + 0.5).astype(np.uint8), "RGBA").save(os.path.join(d, "splat.png"))
    return {**c, "y0": round(y0, 2), "relief": round(relief, 2), "burned": round(float(road_like.mean() + bld.mean()), 3)}, float(road_like.mean()), float(bld.mean()), lo, hi


def main():
    folder, tran_dir, bldg_dir, elev_out = sys.argv[1:5]
    lat0, lon0 = float(sys.argv[5]), float(sys.argv[6])
    h0 = float(sys.argv[7]) if len(sys.argv) > 7 else 0.0
    frame = Frame(lat0, lon0, h0)
    t0 = time.time()
    meta, first = prepare(folder)
    dem = dem_arrays(folder, first)
    dem_fn = lambda x, z: dem_at(dem, x, z)  # noqa: E731
    r_tri, r_pl, r_cat, r_ground, refs = collect_roads(tran_dir, frame, dem_fn)
    print(f"roads: {len(r_tri)} triangles, {int(r_ground.sum())} at ground level, {time.time() - t0:.0f} s", flush=True)
    b_tri, b_pl = collect_building_bases(bldg_dir, frame, dem_fn)
    print(f"buildings: {len(b_tri)} ground faces, {time.time() - t0:.0f} s", flush=True)
    g = r_ground
    order = [(b_tri, b_pl, np.uint8(5)), (r_tri[g & (r_cat == 3)], r_pl[g & (r_cat == 3)], np.uint8(3)), (r_tri[g & (r_cat == 2)], r_pl[g & (r_cat == 2)], np.uint8(2)), (r_tri[g & (r_cat == 1)], r_pl[g & (r_cat == 1)], np.uint8(1))]
    new_meta = []
    pix = np.clip(np.round(np.arange(1001) / 1000.0 * N - 0.5).astype(int), 0, N - 1)
    for key, c in sorted(first.items()):
        layers = [(t, p, np.full(len(t), cl, dtype=np.uint8)) for t, p, cl in order]
        cls, hr = raster(c["cx"], c["cz"], layers)
        entry, rl, bl, lo, hi = burn_cell(folder, c, dem[key], cls, hr, pix)
        new_meta.append(entry)
        print(f"  cell {key[0]},{key[1]}: roads/paving {rl:.2f}, buildings {bl:.2f}, y {lo:.1f}..{hi:.1f}, {time.time() - t0:.0f} s", flush=True)
    json.dump({"origin": meta["origin"], "cells": new_meta}, open(os.path.join(folder, "cells.json"), "w"), separators=(",", ":"))
    n = write_elevated(tran_dir, elev_out, frame, r_ground, refs)
    print(f"{len(new_meta)} cells, {n} elevated road tiles, {time.time() - t0:.0f} s")


if __name__ == "__main__":
    main()
