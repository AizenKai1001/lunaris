"""A real place as a Lunaris scene, from the open data `fetch.py` downloaded (USGS 3DEP elevation, USGS NAIP aerial imagery, OpenStreetMap):

    python tools/realmap/build.py NAME            ->  realmap-data\\NAME\\scene\\

  heightmap.png   the real ground (16 bits, one sample every metre, hydro-flattened water carved a little so that water can lie in it)
  terrain.jpg     the real aerial photograph of the same ground
  water.png       where OpenStreetMap says there is water: the surface height (0 = dry); the 3DEP DEM is flat at the water level there
  buildings.glb   the real footprints of OpenStreetMap, raised to their tagged height (or levels), roofs as tagged; the roof colour is the colour of the aerial photograph
                  under the footprint
  roads.glb       every OpenStreetMap way with a `highway` tag as a ribbon of its tagged (or class) width laid on the ground
  NAME.lunaris.json   the scene

Nothing here is modelled by hand: everything is a function of the survey data. Where the data says nothing (the colour of a wall, the shape of a roof that is not tagged) a plain default
is used and listed in the printout, so the gaps are visible: those are what lidar point clouds and official 3D city models are for.
"""
import json
import math
import os
import re
import struct
import sys
import xml.etree.ElementTree as ET

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

sys.path.insert(0, os.path.dirname(__file__))
import geo

Image.MAX_IMAGE_PIXELS = None
ROOT = r"realmap-data"

HIGHWAY_WIDTH = {
    "motorway": 12, "trunk": 11, "primary": 9.5, "secondary": 8.5, "tertiary": 7.5, "unclassified": 6, "residential": 6, "living_street": 5, "service": 4, "track": 3.2,
    "motorway_link": 6, "trunk_link": 6, "primary_link": 6, "secondary_link": 5.5, "tertiary_link": 5, "road": 5, "footway": 1.6, "path": 1.4, "pedestrian": 4, "cycleway": 2.2,
    "steps": 1.5, "bridleway": 2.0, "corridor": 1.5,
}
UNPAVED = {"unpaved", "gravel", "dirt", "ground", "grass", "compacted", "fine_gravel", "earth", "mud", "sand", "pebblestone"}


# ------------------------------------------------------------------------------------------------ the data
def load(name):
    d = os.path.join(ROOT, name)
    meta = json.load(open(os.path.join(d, "meta.json")))
    return d, meta


def read_dem(d, meta):
    n = int(round(meta["size_m"] / 1000))
    size = int(meta["size_m"])
    big = np.full((size, size), np.nan, dtype=np.float32)
    for t in meta["dem_tiles"]:
        a = np.array(Image.open(os.path.join(d, t["file"])), dtype=np.float32)
        a[a < -500] = np.nan
        r0 = (n - 1 - t["j"]) * 1000  # the picture's rows run from the north
        c0 = t["i"] * 1000
        big[r0 : r0 + 1000, c0 : c0 + 1000] = a
    bad = np.isnan(big)
    if bad.any():
        print("  elevation holes:", int(bad.sum()))
        big[bad] = np.nanmin(big)
    return big


def read_imagery(d, meta):
    n = int(round(meta["size_m"] / 1000))
    px = 2000
    big = Image.new("RGB", (n * px, n * px))
    for t in meta["imagery_tiles"]:
        big.paste(Image.open(os.path.join(d, t["file"])).convert("RGB"), (t["i"] * px, (n - 1 - t["j"]) * px))
    return big


class Frame:
    """UTM metres to the engine's x (east) and z (south), the window's centre being the origin."""

    def __init__(self, meta):
        self.zone = meta["zone"]
        self.north = meta["lat"] >= 0
        self.cx = meta["x0"] + meta["size_m"] / 2
        self.cy = meta["y0"] + meta["size_m"] / 2
        self.half = meta["size_m"] / 2

    def local(self, lat, lon):
        e, n = geo.forward(lat, lon, self.zone)
        return (e - self.cx, -(n - self.cy))


