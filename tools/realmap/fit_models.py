r"""Fitting library house models to real buildings: a building of the official survey (swissBUILDINGS3D) is drawn as the photographed models of realmap-data\models\chalets, turned to the building's own
orientation and scaled to its own length, width and height. A plain block is one model (a very long one is several side by side, so that no model is stretched more than about one and a half times); a building of a
complex shape is cut into rectangles along its own axes (the largest rectangles that fit in its footprint) and each gets a model; only a building that cannot be covered keeps its survey geometry (real roof
planes, walls with a timber or render texture). Position, size and direction are the survey's: only the look is the model's.

    fit_all(building, e0, n0, lo) -> None or [ {model, pos, rot, scale, ...}, ... ]

`building` is the dict of arrays swiss_region.parse_sheets writes (roof, wall, ground polygons in LV95: x east, y north, z height), (e0, n0) the middle of the tile and `lo` the lowest ground of the tile: the
results are in the tile's frame (x east, z south, y above `lo`). The library is the table LIBRARY below: a file, what kind of building it is, and how often it is picked.
"""
import math
import os
import zlib

import numpy as np
from PIL import Image, ImageDraw

import glb_info as G

MODELS = os.path.join(r"realmap-data", "models", "chalets")
# role: house (a dwelling of any size), barn (long and low), shed (small); weight: how often among the buildings of that role
LIBRARY = [
    {"file": "alpine_chalet_house.glb", "role": "house", "weight": 3},
    {"file": "log_house.glb", "role": "house", "weight": 3},
    {"file": "scandinavian_log_cabin.glb", "role": "house", "weight": 3},
    {"file": "log_cabin.glb", "role": "house", "weight": 2},
    {"file": "medieval_house.glb", "role": "house", "weight": 2},
    {"file": "red_barn.glb", "role": "barn", "weight": 1},
    {"file": "wooden_log_cabin.glb", "role": "shed", "weight": 2},
    {"file": "wooden_cabin.glb", "role": "shed", "weight": 2},
    {"file": "old_wooden_shed.glb", "role": "shed", "weight": 1},
]
_CACHE = {}
MAX_SEGMENT = 20.0  # metres: a longer building is made of several models side by side


def library():
    """The models with their bounds (read once)."""
    if not _CACHE:
        for m in LIBRARY:
            j, _ = G.load_glb(os.path.join(MODELS, m["file"]))
            lo, hi, tris = G.bounds(j)
            m = dict(m)
            m["lo"], m["hi"], m["tris"] = lo, hi, tris
            _CACHE[m["file"]] = m
    return list(_CACHE.values())


def obb(points):
    """The smallest rectangle round points (N x 2): (centre, long axis unit vector, length, width)."""
    pts = np.unique(np.round(points, 3), axis=0)
    if len(pts) < 3:
        return None
    # convex hull (monotone chain)
    p = sorted(map(tuple, pts))

    def cross(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])

    lower, upper = [], []
    for q in p:
        while len(lower) >= 2 and cross(lower[-2], lower[-1], q) <= 0:
            lower.pop()
        lower.append(q)
    for q in reversed(p):
        while len(upper) >= 2 and cross(upper[-2], upper[-1], q) <= 0:
            upper.pop()
        upper.append(q)
    hull = np.array(lower[:-1] + upper[:-1])
    best = None
    for i in range(len(hull)):
        e = hull[(i + 1) % len(hull)] - hull[i]
        n = np.linalg.norm(e)
        if n < 1e-6:
            continue
        u = e / n
        v = np.array([-u[1], u[0]])
        a, b = hull @ u, hull @ v
        area = (a.max() - a.min()) * (b.max() - b.min())
        if best is None or area < best[0]:
            best = (area, u, v, a.min(), a.max(), b.min(), b.max())
    if best is None:
        return None
    _, u, v, a0, a1, b0, b1 = best
    centre = u * (a0 + a1) / 2 + v * (b0 + b1) / 2
    length, width = a1 - a0, b1 - b0
    if width > length:
        u, length, width = v, width, length
    return centre, u, length, width


