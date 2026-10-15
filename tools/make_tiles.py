"""Writes the pixel-art test assets for the 2D example into examples/tiles/ using only the standard library:

  tileset.png   8 tiles of 16 x 16 (grass, dirt, stone, water, brick, sand, planks, coin) in a 4 x 2 sheet
  hero.png      4 walking frames of a small astronaut, 16 x 16 each, in a 4 x 1 sheet

    python -I tools/make_tiles.py
"""
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "tiles")
os.makedirs(OUT, exist_ok=True)


def write_png(path, width, height, pixels):
    """pixels: list of rows of (r, g, b, a)."""
    raw = b"".join(b"\x00" + b"".join(bytes(p) for p in row) for row in pixels)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def noise(x, y, seed):
    h = (x * 374761393 + y * 668265263 + seed * 1442695041) & 0xFFFFFFFF
    h = (h ^ (h >> 13)) * 1274126177 & 0xFFFFFFFF
    return (h ^ (h >> 16)) & 255


def shade(c, d):
    return tuple(max(0, min(255, v + d)) for v in c[:3]) + (c[3] if len(c) > 3 else 255,)


def tile(kind):
    px = [[(0, 0, 0, 0)] * 16 for _ in range(16)]
    for y in range(16):
        for x in range(16):
            n = noise(x, y, kind) % 24 - 12
            if kind == 0:  # grass over dirt
                px[y][x] = shade((92, 170, 68, 255), n) if y < 4 else shade((134, 94, 60, 255), n)
                if y == 0:
                    px[y][x] = shade((140, 210, 90, 255), n // 2)
            elif kind == 1:  # dirt
                px[y][x] = shade((134, 94, 60, 255), n)
            elif kind == 2:  # stone blocks
                mortar = y % 8 == 0 or (x + (4 if (y // 8) % 2 else 0)) % 8 == 0
                px[y][x] = shade((96, 100, 112, 255), n // 2 - (28 if mortar else 0))
            elif kind == 3:  # water with a lighter wave line
                wave = ((x + y * 2) // 3) % 5 == 0
                px[y][x] = shade((52, 110, 200, 255), 18 if wave else n // 3)
            elif kind == 4:  # brick
                mortar = y % 4 == 0 or (x + (4 if (y // 4) % 2 else 0)) % 8 == 0
                px[y][x] = shade((176, 78, 60, 255), n // 2 - (34 if mortar else 0))
            elif kind == 5:  # sand
                px[y][x] = shade((226, 204, 140, 255), n)
            elif kind == 6:  # planks
                px[y][x] = shade((170, 124, 70, 255), n // 2 - (30 if y % 5 == 0 else 0))
            else:  # coin: gold disc on transparent
                d = (x - 7.5) ** 2 + (y - 7.5) ** 2
                if d <= 28:
                    px[y][x] = shade((250, 204, 40, 255), -40 if d > 18 else (30 if 3 < x < 8 and 3 < y < 8 else 0))
    return px


tiles = [tile(k) for k in range(8)]
sheet = [[(0, 0, 0, 0)] * 64 for _ in range(32)]
for k, t in enumerate(tiles):
    ox, oy = (k % 4) * 16, (k // 4) * 16
    for y in range(16):
        for x in range(16):
            sheet[oy + y][ox + x] = t[y][x]
write_png(os.path.join(OUT, "tileset.png"), 64, 32, sheet)


def hero_frame(f):
    px = [[(0, 0, 0, 0)] * 16 for _ in range(16)]

    def rect(x0, y0, x1, y1, c):
        for y in range(y0, y1):
            for x in range(x0, x1):
                px[y][x] = c

    white, grey, visor, dark = (236, 238, 245, 255), (176, 182, 200, 255), (60, 160, 230, 255), (70, 76, 96, 255)
    bob = 1 if f in (1, 3) else 0
    rect(4, 1 + bob, 12, 9 + bob, white)  # helmet and body
    rect(5, 3 + bob, 11, 6 + bob, visor)  # visor
    rect(3, 9 + bob, 13, 13 + bob, grey)  # suit
    rect(2, 9 + bob, 3, 12 + bob, dark)  # backpack
    # legs: alternate for the walking frames
    left = {0: (5, 13), 1: (4, 13), 2: (5, 13), 3: (6, 13)}[f]
    right = {0: (9, 13), 1: (10, 13), 2: (9, 13), 3: (8, 13)}[f]
    rect(left[0], 13, left[0] + 2, 16, dark)
    rect(right[0], 13, right[0] + 2, 16, dark)
    return px


hero = [[(0, 0, 0, 0)] * 64 for _ in range(16)]
for f in range(4):
    fr = hero_frame(f)
    for y in range(16):
        for x in range(16):
            hero[y][f * 16 + x] = fr[y][x]
write_png(os.path.join(OUT, "hero.png"), 64, 16, hero)
print("wrote", sorted(os.listdir(OUT)))