class Ground:
    """The ground as the engine has it: the samples every `step` metres, two triangles a cell, the diagonal from the north-east to the south-west corner."""

    def __init__(self, heights, size, step):
        self.h = heights[::step, ::step].astype(np.float64)
        self.cell = size / (self.h.shape[0] - 1)
        self.half = size / 2
        self.m = self.h.shape[0]

    def height(self, x, z):
        u = (x + self.half) / self.cell
        v = (z + self.half) / self.cell
        c = min(max(int(math.floor(u)), 0), self.m - 2)
        r = min(max(int(math.floor(v)), 0), self.m - 2)
        fx, fz = min(max(u - c, 0.0), 1.0), min(max(v - r, 0.0), 1.0)
        nw, ne, sw, se = self.h[r, c], self.h[r, c + 1], self.h[r + 1, c], self.h[r + 1, c + 1]
        if fx + fz <= 1.0:
            return nw + fx * (ne - nw) + fz * (sw - nw)
        return se + (1 - fx) * (sw - se) + (1 - fz) * (ne - se)


# ------------------------------------------------------------------------------------------------ OpenStreetMap
def read_osm(d, meta, frame):
    nodes, ways, rels = {}, {}, {}
    for f in meta["osm_files"]:
        root = ET.parse(os.path.join(d, f)).getroot()
        for el in root:
            if el.tag == "node":
                nodes[int(el.get("id"))] = (float(el.get("lat")), float(el.get("lon")), {t.get("k"): t.get("v") for t in el.findall("tag")})
            elif el.tag == "way":
                ways[int(el.get("id"))] = ([int(n.get("ref")) for n in el.findall("nd")], {t.get("k"): t.get("v") for t in el.findall("tag")})
            elif el.tag == "relation":
                rels[int(el.get("id"))] = ([(m.get("type"), int(m.get("ref")), m.get("role")) for m in el.findall("member")], {t.get("k"): t.get("v") for t in el.findall("tag")})
    loc = {i: frame.local(n[0], n[1]) for i, n in nodes.items()}
    print(f"  openstreetmap: {len(nodes)} nodes, {len(ways)} ways, {len(rels)} relations")
    return nodes, ways, rels, loc


def way_points(way, loc):
    pts = [loc[n] for n in way[0] if n in loc]
    return pts


def rings_of(rel, ways, loc):
    """The outer rings of a multipolygon relation (the way pieces joined end to end)."""
    pieces = [list(ways[m[1]][0]) for m in rel[0] if m[0] == "way" and m[2] in ("outer", "") and m[1] in ways]
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
            rings.append([loc[n] for n in ring if n in loc])
    return rings


def number(v, default=None):
    if v is None:
        return default
    m = re.match(r"\s*([0-9]+(?:[.,][0-9]+)?)\s*(m|ft|'|feet)?", v)
    if not m:
        return default
    x = float(m.group(1).replace(",", "."))
    return x * 0.3048 if m.group(2) in ("ft", "'", "feet") else x


def hex_color(v):
    if not v:
        return None
    names = {"white": (0.9, 0.9, 0.88), "grey": (0.55, 0.55, 0.55), "gray": (0.55, 0.55, 0.55), "red": (0.55, 0.18, 0.14), "brown": (0.38, 0.26, 0.18), "beige": (0.78, 0.72, 0.58), "yellow": (0.8, 0.7, 0.3), "black": (0.12, 0.12, 0.12), "blue": (0.25, 0.35, 0.6), "green": (0.3, 0.45, 0.3), "orange": (0.7, 0.4, 0.2)}
    if v.lower() in names:
        return names[v.lower()]
    m = re.match(r"#?([0-9a-fA-F]{6})$", v.strip())
    if m:
        h = m.group(1)
        return tuple(int(h[i : i + 2], 16) / 255.0 for i in (0, 2, 4))
    return None