def polygon_area(v):
    x, y = v[:, 0], v[:, 1]
    return 0.5 * abs(float(np.dot(x, np.roll(y, -1)) - np.dot(y, np.roll(x, -1))))


def pick_model(role, key):
    pool = [m for m in library() if m["role"] == role] or [m for m in library() if m["role"] == "house"]
    total = sum(m["weight"] for m in pool)
    pick = key % total
    for m in pool:
        if pick < m["weight"]:
            return m
        pick -= m["weight"]
    return pool[0]


def place(centre, u, length, width, ground_z, ridge_z, e0, n0, lo, key):
    """One model on the rectangle (centre in LV95, long axis `u`, its length and width), or None when it is too small or too tall for any."""
    height = ridge_z - ground_z
    if length < 4.0 or width < 3.0 or not (2.2 <= height <= 30.0):
        return None
    # which kind of building: long and low is a barn, small is a shed, the rest a house
    if length / width >= 2.0 and length >= 12.0:
        role = "barn"
    elif length * width < 45.0:
        role = "shed"
    else:
        role = "house"
    model = pick_model(role, key)
    size = model["hi"] - model["lo"]
    mid = (model["lo"] + model["hi"]) / 2
    long_is_x = size[0] >= size[2]
    lm, sm = (size[0], size[2]) if long_is_x else (size[2], size[0])
    s_long, s_short = length / lm, width / sm
    # the height follows the survey, but within reason of how the model's footprint was scaled (a model in centimetres has a huge ratio of its own)
    s_h = math.sqrt(s_long * s_short)
    s_y = float(np.clip(height / size[1], 0.6 * s_h, 1.8 * s_h))
    # the model's long axis (x or z of its own) is turned onto the rectangle's: theta is its long axis in the (x east, z south) plane
    cx_w, cz_w = centre[0] - e0, -(centre[1] - n0)
    theta = math.atan2(-u[1], u[0])
    phi = -theta if long_is_x else math.pi / 2 - theta
    scale = [s_long, s_y, s_short] if long_is_x else [s_short, s_y, s_long]
    c, s = math.cos(phi), math.sin(phi)
    # R rotates about y by phi: (x, z) -> (c*x + s*z, -s*x + c*z)
    ox, oz = scale[0] * mid[0], scale[2] * mid[2]
    pos = [cx_w - (c * ox + s * oz), (ground_z - lo) - s_y * model["lo"][1], cz_w - (-s * ox + c * oz)]
    return {"model": f"models/chalets/{model['file']}", "pos": [round(float(p), 3) for p in pos], "rot": [0.0, round(math.sin(phi / 2), 5), 0.0, round(math.cos(phi / 2), 5)], "scale": [round(float(v), 4) for v in scale],
            "length": round(float(length), 1), "width": round(float(width), 1), "height": round(float(height), 1), "role": role}


def segments(centre, u, length, width, ground_z, ridge_z, e0, n0, lo, key):
    """A rectangle as one model, or as several side by side when it is long."""
    n = max(1, math.ceil(length / MAX_SEGMENT))
    out = []
    for k in range(n):
        c = centre + u * ((k + 0.5) * length / n - length / 2)
        p = place(c, u, length / n, width, ground_z, ridge_z, e0, n0, lo, key + k * 7919)
        if p is None:
            return None
        out.append(p)
    return out


def largest_rectangle(mask):
    """The largest rectangle of ones in a binary matrix: (area, row0, row1, col0, col1) with the end rows and columns exclusive."""
    rows, cols = mask.shape
    heights = np.zeros(cols, dtype=np.int32)
    best = (0, 0, 0, 0, 0)
    for r in range(rows):
        heights = np.where(mask[r], heights + 1, 0)
        stack = []
        for c in range(cols + 1):
            h = int(heights[c]) if c < cols else 0
            start = c
            while stack and stack[-1][1] >= h:
                s, sh = stack.pop()
                area = sh * (c - s)
                if area > best[0]:
                    best = (area, r - sh + 1, r + 1, s, c)
                start = s
            stack.append((start, h))
    return best


