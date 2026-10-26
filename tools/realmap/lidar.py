"""The real laser survey of a place: USGS 3DEP lidar point clouds (public domain), read from the Entwine Point Tile copy on AWS (s3://usgs-lidar-public, no account needed).

    python tools/realmap/lidar.py NAME PROJECT [HALF_M=500]

PROJECT is the name of an EPT project that covers the place (the list: https://raw.githubusercontent.com/hobuinc/usgs-lidar/master/boundaries/resources.geojson). The points inside a
square of 2 * HALF_M metres round the window's centre are downloaded (every node of the octree down to the leaves), turned from Web Mercator into the window's own UTM frame (x east,
z south, metres) and written to realmap-data\\NAME\\lidar.npz with their elevation, class (2 ground, 5 high vegetation, 6 building, 9 water ...) and return numbers.
Needs laspy with the lazrs backend (the venv realmap-venv has them).
"""
import json
import math
import os
import sys
import time
import urllib.request

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))
import geo

ROOT = r"realmap-data"
UA = {"User-Agent": "lunaris-realmap/0.1 (game-engine feasibility)"}
BUCKET = "https://usgs-lidar-public.s3.amazonaws.com"
R = 6378137.0


def fetch(url, retries=4):
    for k in range(retries):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=120) as r:
                return r.read()
        except Exception as e:  # noqa: BLE001
            print("  retry", k, str(e)[:80])
            time.sleep(2 + 3 * k)
    raise RuntimeError("download failed: " + url)


def merc_of(lat, lon):
    return math.radians(lon) * R, math.log(math.tan(math.pi / 4 + math.radians(lat) / 2)) * R


def utm_forward_np(lat_deg, lon_deg, zone):
    """geo.forward for arrays."""
    lon0 = math.radians((zone - 1) * 6 - 180 + 3)
    phi, lam = np.radians(lat_deg), np.radians(lon_deg) - lon0
    t = np.sinh(np.arctanh(np.sin(phi)) - geo.E * np.arctanh(geo.E * np.sin(phi)))
    xi = np.arctan2(t, np.cos(lam))
    eta = np.arctanh(np.sin(lam) / np.sqrt(1 + t * t))
    x = eta + sum(geo.ALPHA[j] * np.cos(2 * (j + 1) * xi) * np.sinh(2 * (j + 1) * eta) for j in range(3))
    y = xi + sum(geo.ALPHA[j] * np.sin(2 * (j + 1) * xi) * np.cosh(2 * (j + 1) * eta) for j in range(3))
    return 500000.0 + geo.K0 * geo.A_HAT * x, geo.K0 * geo.A_HAT * y


