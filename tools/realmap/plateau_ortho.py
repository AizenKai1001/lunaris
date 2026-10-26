r"""Aerial pictures of the ground cells made by plateau_dem.py, from PLATEAU's orthophotos (GeoTIFF, JGD2011 longitude/latitude, about 0.5 m a pixel).

    python plateau_ortho.py DEM_FOLDER TIF [TIF ...]

For every `DEM_FOLDER/cell_<i>_<j>/` the picture of the cell's square kilometre (north up, as the heightmap) as terrain.jpg (2048 px), terrain_l1.jpg (1024) and terrain_l2.jpg (512). The TIFs are the sheets
that cover the area (they may meet in the middle of a cell). The data is (c) MLIT Japan, Project PLATEAU (processed here: cut and resampled).
"""
import json
import os
import sys
import time

import numpy as np
import rasterio
from PIL import Image
from pyproj import Transformer
from scipy.ndimage import map_coordinates

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from plateau_tiles import Frame  # noqa: E402

PX = 2048


def main():
    folder = sys.argv[1]
    tifs = [rasterio.open(p) for p in sys.argv[2:]]
    meta = json.load(open(os.path.join(folder, "cells.json")))
    o = meta["origin"]
    frame = Frame(o["lat"], o["lon"], o["h"])
    to_geo = Transformer.from_crs("EPSG:4978", "EPSG:4326", always_xy=True)
    res_lon, res_lat = tifs[0].res
    t0 = time.time()
    for c in meta["cells"]:
        d = os.path.join(folder, f"cell_{c['i']}_{c['j']}")
        if os.path.exists(os.path.join(d, "terrain.jpg")):
            continue
        # where the cell's picture pixels are in longitude and latitude: a coarse grid of places, then bilinear between them (the map is smooth)
        m = 17
        gx = c["cx"] - 500 + np.linspace(0, 1000, m)
        gz = c["cz"] - 500 + np.linspace(0, 1000, m)
        xx, zz = np.meshgrid(gx, gz)
        enu = np.stack([xx.ravel(), -zz.ravel(), np.zeros(xx.size)], axis=1)
        ecef = frame.origin + enu @ frame.r
        lon, lat, _ = to_geo.transform(ecef[:, 0], ecef[:, 1], ecef[:, 2])
        lon, lat = np.array(lon).reshape(m, m), np.array(lat).reshape(m, m)
        # the pixel centres of a PX x PX picture of the cell in 0..m-1 coordinates of that grid
        u = (np.arange(PX) + 0.5) / PX * (m - 1)
        uu, vv = np.meshgrid(u, u)
        lon_p = map_coordinates(lon, [vv, uu], order=1)
        lat_p = map_coordinates(lat, [vv, uu], order=1)
        lat_hi, lat_lo, lon_lo, lon_hi = lat_p.max() + 4 * res_lat, lat_p.min() - 4 * res_lat, lon_p.min() - 4 * res_lon, lon_p.max() + 4 * res_lon
        # the pixels of the sheets that cover it, stacked from north to south into one array (each sheet is a grid of the same size and step)
        pieces = []
        for ds in tifs:
            b = ds.bounds
            if lat_lo >= b.top or lat_hi <= b.bottom or lon_hi <= b.left or lon_lo >= b.right:
                continue
            r0 = max(int((b.top - min(lat_hi, b.top)) / res_lat), 0)
            r1 = min(int(np.ceil((b.top - max(lat_lo, b.bottom)) / res_lat)), ds.height)
            c0 = max(int((max(lon_lo, b.left) - b.left) / res_lon), 0)
            c1 = min(int(np.ceil((min(lon_hi, b.right) - b.left) / res_lon)), ds.width)
            if r1 <= r0 or c1 <= c0:
                continue
            arr = ds.read(window=rasterio.windows.Window(c0, r0, c1 - c0, r1 - r0))
            pieces.append((b.top - r0 * res_lat, b.left + c0 * res_lon, arr))
        if not pieces:
            continue
        # the pieces side by side and one over the other, each at its place on the common canvas (sheets meet along rows and along columns)
        top = max(p[0] for p in pieces)
        left = min(p[1] for p in pieces)
        bottom = min(p[0] - p[2].shape[1] * res_lat for p in pieces)
        right = max(p[1] + p[2].shape[2] * res_lon for p in pieces)
        height, width = int(round((top - bottom) / res_lat)), int(round((right - left) / res_lon))
        canvas = np.zeros((3, height, width), dtype=np.uint8)
        for pt, pl, arr in pieces:
            r0, c0 = int(round((top - pt) / res_lat)), int(round((pl - left) / res_lon))
            canvas[:, r0:r0 + arr.shape[1], c0:c0 + arr.shape[2]] = arr[:, :height - r0, :width - c0]
        rows = (top - lat_p) / res_lat - 0.5
        cols = (lon_p - left) / res_lon - 0.5
        out = np.stack([map_coordinates(canvas[k].astype(np.float32), [rows, cols], order=1, mode="nearest") for k in range(3)], axis=2)
        im = Image.fromarray(np.clip(out + 0.5, 0, 255).astype(np.uint8))
        im.save(os.path.join(d, "terrain.jpg"), quality=92)
        im.resize((1024, 1024), Image.LANCZOS).save(os.path.join(d, "terrain_l1.jpg"), quality=92)
        im.resize((512, 512), Image.LANCZOS).save(os.path.join(d, "terrain_l2.jpg"), quality=92)
        print(f"cell {c['i']},{c['j']}: {len(pieces)} sheets, {time.time() - t0:.0f} s", flush=True)


if __name__ == "__main__":
    main()
