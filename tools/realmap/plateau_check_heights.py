"""How the PLATEAU tiles sit on the relief: for the vertices of tiles (a converted kind folder) the height above the ground picture at the same place. Run on the machine with the data.

    python3 plateau_check_heights.py DEM_FOLDER KIND_FOLDER [...]
"""
import json
import os
import sys

import numpy as np
from PIL import Image


def load_cells(folder):
    meta = json.load(open(os.path.join(folder, "cells.json")))
    cells = {}
    for c in meta["cells"]:
        im = np.asarray(Image.open(os.path.join(folder, f"cell_{c['i']}_{c['j']}", "heightmap.png")), dtype=np.float32) / 65535.0
        cells[(c["i"], c["j"])] = (c["y0"], c["relief"], im)
    return cells


def ground(cells, x, z):
    i, j = np.floor((x + 500) / 1000).astype(int), np.floor((z + 500) / 1000).astype(int)
    out = np.full(len(x), np.nan)
    for key in set(zip(i.tolist(), j.tolist())):
        if key not in cells:
            continue
        y0, relief, im = cells[key]
        m = (i == key[0]) & (j == key[1])
        u = np.clip(x[m] - (key[0] * 1000 - 500), 0, 1000)
        v = np.clip(z[m] - (key[1] * 1000 - 500), 0, 1000)
        u0, v0 = np.minimum(u.astype(int), 999), np.minimum(v.astype(int), 999)
        fu, fv = u - u0, v - v0
        h = (im[v0, u0] * (1 - fu) * (1 - fv) + im[v0, u0 + 1] * fu * (1 - fv) + im[v0 + 1, u0] * (1 - fu) * fv + im[v0 + 1, u0 + 1] * fu * fv)
        out[m] = y0 + h * relief
    return out


def main():
    cells = load_cells(sys.argv[1])
    for kind in sys.argv[2:]:
        idx = json.load(open(os.path.join(kind, "index.json")))
        diffs = []
        low = []
        for t in idx["tiles"]:
            if not t["leaf"]:
                continue
            g = json.load(open(os.path.join(kind, t["file"])))
            b = open(os.path.join(kind, g["buffers"][0]["uri"]), "rb").read()
            for p in g["meshes"][0]["primitives"]:
                a = g["accessors"][p["attributes"]["POSITION"]]
                v = g["bufferViews"][a["bufferView"]]
                pos = np.frombuffer(b, dtype=np.float32, count=a["count"] * 3, offset=v["byteOffset"]).reshape(-1, 3)
                gy = ground(cells, pos[:, 0].astype(np.float64), pos[:, 2].astype(np.float64))
                d = pos[:, 1] - gy
                ok = np.isfinite(d)
                if ok.any():
                    diffs.append(d[ok])
                    low.append(np.percentile(d[ok], 2))
        d = np.concatenate(diffs)
        print(f"{kind}: vertices {len(d)}, above the ground: min {d.min():.2f}, 2% {np.percentile(d, 2):.2f}, 10% {np.percentile(d, 10):.2f}, median {np.median(d):.2f}; per-tile lowest-2% median {np.median(low):.2f}")


main()
