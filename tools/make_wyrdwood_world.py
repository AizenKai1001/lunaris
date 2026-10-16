"""Makes the pictures and the map data of Wyrdwood's first island (Shore) with nothing but the standard library: examples/wyrdwood/world/ground.png (a 16-bit heightmap), splat.png (an RGBA map of four
materials: grass, rock, sand and forest floor), water.png (the surface heights of the sea and of the pond), shot_map.png (a shaded picture of the whole island, for looking at it without the engine) and
world.json (where the landing beach, the hill and the pond are, with the height of the ground under each).

    python -I tools/make_wyrdwood_world.py            (a few seconds; the same bytes every time)

The ground is worked out in metres first (`ground_at_m`), then written as 16 bits (`RELIEF` metres from black to white) and read back, so that every height in world.json is what the engine stands on.
"""
import json
import math
import os
import struct
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import make_heightmap as mh  # noqa: E402

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "examples", "wyrdwood", "world")
N, SIZE, RELIEF, SEED = 321, 320.0, 40.0, 21
CELL = SIZE / (N - 1)
SEA = 4.0  # the height of the sea's surface in metres over the heightmap's black
POND_AT, POND_R, POND_LEVEL = (38.0, -34.0), 13.0, 8.2
HILL = (-34.0, 30.0)


def smooth(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3.0 - 2.0 * t)


def png_chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def write_png8(path, w, h, channels, rows):
    raw = bytearray()
    for row in rows:
        raw.append(0)
        raw += bytes(row)
    colour = {1: 0, 3: 2, 4: 6}[channels]
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + png_chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, colour, 0, 0, 0)) + png_chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + png_chunk(b"IEND", b""))


def coast(ang):
    """The island's radius at an angle: a smooth wobble round 120 m."""
    c, s = math.cos(ang), math.sin(ang)
    return 120.0 + 16.0 * (mh.fbm(c * 1.5 + 5.0, s * 1.5 + 5.0, SEED + 3, 3) - 0.5) * 2.0


def ground_at_m(x, z):
    """The ground's height in metres at world (x, z): sea floor, a coast with beaches, meadows and a hill, and a pond in a bowl."""
    r = math.hypot(x, z)
    ang = math.atan2(z, x)
    big = mh.fbm(x / 80.0 + 3.0, z / 80.0 + 7.0, SEED, 5)
    small = mh.fbm(x / 20.0, z / 20.0, SEED + 50, 3)
    land = 11.5 + 5.0 * (big - 0.45) * 2.0 + 1.2 * (small - 0.5)
    land += 17.0 * smooth(62.0, 8.0, math.hypot(x - HILL[0], z - HILL[1]))
    pd = math.hypot(x - POND_AT[0], z - POND_AT[1]) / POND_R
    land -= 8.0 * math.exp(-1.6 * pd * pd) * smooth(2.6, 1.0, pd)
    R = coast(ang)
    m = smooth(R + 6.0, R - 40.0, r)
    floor = 0.7 + 1.6 * mh.fbm(x / 30.0, z / 30.0, SEED + 9, 3)
    g = floor + (land - floor) * m ** 1.7
    e = min(SIZE / 2 - abs(x), SIZE / 2 - abs(z))
    g = g * smooth(0.0, 24.0, e) + floor * (1.0 - smooth(0.0, 24.0, e))
    return max(0.2, g)


