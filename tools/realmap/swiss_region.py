"""A whole region of Switzerland, tile by tile, from swisstopo's open data (swisstopo: free for any use with the credit "Federal Office of Topography swisstopo"):

    python tools/realmap/swiss_region.py NAME LAT LON WIDTH_KM HEIGHT_KM [fetch|sheets|build|refit]

NAME is the folder realmap-data\\NAME. The region is a rectangle of whole 1 km tiles of the Swiss grid (LV95) round the point. Everything is resumable (what is on disk is not done again) and
nothing holds more than one tile and its neighbours in memory, so a region of 100 tiles costs time, not RAM. Steps (all by default, or the one named):

  fetch   swissALTI3D (terrain 0.5 m), swisssurface3d (surface 0.5 m), swissIMAGE (photograph 10 cm) and the swissBUILDINGS3D sheets, with 4 downloads at a time; OpenStreetMap water in boxes of 0.0075 degrees
  sheets  every building of every CityGML sheet, sorted into the tile its middle is in (raw/bld/)
  build   for each tile a folder cells/cell_E_N/: heightmap.png (16 bits, the tile's own lowest ground is black), water.png, terrain.jpg + _l1 + _l2 (the photograph at 8192, 2048 and 512 px), splat.png
          (what the ground is made of, from the photograph and the slope), ../buildings_E_N.gltf (the official LoD2 roofs with the photograph, walls of timber or render), trees.json (measured tree tops);
          then world.json, the list of cells that world.py makes a streamed world of
  refit   make the buildings of the built cells again (plain blocks as fitted library models: cells/cell_E_N/fits.json, the rest as survey geometry) without touching the rest

The world's frame: x east and z south from the region's middle (a grid corner), y metres above `z0` (the lowest ground of the region, rounded down to 100 m). A cell's prefab stands at
(centre x, its lowest ground - z0, centre z).
"""
import json
import math
import os
import pickle
import sys
import time
import xml.etree.ElementTree as ET
import zipfile
from concurrent.futures import ThreadPoolExecutor

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import build as B  # noqa: E402
import fit_models as FM  # noqa: E402
import swiss_build as SB  # noqa: E402
import swiss_fetch as F  # noqa: E402

Image.MAX_IMAGE_PIXELS = None
ROOT = B.ROOT
TILE_PX = SB.TILE_PX
KINDS = (("dtm", "ch.swisstopo.swissalti3d", "_0.5_2056_5728.tif"), ("dsm", "ch.swisstopo.swisssurface3d-raster", "_0.5_2056_5728.tif"), ("image", "ch.swisstopo.swissimage-dop10", "_0.1_2056.tif"))


class Region:
    def __init__(self, name, lat, lon, w_km, h_km):
        self.name = name
        e, n = F.lv95(lat, lon)
        kc, nc = round(e / 1000), round(n / 1000)
        self.kx0, self.ky0 = kc - w_km // 2, nc - h_km // 2
        self.w, self.h = w_km, h_km
        self.e0, self.n0 = kc * 1000, nc * 1000
        self.dir = os.path.join(ROOT, name)
        self.raw = os.path.join(self.dir, "raw")
        self.cells = os.path.join(self.dir, "cells")
        for d in (self.raw, self.cells, os.path.join(self.raw, "bld")):
            os.makedirs(d, exist_ok=True)
        self.keys = [(self.kx0 + i, self.ky0 + j) for j in range(h_km) for i in range(w_km)]

    def centre(self, kx, ky):
        """The tile's middle in the world's frame (x east, z south)."""
        return (kx * 1000 + 500 - self.e0, -((ky * 1000 + 500) - self.n0))

    def cell_dir(self, kx, ky):
        return os.path.join(self.cells, f"cell_{kx}_{ky}")


def tile_latlon(kx, ky):
    return F.wgs84(kx * 1000 + 500, ky * 1000 + 500)


