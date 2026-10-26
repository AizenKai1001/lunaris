"""Buildings with their REAL roofs and trees with their REAL heights, measured by the USGS laser survey (`lidar.py`), on the real footprints of OpenStreetMap:

    python tools/realmap/build_lidar.py NAME

  lidar_buildings.glb   every footprint inside the lidar square: walls from the ground up to the surveyed roof edge and a roof surface that is the surveyed one (a surface model of the
                        building-class returns, sampled every 1.5 m, so gables, hips and dormers are as they are); roof colour per vertex from the aerial photograph
  trees.json            tree tops found in the canopy height model (high vegetation returns above the bare ground): position, height, crown radius
  lidar_info.json       counts and the classes the survey carries
"""
import json
import math
import os
import sys

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, os.path.dirname(__file__))
import build as B  # noqa: E402

ROOT = B.ROOT
CELL = 0.5


def raster_max(x, z, y, half, cell):
    w = int(2 * half / cell)
    ix = np.clip(((x + half) / cell).astype(np.int64), 0, w - 1)
    iz = np.clip(((z + half) / cell).astype(np.int64), 0, w - 1)
    flat = iz * w + ix
    top = np.full(w * w, -np.inf, dtype=np.float32)
    np.maximum.at(top, flat, y)
    # the mean of the returns within 0.35 m of the top (one stray high return does not make a spike)
    near = y >= top[flat] - 0.35
    s = np.bincount(flat[near], weights=y[near].astype(np.float64), minlength=w * w)
    c = np.bincount(flat[near], minlength=w * w)
    out = np.where(c > 0, s / np.maximum(c, 1), np.nan).astype(np.float32)
    return out.reshape(w, w)


def raster_min(x, z, y, half, cell):
    """The lowest return in each cell: with the canopy over a roof, the roof is the lowest thing above the ground."""
    w = int(2 * half / cell)
    ix = np.clip(((x + half) / cell).astype(np.int64), 0, w - 1)
    iz = np.clip(((z + half) / cell).astype(np.int64), 0, w - 1)
    flat = iz * w + ix
    low = np.full(w * w, np.inf, dtype=np.float32)
    np.minimum.at(low, flat, y)
    # the mean of the returns within 0.3 m of the lowest
    near = y <= low[flat] + 0.3
    s = np.bincount(flat[near], weights=y[near].astype(np.float64), minlength=w * w)
    c = np.bincount(flat[near], minlength=w * w)
    out = np.where(c > 0, s / np.maximum(c, 1), np.nan).astype(np.float32)
    return out.reshape(w, w)


def fill_nan(a, iters=12):
    a = a.copy()
    for _ in range(iters):
        bad = np.isnan(a)
        if not bad.any():
            break
        p = np.pad(a, 1, mode="edge")
        nb = np.stack([p[0:-2, 1:-1], p[2:, 1:-1], p[1:-1, 0:-2], p[1:-1, 2:], p[0:-2, 0:-2], p[0:-2, 2:], p[2:, 0:-2], p[2:, 2:]])
        cnt = (~np.isnan(nb)).sum(axis=0)
        s = np.nansum(nb, axis=0)
        fill = np.where(cnt > 0, s / np.maximum(cnt, 1), np.nan)
        a[bad] = fill[bad]
    return a


def fit_planes(pts, rng, max_planes=5):
    """Planes z = a x + b y + c through a cloud (x, z, height columns), by RANSAC then least squares, the points of each taken away before the next: a roof is a few planes, a tree crown over it
    is scattered points that belong to none. Returns [(a, b, c, inlier_count)]."""
    planes = []
    left = pts.copy()
    n0 = len(pts)
    for _ in range(max_planes):
        if len(left) < max(14, 0.08 * n0):
            break
        best, best_n = None, 0
        m = len(left)
        sub = left if m <= 3000 else left[rng.choice(m, 3000, replace=False)]
        for _it in range(160):
            i = rng.choice(len(sub), 3, replace=False)
            P = sub[i]
            A = np.column_stack([P[:, 0], P[:, 1], np.ones(3)])
            try:
                a, b, c = np.linalg.solve(A, P[:, 2])
            except np.linalg.LinAlgError:
                continue
            if math.hypot(a, b) > 1.6:  # steeper than about 58 degrees is no roof
                continue
            res = np.abs(sub[:, 0] * a + sub[:, 1] * b + c - sub[:, 2])
            cnt = int((res < 0.22).sum())
            if cnt > best_n:
                best, best_n = (a, b, c), cnt
        if best is None or best_n < max(12, 0.06 * len(sub)):
            break
        a, b, c = best
        res = np.abs(left[:, 0] * a + left[:, 1] * b + c - left[:, 2])
        inl = res < 0.25
        A = np.column_stack([left[inl, 0], left[inl, 1], np.ones(int(inl.sum()))])
        sol, *_ = np.linalg.lstsq(A, left[inl, 2], rcond=None)
        a, b, c = sol
        res = np.abs(left[:, 0] * a + left[:, 1] * b + c - left[:, 2])
        inl = res < 0.25
        planes.append((float(a), float(b), float(c), int(inl.sum())))
        left = left[~inl]
    return planes