def decompose(grounds, u0, v0, cell=0.5, min_area=18.0, min_side=3.0, stop_left=0.08, max_rects=8):
    """The rectangles (in the frame of axes u0, v0) that cover a footprint: (u_min, u_max, v_min, v_max) in metres, and the share of the footprint they cover."""
    pts = np.concatenate([g[:, :2] for g in grounds])
    pu, pv = pts @ u0, pts @ v0
    ou, ov = pu.min(), pv.min()
    w, h = int(math.ceil((pu.max() - ou) / cell)) + 2, int(math.ceil((pv.max() - ov) / cell)) + 2
    if w * h > 4_000_000:
        return [], 0.0
    img = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(img)
    for g in grounds:
        d.polygon([(float((p[:2] @ u0 - ou) / cell + 0.5), float((p[:2] @ v0 - ov) / cell + 0.5)) for p in g], fill=1)
    mask = np.array(img, dtype=bool)
    total = int(mask.sum())
    if total == 0:
        return [], 0.0
    rects = []
    left = mask.copy()
    for _ in range(max_rects):
        area, r0, r1, c0, c1 = largest_rectangle(left)
        if area * cell * cell < min_area or min(r1 - r0, c1 - c0) * cell < min_side:
            break
        rects.append((ou + (c0 - 0.5) * cell, ou + (c1 - 0.5) * cell, ov + (r0 - 0.5) * cell, ov + (r1 - 0.5) * cell))
        left[r0:r1, c0:c1] = False
        if left.sum() < stop_left * total:
            break
    covered = 1.0 - float(left.sum()) / total
    return rects, covered


def fit_all(b, e0, n0, lo, min_fill=0.82, min_cover=0.72):
    """The model placements for building `b`, or None when the building should keep its survey geometry."""
    grounds = b["ground"]
    roofs = b["roof"]
    if not grounds or not roofs:
        return None
    pts = np.concatenate([g[:, :2] for g in grounds])
    box = obb(pts)
    if box is None:
        return None
    centre, u, length, width = box
    foot = sum(polygon_area(g[:, :2]) for g in grounds)
    ground_z = min(float(g[:, 2].min()) for g in grounds)
    ridge_all = max(float(r[:, 2].max()) for r in roofs)
    key = zlib.crc32(f"{centre[0]:.1f},{centre[1]:.1f}".encode())
    if foot / max(length * width, 1e-6) >= min_fill:
        return segments(centre, u, length, width, ground_z, ridge_all, e0, n0, lo, key)
    # a complex shape: the rectangles that fit in it, along the building's own axes
    v = np.array([-u[1], u[0]])
    rects, covered = decompose(grounds, u, v)
    if covered < min_cover or not rects:
        return None
    out = []
    for k, (u0, u1, v0, v1) in enumerate(rects):
        c = u * (u0 + u1) / 2 + v * (v0 + v1) / 2
        lu, lv = u1 - u0, v1 - v0
        axis, ln, wd = (u, lu, lv) if lu >= lv else (v, lv, lu)
        # the roof over this rectangle gives its height (the building's own ridge where nothing is found)
        zs = [float(r[:, 2].max()) for r in roofs if u0 - 0.5 <= (r[:, :2].mean(axis=0) @ u) <= u1 + 0.5 and v0 - 0.5 <= (r[:, :2].mean(axis=0) @ v) <= v1 + 0.5]
        ridge = max(zs) if zs else ridge_all
        got = segments(c, axis, ln, wd, ground_z, ridge, e0, n0, lo, key + 101 * k)
        if got:
            out += got
    return out or None