# ------------------------------------------------------------------------------------------------ fetch
def stac_best(cid, kx, ky, suffix):
    lat, lon = tile_latlon(kx, ky)
    items = json.loads(F.get(f"{F.STAC}/{cid}/items?bbox={lon - 0.0004:.5f},{lat - 0.0004:.5f},{lon + 0.0004:.5f},{lat + 0.0004:.5f}&limit=100"))["features"]
    best = None
    for it in items:
        parts = it["id"].rsplit("_", 2)
        if len(parts) < 3 or parts[-1] != f"{kx}-{ky}" or not parts[-2].isdigit():
            continue
        a = next((a for an, a in it["assets"].items() if an.endswith(suffix)), None)
        if a and (best is None or int(parts[-2]) > best[0]):
            best = (int(parts[-2]), a["href"])
    return best


def sheets_at(kx, ky):
    lat, lon = tile_latlon(kx, ky)
    out = {}
    items = json.loads(F.get(f"{F.STAC}/ch.swisstopo.swissbuildings3d_3_0/items?bbox={lon - 0.004:.5f},{lat - 0.003:.5f},{lon + 0.004:.5f},{lat + 0.003:.5f}&limit=100"))["features"]
    for it in items:
        parts = it["id"].rsplit("_", 2)
        if len(parts) < 3 or not parts[-2].isdigit():
            continue
        year, sheet = int(parts[-2]), parts[-1]
        a = next((a for an, a in it["assets"].items() if an.endswith(".citygml.zip")), None)
        if a and (sheet not in out or year > out[sheet][0]):
            out[sheet] = (year, a["href"])
    return out


def fetch(rg):
    t0 = time.time()
    jobs = []
    for kx, ky in rg.keys:
        for kind, cid, suffix in KINDS:
            path = os.path.join(rg.raw, f"{kind}_{kx}_{ky}.tif")
            if not (os.path.exists(path) and os.path.getsize(path) > 1000):
                jobs.append((kind, cid, suffix, kx, ky, path))

    def one(job):
        kind, cid, suffix, kx, ky, path = job
        best = stac_best(cid, kx, ky, suffix)
        if best is None:
            print(f"  missing {kind} {kx}-{ky}", flush=True)
            return
        F.download(best[1], path)

    print(f"fetch: {len(rg.keys)} tiles, {len(jobs)} files to download", flush=True)
    with ThreadPoolExecutor(4) as pool:
        list(pool.map(one, jobs))
    # building sheets (a sheet covers several tiles)
    sheets = {}
    for kx, ky in rg.keys:
        for sheet, v in sheets_at(kx, ky).items():
            if sheet not in sheets or v[0] > sheets[sheet][0]:
                sheets[sheet] = v
    print(f"fetch: {len(sheets)} building sheets", flush=True)
    for sheet, (year, href) in sorted(sheets.items()):
        F.download(href, os.path.join(rg.raw, f"buildings_{sheet}.citygml.zip"))
    # OpenStreetMap, in small boxes through the official API (water only is used)
    lat0, lon0 = F.wgs84(rg.kx0 * 1000, rg.ky0 * 1000)
    lat1, lon1 = F.wgs84((rg.kx0 + rg.w) * 1000, (rg.ky0 + rg.h) * 1000)
    south, north = min(lat0, lat1) - 0.002, max(lat0, lat1) + 0.002
    west, east = min(lon0, lon1) - 0.002, max(lon0, lon1) + 0.002
    step = 0.0075
    ni, nj = int(math.ceil((east - west) / step)), int(math.ceil((north - south) / step))
    files = []
    for j in range(nj):
        for i in range(ni):
            path = os.path.join(rg.raw, f"osm_{i}_{j}.xml")
            files.append(os.path.basename(path))
            if os.path.exists(path) and os.path.getsize(path) > 100:
                continue
            bw, bs = west + i * step, south + j * step
            print(f"  openstreetmap {i},{j} of {ni},{nj}", flush=True)
            open(path, "wb").write(F.get("https://api.openstreetmap.org/api/0.6/map?bbox=" + f"{bw:.6f},{bs:.6f},{min(bw + step, east):.6f},{min(bs + step, north):.6f}"))
            time.sleep(1.5)
    json.dump({"osm_files": files, "sheets": sorted(sheets)}, open(os.path.join(rg.raw, "fetched.json"), "w"))
    print(f"fetch done in {time.time() - t0:.0f} s", flush=True)