def subdivide(poly3, maxlen):
    """Triangles (as 2D point triples) split at their longest edge's midpoint until every edge is short."""
    out = []
    stack = [poly3]
    while stack:
        t = stack.pop()
        e = [math.hypot(t[(i + 1) % 3][0] - t[i][0], t[(i + 1) % 3][1] - t[i][1]) for i in range(3)]
        k = max(range(3), key=lambda i: e[i])
        if e[k] <= maxlen:
            out.append(t)
            continue
        a, b, c = t[k], t[(k + 1) % 3], t[(k + 2) % 3]
        m = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
        stack.append((a, m, c))
        stack.append((m, b, c))
    return out


def main():
    name = sys.argv[1]
    d, meta = B.load(name)
    info = json.load(open(os.path.join(d, "scene", "info.json")))
    frame = B.Frame(meta)
    size = int(meta["size_m"])
    lo = info["lo"]
    L = np.load(os.path.join(d, "lidar.npz"))
    x, z, y, cls = L["x"], L["z"], L["y"], L["cls"]
    half = float(np.ceil(max(np.abs(x).max(), np.abs(z).max())))
    half = float(int(half // 10 * 10))
    print(f"lidar: {len(x):,} points, square {2 * half:.0f} m, classes {dict(zip(*[v.tolist() for v in np.unique(cls, return_counts=True)]))}")
    # ground: the engine's own (what the buildings stand on)
    dem = B.read_dem(d, meta)
    n = size + 1
    pad = np.pad(dem, 1, mode="edge")
    corner = (0.25 * (pad[0:n, 0:n] + pad[0:n, 1 : n + 1] + pad[1 : n + 1, 0:n] + pad[1 : n + 1, 1 : n + 1])).astype(np.float32)
    ground = B.Ground(corner - lo, float(size), 2)
    print("surface models")
    # this survey is classified as ground (2) and everything else (1) only: roofs and canopy are told apart below by the real footprints
    nonground = (cls == 1) | (cls == 6) | (cls == 3) | (cls == 4) | (cls == 5)
    dsm_v = raster_max(x[nonground], z[nonground], y[nonground], half, CELL) - lo
    wdim = dsm_v.shape[0]
    # roofs: the lowest return that is at least 2.2 m above the bare ground (tree crowns over a roof lie above it; cars and people lie below)
    gi_x = np.clip(np.round((x + frame.half) / ground.cell).astype(np.int64), 0, ground.m - 1)
    gi_z = np.clip(np.round((z + frame.half) / ground.cell).astype(np.int64), 0, ground.m - 1)
    above = (y - lo) - ground.h[gi_z, gi_x]
    roofish = nonground & (above >= 2.2)
    dsm_b = raster_min(x[roofish], z[roofish], y[roofish], half, CELL) - lo
    print(f"  above-ground returns {int(nonground.sum()):,}")
    nodes, ways, rels, loc = B.read_osm(d, meta, frame)
    aerial = B.read_imagery(d, meta)
    scale = aerial.width / size
    aerial_np = np.asarray(aerial)
    # footprints inside the lidar square
    prints = []
    for wid, w in ways.items():
        t = w[1]
        if "building" in t and t["building"] != "no" and w[0][0] == w[0][-1]:
            prints.append((t, [loc[k] for k in w[0] if k in loc]))
    for rid, r in rels.items():
        if "building" in r[1] and r[1].get("type") == "multipolygon":
            for ring in B.rings_of(r, ways, loc):
                prints.append((r[1], ring))
    mesh = B.MeshOut()
    stats = {"surveyed": 0, "no building returns": 0, "outside": 0}
    for tags, poly in prints:
        poly = B.dedupe(poly)
        if len(poly) < 3:
            continue
        cxm = sum(p[0] for p in poly) / len(poly)
        czm = sum(p[1] for p in poly) / len(poly)
        if max(abs(p[0]) for p in poly) > half - 3 or max(abs(p[1]) for p in poly) > half - 3:
            stats["outside"] += 1
            continue
        if B.signed_area(poly) < 0:
            poly = poly[::-1]
        # the surveyed roof inside this footprint
        xs = [p[0] for p in poly]
        zs = [p[1] for p in poly]
        c0, c1 = int((min(xs) + half) / CELL) - 4, int((max(xs) + half) / CELL) + 5
        r0, r1 = int((min(zs) + half) / CELL) - 4, int((max(zs) + half) / CELL) + 5
        c0, r0, c1, r1 = max(c0, 0), max(r0, 0), min(c1, wdim), min(r1, wdim)
        crop = dsm_b[r0:r1, c0:c1]
        msk = Image.new("L", (c1 - c0, r1 - r0), 0)
        ImageDraw.Draw(msk).polygon([((p[0] + half) / CELL - c0, (p[1] + half) / CELL - r0) for p in poly], fill=255)
        inside = np.array(msk) > 0
        have = inside & ~np.isnan(crop)
        if inside.sum() == 0 or have.sum() < 0.25 * inside.sum() or have.sum() < 12:
            stats["no building returns"] += 1
            continue
        # the roof planes of this footprint (cell centres in the footprint's own frame, heights from the survey)
        rr, cc = np.nonzero(have)
        px_ = (cc + c0 + 0.5) * CELL - half - cxm
        pz_ = (rr + r0 + 0.5) * CELL - half - czm
        pts = np.column_stack([px_, pz_, crop[rr, cc]]).astype(np.float64)
        rng = np.random.default_rng(int(abs(cxm) * 1000 + abs(czm) * 7))
        planes = fit_planes(pts, rng)
        if not planes:
            stats["no building returns"] += 1
            continue
        # which plane each cell belongs to (the nearest in height), the cells that fit none are outliers (tree crowns, antennas)
        pid = np.full(crop.shape, -1, dtype=np.int32)
        hgt = np.stack([px_ * a + pz_ * b + c for a, b, c, _ in planes])
        resid = np.abs(hgt - pts[:, 2][None, :])
        near = resid.argmin(axis=0)
        ok = resid[near, np.arange(len(near))] < 0.3
        pid[rr[ok], cc[ok]] = near[ok]
        roof_min = float(min(h for h in [min(pl[2], 1e9) for pl in planes]))

        def roof_at(px, pz):
            ci = min(max(int((px + half) / CELL) - c0, 0), crop.shape[1] - 1)
            ri = min(max(int((pz + half) / CELL) - r0, 0), crop.shape[0] - 1)
            k = pid[ri, ci]
            if k < 0:
                # the nearest cell that has a plane
                found = -1
                for rad in range(1, 9):
                    for dr in range(-rad, rad + 1):
                        for dc in range(-rad, rad + 1):
                            if max(abs(dr), abs(dc)) != rad:
                                continue
                            r2, c2 = ri + dr, ci + dc
                            if 0 <= r2 < pid.shape[0] and 0 <= c2 < pid.shape[1] and pid[r2, c2] >= 0:
                                found = int(pid[r2, c2])
                                break
                        if found >= 0:
                            break
                    if found >= 0:
                        break
                k = found if found >= 0 else 0
            a, b, c, _ = planes[k]
            return a * (px - cxm) + b * (pz - czm) + c

        # a roof must stand clear of the ground
        gmin = min(ground.height(p[0], p[1]) for p in poly)
        if float(np.nanmedian(crop[have])) - gmin < 2.0:
            stats["no building returns"] += 1
            continue
        wall = tuple(B.hex_color(tags.get("building:colour")) or (0.76, 0.74, 0.70))

        def aerial_rgb(px, pz):
            ci = min(max(int((px + half_frame) * scale), 0), aerial_np.shape[1] - 1)
            ri = min(max(int((pz + half_frame) * scale), 0), aerial_np.shape[0] - 1)
            v = aerial_np[ri, ci].astype(np.float32) / 255.0
            return tuple(np.power(v, 2.2).tolist())

        half_frame = frame.half
        # the boundary, cut into pieces of at most 1.5 m, each with the surveyed roof height
        ring = []
        for i in range(len(poly)):
            a, b = poly[i], poly[(i + 1) % len(poly)]
            ln = math.hypot(b[0] - a[0], b[1] - a[1])
            k = max(1, int(math.ceil(ln / 1.5)))
            for s in range(k):
                ring.append((a[0] + (b[0] - a[0]) * s / k, a[1] + (b[1] - a[1]) * s / k))
        top = [roof_at(p[0], p[1]) for p in ring]
        for i in range(len(ring)):
            a, b = ring[i], ring[(i + 1) % len(ring)]
            ga, gb = ground.height(a[0], a[1]) - 0.3, ground.height(b[0], b[1]) - 0.3
            # the wall top is the roof's edge, a little under the surveyed top (the eave)
            ta, tb = top[i] - 0.05, top[(i + 1) % len(ring)] - 0.05
            mesh.quad([a[0], ga, a[1]], [b[0], gb, b[1]], [b[0], tb, b[1]], [a[0], ta, a[1]], wall)
        # the roof: the footprint triangulated, cut small, every vertex on the surveyed surface
        for ta, tb, tc in B.ear_clip(poly):
            for tri in subdivide((poly[ta], poly[tb], poly[tc]), 1.5):
                v = []
                for p in tri:
                    v.append([p[0], roof_at(p[0], p[1]), p[1]])
                cx_, cz_ = sum(p[0] for p in tri) / 3, sum(p[1] for p in tri) / 3
                rgb = aerial_rgb(cx_, cz_)
                rgb = tuple(max(0.05, min(0.9, c)) for c in rgb)
                mesh.tri(v[0], v[2], v[1], rgb)
        stats["surveyed"] += 1
    mesh.write_glb(os.path.join(d, "scene", "lidar_buildings.glb"))
    print("  buildings with a surveyed roof:", stats, f"{len(mesh.idx) // 3} triangles")
    # ---- trees from the canopy height model
    gs0 = int(2 * half / CELL)
    bimg = Image.new("L", (gs0, gs0), 0)
    bdraw = ImageDraw.Draw(bimg)
    for tags, poly in prints:
        bdraw.polygon([((p_[0] + half) / CELL, (p_[1] + half) / CELL) for p_ in poly], fill=255)
    from PIL import ImageFilter
    bmask = np.array(bimg.filter(ImageFilter.MaxFilter(7))) > 0
    dtm = np.zeros((wdim, wdim), dtype=np.float32)
    gs = int(2 * half / CELL)
    xs_ = (np.arange(gs) + 0.5) * CELL - half
    # ground under each cell from the engine's own ground (every 2 m is plenty; nearest sample)
    gi = np.clip(((xs_ + frame.half) / ground.cell).astype(int), 0, ground.m - 1)
    dtm = ground.h[np.ix_(gi, gi)].astype(np.float32)  # [z, x]
    chm = dsm_v - dtm
    chm = np.where(np.isnan(chm), 0.0, chm)
    chm[bmask] = 0.0
    chm[chm < 0] = 0
    # one metre cells for the tree tops
    k = int(1.0 / CELL)
    h1 = chm.reshape(gs // k, k, gs // k, k).max(axis=(1, 3))
    p = np.pad(h1, 1, mode="edge")
    # smooth (a 3x3 mean) so that one branch is not one tree
    sm = sum(p[1 + dy : 1 + dy + h1.shape[0], 1 + dx : 1 + dx + h1.shape[1]] for dy in (-1, 0, 1) for dx in (-1, 0, 1)) / 9.0
    pp = np.pad(sm, 2, mode="constant")
    is_top = np.ones(sm.shape, dtype=bool)
    for dy in range(-2, 3):
        for dx in range(-2, 3):
            if dy == 0 and dx == 0:
                continue
            is_top &= sm >= pp[2 + dy : 2 + dy + sm.shape[0], 2 + dx : 2 + dx + sm.shape[1]]
    is_top &= sm > 4.0
    tz, tx = np.nonzero(is_top)
    trees = []
    for zi, xi in zip(tz.tolist(), tx.tolist()):
        wx, wz = xi + 0.5 - half, zi + 0.5 - half
        height = float(h1[zi, xi])
        # crown radius: how far the canopy stays above half the tree's height
        rad = 1.0
        for r in range(1, 9):
            ring_v = []
            for a in range(0, 360, 45):
                px = xi + int(round(r * math.cos(math.radians(a))))
                pz = zi + int(round(r * math.sin(math.radians(a))))
                if 0 <= px < h1.shape[1] and 0 <= pz < h1.shape[0]:
                    ring_v.append(h1[pz, px])
            if not ring_v or np.mean(ring_v) < 0.5 * height:
                break
            rad = r + 0.5
        trees.append({"x": round(wx, 2), "z": round(wz, 2), "y": round(float(ground.height(wx, wz)), 2), "h": round(height, 2), "r": round(rad, 2)})
    json.dump(trees, open(os.path.join(d, "scene", "trees.json"), "w"))
    hs = np.array([t["h"] for t in trees]) if trees else np.zeros(1)
    print(f"  trees found: {len(trees)}; height median {np.median(hs):.1f} m, tallest {hs.max():.1f} m")
    json.dump({"lidar_points": int(len(x)), "buildings": stats, "trees": len(trees)}, open(os.path.join(d, "scene", "lidar_info.json"), "w"))


if __name__ == "__main__":
    main()
