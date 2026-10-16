"""Classifies a world that Tellus built for Wyrdwood (tools/make_wyrdwood_graph.py, `tellus world`): which biome every place is, where the start is, where the five altars and the camps are, and what grows where.

    python -I tools/wyrdwood_world.py examples/wyrdwood/worlds/seed11

Writes next to the world: `biome.png` (8 bit, one pixel per `BIOME_CELL` metres, the biome number times 40), `biome_map.png` (a picture of it for looking at) and `wyrdwood.json` (the numbers the game needs).

The rules have the shape of the reference game's (rings of distance round the start, a wobble in the distance, noise gates, mountains by height) and are our own: from the start, the **meadow** is the first few hundred
metres; the **gloomwood** is the forest from there outwards; the **moor** is the low, wet, flat ground where a noise says swamp; the **crag** is the high ground; the **barrens** are the far dry end of the world.
Nothing here is random but the seed: the same world gives the same biomes on every machine.
"""
import json
import math
import os
import struct
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import make_heightmap as mh  # noqa: E402

BIOMES = ["ocean", "meadow", "gloomwood", "moor", "crag", "barrens"]
COLORS = [(40, 90, 160), (120, 190, 80), (40, 100, 50), (110, 120, 80), (170, 165, 160), (190, 160, 110)]
BIOME_CELL = 8.0  # metres a pixel of the biome picture stands for
SEED_SALT = 0x5EED


def read_png(path):
    """(width, height, depth, rows of ints) of a grey PNG (8 or 16 bit); stdlib only."""
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", path
    pos, idat, w = 8, b"", 0
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, color = struct.unpack(">IIBB", body[:10])
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    assert color == 0, "a grey picture is needed"
    bpp = depth // 8
    raw = zlib.decompress(idat)
    stride = w * bpp
    rows, prev = [], bytes(stride)
    p = 0
    for _ in range(h):
        f = raw[p]
        line = bytearray(raw[p + 1:p + 1 + stride])
        p += 1 + stride
        if f == 1:
            for i in range(bpp, stride):
                line[i] = (line[i] + line[i - bpp]) & 255
        elif f == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 255
        elif f == 3:
            for i in range(stride):
                left = line[i - bpp] if i >= bpp else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 255
        elif f == 4:
            for i in range(stride):
                a = line[i - bpp] if i >= bpp else 0
                b = prev[i]
                c = prev[i - bpp] if i >= bpp else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        prev = bytes(line)
        rows.append(list(struct.unpack(">%dH" % w, prev)) if bpp == 2 else list(prev))
    return w, h, depth, rows


def write_png8(path, w, h, channels, rows):
    raw = bytearray()
    for row in rows:
        raw.append(0)
        raw += bytes(row)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    colour = {1: 0, 3: 2, 4: 6}[channels]
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, colour, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))


def smooth(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3.0 - 2.0 * t)


class World:
    """The heights of all the tiles on one coarse grid (one sample per BIOME_CELL metres), with the sea level."""

    def __init__(self, folder):
        self.folder = folder
        info = json.load(open(os.path.join(folder, "world.json")))
        self.info = info
        self.size = info["size_m"]
        self.relief = info["heightmaps"]["out"]["height_m"]
        self.base = info["heightmaps"]["out"]["base_m"]
        self.sea_y = info["sea_level"] * info["relief_m"]  # heights are absolute: the sea is `sea_level` of the project relief
        self.n = int(round(self.size / BIOME_CELL))
        tiles = info["tiles"] if isinstance(info["tiles"], int) else int(math.sqrt(len(info["tile_list"])))
        self.tiles = int(round(math.sqrt(len(info["tile_list"]))))
        self.h = [[0.0] * self.n for _ in range(self.n)]
        tile_m = self.size / self.tiles
        per = int(round(tile_m / BIOME_CELL))
        for t in info["tile_list"]:
            w, hh, depth, rows = read_png(os.path.join(folder, f"heightmap_{t['x']}_{t['y']}.png"))
            scale = self.relief / (65535.0 if depth == 16 else 255.0)
            for j in range(per):
                src = rows[min(hh - 1, int(round(j * (hh - 1) / per)))]
                dst = self.h[t["y"] * per + j]
                for i in range(per):
                    dst[t["x"] * per + i] = self.base + src[min(w - 1, int(round(i * (w - 1) / per)))] * scale

    def xz(self, i, j):
        """The world position (x east, z south) of the middle of cell (i, j)."""
        return (i + 0.5) * BIOME_CELL - self.size / 2, (j + 0.5) * BIOME_CELL - self.size / 2

    def cell(self, x, z):
        return max(0, min(self.n - 1, int((x + self.size / 2) / BIOME_CELL))), max(0, min(self.n - 1, int((z + self.size / 2) / BIOME_CELL)))

    def height(self, x, z):
        i, j = self.cell(x, z)
        return self.h[j][i]

    def slope(self, i, j):
        h = self.h
        gx = (h[j][min(self.n - 1, i + 1)] - h[j][max(0, i - 1)]) / (2 * BIOME_CELL)
        gz = (h[min(self.n - 1, j + 1)][i] - h[max(0, j - 1)][i]) / (2 * BIOME_CELL)
        return math.degrees(math.atan(math.hypot(gx, gz)))


