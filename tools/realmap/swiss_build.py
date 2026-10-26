"""A Swiss place as a Lunaris scene from the swisstopo data `swiss_fetch.py` downloaded:

    python tools/realmap/swiss_build.py NAME

  heightmap.png        swissALTI3D, 0.5 m (4001 x 4001 samples for the 2 km window)
  terrain.jpg          swissIMAGE, 10 cm, brought to 8192 x 8192 (24 cm) for the one texture the ground can carry today
  water.png / roads.glb  OpenStreetMap (as for the other places)
  buildings.glb        swissBUILDINGS3D 3.0: the real LoD2 surfaces of every building (roof, wall and ground polygons, triangulated). Roofs carry the aerial photograph projected from above
                       (so the real roof tiles show), walls weathered timber or render (the survey has no wall material; real CC0 photographs, each building tinted)
  trees.json           tree tops from the surface model minus the terrain (0.5 m), outside the buildings
  NAME.lunaris.json    the scene

The local frame is the Swiss grid: x east, z south, the window's centre at the origin, y metres above the window's lowest ground.
"""
import json
import math
import os
import shutil
import struct
import sys
import xml.etree.ElementTree as ET
import zipfile

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

sys.path.insert(0, os.path.dirname(__file__))
import build as B  # noqa: E402
from swiss_fetch import lv95  # noqa: E402

Image.MAX_IMAGE_PIXELS = None
ROOT = B.ROOT
TILE_PX = int(os.environ.get("TILE_PX", "8192"))  # the aerial photograph of a 1 km ground tile, a side in pixels (the engine takes 16384; swissIMAGE has 10,000 for 1 km)
WOOD_M, PLASTER_M = 2.0, 1.0  # metres a wall texture covers before it repeats (the Poly Haven photographs: Dark Planks 2 m square, White Plaster 02 1 m)
WOOD_TEXTURE = os.path.join(ROOT, "textures", "dark_planks", "dark_planks_diff_2k.jpg")  # Poly Haven 'Dark Planks' (CC0)
PLASTER_TEXTURE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "examples", "hollowlight", "textures", "white_plaster_02", "white_plaster_02_diff.jpg")  # Poly Haven 'White Plaster 02' (CC0)


class SwissFrame:
    def __init__(self, meta):
        self.cx = meta["x0"] + meta["size_m"] / 2
        self.cy = meta["y0"] + meta["size_m"] / 2
        self.half = meta["size_m"] / 2

    def local(self, lat, lon):
        e, n = lv95(lat, lon)
        return (e - self.cx, -(n - self.cy))


def read_tiles(raw, files, px=2000, tile_m=1000, x0=0, y0=0, n_tiles=2, scale=1):
    size = n_tiles * px
    out = np.full((size, size), np.nan, dtype=np.float32)
    for t in files:
        a = np.array(Image.open(os.path.join(raw, t["file"])), dtype=np.float32)
        a[(a < -500) | (a > 9000)] = np.nan
        i, j = t["kx"] - x0 // tile_m, t["ky"] - y0 // tile_m
        r0 = (n_tiles - 1 - j) * px
        out[r0 : r0 + px, i * px : (i + 1) * px] = a
    return out