# ------------------------------------------------------------------------------------------------ geometry
class MeshOut:
    def __init__(self):
        self.pos, self.nrm, self.col, self.idx = [], [], [], []

    def tri(self, a, b, c, color):
        n = np.cross(np.subtract(b, a), np.subtract(c, a))
        ln = float(np.linalg.norm(n))
        n = (n / ln).tolist() if ln > 1e-12 else [0.0, 1.0, 0.0]
        base = len(self.pos)
        for p in (a, b, c):
            self.pos.append([float(p[0]), float(p[1]), float(p[2])])
            self.nrm.append(n)
            self.col.append(list(color))
        self.idx += [base, base + 1, base + 2]

    def quad(self, a, b, c, d, color):
        self.tri(a, b, c, color)
        self.tri(a, c, d, color)

    def write_glb(self, path):
        if not self.pos:
            self.tri([0, 0, 0], [0, 0.001, 0], [0.001, 0, 0], (0.5, 0.5, 0.5))
        pos = np.array(self.pos, dtype="<f4")
        nrm = np.array(self.nrm, dtype="<f4")
        col = np.array(self.col, dtype="<f4")
        idx = np.array(self.idx, dtype="<u4")
        chunks = []
        views = []
        accessors = []

        def add(arr, target, comp, kind, count, mn=None, mx=None):
            off = sum(len(c) for c in chunks)
            raw = arr.tobytes()
            chunks.append(raw + b"\x00" * ((4 - len(raw) % 4) % 4))
            views.append({"buffer": 0, "byteOffset": off, "byteLength": len(raw), "target": target})
            acc = {"bufferView": len(views) - 1, "componentType": comp, "count": count, "type": kind}
            if mn is not None:
                acc["min"], acc["max"] = mn, mx
            accessors.append(acc)
            return len(accessors) - 1

        a_pos = add(pos, 34962, 5126, "VEC3", len(pos), pos.min(axis=0).tolist(), pos.max(axis=0).tolist())
        a_nrm = add(nrm, 34962, 5126, "VEC3", len(nrm))
        a_col = add(col, 34962, 5126, "VEC3", len(col))
        a_idx = add(idx, 34963, 5125, "SCALAR", len(idx))
        binary = b"".join(chunks)
        gltf = {
            "asset": {"version": "2.0", "generator": "lunaris realmap"},
            "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": a_pos, "NORMAL": a_nrm, "COLOR_0": a_col}, "indices": a_idx, "material": 0}]}],
            "materials": [{"pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.0, "roughnessFactor": 0.9}}],
            "buffers": [{"byteLength": len(binary)}], "bufferViews": views, "accessors": accessors,
        }
        js = json.dumps(gltf, separators=(",", ":")).encode()
        js += b" " * ((4 - len(js) % 4) % 4)
        total = 12 + 8 + len(js) + 8 + len(binary)
        with open(path, "wb") as f:
            f.write(struct.pack("<4sII", b"glTF", 2, total))
            f.write(struct.pack("<I4s", len(js), b"JSON") + js)
            f.write(struct.pack("<I4s", len(binary), b"BIN\x00") + binary)


def signed_area(poly):
    return 0.5 * sum(poly[i][0] * poly[(i + 1) % len(poly)][1] - poly[(i + 1) % len(poly)][0] * poly[i][1] for i in range(len(poly)))


def ear_clip(poly):
    """Triangles (index triples) of a simple polygon given counter-clockwise."""
    idx = list(range(len(poly)))
    out = []

    def cp(p, q, r):
        return (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])

    def cross(o, a, b):
        return cp(poly[o], poly[a], poly[b])

    def inside(p, a, b, c):
        pa, pb, pc = poly[a], poly[b], poly[c]
        return cp(pa, pb, p) >= -1e-9 and cp(pb, pc, p) >= -1e-9 and cp(pc, pa, p) >= -1e-9

    guard = 0
    while len(idx) > 3 and guard < 10000:
        guard += 1
        clipped = False
        for k in range(len(idx)):
            a, b, c = idx[k - 1], idx[k], idx[(k + 1) % len(idx)]
            if cross(a, b, c) <= 1e-9:
                continue
            if any(inside(poly[o], a, b, c) for o in idx if o not in (a, b, c)):
                continue
            out.append((a, b, c))
            idx.pop(k)
            clipped = True
            break
        if not clipped:
            break
    if len(idx) == 3:
        out.append(tuple(idx))
    return out