# ------------------------------------------------------------------------------------------------ buildings: sort every sheet's buildings into tiles
def parse_sheets(rg):
    sheets = json.load(open(os.path.join(rg.raw, "fetched.json")))["sheets"]
    for sheet in sheets:
        marker = os.path.join(rg.raw, "bld", f"{sheet}.done")
        if os.path.exists(marker):
            continue
        t0 = time.time()
        per_tile = {}
        z = zipfile.ZipFile(os.path.join(rg.raw, f"buildings_{sheet}.citygml.zip"))
        n = 0
        for name in z.namelist():
            if not name.lower().endswith((".gml", ".xml")):
                continue
            with z.open(name) as fh:
                for ev, el in ET.iterparse(fh, events=("end",)):
                    if el.tag.rsplit("}", 1)[-1] != "Building":
                        continue
                    polys = {"roof": [], "wall": [], "ground": []}
                    for sub in el.iter():
                        kind = {"RoofSurface": "roof", "WallSurface": "wall", "GroundSurface": "ground", "OuterCeilingSurface": "roof", "OuterFloorSurface": "ground", "ClosureSurface": "wall"}.get(sub.tag.rsplit("}", 1)[-1])
                        if not kind:
                            continue
                        for pl in sub.iter():
                            if pl.tag.rsplit("}", 1)[-1] != "Polygon":
                                continue
                            ext = next((x for x in pl if x.tag.rsplit("}", 1)[-1] == "exterior"), None)
                            pos = next((x for x in ext.iter() if x.tag.rsplit("}", 1)[-1] == "posList"), None) if ext is not None else None
                            if pos is None or not pos.text:
                                continue
                            v = np.array(pos.text.split(), dtype=np.float64).reshape(-1, 3)
                            if len(v) > 1 and np.allclose(v[0], v[-1]):
                                v = v[:-1]
                            if len(v) >= 3:
                                polys[kind].append(v)
                    if polys["roof"] or polys["wall"]:
                        allv = np.concatenate([v for k in polys for v in polys[k]])
                        cen = allv.mean(axis=0)
                        per_tile.setdefault((int(cen[0] // 1000), int(cen[1] // 1000)), []).append(polys)
                        n += 1
                    el.clear()
        for (kx, ky), blds in per_tile.items():
            with open(os.path.join(rg.raw, "bld", f"{sheet}_{kx}_{ky}.pkl"), "wb") as f:
                pickle.dump(blds, f)
        open(marker, "w").write(str(n))
        print(f"sheet {sheet}: {n} buildings in {len(per_tile)} tiles, {time.time() - t0:.0f} s", flush=True)


# ------------------------------------------------------------------------------------------------ water from OpenStreetMap (only what the water map needs)
def read_water(rg):
    files = json.load(open(os.path.join(rg.raw, "fetched.json")))["osm_files"]
    way_nodes, way_tags, rels = {}, {}, {}

    def is_water(t):
        return t.get("natural") == "water" or t.get("landuse") in ("reservoir", "basin") or t.get("waterway") in ("riverbank", "river", "stream", "canal", "brook")

    for f in files:
        for ev, el in ET.iterparse(os.path.join(rg.raw, f), events=("end",)):
            if el.tag == "relation":
                tags = {t.get("k"): t.get("v") for t in el.findall("tag")}
                if tags.get("natural") == "water" and tags.get("type") == "multipolygon":
                    rels[int(el.get("id"))] = ([("way", int(m.get("ref")), m.get("role")) for m in el.findall("member") if m.get("type") == "way"], tags)
            el.clear() if el.tag in ("relation", "way", "node") else None
    wanted = {m[1] for r in rels.values() for m in r[0]}
    for f in files:
        for ev, el in ET.iterparse(os.path.join(rg.raw, f), events=("end",)):
            if el.tag == "way":
                wid = int(el.get("id"))
                tags = {t.get("k"): t.get("v") for t in el.findall("tag")}
                if wid in wanted or is_water(tags):
                    way_nodes[wid] = [int(n.get("ref")) for n in el.findall("nd")]
                    way_tags[wid] = tags
            el.clear() if el.tag in ("relation", "way", "node") else None
    need = {n for v in way_nodes.values() for n in v}
    loc = {}
    for f in files:
        for ev, el in ET.iterparse(os.path.join(rg.raw, f), events=("end",)):
            if el.tag == "node":
                nid = int(el.get("id"))
                if nid in need and nid not in loc:
                    loc[nid] = F.lv95(float(el.get("lat")), float(el.get("lon")))
            el.clear() if el.tag in ("relation", "way", "node") else None
    ways = {wid: (way_nodes[wid], way_tags[wid]) for wid in way_nodes}
    print(f"  water: {len(ways)} ways, {len(rels)} relations, {len(loc)} nodes", flush=True)
    return ways, rels, loc


def water_mask(ways, rels, loc, kx, ky, n=2001):
    """The water of one tile as a mask of n x n samples (0.5 m, corners of the cells, row 0 north)."""
    mask = Image.new("L", (n, n), 0)
    dr = ImageDraw.Draw(mask)
    ew, nn = kx * 1000, (ky + 1) * 1000
    s = (n - 1) / 1000.0

    def px(p):
        return ((p[0] - ew) * s, (nn - p[1]) * s)

    def near(pts):
        return any(ew - 50 <= p[0] <= ew + 1050 and nn - 1050 <= p[1] <= nn + 50 for p in pts)

    for wid, (nodes, t) in ways.items():
        pts = [loc[i] for i in nodes if i in loc]
        if len(pts) < 2 or not near(pts):
            continue
        if (t.get("natural") == "water" or t.get("landuse") in ("reservoir", "basin") or t.get("waterway") == "riverbank") and nodes[0] == nodes[-1]:
            dr.polygon([px(p) for p in pts], fill=255)
        elif t.get("waterway") in ("river", "stream", "canal", "brook"):
            wd = B.number(t.get("width")) or {"river": 14, "canal": 8, "stream": 3.5, "brook": 2.5}.get(t["waterway"], 1.5)
            dr.line([px(p) for p in pts], fill=255, width=max(2, int(wd * s)))
    for rid, r in rels.items():
        pieces = [list(ways[m[1]][0]) for m in r[0] if m[2] in ("outer", "") and m[1] in ways]
        rings = []
        while pieces:
            ring = pieces.pop()
            grown = True
            while grown and ring[0] != ring[-1]:
                grown = False
                for k, p in enumerate(pieces):
                    if p[0] == ring[-1]:
                        ring += p[1:]
                    elif p[-1] == ring[-1]:
                        ring += p[::-1][1:]
                    elif p[-1] == ring[0]:
                        ring = p[:-1] + ring
                    elif p[0] == ring[0]:
                        ring = p[::-1][:-1] + ring
                    else:
                        continue
                    pieces.pop(k)
                    grown = True
                    break
            if ring[0] == ring[-1] and len(ring) >= 4:
                rings.append([loc[i] for i in ring if i in loc])
        for ring in rings:
            if ring and near(ring):
                dr.polygon([px(p) for p in ring], fill=255)
    return np.array(mask, dtype=np.float32) / 255.0


# ------------------------------------------------------------------------------------------------ one tile
def tif(rg, kind, kx, ky):
    path = os.path.join(rg.raw, f"{kind}_{kx}_{ky}.tif")
    if not os.path.exists(path):
        return None
    a = np.array(Image.open(path), dtype=np.float32)
    a[(a < -500) | (a > 9000)] = np.nan
    return a


def cells_with_border(rg, kind, kx, ky):
    """The tile's 2000 x 2000 cells with one cell of its neighbours all round (2002 x 2002; row 0 north), NaN where there is no data."""
    big = np.full((2002, 2002), np.nan, dtype=np.float32)
    for dx in (-1, 0, 1):
        for dy in (-1, 0, 1):
            a = tif(rg, kind, kx + dx, ky + dy) if (dx, dy) != (0, 0) else tif(rg, kind, kx, ky)
            if a is None:
                continue
            # which cells of the neighbour touch this tile
            src_r = slice(0, 2000) if dy == 0 else (slice(1999, 2000) if dy == 1 else slice(0, 1))
            src_c = slice(0, 2000) if dx == 0 else (slice(1999, 2000) if dx == -1 else slice(0, 1))
            dst_r = slice(1, 2001) if dy == 0 else (slice(0, 1) if dy == 1 else slice(2001, 2002))
            dst_c = slice(1, 2001) if dx == 0 else (slice(0, 1) if dx == -1 else slice(2001, 2002))
            big[dst_r, dst_c] = a[src_r, src_c]
    return big


def corners_of(cells):
    """The 2001 x 2001 samples at the corners of the cells: the mean of the (up to four) cells that meet there."""
    stack = np.stack([cells[0:2001, 0:2001], cells[0:2001, 1:2002], cells[1:2002, 0:2001], cells[1:2002, 1:2002]])
    with np.errstate(all="ignore"):
        import warnings

        with warnings.catch_warnings():
            warnings.simplefilter("ignore", RuntimeWarning)
            c = np.nanmean(stack, axis=0)
    if np.isnan(c).any():
        c[np.isnan(c)] = np.nanmin(c) if not np.isnan(c).all() else 0.0
    return c.astype(np.float32)


def build_buildings(rg, kx, ky, cx, cz, lo_t, z0, tile_img, out):
    """The tile's buildings as one glTF (roofs with the tile's photograph, walls of timber or render), positions relative to the cell's prefab; returns (count, footprints in tile-local metres)."""
    blds = []
    for fn in os.listdir(os.path.join(rg.raw, "bld")):
        if fn.endswith(f"_{kx}_{ky}.pkl"):
            blds += pickle.load(open(os.path.join(rg.raw, "bld", fn), "rb"))
    fits = []
    roofs = {"pos": [], "nrm": [], "uv": [], "idx": [], "mat": 0}
    wood = {"pos": [], "nrm": [], "uv": [], "col": [], "idx": [], "mat": 1, "m": SB.WOOD_M}
    plaster = {"pos": [], "nrm": [], "uv": [], "col": [], "idx": [], "mat": 2, "m": SB.PLASTER_M}
    ex, ny = kx * 1000 + 500, ky * 1000 + 500  # the tile's middle (LV95)
    prints = []
    for b in blds:
        allv = np.concatenate([v for k in b for v in b[k]])
        cen = allv.mean(axis=0)
        bx, bz = cen[0] - ex, -(cen[1] - ny)
        # a plain block of a building is drawn as a photographed library house fitted to its footprint and height (fit_models.py); the rest keeps its survey geometry
        placed = FM.fit_all(b, ex, ny, lo_t) if os.environ.get("FIT_MODELS", "1") != "0" else None
        if placed:
            fits.extend(placed)
            for v in b["ground"]:
                prints.append([(p[0] - ex, -(p[1] - ny)) for p in v])
            continue
        hb = ((int(round(bx * 10)) * 73856093) ^ (int(round(bz * 10)) * 19349663)) & 0xFFFF
        walls = wood if hb % 100 < 62 else plaster
        shade = 0.82 + 0.36 * ((hb >> 7) % 64) / 63.0
        tint = [shade, shade * 0.97, shade * 0.93] if walls is plaster else [shade] * 3
        for kind in ("roof", "wall", "ground"):
            for v in b[kind]:
                loc3 = np.stack([v[:, 0] - ex, v[:, 2] - lo_t, -(v[:, 1] - ny)], axis=1)
                if kind == "ground":
                    prints.append([(p[0], p[2]) for p in loc3])
                    continue
                for (a, bb, c) in SB.tri_polygon(v):
                    P = [loc3[a], loc3[bb], loc3[c]]
                    nrm = np.cross(P[1] - P[0], P[2] - P[0])
                    ln = np.linalg.norm(nrm)
                    if ln < 1e-9:
                        continue
                    nrm = nrm / ln
                    centre = (P[0] + P[1] + P[2]) / 3
                    out_dir = centre - np.array([bx, centre[1], bz])
                    ok = nrm[1] > 0 if kind == "roof" else (nrm[0] * out_dir[0] + nrm[2] * out_dir[2]) > 0
                    if not ok:
                        P = [P[0], P[2], P[1]]
                        nrm = -nrm
                    tgt = roofs if kind == "roof" else walls
                    base = len(tgt["pos"])
                    for q in P:
                        tgt["pos"].append([float(q[0]), float(q[1]), float(q[2])])
                        tgt["nrm"].append(nrm.tolist())
                        if kind == "roof":
                            tgt["uv"].append([float((q[0] + 500) / 1000), float((q[2] + 500) / 1000)])
                        else:
                            hz = math.hypot(nrm[0], nrm[2])
                            t = np.array([nrm[2] / hz, 0.0, -nrm[0] / hz]) if hz > 1e-6 else np.array([1.0, 0.0, 0.0])
                            q_abs = np.array([q[0] + ex, q[1], q[2] - ny])
                            tgt["uv"].append([float(np.dot(q_abs, t)) / tgt["m"], float(q[1]) / tgt["m"]])
                            tgt["col"].append(tint)
                    tgt["idx"] += [base, base + 1, base + 2]
    if roofs["idx"] or wood["idx"] or plaster["idx"]:
        # (beside the cells' folder, not in it: a model may not name files above its own folder, and it needs the cell's photograph and the wall photographs)
        SB.write_gltf(os.path.join(rg.dir, f"buildings_{kx}_{ky}"), [roofs, wood, plaster], [f"cells/cell_{kx}_{ky}/terrain.jpg", "walls_wood.jpg", "walls_plaster.jpg"], 1)
    json.dump(fits, open(os.path.join(rg.cell_dir(kx, ky), "fits.json"), "w"))
    return len(blds), prints


def refit(rg):
    """Make the buildings of every built cell again (the fitted models and the geometry of the rest) without touching anything else of the cell."""
    for kx, ky in rg.keys:
        cj = os.path.join(rg.cell_dir(kx, ky), "cell.json")
        if not os.path.exists(cj):
            continue
        cell = json.load(open(cj))
        for fn in (f"buildings_{kx}_{ky}.gltf", f"buildings_{kx}_{ky}.bin"):
            path = os.path.join(rg.dir, fn)
            if os.path.exists(path):
                os.replace(path, path + ".old")
        nb, prints = build_buildings(rg, kx, ky, cell["cx"], cell["cz"], cell["lo"], 0.0, None, rg.cell_dir(kx, ky))
        fits = json.load(open(os.path.join(rg.cell_dir(kx, ky), "fits.json")))
        cell["has_buildings"] = os.path.exists(os.path.join(rg.dir, f"buildings_{kx}_{ky}.gltf"))
        cell["fitted"] = len(fits)
        json.dump(cell, open(cj, "w"))
        print(f"  tile {kx},{ky}: {nb} buildings, {len(fits)} drawn as library models", flush=True)
    wj = os.path.join(rg.dir, "world.json")
    w = json.load(open(wj))
    w["cells"] = [json.load(open(os.path.join(rg.cell_dir(c["kx"], c["ky"]), "cell.json"))) for c in w["cells"]]
    json.dump(w, open(wj, "w"), indent=1)


def find_trees(dsm_cells, dtm_cells, carved_lo, prints, wmask, lo_t, z0, rg, kx, ky):
    ndsm = dsm_cells - dtm_cells
    ndsm[np.isnan(ndsm)] = 0
    ndsm[ndsm < 0] = 0
    h1 = ndsm.reshape(1000, 2, 1000, 2).max(axis=(1, 3))  # 1 m cells, row 0 north
    bimg = Image.new("L", (1000, 1000), 0)
    bd = ImageDraw.Draw(bimg)
    for poly in prints:
        bd.polygon([(p[0] + 500, p[1] + 500) for p in poly], fill=255)
    bmask = np.array(bimg.filter(ImageFilter.MaxFilter(5))) > 0
    w1 = np.array(Image.fromarray((wmask > 0.5).astype(np.uint8) * 255).resize((1000, 1000), Image.NEAREST)) > 0
    h1[bmask | w1] = 0
    p3 = np.pad(h1, 1, mode="edge")
    sm = sum(p3[1 + dy : 1 + dy + 1000, 1 + dx : 1 + dx + 1000] for dy in (-1, 0, 1) for dx in (-1, 0, 1)) / 9.0
    pp = np.pad(sm, 2, mode="constant")
    top = np.ones(sm.shape, dtype=bool)
    for dy in range(-2, 3):
        for dx in range(-2, 3):
            if dy or dx:
                top &= sm >= pp[2 + dy : 2 + dy + 1000, 2 + dx : 2 + dx + 1000]
    top &= (sm > 3.5) & (sm < 45)
    tz, tx = np.nonzero(top)
    ground = B.Ground(carved_lo, 1000.0, 2)
    cx, cz = rg.centre(kx, ky)
    trees = []
    for zi, xi in zip(tz.tolist(), tx.tolist()):
        height = float(h1[zi, xi])
        rad = 1.0
        for r in range(1, 8):
            vals = []
            for a in range(0, 360, 45):
                px = xi + int(round(r * math.cos(math.radians(a))))
                pz = zi + int(round(r * math.sin(math.radians(a))))
                if 0 <= px < 1000 and 0 <= pz < 1000:
                    vals.append(h1[pz, px])
            if not vals or np.mean(vals) < 0.5 * height:
                break
            rad = r + 0.5
        lx, lz = xi + 0.5 - 500, zi + 0.5 - 500
        trees.append({"x": round(cx + lx, 2), "z": round(cz + lz, 2), "y": round(float(ground.height(lx, lz)) + (lo_t - z0), 2), "h": round(height, 2), "r": round(rad, 2)})
    return trees


def build_tile(rg, kx, ky, ctx):
    t0 = time.time()
    out = rg.cell_dir(kx, ky)
    if os.path.exists(os.path.join(out, "cell.json")):
        return json.load(open(os.path.join(out, "cell.json")))
    os.makedirs(out, exist_ok=True)
    cells = cells_with_border(rg, "dtm", kx, ky)
    if np.isnan(cells[1:2001, 1:2001]).all():
        print(f"  tile {kx},{ky}: no terrain, skipped", flush=True)
        return None
    corner = corners_of(cells)
    wm = water_mask(ctx["ways"], ctx["rels"], ctx["loc"], kx, ky)
    soft = np.array(Image.fromarray((wm * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(6.0)), dtype=np.float32) / 255.0
    carved = corner - 1.2 * np.maximum(soft, wm)
    lo_t, hi_t = float(carved.min()), float(corner.max())
    relief = max(hi_t - lo_t, 1.0)
    Image.fromarray(np.clip((carved - lo_t) / relief * 65535.0 + 0.5, 0, 65535).astype("<u2")).save(os.path.join(out, "heightmap.png"))
    has_water = bool((wm > 0.5).any())
    if has_water:
        Image.fromarray(np.where(wm > 0.5, np.clip((corner - lo_t) / relief * 65535.0 + 0.5, 1, 65535), 0).astype("<u2")).save(os.path.join(out, "water.png"))
    # the photograph at three resolutions, and what the ground is made of
    path = os.path.join(rg.raw, f"image_{kx}_{ky}.tif")
    if os.path.exists(path):
        im = Image.open(path).convert("RGB").resize((TILE_PX, TILE_PX), Image.LANCZOS)
        im.save(os.path.join(out, "terrain.jpg"), quality=93)
        for lvl, px in (("l1", 2048), ("l2", 512)):
            im.resize((px, px), Image.LANCZOS).save(os.path.join(out, f"terrain_{lvl}.jpg"), quality=92)
        small = np.asarray(im.resize((1001, 1001), Image.LANCZOS), dtype=np.float32) / 255.0
        hs = carved[::2, ::2]
        gy, gx = np.gradient(hs.astype(np.float64), 1.0)
        slope = np.degrees(np.arctan(np.hypot(gx, gy)))
        r_, g_, b_ = small[..., 0], small[..., 1], small[..., 2]
        v = small.max(axis=2)
        exg = 2 * g_ - r_ - b_
        ss = lambda a, b, x: np.clip((x - a) / (b - a), 0, 1) ** 2 * (3 - 2 * np.clip((x - a) / (b - a), 0, 1))  # noqa: E731
        veg = ss(0.0, 0.07, exg)
        w_grass = veg * ss(0.22, 0.38, v)
        w_forest = veg * (1 - ss(0.22, 0.38, v))
        w_rock = np.clip(ss(30, 46, slope) + (1 - veg) * ss(0.30, 0.55, v) * 0.9, 0, 1)
        # an alpine region: the fourth layer is snow and ice (bright, hardly coloured, above the snow line) and bare ground is rock
        sat = (small.max(axis=2) - small.min(axis=2)) / np.maximum(small.max(axis=2), 1e-3)
        w_snow = ss(0.60, 0.82, v) * (1 - ss(0.06, 0.16, sat)) * ss(1700.0, 2300.0, carved[::2, ::2])
        w_rock = np.clip(w_rock + (1 - veg) * (1 - ss(0.30, 0.55, v)), 0, 1)
        keep = 1 - w_snow
        wts = np.stack([(w_grass + 0.02) * keep, w_rock * keep, w_forest * keep, w_snow], axis=2)
        wts = wts / np.maximum(wts.sum(axis=2, keepdims=True), 1e-6)
        Image.fromarray((wts * 255 + 0.5).astype(np.uint8), "RGBA").save(os.path.join(out, "splat.png"))
        del im
    cx, cz = rg.centre(kx, ky)
    nb, prints = build_buildings(rg, kx, ky, cx, cz, lo_t, ctx["z0"], None, out)
    dsm = cells_with_border(rg, "dsm", kx, ky)[1:2001, 1:2001]
    trees = find_trees(dsm, cells[1:2001, 1:2001], carved - lo_t, prints, wm[::2, ::2], lo_t, ctx["z0"], rg, kx, ky)
    json.dump(trees, open(os.path.join(out, "trees.json"), "w"))
    cell = {"kx": kx, "ky": ky, "cx": cx, "cz": cz, "lo": lo_t, "y0": lo_t - ctx["z0"], "relief": relief, "water": has_water, "buildings": nb, "trees": len(trees), "photo": os.path.exists(path), "has_buildings": os.path.exists(os.path.join(rg.dir, f"buildings_{kx}_{ky}.gltf"))}
    json.dump(cell, open(os.path.join(out, "cell.json"), "w"))
    print(f"  tile {kx},{ky}: relief {relief:.0f} m, {nb} buildings, {len(trees)} trees, water {has_water}, {time.time() - t0:.0f} s", flush=True)
    return cell


def build(rg):
    print("water map from OpenStreetMap", flush=True)
    ways, rels, loc = read_water(rg)
    # the datum: the lowest ground of the region, rounded down to 100 m (read from the corners of every tile's file: cheap, the metadata of a pass over the files)
    lows = []
    for kx, ky in rg.keys:
        a = tif(rg, "dtm", kx, ky)
        if a is not None and not np.isnan(a).all():
            lows.append(float(np.nanmin(a)))
    z0 = math.floor(min(lows) / 100.0) * 100.0
    ctx = {"ways": ways, "rels": rels, "loc": loc, "z0": z0}
    # the walls' two photographs sit beside the cells
    import shutil

    for src, dst in ((SB.WOOD_TEXTURE, "walls_wood.jpg"), (SB.PLASTER_TEXTURE, "walls_plaster.jpg")):
        shutil.copy(src, os.path.join(rg.dir, dst))
    cells = []
    for kx, ky in rg.keys:
        c = build_tile(rg, kx, ky, ctx)
        if c:
            cells.append(c)
    json.dump({"name": rg.name, "e0": rg.e0, "n0": rg.n0, "z0": z0, "tile_px": TILE_PX, "cells": cells}, open(os.path.join(rg.dir, "world.json"), "w"), indent=1)
    print(f"world.json: {len(cells)} cells, z0 {z0:.0f} m")


def main():
    name, lat, lon, w, h = sys.argv[1], float(sys.argv[2]), float(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
    steps = sys.argv[6:] or ["fetch", "sheets", "build"]
    rg = Region(name, lat, lon, w, h)
    print(f"{name}: tiles E {rg.kx0}..{rg.kx0 + w - 1} N {rg.ky0}..{rg.ky0 + h - 1} km ({len(rg.keys)} tiles), origin LV95 {rg.e0} {rg.n0}", flush=True)
    if "fetch" in steps:
        fetch(rg)
    if "sheets" in steps:
        parse_sheets(rg)
    if "build" in steps:
        build(rg)
    if "refit" in steps:
        refit(rg)


if __name__ == "__main__":
    main()
