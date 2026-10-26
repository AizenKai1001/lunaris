"""A coast made with nothing but the standard library, for the renderer's landscape (examples/gfx_lab/coast/): the sea to the west over a seabed that deepens slowly, a beach, dunes with their own grass, and a rolling meadow
inland, as a 16-bit heightmap and a splat map (R meadow grass, G rock, B sand, A dune grass) that `TerrainLayers` reads. Black in the heightmap is `base_m` (-14 m: the seabed), the sea is at 0 m.

    python -I tools/make_coast.py examples/gfx_lab/coast [--size 1025] [--extent 2400] [--seed 5]

The same arguments always give the same two pictures. Writes heightmap.png, splat.png and coast.json (the numbers the scene needs: size, base, relief, the shoreline's meander).
"""
import argparse
import json
import math
import os
import struct
import zlib


def png(path, w, h, color_type, depth, rows):
    raw = bytearray()
    for r in rows:
        raw.append(0)
        raw += r
    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, color_type, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(bytes(raw), 6)) + chunk(b"IEND", b""))


def hash2(ix, iz, seed):
    h = (ix * 374761393 + iz * 668265263 + seed * 2246822519) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((h ^ (h >> 16)) & 0xFFFFFF) / 16777216.0


def vnoise(x, z, seed):
    ix, iz = math.floor(x), math.floor(z)
    fx, fz = x - ix, z - iz
    ux, uz = fx * fx * (3 - 2 * fx), fz * fz * (3 - 2 * fz)
    a, b = hash2(ix, iz, seed), hash2(ix + 1, iz, seed)
    c, d = hash2(ix, iz + 1, seed), hash2(ix + 1, iz + 1, seed)
    return (a + (b - a) * ux) + ((c + (d - c) * ux) - (a + (b - a) * ux)) * uz


def fbm(x, z, seed, octaves=4):
    s, a, f = 0.0, 0.5, 1.0
    for o in range(octaves):
        s += a * vnoise(x * f, z * f, seed + o * 31)
        a *= 0.5
        f *= 2.03
    return s / (1.0 - 0.5 ** octaves)


def smooth(a, b, x):
    t = min(max((x - a) / (b - a), 0.0), 1.0)
    return t * t * (3 - 2 * t)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--size", type=int, default=1025)
    ap.add_argument("--extent", type=float, default=2400.0, help="metres across")
    ap.add_argument("--seed", type=int, default=5)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    n, ext, seed = a.size, a.extent, a.seed
    base_m, relief_m = -14.0, 46.0
    step = ext / (n - 1)

    def coast_x(z):
        # the shoreline's meander: a long bay and a headland, a ripple on top
        return -80.0 + 130.0 * math.sin(z / 520.0 + 0.8) + 55.0 * math.sin(z / 190.0 + 2.0) + 18.0 * math.sin(z / 71.0)

    heights = []
    hrows = []
    for j in range(n):
        z = -ext / 2 + j * step
        cx = coast_x(z)
        hr = bytearray()
        for i in range(n):
            x = -ext / 2 + i * step
            d = x - cx  # metres inland (negative: at sea)
            wob = fbm(x / 90.0, z / 90.0, seed + 3) - 0.5
            if d < 0:
                h = -14.0 * (1.0 - math.exp(d / 160.0)) + 0.35 * wob
            elif d < 45:
                h = 0.034 * d + 0.12 * wob
            else:
                dune = fbm(x / 34.0, z / 120.0, seed + 11)
                ridge = 1.0 - abs(2.0 * dune - 1.0)
                ramp = smooth(45.0, 150.0, d) * (1.0 - 0.55 * smooth(260.0, 520.0, d))
                h = 1.53 + 0.011 * (d - 45.0) + ramp * (3.2 * ridge * ridge + 1.2 * (fbm(x / 60.0, z / 60.0, seed + 5) - 0.5))
            hills = smooth(110.0, 420.0, d) * (fbm(x / 260.0, z / 330.0, seed + 17) * 15.0 + fbm(x / 70.0, z / 80.0, seed + 19) * 3.0) + 0.016 * max(d - 160.0, 0.0)
            h += hills
            heights.append(h)
            hr += struct.pack(">H", int(min(max((h - base_m) / relief_m, 0.0), 1.0) * 65535 + 0.5))
        hrows.append(hr)
    # the heights as one grid for the slopes
    grid = [heights[j * n:(j + 1) * n] for j in range(n)]
    srows = []
    for j in range(n):
        z = -ext / 2 + j * step
        cx = coast_x(z)
        sr = bytearray()
        for i in range(n):
            x = -ext / 2 + i * step
            d = x - cx
            gx = (grid[j][min(i + 1, n - 1)] - grid[j][max(i - 1, 0)]) / (2 * step)
            gz = (grid[min(j + 1, n - 1)][i] - grid[max(j - 1, 0)][i]) / (2 * step)
            slope = math.hypot(gx, gz)
            n1 = fbm(x / 45.0, z / 45.0, seed + 23) * 2 - 1
            n2 = fbm(x / 70.0, z / 70.0, seed + 29) * 2 - 1
            n3 = fbm(x / 160.0, z / 160.0, seed + 31) * 2 - 1
            sand = 1.0 - smooth(38.0, 100.0, d + 22.0 * n1)
            dune = smooth(34.0, 84.0, d + 24.0 * n2) * (1.0 - smooth(150.0, 270.0, d + 40.0 * n3))
            meadow = smooth(120.0, 210.0, d + 38.0 * n3)
            rock = smooth(0.42, 0.7, slope) * smooth(10.0, 40.0, d)
            tot = sand + dune + meadow + rock + 1e-6
            sr += bytes([int(255 * meadow / tot + 0.5), int(255 * rock / tot + 0.5), int(255 * sand / tot + 0.5), int(255 * dune / tot + 0.5)])
        srows.append(sr)
    png(os.path.join(a.out, "heightmap.png"), n, n, 0, 16, hrows)
    png(os.path.join(a.out, "splat.png"), n, n, 6, 8, srows)
    json.dump({"size_m": ext, "samples": n, "base_m": base_m, "relief_m": relief_m, "sea_level_m": 0.0, "seed": seed}, open(os.path.join(a.out, "coast.json"), "w"), indent=1)
    print("wrote", a.out, "min/max height", round(min(heights), 2), round(max(heights), 2))


main()