def dedupe(poly):
    out = []
    for p in poly:
        if not out or math.hypot(p[0] - out[-1][0], p[1] - out[-1][1]) > 0.05:
            out.append(p)
    if len(out) > 1 and math.hypot(out[0][0] - out[-1][0], out[0][1] - out[-1][1]) < 0.05:
        out.pop()
    return out


def roof_color_from(img, scale, frame, poly):
    """The mean colour of the aerial photograph under a footprint (a real measurement of the roof)."""
    xs = [p[0] for p in poly]
    zs = [p[1] for p in poly]
    x0, x1 = int((min(xs) + frame.half) * scale), int((max(xs) + frame.half) * scale) + 1
    z0, z1 = int((min(zs) + frame.half) * scale), int((max(zs) + frame.half) * scale) + 1
    x0, z0 = max(x0, 0), max(z0, 0)
    x1, z1 = min(x1, img.width), min(z1, img.height)
    if x1 - x0 < 2 or z1 - z0 < 2:
        return None
    crop = img.crop((x0, z0, x1, z1))
    mask = Image.new("L", crop.size, 0)
    ImageDraw.Draw(mask).polygon([((p[0] + frame.half) * scale - x0, (p[1] + frame.half) * scale - z0) for p in poly], fill=255)
    m = np.array(mask) > 0
    if m.sum() < 4:
        return None
    a = np.array(crop, dtype=np.float32)[m] / 255.0
    lin = np.power(a, 2.2).mean(axis=0)
    return tuple(np.power(lin, 1 / 2.2).tolist())


