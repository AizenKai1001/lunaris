r"""The ground of a PLATEAU city: the CityGML relief (TIN, `udx/dem/*.gml`) as 16-bit height pictures in cells of 1 km, in the frame of plateau_tiles.py (x east, y up, z south, origin at LAT0 LON0 H0).

    python plateau_dem.py DEM_GML_FOLDER OUT_FOLDER LAT0 LON0 [H0 [GEOID]]

The relief's heights are above the geoid (JGD2011, EPSG:6697: metres above sea level); the 3D Tiles are above the ellipsoid, so GEOID (default 36.7 m, the geoid's height over the ellipsoid in Tokyo) is added: a point of the
ground is at y = height + GEOID - H0. The cells are centred on whole kilometres from the origin: OUT_FOLDER/cell_<i>_<j>/heightmap.png (1001 x 1001 samples a kilometre, 1 m apart; row 0 is the north-most one... as the engine
reads it: rows run towards +z, the south) and OUT_FOLDER/cells.json with each cell's centre, lowest ground (y0) and relief (the height range the picture's 0..65535 stands for).
The data is (c) MLIT Japan, Project PLATEAU (processed here: triangles resampled to a grid).
"""
import glob
import json
import math
import os
import re
import sys
import time

import numpy as np
from PIL import Image
from scipy.interpolate import LinearNDInterpolator

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from plateau_tiles import Frame  # noqa: E402

POS = re.compile(rb"<gml:posList>([^<]+)</gml:posList>")


def read_vertices(path):
    """The corners of every triangle of a CityGML relief file: an array of (lat, lon, height), repeats removed."""
    out = []
    tail = b""
    with open(path, "rb") as fh:
        while True:
            chunk = fh.read(128 << 20)
            if not chunk:
                break
            buf = tail + chunk
            last = buf.rfind(b"</gml:posList>")
            if last < 0:
                tail = buf
                continue
            body, tail = buf[:last + len(b"</gml:posList>")], buf[last + len(b"</gml:posList>"):]
            texts = POS.findall(body)
            if texts:
                nums = np.fromstring(b" ".join(texts).decode("ascii"), dtype=np.float64, sep=" ")
                out.append(nums.reshape(-1, 3))
    pts = np.concatenate(out)
    key = np.round(pts[:, 0] * 1e9).astype(np.int64) * 4_000_000_000 + np.round(pts[:, 1] * 1e9).astype(np.int64) % 4_000_000_000
    _, first = np.unique(key, return_index=True)
    return pts[first]


def main():
    folder, out = sys.argv[1], sys.argv[2]
    lat0, lon0 = float(sys.argv[3]), float(sys.argv[4])
    h0 = float(sys.argv[5]) if len(sys.argv) > 5 else 0.0
    geoid = float(sys.argv[6]) if len(sys.argv) > 6 else 36.7
    os.makedirs(out, exist_ok=True)
    t0 = time.time()
    frame = Frame(lat0, lon0, h0)
    parts = []
    for f in sorted(glob.glob(os.path.join(folder, "*.gml"))):
        v = read_vertices(f)
        print(os.path.basename(f), len(v), "vertices", f"{time.time() - t0:.0f} s", flush=True)
        parts.append(v)
    pts = np.concatenate(parts)
    # (the same corner in two files: once)
    key = np.round(pts[:, 0] * 1e8).astype(np.int64) * 10_000_000_000 + np.round(pts[:, 1] * 1e8).astype(np.int64) % 10_000_000_000
    _, first = np.unique(key, return_index=True)
    pts = pts[first]
    from plateau_tiles import geodetic_to_ecef

    # horizontal position: the ellipsoid surface point is close enough (the height changes a point's place by a centimetre over kilometres)
    ecef = np.array([geodetic_to_ecef(la, lo, 0.0) for la, lo in pts[::97, :2]])  # a sample, to pick the affine fit below
    # a plain affine map from (lat, lon) to (x, z) fitted on the sample is exact enough over a few kilometres; the fit residual is reported
    local = frame.to_local(ecef)
    A = np.column_stack([pts[::97, 0] - lat0, pts[::97, 1] - lon0, (pts[::97, 0] - lat0) ** 2, (pts[::97, 1] - lon0) ** 2, (pts[::97, 0] - lat0) * (pts[::97, 1] - lon0), np.ones(len(local))])
    cx_, *_ = np.linalg.lstsq(A, local[:, 0], rcond=None)
    cz_, *_ = np.linalg.lstsq(A, local[:, 2], rcond=None)
    print("fit residual x", float(np.abs(A @ cx_ - local[:, 0]).max()), "z", float(np.abs(A @ cz_ - local[:, 2]).max()), "m", flush=True)
    la, lo = pts[:, 0] - lat0, pts[:, 1] - lon0
    B = np.column_stack([la, lo, la * la, lo * lo, la * lo, np.ones(len(pts))])
    x, z = B @ cx_, B @ cz_
    y = pts[:, 2] + geoid - h0
    print(f"{len(pts)} vertices: x {x.min():.0f}..{x.max():.0f}, z {z.min():.0f}..{z.max():.0f}, y {y.min():.1f}..{y.max():.1f}", flush=True)
    interp = LinearNDInterpolator(np.column_stack([x, z]), y)
    print(f"triangulated {time.time() - t0:.0f} s", flush=True)
    cells = []
    ki0, ki1 = math.floor((x.min() + 500) / 1000), math.floor((x.max() + 500) / 1000)
    kj0, kj1 = math.floor((z.min() + 500) / 1000), math.floor((z.max() + 500) / 1000)
    n = 1001
    for kj in range(kj0, kj1 + 1):
        for ki in range(ki0, ki1 + 1):
            cx, cz = ki * 1000.0, kj * 1000.0
            gx = cx - 500 + np.arange(n)
            gz = cz - 500 + np.arange(n)
            xx, zz = np.meshgrid(gx, gz)
            h = interp(xx, zz)
            ok = np.isfinite(h)
            if ok.mean() < 0.02:
                continue
            # holes (outside the relief): the nearest known ground, so that a cell at the edge has no cliff
            if not ok.all():
                from scipy.ndimage import distance_transform_edt

                idx = distance_transform_edt(~ok, return_distances=False, return_indices=True)
                h = h[tuple(idx)]
            lo_h, hi_h = float(h.min()), float(h.max())
            y0 = math.floor(lo_h) - 0.5
            relief = max(math.ceil(hi_h - y0) + 0.5, 1.0)
            d = os.path.join(out, f"cell_{ki}_{kj}")
            os.makedirs(d, exist_ok=True)
            Image.fromarray(np.clip((h - y0) / relief * 65535.0 + 0.5, 0, 65535).astype("<u2")).save(os.path.join(d, "heightmap.png"))
            cells.append({"i": ki, "j": kj, "cx": cx, "cz": cz, "y0": round(y0, 2), "relief": round(relief, 2), "known": round(float(ok.mean()), 3)})
            print(f"  cell {ki},{kj}: y {lo_h:.1f}..{hi_h:.1f}, known {ok.mean():.2f}, {time.time() - t0:.0f} s", flush=True)
    json.dump({"origin": {"lat": lat0, "lon": lon0, "h": h0, "geoid": geoid}, "cells": cells}, open(os.path.join(out, "cells.json"), "w"), separators=(",", ":"))
    print(len(cells), "cells", f"{time.time() - t0:.0f} s")


if __name__ == "__main__":
    main()
