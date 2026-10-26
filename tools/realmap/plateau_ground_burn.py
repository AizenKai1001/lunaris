r"""The ground of a whole PLATEAU city: every cell of plateau_dem_city.py gets its roads and pavements burned in, its macro picture and its terrain-layer weights.

    python plateau_ground_burn.py DEM_FOLDER NPZ_FOLDER

DEM_FOLDER has the cells (heightmap.png from the relief, terrain.jpg from plateau_ortho.py) and NPZ_FOLDER the `<code>.npz` of every ward (plateau_ground_collect.py). The triangles of all the wards are put into one grid of kilometre cells, so a cell
only looks at its own and the cells around it; then each cell is what plateau_ground.py makes of it (see there): the relief lowered by LOWER, the heights of road surfaces and building bases burned in, the macro picture (class colours, white
paint found in the photograph, the photograph elsewhere) as terrain.jpg / _l1 / _l2 (the photograph is kept as photo.jpg), and splat.png. Cells are done in PROCS processes (default 4); a cell that has splat.png is skipped, so a run can be
resumed. cells.json is rewritten at the end with the new heights range of every cell. The data is (c) MLIT Japan, Project PLATEAU (processed here).
"""
import glob
import json
import math
import multiprocessing
import os
import sys
import time

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import plateau_ground as G  # noqa: E402

STATE = {}


def build_grid(tri):
    """For a (T, 3, 2) array of triangles: (order, keys, starts) so that the triangles of a cell are order[starts[k]:starts[k+1]] for key k (a cell is the kilometre their middle is in)."""
    mid = tri.mean(axis=1)
    ci = np.floor((mid[:, 0] + 500) / 1000).astype(np.int64)
    cj = np.floor((mid[:, 1] + 500) / 1000).astype(np.int64)
    key = (ci + 1000) * 100000 + (cj + 1000)
    order = np.argsort(key, kind="stable")
    ks = key[order]
    uniq, starts = np.unique(ks, return_index=True)
    return order, uniq, np.append(starts, len(ks))


def pick(grid, i, j):
    """The indices of the triangles in the cell (i, j) and the eight around it."""
    order, uniq, starts = grid
    out = []
    for di in (-1, 0, 1):
        for dj in (-1, 0, 1):
            k = (i + di + 1000) * 100000 + (j + dj + 1000)
            at = np.searchsorted(uniq, k)
            if at < len(uniq) and uniq[at] == k:
                out.append(order[starts[at]:starts[at + 1]])
    return np.concatenate(out) if out else np.zeros(0, dtype=np.int64)


def init(npz_folder):
    r_tri, r_pl, r_cat, b_tri, b_pl = [], [], [], [], []
    for f in sorted(glob.glob(os.path.join(npz_folder, "*.npz"))):
        z = np.load(f)
        g = z["r_ground"]
        r_tri.append(z["r_tri"][g])
        r_pl.append(z["r_pl"][g])
        r_cat.append(z["r_cat"][g])
        b_tri.append(z["b_tri"])
        b_pl.append(z["b_pl"])
    STATE["r_tri"], STATE["r_pl"], STATE["r_cat"] = np.concatenate(r_tri), np.concatenate(r_pl), np.concatenate(r_cat)
    STATE["b_tri"], STATE["b_pl"] = np.concatenate(b_tri), np.concatenate(b_pl)
    STATE["r_grid"], STATE["b_grid"] = build_grid(STATE["r_tri"]), build_grid(STATE["b_tri"])


def one(job):
    folder, c = job
    i, j = c["i"], c["j"]
    d = os.path.join(folder, f"cell_{i}_{j}")
    ri, bi = pick(STATE["r_grid"], i, j), pick(STATE["b_grid"], i, j)
    rt, rp, rc = STATE["r_tri"][ri], STATE["r_pl"][ri], STATE["r_cat"][ri]
    layers = [(STATE["b_tri"][bi], STATE["b_pl"][bi], np.full(len(bi), 5, dtype=np.uint8))]
    for cat in (3, 2, 1):
        m = rc == cat
        layers.append((rt[m], rp[m], np.full(int(m.sum()), cat, dtype=np.uint8)))
    cls, hr = G.raster(c["cx"], c["cz"], [(t.astype(np.float64), p, k) for t, p, k in layers])
    from PIL import Image
    im = np.asarray(Image.open(os.path.join(d, "heightmap_dem.png")), dtype=np.float32) / 65535.0
    dem_cell = c["y0"] + im * c["relief"]
    pix = np.clip(np.round(np.arange(1001) / 1000.0 * G.N - 0.5).astype(int), 0, G.N - 1)
    entry, rl, bl, lo, hi = G.burn_cell(folder, c, dem_cell, cls, hr, pix)
    return entry, rl, bl


def main():
    folder, npz_folder = sys.argv[1], sys.argv[2]
    t0 = time.time()
    meta, first = G.prepare(folder)
    sig = ",".join(os.path.splitext(os.path.basename(f))[0] for f in sorted(glob.glob(os.path.join(npz_folder, "*.npz"))))
    # a cell is made again when the set of wards has changed since it was made (its neighbours' roads and bases may be new)
    done_path = os.path.join(folder, "cells_burned.json")
    done = {}
    if os.path.exists(done_path):
        done = {(e["i"], e["j"]): e for e in json.load(open(done_path))}
    cells = meta["cells"]
    if os.environ.get("ONLY_NEAR"):  # "x,z,r": only the cells whose middle is within r metres of (x, z), for a try
        ox, oz, orr = (float(v) for v in os.environ["ONLY_NEAR"].split(","))
        cells = [c for c in cells if math.hypot(c["cx"] - ox, c["cz"] - oz) <= orr]
    todo = [c for c in cells if done.get((c["i"], c["j"]), {}).get("sig") != sig or not os.path.exists(os.path.join(folder, f"cell_{c['i']}_{c['j']}", "splat.png"))]
    print(len(meta["cells"]), "cells,", len(cells), "wanted,", len(todo), "to do (wards:", sig + ")", flush=True)
    if todo:
        with multiprocessing.Pool(int(os.environ.get("PROCS", "4")), initializer=init, initargs=(npz_folder,)) as pool:
            for k, (entry, rl, bl) in enumerate(pool.imap_unordered(one, [(folder, c) for c in todo])):
                entry["sig"] = sig
                done[(entry["i"], entry["j"])] = entry
                json.dump(list(done.values()), open(done_path, "w"), separators=(",", ":"))
                if k % 10 == 0:
                    print(f"  {k + 1}/{len(todo)}: cell {entry['i']},{entry['j']} roads {rl:.2f} buildings {bl:.2f}, {time.time() - t0:.0f} s", flush=True)
    out = [{k: v for k, v in done[(c["i"], c["j"])].items() if k != "sig"} if (c["i"], c["j"]) in done else c for c in meta["cells"]]
    json.dump({"origin": meta["origin"], "cells": out}, open(os.path.join(folder, "cells.json"), "w"), separators=(",", ":"))
    print(len(out), "cells", f"{time.time() - t0:.0f} s")


if __name__ == "__main__":
    main()