def build_buildings(nodes, ways, rels, loc, ground, frame, aerial, scale, notes):
    out = MeshOut()
    count = 0
    footprints = []
    for wid, w in ways.items():
        tags = w[1]
        if "building" in tags and tags["building"] != "no" and w[0][0] == w[0][-1]:
            footprints.append((tags, [loc[n] for n in w[0] if n in loc]))
    for rid, r in rels.items():
        tags = r[1]
        if "building" in tags and tags.get("type") == "multipolygon":
            for ring in rings_of(r, ways, loc):
                footprints.append((tags, ring))
    half = frame.half
    for tags, poly in footprints:
        poly = dedupe(poly)
        if len(poly) < 3:
            continue
        if signed_area(poly) < 0:
            poly = poly[::-1]
        # the window's edge: leave out a building whose every corner is outside
        if all(abs(p[0]) > half + 20 or abs(p[1]) > half + 20 for p in poly):
            continue
        kind = tags.get("building", "yes")
        floors = number(tags.get("building:levels"))
        height = number(tags.get("height")) or number(tags.get("building:height"))
        if height is None and floors:
            height = floors * 3.1
        if height is None:
            height = {"house": 6.5, "residential": 7.0, "detached": 6.5, "garage": 3.0, "garages": 3.0, "shed": 2.8, "roof": 3.5, "barn": 7.0, "cabin": 5.0, "church": 10.0, "hotel": 10.0, "commercial": 8.0, "retail": 7.0, "apartments": 11.0, "industrial": 8.0}.get(kind, 6.0)
            notes["untagged building height"] = notes.get("untagged building height", 0) + 1
        roof_shape = tags.get("roof:shape")
        roof_h = number(tags.get("roof:height"))
        mnh = number(tags.get("min_height"), 0.0)
        ground_h = [ground.height(p[0], p[1]) for p in poly]
        base = min(ground_h)
        eave = base + height
        roof_rgb = roof_color_from(aerial, scale, frame, poly)
        if roof_rgb is None:
            roof_rgb = hex_color(tags.get("roof:colour")) or (0.25, 0.25, 0.27)
            notes["no roof colour from photo"] = notes.get("no roof colour from photo", 0) + 1
        if tags.get("roof:colour") and hex_color(tags["roof:colour"]):
            roof_rgb = hex_color(tags["roof:colour"])
        wall_rgb = hex_color(tags.get("building:colour")) or (0.74, 0.72, 0.68)
        if not tags.get("building:colour"):
            notes["wall colour default"] = notes.get("wall colour default", 0) + 1
        # walls: from the ground (each corner on its own ground) up to a flat eave
        n = len(poly)
        for i in range(n):
            a, b = poly[i], poly[(i + 1) % n]
            if mnh > 0:
                ba = base + mnh
                bb = base + mnh
            else:
                ba, bb = ground_h[i] - 0.25, ground_h[(i + 1) % n] - 0.25
            out.quad([a[0], ba, a[1]], [b[0], bb, b[1]], [b[0], eave, b[1]], [a[0], eave, a[1]], wall_rgb)
        # the roof: gabled on a four-cornered footprint that is a near rectangle and is tagged so; flat otherwise
        rect = len(poly) == 4
        if rect:
            e0 = (poly[1][0] - poly[0][0], poly[1][1] - poly[0][1])
            e1 = (poly[2][0] - poly[1][0], poly[2][1] - poly[1][1])
            l0, l1 = math.hypot(*e0), math.hypot(*e1)
            cosang = abs(e0[0] * e1[0] + e0[1] * e1[1]) / max(l0 * l1, 1e-9)
            rect = cosang < 0.26 and min(l0, l1) > 2.5
        if rect and roof_shape in ("gabled", "hipped", "gambrel", "half-hipped"):
            if l0 >= l1:
                r0, r1 = ((poly[0][0] + poly[3][0]) / 2, (poly[0][1] + poly[3][1]) / 2), ((poly[1][0] + poly[2][0]) / 2, (poly[1][1] + poly[2][1]) / 2)
                width = l1
            else:
                r0, r1 = ((poly[0][0] + poly[1][0]) / 2, (poly[0][1] + poly[1][1]) / 2), ((poly[2][0] + poly[3][0]) / 2, (poly[2][1] + poly[3][1]) / 2)
                width = l0
            rh = roof_h if roof_h else width / 2 * math.tan(math.radians(28))
            top = eave + rh
            if roof_shape == "hipped" and max(l0, l1) > width:
                # pull the ridge ends in by half the width
                dx, dz = r1[0] - r0[0], r1[1] - r0[1]
                ln = math.hypot(dx, dz)
                ux, uz = dx / ln, dz / ln
                inset = min(width / 2, ln / 2 - 0.1)
                r0 = (r0[0] + ux * inset, r0[1] + uz * inset)
                r1 = (r1[0] - ux * inset, r1[1] - uz * inset)
            P = [[p[0], eave, p[1]] for p in poly]
            R0, R1 = [r0[0], top, r0[1]], [r1[0], top, r1[1]]
            if l0 >= l1:
                out.quad(P[0], P[1], R1, R0, roof_rgb)
                out.quad(P[2], P[3], R0, R1, roof_rgb)
                out.tri(P[1], P[2], R1, roof_rgb) if roof_shape != "hipped" else out.tri(P[1], P[2], R1, roof_rgb)
                out.tri(P[3], P[0], R0, roof_rgb)
            else:
                out.quad(P[1], P[2], R1, R0, roof_rgb)
                out.quad(P[3], P[0], R0, R1, roof_rgb)
                out.tri(P[0], P[1], R0, roof_rgb)
                out.tri(P[2], P[3], R1, roof_rgb)
        else:
            for a, b, c in ear_clip(poly):
                out.tri([poly[a][0], eave, poly[a][1]], [poly[c][0], eave, poly[c][1]], [poly[b][0], eave, poly[b][1]], roof_rgb)
            if not roof_shape:
                notes["flat roof (shape not tagged)"] = notes.get("flat roof (shape not tagged)", 0) + 1
        count += 1
    print(f"  buildings: {count}, {len(out.idx) // 3} triangles")
    return out, count


