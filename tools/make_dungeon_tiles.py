"""Writes examples/dungeon_tiles.png for the dungeon example, using only the standard library: a 17 x 1 sheet of 16 x 16 tiles.

  tile 0       the floor (flagstones)
  tiles 1-16   the wall, in the order of the `autotile` tool's 16-tile set (`base + N + 2 E + 4 S + 8 W`, base 1, each letter 1 when the neighbour on that side is
               wall): a side without a wall neighbour gets a light rim, so the rock looks cut out of the floor

    python -I tools/make_dungeon_tiles.py
"""
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "dungeon_tiles.png")


def write_png(path, width, height, pixels):
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
    return tuple(max(0, min(255, v + d)) for v in c[:3]) + (255,)


FLOOR = (52, 50, 70)
ROCK = (34, 30, 46)
RIM = (128, 112, 160, 255)
RIM_DARK = (86, 74, 112)


def floor_tile():
    px = [[(0, 0, 0, 255)] * 16 for _ in range(16)]
    for y in range(16):
        for x in range(16):
            d = (noise(x, y, 3) - 128) // 24
            if x % 8 == 0 or y % 8 == 0:
                d -= 7  # the joints between the flagstones
            px[y][x] = shade(FLOOR, d)
    # a crack, now and then
    for i in range(5):
        px[10 + i // 2][3 + i] = shade(FLOOR, -16)
    return px


def wall_tile(mask):
    north, east, south, west = mask & 1, mask >> 1 & 1, mask >> 2 & 1, mask >> 3 & 1
    px = [[(0, 0, 0, 255)] * 16 for _ in range(16)]
    for y in range(16):
        for x in range(16):
            d = (noise(x, y, 11) - 128) // 30
            if y % 4 == 0 or (x + (y // 4 % 2) * 4) % 8 == 0:
                d -= 5  # brick joints
            c = shade(ROCK, d)
            # a rim on every side that looks at the floor: 3 pixels, the outer one brightest
            for exposed, dist in ((not north, y), (not south, 15 - y), (not west, x), (not east, 15 - x)):
                if exposed and dist < 3:
                    c = RIM if dist == 0 else shade(RIM_DARK, -10 * (dist - 1))
            px[y][x] = c
    return px


tiles = [floor_tile()] + [wall_tile(m) for m in range(16)]
rows = [[px for t in tiles for px in t[y]] for y in range(16)]
write_png(OUT, 16 * len(tiles), 16, rows)
print(f"wrote {os.path.relpath(OUT)} ({16 * len(tiles)} x 16)")