def find_start(w):
    """The landing: on the south coast, a flat beach with land behind it, near the middle of the south side."""
    best = None
    for k in range(-40, 41):
        x = k * 14.0
        z = w.size / 2 - 40.0
        while z > -w.size / 2 and w.height(x, z) < w.sea_y + 1.6:
            z -= BIOME_CELL / 2
        i, j = w.cell(x, z)
        flat = w.slope(i, j)
        score = flat + abs(x) * 0.01 + (0 if w.height(x, z - 60.0) > w.sea_y + 2.0 else 30)
        if best is None or score < best[0]:
            best = (score, x, z - 8.0)
    return best[1], best[2]


def classify(w, start):
    sx, sz = start
    n = w.n
    seed = int(w.info["seed"]) + SEED_SALT
    biome = [[0] * n for _ in range(n)]
    moist = [[0.0] * n for _ in range(n)]
    hmax = max(max(r) for r in w.h)
    land_h = sorted(g for row in w.h for g in row if g > w.sea_y + 0.2)
    rank = lambda g: __import__("bisect").bisect_left(land_h, g) / max(1, len(land_h))  # noqa: E731  (0 at the lowest land, 1 on the highest peak)
    for j in range(n):
        for i in range(n):
            x, z = w.xz(i, j)
            g = w.h[j][i]
            if g <= w.sea_y + 0.2:
                continue
            up = rank(g)  # the share of the land that is lower than this place
            d = math.hypot(x - sx, z - sz)
            wobble = math.sin(math.atan2(x - sx, z - sz) * 14.0) * 0.5
            dd = d + 90.0 * wobble + 140.0 * (mh.fbm(x / 400.0, z / 400.0, seed, 3) - 0.5)
            swamp_noise = mh.fbm(x / 520.0 + 3.0, z / 520.0, seed + 1, 4)
            forest_noise = mh.fbm(x / 600.0 + 9.0, z / 600.0, seed + 2, 4)
            dry_noise = mh.fbm(x / 700.0 + 5.0, z / 700.0, seed + 3, 4)
            moist[j][i] = swamp_noise
            if up > 0.82:
                b = 4
            elif swamp_noise > 0.50 and dd > 500.0 and up < 0.5 and w.slope(i, j) < 14.0:
                b = 3
            elif dd > 1700.0 and dry_noise > 0.45 and up < 0.65:
                b = 5
            elif dd > 760.0 and (forest_noise > 0.42 or dd > 1500.0):
                b = 2
            else:
                b = 1
            biome[j][i] = b
    return biome, moist, hmax


def pick(w, biome, start, want, near, tol, slope_max=14.0, taken=(), apart=140.0, salt=0):
    """A flat spot of `want` (a biome number) at about `near` metres from the start: the best of the cells within `tol`, spread by a hash so that the choice is the world's own."""
    sx, sz = start
    best = None
    for j in range(w.n):
        for i in range(w.n):
            if biome[j][i] != want:
                continue
            x, z = w.xz(i, j)
            d = math.hypot(x - sx, z - sz)
            if abs(d - near) > tol or w.h[j][i] < w.sea_y + 3.0 or w.slope(i, j) > slope_max:
                continue
            if any(math.hypot(x - tx, z - tz) < apart for tx, tz in taken):
                continue
            score = abs(d - near) * 0.02 + w.slope(i, j) * 0.1 + mh._hash(i, j, 7 + salt) * 2.0
            if best is None or score < best[0]:
                best = (score, x, z)
    return None if best is None else (best[1], best[2])