def build_roads(ways, loc, ground, frame, notes):
    out = MeshOut()
    count = 0
    length = 0.0
    for wid, w in ways.items():
        tags = w[1]
        hw = tags.get("highway")
        if not hw or hw in ("proposed", "construction", "raceway"):
            continue
        pts = way_points(w, loc)
        if len(pts) < 2:
            continue
        width = number(tags.get("width")) or HIGHWAY_WIDTH.get(hw)
        if width is None:
            width = 4.0
            notes["road class without a width"] = notes.get("road class without a width", 0) + 1
        lanes = number(tags.get("lanes"))
        if lanes and not tags.get("width"):
            width = max(width, lanes * 3.2)
        surface = tags.get("surface", "")
        if surface in UNPAVED or hw == "track":
            color = (0.16, 0.13, 0.10)
        elif hw in ("footway", "path", "cycleway", "pedestrian", "steps", "bridleway"):
            color = (0.22, 0.20, 0.17) if surface not in ("asphalt", "paved") else (0.12, 0.12, 0.13)
        else:
            color = (0.17, 0.17, 0.18)
        lift = 0.10 + 0.004 * (width / 3.0)
        # subdivide so the ribbon follows the ground
        dense = [pts[0]]
        for a, b in zip(pts, pts[1:]):
            d = math.hypot(b[0] - a[0], b[1] - a[1])
            k = max(1, int(math.ceil(d / 3.0)))
            for s in range(1, k + 1):
                dense.append((a[0] + (b[0] - a[0]) * s / k, a[1] + (b[1] - a[1]) * s / k))
        left, right = [], []
        for i, p in enumerate(dense):
            q0 = dense[max(i - 1, 0)]
            q1 = dense[min(i + 1, len(dense) - 1)]
            dx, dz = q1[0] - q0[0], q1[1] - q0[1]
            ln = math.hypot(dx, dz) or 1.0
            nx, nz = -dz / ln, dx / ln
            for sign, side in ((1, left), (-1, right)):
                x, z = p[0] + nx * width / 2 * sign, p[1] + nz * width / 2 * sign
                side.append([x, ground.height(x, z) + lift, z])
        for i in range(len(dense) - 1):
            out.quad(left[i], left[i + 1], right[i + 1], right[i], color)
        count += 1
        length += sum(math.hypot(b[0] - a[0], b[1] - a[1]) for a, b in zip(pts, pts[1:]))
    print(f"  roads: {count} ways, {length / 1000:.1f} km, {len(out.idx) // 3} triangles")
    return out


