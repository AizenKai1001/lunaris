r"""The ground of a whole PLATEAU city: the relief of every ward as 16-bit height cells of 1 km in one frame.

    python plateau_dem_city.py OUT_FOLDER LAT0 LON0 H0 GML_FOLDER [GML_FOLDER ...]

Each GML_FOLDER holds `*_dem_*.gml` files (the `udx/dem` of a ward's CityGML package) or the `.npy` point files plateau_dem_pts.py made of them. Every file is read for its triangle corners (latitude, longitude, height above the geoid, JGD2011), which become points of the
local frame through Earth-centred coordinates (so the curvature of the Earth is in the heights: 31 m at 20 km: x east, y up, z south, the point (LAT0, LON0, H0 above the ellipsoid) the origin). The cells are centred on whole
kilometres; each takes the points within it and 80 m around it and interpolates them linearly to 1001 x 1001 samples a kilometre. OUT_FOLDER/cell_<i>_<j>/heightmap.png and OUT_FOLDER/cells.json (centre, lowest ground y0 and
relief of each cell, as plateau_dem.py). The geoid height over the ellipsoid is GEOID (default 36.7 m: Tokyo) minus LOWER (0.3 m: the ground lies a little under the pavement the next step puts on it).
The data is (c) MLIT Japan, Project PLATEAU (processed here).
"""
import glob
import json
import math
import os
import sys
import time

import numpy as np
from PIL import Image
from scipy.interpolate import LinearNDInterpolator
from scipy.ndimage import distance_transform_edt

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from plateau_dem import POS, read_vertices  # noqa: E402
from plateau_tiles import A, E2, Frame  # noqa: E402


def to_local(frame, lat, lon, h):
    la, lo = np.radians(lat), np.radians(lon)
    n = A / np.sqrt(1 - E2 * np.sin(la) ** 2)
    ecef = np.stack([(n + h) * np.cos(la) * np.cos(lo), (n + h) * np.cos(la) * np.sin(lo), (n * (1 - E2) + h) * np.sin(la)], axis=1)
    return frame.to_local(ecef)


def main():
    out = sys.argv[1]
    lat0, lon0, h0 = float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
    folders = sys.argv[5:]
    geoid = float(os.environ.get("GEOID", "36.7")) - float(os.environ.get("LOWER", "0.3"))
    os.makedirs(out, exist_ok=True)
    frame = Frame(lat0, lon0, h0)
    t0 = time.time()
    seen = set()
    chunks = []
    for folder in folders:
        for f in sorted(glob.glob(os.path.join(folder, "*.npy")) + glob.glob(os.path.join(folder, "*.gml"))):
            base = os.path.splitext(f)[0]
            if base in seen:
                continue  # the .gml beside its .npy: once (the wards' packages hold different pieces of a mesh of the same name: each is read)
            seen.add(base)
            if f.endswith(".npy"):
                chunks.append(np.load(f))  # plateau_dem_pts.py: already in the frame, geoid added
            else:
                v = read_vertices(f)
                chunks.append(to_local(frame, v[:, 0], v[:, 1], v[:, 2] + geoid).astype(np.float32))
            print(base, len(chunks[-1]), "vertices", f"{time.time() - t0:.0f} s", flush=True)
    pts = np.concatenate(chunks)
    del chunks
    print(len(pts), "points: x", float(pts[:, 0].min()), float(pts[:, 0].max()), "z", float(pts[:, 2].min()), float(pts[:, 2].max()), flush=True)
    x, y, z = pts[:, 0], pts[:, 1], pts[:, 2]
    ci, cj = np.round(x / 1000.0).astype(np.int32), np.round(z / 1000.0).astype(np.int32)
    cells = []
    n = 1001
    margin = 80.0
    keys = sorted(set(zip(ci.tolist(), cj.tolist())))
    for (i, j) in keys:
        cx, cz = i * 1000.0, j * 1000.0
        sel = (x >= cx - 500 - margin) & (x <= cx + 500 + margin) & (z >= cz - 500 - margin) & (z <= cz + 500 + margin)
        if sel.sum() < 50:
            continue
        interp = LinearNDInterpolator(np.column_stack([x[sel], z[sel]]), y[sel])
        gx, gz = np.meshgrid(cx - 500 + np.arange(n), cz - 500 + np.arange(n))
        h = interp(gx, gz)
        ok = np.isfinite(h)
        if ok.mean() < 0.02:
            continue
        if not ok.all():
            idx = distance_transform_edt(~ok, return_distances=False, return_indices=True)
            h = h[tuple(idx)]
        lo_h, hi_h = float(h.min()), float(h.max())
        y0 = math.floor(lo_h) - 0.5
        relief = max(math.ceil(hi_h - y0) + 0.5, 1.0)
        d = os.path.join(out, f"cell_{i}_{j}")
        os.makedirs(d, exist_ok=True)
        Image.fromarray(np.clip((h - y0) / relief * 65535.0 + 0.5, 0, 65535).astype("<u2")).save(os.path.join(d, "heightmap.png"))
        cells.append({"i": i, "j": j, "cx": cx, "cz": cz, "y0": round(y0, 2), "relief": round(relief, 2), "known": round(float(ok.mean()), 3)})
        if len(cells) % 25 == 0:
            print(f"  {len(cells)} cells, {time.time() - t0:.0f} s", flush=True)
    json.dump({"origin": {"lat": lat0, "lon": lon0, "h": h0, "geoid": geoid}, "cells": cells}, open(os.path.join(out, "cells.json"), "w"), separators=(",", ":"))
    print(len(cells), "cells", f"{time.time() - t0:.0f} s")


if __name__ == "__main__":
    main()