def main():
    import laspy

    name, project = sys.argv[1], sys.argv[2]
    half = float(sys.argv[3]) if len(sys.argv) > 3 else 500.0
    d = os.path.join(ROOT, name)
    meta = json.load(open(os.path.join(d, "meta.json")))
    cx, cy = meta["x0"] + meta["size_m"] / 2, meta["y0"] + meta["size_m"] / 2
    zone = meta["zone"]
    cache = os.path.join(d, "lidar_cache")
    os.makedirs(cache, exist_ok=True)
    ept = json.loads(fetch(f"{BUCKET}/{project}/ept.json"))
    bx0, by0, bz0, bx1, by1, bz1 = ept["bounds"]
    edge = bx1 - bx0
    # the square in Web Mercator that contains the wanted UTM square (a little margin)
    ll = [geo.inverse(cx + dx, cy + dy, zone) for dx in (-half - 30, half + 30) for dy in (-half - 30, half + 30)]
    mm = [merc_of(a, b) for a, b in ll]
    wx0, wx1 = min(m[0] for m in mm), max(m[0] for m in mm)
    wy0, wy1 = min(m[1] for m in mm), max(m[1] for m in mm)
    print(f"{project}: {ept['points']:,} points in the project; window in Web Mercator x {wx0:.0f}..{wx1:.0f} y {wy0:.0f}..{wy1:.0f}")
    # the hierarchy: key -> point count (-1: the subtree is in its own file)
    hier = {}

    def load_hier(key):
        data = json.loads(fetch(f"{BUCKET}/{project}/ept-hierarchy/{key}.json"))
        for k, v in data.items():
            if v == -1:
                load_hier(k)
            else:
                hier[k] = v

    def overlaps(key):
        dpt, x, y, z = (int(v) for v in key.split("-"))
        s = edge / (1 << dpt)
        nx0, ny0 = bx0 + x * s, by0 + y * s
        return nx0 < wx1 and nx0 + s > wx0 and ny0 < wy1 and ny0 + s > wy0

    # walk the tree from the root, only into nodes that touch the window (the hierarchy files are loaded when needed)
    wanted = []
    stack = ["0-0-0-0"]
    pending = {}
    root = json.loads(fetch(f"{BUCKET}/{project}/ept-hierarchy/0-0-0-0.json"))
    pending.update(root)
    seen = set()
    while stack:
        key = stack.pop()
        if key in seen:
            continue
        seen.add(key)
        if not overlaps(key):
            continue
        cnt = pending.get(key)
        if cnt is None:
            continue
        if cnt == -1:
            sub = json.loads(fetch(f"{BUCKET}/{project}/ept-hierarchy/{key}.json"))
            pending.update(sub)
            stack.append(key)
            seen.discard(key)
            pending[key] = sub.get(key, 0)
            continue
        if cnt > 0:
            wanted.append((key, cnt))
        dpt, x, y, z = (int(v) for v in key.split("-"))
        for dx in (0, 1):
            for dy in (0, 1):
                for dz in (0, 1):
                    stack.append(f"{dpt + 1}-{2 * x + dx}-{2 * y + dy}-{2 * z + dz}")
    total = sum(c for _, c in wanted)
    print(f"  {len(wanted)} nodes, {total:,} points to read")
    xs, ys, zs, cl, rn, nr, it = [], [], [], [], [], [], []
    done = 0
    for key, cnt in wanted:
        path = os.path.join(cache, key + ".laz")
        if not os.path.exists(path):
            open(path, "wb").write(fetch(f"{BUCKET}/{project}/ept-data/{key}.laz"))
        las = laspy.read(path)
        X, Y, Z = np.asarray(las.x), np.asarray(las.y), np.asarray(las.z)
        m = (X >= wx0) & (X <= wx1) & (Y >= wy0) & (Y <= wy1)
        if m.any():
            xs.append(X[m]); ys.append(Y[m]); zs.append(Z[m])
            cl.append(np.asarray(las.classification)[m]); rn.append(np.asarray(las.return_number)[m]); nr.append(np.asarray(las.number_of_returns)[m]); it.append(np.asarray(las.intensity)[m])
        done += 1
        if done % 25 == 0:
            print(f"  {done}/{len(wanted)} nodes", flush=True)
    X, Y, Z = np.concatenate(xs), np.concatenate(ys), np.concatenate(zs)
    lon = np.degrees(X / R)
    lat = np.degrees(np.arctan(np.sinh(Y / R)))
    e, n = utm_forward_np(lat, lon, zone)
    lx, lz = e - cx, -(n - cy)
    keep = (np.abs(lx) <= half) & (np.abs(lz) <= half)
    out = dict(x=lx[keep].astype(np.float32), z=lz[keep].astype(np.float32), y=Z[keep].astype(np.float32), cls=np.concatenate(cl)[keep].astype(np.uint8), ret=np.concatenate(rn)[keep].astype(np.uint8), nret=np.concatenate(nr)[keep].astype(np.uint8), inten=np.concatenate(it)[keep].astype(np.uint16))
    np.savez_compressed(os.path.join(d, "lidar.npz"), **out)
    u, c = np.unique(out["cls"], return_counts=True)
    area = (2 * half) ** 2
    print(f"  {int(keep.sum()):,} points in {area / 1e6:.2f} km2 = {keep.sum() / area:.1f} points a square metre; classes {dict(zip(u.tolist(), c.tolist()))}")
    print(f"  elevation {out['y'].min():.1f} to {out['y'].max():.1f} m")


if __name__ == "__main__":
    main()