def main():
    folder = sys.argv[1]
    w = World(folder)
    start = find_start(w)
    biome, moist, hmax = classify(w, start)
    # the pictures
    write_png8(os.path.join(folder, "biome.png"), w.n, w.n, 1, [[b * 40 for b in row] for row in biome])
    counts = [0] * len(BIOMES)
    for row in biome:
        for b in row:
            counts[b] += 1
    land = sum(counts[1:])
    # the places
    altars, taken = {}, [start]
    plan = [("meadow", 1, 380.0, 250.0), ("gloomwood", 2, 1000.0, 500.0), ("moor", 3, 1300.0, 900.0), ("crag", 4, 1500.0, 1100.0), ("barrens", 5, 2200.0, 1300.0)]
    for k, (name, b, near, tol) in enumerate(plan):
        spot = pick(w, biome, start, b, near, tol, slope_max=16.0 if name != "crag" else 24.0, taken=taken, apart=300.0, salt=k)
        if spot is None:
            spot = pick(w, biome, start, b, near, 4000.0, slope_max=30.0, taken=taken, apart=200.0, salt=k)
        if spot:
            altars[name] = {"x": round(spot[0], 1), "z": round(spot[1], 1), "y": round(w.height(*spot), 2)}
            taken.append(spot)
    camps = []
    for k in range(10):
        b = (1, 2, 2, 3, 1)[k % 5]
        spot = pick(w, biome, start, b, 300.0 + 200.0 * k, 300.0, slope_max=10.0, taken=taken, apart=160.0, salt=20 + k)
        if spot:
            camps.append({"biome": BIOMES[b], "x": round(spot[0], 1), "z": round(spot[1], 1), "y": round(w.height(*spot), 2)})
            taken.append(spot)
    # a picture to look at: the biome colours shaded by the height, the places marked
    rows = []
    for j in range(w.n):
        row = []
        for i in range(w.n):
            c = COLORS[biome[j][i]]
            lit = 0.75 + 0.5 * (w.h[j][min(w.n - 1, i + 1)] - w.h[j][max(0, i - 1)]) / (2 * BIOME_CELL) * -6.0 + 0.0
            lit = max(0.45, min(1.3, lit))
            row += [int(min(255, c[0] * lit)), int(min(255, c[1] * lit)), int(min(255, c[2] * lit))]
        rows.append(row)

    def mark(x, z, col, r=3):
        i, j = w.cell(x, z)
        for dj in range(-r, r + 1):
            for di in range(-r, r + 1):
                if 0 <= i + di < w.n and 0 <= j + dj < w.n:
                    rows[j + dj][3 * (i + di):3 * (i + di) + 3] = list(col)

    mark(*start, (255, 255, 255), 4)
    for a in altars.values():
        mark(a["x"], a["z"], (255, 60, 60), 4)
    for c in camps:
        mark(c["x"], c["z"], (255, 220, 60), 2)
    write_png8(os.path.join(folder, "biome_map.png"), w.n, w.n, 3, rows)
    out = {
        "wyrdwood": 1, "seed": w.info["seed"], "size_m": w.size, "relief_m": w.relief, "base_m": w.base, "sea_y": round(w.sea_y, 3), "biome_cell_m": BIOME_CELL, "biomes": BIOMES, "biome_picture": "biome.png", "biome_scale": 40,
        "land_share": {BIOMES[b]: round(counts[b] / max(1, land), 3) for b in range(1, len(BIOMES))},
        "start": {"x": round(start[0], 1), "z": round(start[1], 1), "y": round(w.height(*start), 2)}, "altars": altars, "camps": camps, "highest_m": round(hmax, 1),
    }
    json.dump(out, open(os.path.join(folder, "wyrdwood.json"), "w"), indent=1)
    print("land share:", out["land_share"])
    print("start", out["start"], "altars", {k: (v["x"], v["z"]) for k, v in altars.items()}, "camps", len(camps))


if __name__ == "__main__":
    main()