def read_citygml(raw, meta, frame, lo):
    """Every building as lists of triangles: roofs, walls, ground, in the local frame."""
    buildings = []
    for f in meta["files"]["buildings"]:
        z = zipfile.ZipFile(os.path.join(raw, f["file"]))
        for name in z.namelist():
            if not name.lower().endswith((".gml", ".xml")):
                continue
            with z.open(name) as fh:
                for ev, el in ET.iterparse(fh, events=("end",)):
                    tag = el.tag.rsplit("}", 1)[-1]
                    if tag != "Building":
                        continue
                    polys = {"roof": [], "wall": [], "ground": []}
                    for sub in el.iter():
                        st = sub.tag.rsplit("}", 1)[-1]
                        kind = {"RoofSurface": "roof", "WallSurface": "wall", "GroundSurface": "ground", "OuterCeilingSurface": "roof", "OuterFloorSurface": "ground", "ClosureSurface": "wall"}.get(st)
                        if not kind:
                            continue
                        for pl in sub.iter():
                            if pl.tag.rsplit("}", 1)[-1] != "Polygon":
                                continue
                            ext = next((x for x in pl if x.tag.rsplit("}", 1)[-1] == "exterior"), None)
                            if ext is None:
                                continue
                            pos = next((x for x in ext.iter() if x.tag.rsplit("}", 1)[-1] == "posList"), None)
                            if pos is None or not pos.text:
                                continue
                            v = np.array(pos.text.split(), dtype=np.float64).reshape(-1, 3)
                            if len(v) > 1 and np.allclose(v[0], v[-1]):
                                v = v[:-1]
                            if len(v) >= 3:
                                polys[kind].append(v)
                    if polys["roof"] or polys["wall"]:
                        buildings.append(polys)
                    el.clear()
    print(f"  buildings in the CityGML: {len(buildings)}")
    return buildings


def tri_polygon(v, flip_hand=True):
    """Triangles (index triples into v, oriented like the ring) of a planar 3D polygon (any orientation)."""
    n = np.zeros(3)
    for i in range(len(v)):
        a, b = v[i], v[(i + 1) % len(v)]
        n += np.array([(a[1] - b[1]) * (a[2] + b[2]), (a[2] - b[2]) * (a[0] + b[0]), (a[0] - b[0]) * (a[1] + b[1])])
    ax = int(np.argmax(np.abs(n)))
    keep = [i for i in range(3) if i != ax]
    pts = [(float(p[keep[0]]), float(p[keep[1]])) for p in v]
    area = B.signed_area(pts)
    idx = list(range(len(v)))
    if area < 0:
        idx = idx[::-1]
        pts = pts[::-1]
    tris = B.ear_clip(pts)
    out = []
    for a, b, c in tris:
        t = (idx[a], idx[b], idx[c])
        if area < 0:
            t = (t[0], t[2], t[1])
        out.append(t)
    return out


def write_gltf(path_base, prims, images, n_roof):
    """A .gltf: one textured primitive for the roofs of each ground tile (material i, image i, UVs over the tile's photograph) and one for each kind of wall (the images after the roofs'; UVs in
    wall metres, so the texture repeats; vertex colours tint each building)."""
    binary = bytearray()
    views, accs, primitives = [], [], []

    def add(arr, target, comp, kind, count, mn=None, mx=None):
        while len(binary) % 4:
            binary.append(0)
        off = len(binary)
        raw = arr.tobytes()
        binary.extend(raw)
        views.append({"buffer": 0, "byteOffset": off, "byteLength": len(raw), "target": target})
        acc = {"bufferView": len(views) - 1, "componentType": comp, "count": count, "type": kind}
        if mn is not None:
            acc["min"], acc["max"] = mn, mx
        accs.append(acc)
        return len(accs) - 1

    mats = [{"name": f"roofs {i}", "pbrMetallicRoughness": {"baseColorTexture": {"index": i}, "metallicFactor": 0.0, "roughnessFactor": 0.85}} for i in range(n_roof)]
    mats += [{"name": f"walls {i - n_roof}", "pbrMetallicRoughness": {"baseColorTexture": {"index": i}, "metallicFactor": 0.0, "roughnessFactor": 0.9}} for i in range(n_roof, len(images))]
    for p in prims:
        pos = np.array(p["pos"], dtype="<f4").reshape(-1, 3)
        if len(pos) == 0:
            continue
        nrm = np.array(p["nrm"], dtype="<f4").reshape(-1, 3)
        idx = np.array(p["idx"], dtype="<u4")
        attrs = {"POSITION": add(pos, 34962, 5126, "VEC3", len(pos), pos.min(axis=0).tolist(), pos.max(axis=0).tolist()), "NORMAL": add(nrm, 34962, 5126, "VEC3", len(nrm))}
        if "uv" in p:
            uv = np.array(p["uv"], dtype="<f4").reshape(-1, 2)
            attrs["TEXCOORD_0"] = add(uv, 34962, 5126, "VEC2", len(uv))
        if "col" in p:
            col = np.array(p["col"], dtype="<f4").reshape(-1, 3)
            attrs["COLOR_0"] = add(col, 34962, 5126, "VEC3", len(col))
        primitives.append({"attributes": attrs, "indices": add(idx, 34963, 5125, "SCALAR", len(idx)), "material": p["mat"]})
    gltf = {"asset": {"version": "2.0", "generator": "lunaris realmap swiss_build"}, "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}], "meshes": [{"primitives": primitives}], "materials": mats,
            "images": [{"uri": u} for u in images], "textures": [{"source": i, "sampler": 0 if i < n_roof else 1} for i in range(len(images))],
            "samplers": [{"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}, {"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}],
            "buffers": [{"uri": os.path.basename(path_base) + ".bin", "byteLength": len(binary)}], "bufferViews": views, "accessors": accs}
    open(path_base + ".bin", "wb").write(bytes(binary))
    json.dump(gltf, open(path_base + ".gltf", "w"))