def build():
    os.makedirs(OUT, exist_ok=True)
    ground_m = []
    for j in range(N):
        z = (j - (N - 1) / 2) * CELL
        ground_m.append([ground_at_m((i - (N - 1) / 2) * CELL, z) for i in range(N)])
    mh.thermal(ground_m, N, 6, 0.85 * CELL)
    heights16 = [[max(0, min(65535, round(g / RELIEF * 65535.0))) for g in row] for row in ground_m]
    mh.write_png16(os.path.join(OUT, "ground.png"), heights16)
    gm = [[h / 65535.0 * RELIEF for h in row] for row in heights16]

    def ground(x, z):
        fx, fz = (x + SIZE / 2) / CELL, (z + SIZE / 2) / CELL
        i, j = max(0, min(N - 2, int(math.floor(fx)))), max(0, min(N - 2, int(math.floor(fz))))
        u, w = fx - i, fz - j
        p00, p10, p01, p11 = gm[j][i], gm[j][i + 1], gm[j + 1][i], gm[j + 1][i + 1]
        return p00 + (p10 - p00) * u + (p01 - p00) * w if u + w <= 1.0 else p11 + (p01 - p11) * (1 - u) + (p10 - p11) * (1 - w)

    def slope_deg(i, j):
        gx = (gm[j][min(N - 1, i + 1)] - gm[j][max(0, i - 1)]) / (2 * CELL)
        gz = (gm[min(N - 1, j + 1)][i] - gm[max(0, j - 1)][i]) / (2 * CELL)
        return math.degrees(math.atan(math.hypot(gx, gz)))

    # ---- the water: the sea wherever the ground is under SEA, the pond in its bowl
    water16 = [[0] * N for _ in range(N)]
    for j in range(N):
        z = (j - (N - 1) / 2) * CELL
        for i in range(N):
            x = (i - (N - 1) / 2) * CELL
            g = gm[j][i]
            level = None
            if math.hypot(x - POND_AT[0], z - POND_AT[1]) < POND_R * 1.9 and g < POND_LEVEL - 0.02 and math.hypot(x, z) < 130.0:
                level = POND_LEVEL
            elif g < SEA - 0.02:
                level = SEA
            if level is not None:
                water16[j][i] = max(1, min(65535, round(level / RELIEF * 65535.0)))
    mh.write_png16(os.path.join(OUT, "water.png"), water16)

    # ---- the splat map: grass, rock, sand, forest floor (red, green, blue, alpha)
    rows = []
    for j in range(N):
        row = []
        for i in range(N):
            x, z = (i - (N - 1) / 2) * CELL, (j - (N - 1) / 2) * CELL
            g, sl = gm[j][i], slope_deg(i, j)
            wob = mh.fbm(i * 0.08, j * 0.08, SEED + 5, 3) - 0.5
            pond_d = math.hypot(x - POND_AT[0], z - POND_AT[1])
            sand = smooth(SEA + 2.6 + wob * 2.0, SEA + 0.6 + wob * 1.5, g)
            sand = max(sand, smooth(POND_LEVEL + 0.9, POND_LEVEL - 0.3, g) * smooth(POND_R * 2.0, POND_R * 1.3, pond_d))
            rock = max(smooth(22.0, 34.0, sl + wob * 6.0), smooth(23.0, 30.0, g + wob * 8.0) * 0.8)
            forest = smooth(0.50, 0.62, mh.fbm(i * 0.035 + 7.0, j * 0.035, SEED + 17, 4)) * smooth(SEA + 3.0, SEA + 6.0, g) * (1.0 - rock)
            grass = max(0.0, 1.0 - sand - rock - forest)
            tot = grass + rock + sand + forest
            if tot <= 0:
                grass, tot = 1.0, 1.0
            row += [round(255 * grass / tot), round(255 * rock / tot), round(255 * sand / tot), round(255 * forest / tot)]
        rows.append(row)
    write_png8(os.path.join(OUT, "splat.png"), N, N, 4, rows)

    # ---- a shaded picture of the island for looking at it (the colour of the dominant material, lit from the north-west, the sea blue)
    shade = []
    cols = [(86, 120, 58), (118, 108, 98), (214, 196, 148), (58, 86, 44)]
    for j in range(N):
        row = []
        for i in range(N):
            if water16[j][i] and gm[j][i] < water16[j][i] / 65535.0 * RELIEF:
                row += [40, 100, 160] if gm[j][i] < SEA - 1.0 else [70, 140, 190]
                continue
            w = rows[j][4 * i: 4 * i + 4]
            c = [sum(w[k] * cols[k][ch] for k in range(4)) / 255.0 for ch in range(3)]
            gx = (gm[j][min(N - 1, i + 1)] - gm[j][max(0, i - 1)]) / (2 * CELL)
            gz = (gm[min(N - 1, j + 1)][i] - gm[max(0, j - 1)][i]) / (2 * CELL)
            lit = max(0.25, min(1.4, 0.85 - 2.2 * (gx * 0.6 + gz * 0.8)))
            row += [round(min(255, c[0] * lit)), round(min(255, c[1] * lit)), round(min(255, c[2] * lit))]
        shade.append(row)
    write_png8(os.path.join(OUT, "shot_map.png"), N, N, 3, shade)
    return gm, ground


def main():
    gm, ground = build()

    def beach(angle_deg, inland=6.0):
        """A point on the sand: walk in from the sea along an angle until the ground is a metre and a half over the sea, then a few steps inland."""
        a = math.radians(angle_deg)
        r = 150.0
        while r > 0 and ground(math.cos(a) * r, math.sin(a) * r) < SEA + 1.5:
            r -= 0.5
        r -= inland
        return math.cos(a) * r, math.sin(a) * r

    def at(x, z, **extra):
        return {"x": round(x, 2), "z": round(z, 2), "y": round(ground(x, z), 3), **extra}

    info = {"size": SIZE, "n": N, "relief": RELIEF, "sea": SEA, "pond_level": POND_LEVEL, "pond": at(*POND_AT, r=POND_R), "hill": at(*HILL), "max_height": round(max(max(r) for r in gm), 2)}
    for name, ang in (("landing", 0.0), ("cove", 130.0), ("north", -80.0)):
        bx, bz = beach(ang)
        info[name] = at(bx, bz)
    json.dump(info, open(os.path.join(OUT, "world.json"), "w"), indent=1)
    print("wrote", sorted(os.listdir(OUT)))
    print("landing", info["landing"], "hill", info["hill"], "pond", info["pond"], "max", info["max_height"])


if __name__ == "__main__":
    main()