def rasterize_water(nodes, ways, rels, loc, frame, size_px, dem_px):
    mask = Image.new("L", (size_px, size_px), 0)
    dr = ImageDraw.Draw(mask)
    s = (size_px - 1) / (2 * frame.half)

    def px(p):
        return ((p[0] + frame.half) * s, (p[1] + frame.half) * s)

    n_poly = n_line = 0
    for wid, w in ways.items():
        t = w[1]
        pts = way_points(w, loc)
        if len(pts) < 2:
            continue
        if (t.get("natural") == "water" or t.get("landuse") in ("reservoir", "basin") or t.get("waterway") == "riverbank") and w[0][0] == w[0][-1]:
            dr.polygon([px(p) for p in pts], fill=255)
            n_poly += 1
        elif t.get("waterway") in ("river", "stream", "canal", "brook", "drain", "ditch"):
            wd = number(t.get("width")) or {"river": 14, "canal": 8, "stream": 3.5, "brook": 2.5}.get(t["waterway"], 1.5)
            if t["waterway"] in ("ditch", "drain"):
                continue
            dr.line([px(p) for p in pts], fill=255, width=max(2, int(wd * s)))
            n_line += 1
    for rid, r in rels.items():
        t = r[1]
        if t.get("natural") == "water" and t.get("type") == "multipolygon":
            for ring in rings_of(r, ways, loc):
                dr.polygon([px(p) for p in ring], fill=255)
                n_poly += 1
    print(f"  water: {n_poly} areas, {n_line} streams and rivers")
    return mask


# ------------------------------------------------------------------------------------------------ main
def main():
    name = sys.argv[1]
    d, meta = load(name)
    size = int(meta["size_m"])
    out = os.path.join(d, "scene")
    os.makedirs(out, exist_ok=True)
    frame = Frame(meta)
    notes = {}
    print("elevation")
    dem = read_dem(d, meta)  # [size, size] pixel-is-area at one metre, row 0 north
    # the engine's samples are on cell corners: (size + 1) of them one metre apart; bilinear from the pixel centres
    pad = np.pad(dem, 1, mode="edge")
    n = size + 1
    corner = 0.25 * (pad[0:n, 0:n] + pad[0:n, 1 : n + 1] + pad[1 : n + 1, 0:n] + pad[1 : n + 1, 1 : n + 1])
    corner = corner.astype(np.float32)
    lo, hi = float(corner.min()), float(corner.max())
    print(f"  ground {lo:.1f} to {hi:.1f} m above sea level, {size} m across")
    print("openstreetmap")
    nodes, ways, rels, loc = read_osm(d, meta, frame)
    print("water")
    wmask = rasterize_water(nodes, ways, rels, loc, frame, n, corner)
    wm = np.array(wmask, dtype=np.float32) / 255.0
    # carve the ground under the water (the hydro-flattened DEM is at the water surface there): 1.5 m deep, a soft bank
    soft = np.array(Image.fromarray((wm * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(4.0)), dtype=np.float32) / 255.0
    water_level = np.where(wm > 0.5, corner, 0.0)
    carved = corner - 1.6 * np.maximum(soft, wm)
    lo2 = float(carved.min())
    relief = hi - lo2
    h16 = np.clip((carved - lo2) / relief * 65535.0 + 0.5, 0, 65535).astype("<u2")
    Image.fromarray(h16).save(os.path.join(out, "heightmap.png"))
    w16 = np.where(wm > 0.5, np.clip((water_level - lo2) / relief * 65535.0 + 0.5, 1, 65535), 0).astype("<u2")
    Image.fromarray(w16).save(os.path.join(out, "water.png"))
    ground = Ground(carved - lo2, float(size), 2)
    print("aerial photograph")
    aerial = read_imagery(d, meta)
    scale = aerial.width / size
    tex = aerial.resize((4096, 4096), Image.LANCZOS)
    tex.save(os.path.join(out, "terrain.jpg"), quality=93)
    print("buildings")
    bmesh, bcount = build_buildings(nodes, ways, rels, loc, ground, frame, aerial, scale, notes)
    bmesh.write_glb(os.path.join(out, "buildings.glb"))
    print("roads")
    rmesh = build_roads(ways, loc, ground, frame, notes)
    rmesh.write_glb(os.path.join(out, "roads.glb"))
    info = {"name": name, "lo": lo2, "relief": relief, "size": size, "buildings": bcount, "notes": notes}
    json.dump(info, open(os.path.join(out, "info.json"), "w"), indent=1)
    print("gaps in the data (defaults used):", json.dumps(notes))
    print("done:", out)


if __name__ == "__main__":
    main()