def main():
    name = sys.argv[1]
    d = os.path.join(ROOT, name)
    meta = json.load(open(os.path.join(d, "meta.json")))
    raw = os.path.join(d, "raw")
    out = os.path.join(d, "scene")
    os.makedirs(out, exist_ok=True)
    frame = SwissFrame(meta)
    size = int(meta["size_m"])
    x0, y0 = int(meta["x0"]), int(meta["y0"])
    print("terrain 0.5 m")
    dtm = read_tiles(raw, meta["files"]["dtm"], x0=x0, y0=y0)
    dsm = read_tiles(raw, meta["files"]["dsm"], x0=x0, y0=y0)
    for a in (dtm, dsm):
        bad = np.isnan(a)
        if bad.any():
            a[bad] = np.nanmin(dtm)
    n = 4001
    pad = np.pad(dtm, 1, mode="edge")
    corner = (0.25 * (pad[0:n, 0:n] + pad[0:n, 1 : n + 1] + pad[1 : n + 1, 0:n] + pad[1 : n + 1, 1 : n + 1])).astype(np.float32)
    nodes, ways, rels, loc = B.read_osm(raw, meta, frame)
    print("water")
    wmask = B.rasterize_water(nodes, ways, rels, loc, frame, n, corner)
    wm = np.array(wmask, dtype=np.float32) / 255.0
    soft = np.array(Image.fromarray((wm * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(6.0)), dtype=np.float32) / 255.0
    # a stream's surface is the bed's own level here (swissALTI3D is hydro-flattened where there is water): carve 1 m
    carved = corner - 1.2 * np.maximum(soft, wm)
    lo, hi = float(carved.min()), float(corner.max())
    relief = hi - lo
    print(f"  ground {lo:.1f} to {hi:.1f} m, relief {relief:.0f} m")
    h16 = np.clip((carved - lo) / relief * 65535.0 + 0.5, 0, 65535).astype("<u2")
    w16 = np.where(wm > 0.5, np.clip((corner - lo) / relief * 65535.0 + 0.5, 1, 65535), 0).astype("<u2")
    Image.fromarray(w16).save(os.path.join(out, "water.png"))
    ground = B.Ground(carved - lo, float(size), 2)
    # the ground is four tiles of 1 km (a tile each carries its own photograph, TILE_PX a side: 12 cm a pixel at 8192; they share their edge samples, so there is no crack)
    print("ground tiles and the aerial photograph 10 cm")
    NT = 2
    tile_m = size // NT
    tiles = []
    for j in range(NT):
        for i in range(NT):
            Image.fromarray(h16[j * 2000 : (j + 1) * 2000 + 1, i * 2000 : (i + 1) * 2000 + 1]).save(os.path.join(out, f"heightmap_{i}_{j}.png"))
            kx, ky = x0 // 1000 + i, y0 // 1000 + (NT - 1 - j)
            t = next(t for t in meta["files"]["image"] if t["kx"] == kx and t["ky"] == ky)
            im = Image.open(os.path.join(raw, t["file"])).convert("RGB").resize((TILE_PX, TILE_PX), Image.LANCZOS)
            im.save(os.path.join(out, f"terrain_{i}_{j}.jpg"), quality=94)
            # the same photograph at lower resolutions for the far levels of a streamed world (world.py)
            for lvl, px in (("l1", 2048), ("l2", 512)):
                im.resize((px, px), Image.LANCZOS).save(os.path.join(out, f"terrain_{i}_{j}_{lvl}.jpg"), quality=92)
            # the splat map of this tile (what the ground is made of, from the real photograph and the real slope): R grass, G rock/gravel road, B forest floor, A bare ground
            small = np.asarray(im.resize((1001, 1001), Image.LANCZOS), dtype=np.float32) / 255.0
            hs = carved[j * 2000 : (j + 1) * 2000 + 1 : 2, i * 2000 : (i + 1) * 2000 + 1 : 2]
            gy, gx = np.gradient(hs.astype(np.float64), 1.0)
            slope = np.degrees(np.arctan(np.hypot(gx, gy)))
            r_, g_, b_ = small[..., 0], small[..., 1], small[..., 2]
            v = small.max(axis=2)
            exg = 2 * g_ - r_ - b_
            ss = lambda a, b, x: np.clip((x - a) / (b - a), 0, 1) ** 2 * (3 - 2 * np.clip((x - a) / (b - a), 0, 1))
            veg = ss(0.0, 0.07, exg)
            w_grass = veg * ss(0.22, 0.38, v)
            w_forest = veg * (1 - ss(0.22, 0.38, v))
            w_rock = np.clip(ss(30, 46, slope) + (1 - veg) * ss(0.30, 0.55, v) * 0.9, 0, 1)
            w_bare = (1 - veg) * (1 - ss(0.30, 0.55, v)) + 0.02
            wts = np.stack([w_grass + 0.02, w_rock, w_forest, w_bare], axis=2)
            wts = wts / wts.sum(axis=2, keepdims=True)
            Image.fromarray((wts * 255 + 0.5).astype(np.uint8), "RGBA").save(os.path.join(out, f"splat_{i}_{j}.png"))
            del im
            tiles.append({"i": i, "j": j, "cx": (i + 0.5) * tile_m - size / 2, "cz": (j + 0.5) * tile_m - size / 2, "size": tile_m, "heightmap": f"heightmap_{i}_{j}.png", "texture": f"terrain_{i}_{j}.jpg", "splat": f"splat_{i}_{j}.png"})
            print("  tile", i, j, flush=True)
    print("buildings (swissBUILDINGS3D)")
    blds = read_citygml(raw, meta, frame, lo)
    roofs = [{"pos": [], "nrm": [], "uv": [], "idx": [], "mat": k} for k in range(NT * NT)]
    # the survey has no wall material: a share of the buildings get weathered timber (the alpine chalet), the rest render; real CC0 photographs of both (Poly Haven), repeating every WOOD_M or PLASTER_M metres
    wood = {"pos": [], "nrm": [], "uv": [], "col": [], "idx": [], "mat": NT * NT, "m": WOOD_M}
    plaster = {"pos": [], "nrm": [], "uv": [], "col": [], "idx": [], "mat": NT * NT + 1, "m": PLASTER_M}
    # the same buildings again for each ground tile (the streamed world: a cell is a tile and loads its own buildings), positions relative to the tile's centre, the roofs with that tile's photograph
    cell_roofs = [{"pos": [], "nrm": [], "uv": [], "idx": [], "mat": 0} for _ in range(NT * NT)]
    cell_wood = [{"pos": [], "nrm": [], "uv": [], "col": [], "idx": [], "mat": 1, "m": WOOD_M} for _ in range(NT * NT)]
    cell_plaster = [{"pos": [], "nrm": [], "uv": [], "col": [], "idx": [], "mat": 2, "m": PLASTER_M} for _ in range(NT * NT)]
    shutil.copy(WOOD_TEXTURE, os.path.join(out, "walls_wood.jpg"))
    shutil.copy(PLASTER_TEXTURE, os.path.join(out, "walls_plaster.jpg"))
    prints = []
    nb = 0
    for b in blds:
        allv = np.concatenate([v for k in b for v in b[k]])
        cen = allv.mean(axis=0)
        cxm, czm = cen[0] - frame.cx, -(cen[1] - frame.cy)
        if abs(cxm) > frame.half + 5 or abs(czm) > frame.half + 5:
            continue
        nb += 1
        bi = (0 if cxm < 0 else 1) + NT * (0 if czm < 0 else 1)
        hb = ((int(round(cxm * 10)) * 73856093) ^ (int(round(czm * 10)) * 19349663)) & 0xFFFF
        walls = wood if hb % 100 < 62 else plaster
        shade = 0.82 + 0.36 * ((hb >> 7) % 64) / 63.0
        tint = [shade * 1.0, shade * 0.97, shade * 0.93] if walls is plaster else [shade] * 3
        for kind in ("roof", "wall", "ground"):
            for v in b[kind]:
                loc3 = np.stack([v[:, 0] - frame.cx, v[:, 2] - lo, -(v[:, 1] - frame.cy)], axis=1)
                if kind == "ground":
                    prints.append([(p[0], p[2]) for p in loc3])
                    continue
                for (a, bb, c) in tri_polygon(v):
                    P = [loc3[a], loc3[bb], loc3[c]]
                    nrm = np.cross(P[1] - P[0], P[2] - P[0])
                    ln = np.linalg.norm(nrm)
                    if ln < 1e-9:
                        continue
                    nrm = nrm / ln
                    centre = (P[0] + P[1] + P[2]) / 3
                    out_dir = centre - np.array([cxm, centre[1], czm])
                    ok = nrm[1] > 0 if kind == "roof" else (nrm[0] * out_dir[0] + nrm[2] * out_dir[2]) > 0
                    if not ok:
                        P = [P[0], P[2], P[1]]
                        nrm = -nrm
                    ti = (0 if centre[0] < 0 else 1) + NT * (0 if centre[2] < 0 else 1)
                    whole = roofs[ti] if kind == "roof" else walls
                    cell = cell_roofs[bi] if kind == "roof" else (cell_wood[bi] if walls is wood else cell_plaster[bi])
                    # (the whole window's mesh, in the window's frame, with the photograph of the tile the triangle is over; the building's tile's mesh, in the tile's frame)
                    for tgt, tile, origin in ((whole, tiles[ti], (0.0, 0.0)), (cell, tiles[bi], (tiles[bi]["cx"], tiles[bi]["cz"]))):
                        base = len(tgt["pos"])
                        for q in P:
                            tgt["pos"].append([float(q[0]) - origin[0], float(q[1]), float(q[2]) - origin[1]])
                            tgt["nrm"].append(nrm.tolist())
                            if kind == "roof":
                                tgt["uv"].append([(q[0] - (tile["cx"] - tile_m / 2)) / tile_m, (q[2] - (tile["cz"] - tile_m / 2)) / tile_m])
                            else:
                                hz = math.hypot(nrm[0], nrm[2])
                                t = np.array([nrm[2] / hz, 0.0, -nrm[0] / hz]) if hz > 1e-6 else np.array([1.0, 0.0, 0.0])
                                tgt["uv"].append([float(np.dot(q, t)) / tgt["m"], float(q[1]) / tgt["m"]])
                                tgt["col"].append(tint)
                        tgt["idx"] += [base, base + 1, base + 2]
    print(f"  {nb} buildings in the window: {sum(len(r['idx']) for r in roofs) // 3} roof triangles, {(len(wood['idx']) + len(plaster['idx'])) // 3} wall triangles")
    write_gltf(os.path.join(out, "buildings"), roofs + [wood, plaster], [t["texture"] for t in tiles] + ["walls_wood.jpg", "walls_plaster.jpg"], NT * NT)
    for k, tile in enumerate(tiles):
        write_gltf(os.path.join(out, f"buildings_{tile['i']}_{tile['j']}"), [cell_roofs[k], cell_wood[k], cell_plaster[k]], [tile["texture"], "walls_wood.jpg", "walls_plaster.jpg"], 1)
    print("roads")
    notes = {}
    B.build_roads(ways, loc, ground, frame, notes).write_glb(os.path.join(out, "roads.glb"))
    print("trees from the surface model")
    ndsm = dsm - dtm
    ndsm[np.isnan(ndsm)] = 0
    ndsm[ndsm < 0] = 0
    h1 = ndsm.reshape(2000, 2, 2000, 2).max(axis=(1, 3))  # 1 m cells (row 0 north)
    # mask the buildings and the water
    bimg = Image.new("L", (2000, 2000), 0)
    bd = ImageDraw.Draw(bimg)
    for poly in prints:
        bd.polygon([(p[0] + frame.half, p[1] + frame.half) for p in poly], fill=255)
    bmask = np.array(bimg.filter(ImageFilter.MaxFilter(5))) > 0
    wmask1 = np.array(Image.fromarray((wm > 0.5).astype(np.uint8) * 255).resize((2000, 2000), Image.NEAREST)) > 0
    h1[bmask | wmask1] = 0
    p3 = np.pad(h1, 1, mode="edge")
    sm = sum(p3[1 + dy : 1 + dy + 2000, 1 + dx : 1 + dx + 2000] for dy in (-1, 0, 1) for dx in (-1, 0, 1)) / 9.0
    pp = np.pad(sm, 2, mode="constant")
    top = np.ones(sm.shape, dtype=bool)
    for dy in range(-2, 3):
        for dx in range(-2, 3):
            if dy or dx:
                top &= sm >= pp[2 + dy : 2 + dy + 2000, 2 + dx : 2 + dx + 2000]
    top &= (sm > 3.5) & (sm < 45)
    tz, tx = np.nonzero(top)
    trees = []
    for zi, xi in zip(tz.tolist(), tx.tolist()):
        height = float(h1[zi, xi])
        rad = 1.0
        for r in range(1, 8):
            vals = []
            for a in range(0, 360, 45):
                px = xi + int(round(r * math.cos(math.radians(a))))
                pz = zi + int(round(r * math.sin(math.radians(a))))
                if 0 <= px < 2000 and 0 <= pz < 2000:
                    vals.append(h1[pz, px])
            if not vals or np.mean(vals) < 0.5 * height:
                break
            rad = r + 0.5
        wx, wz = xi + 0.5 - frame.half, zi + 0.5 - frame.half
        trees.append({"x": round(wx, 2), "z": round(wz, 2), "y": round(float(ground.height(wx, wz)), 2), "h": round(height, 2), "r": round(rad, 2)})
    json.dump(trees, open(os.path.join(out, "trees.json"), "w"))
    hs = np.array([t["h"] for t in trees]) if trees else np.zeros(1)
    print(f"  trees: {len(trees)}, height median {np.median(hs):.1f} m, tallest {hs.max():.1f} m")
    json.dump({"name": name, "lo": lo, "relief": relief, "size": size, "buildings": nb, "trees": len(trees), "tiles": tiles}, open(os.path.join(out, "info.json"), "w"))
    print("done:", out)


if __name__ == "__main__":
    main()
