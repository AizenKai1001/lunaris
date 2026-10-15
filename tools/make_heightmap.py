"""Make a 16-bit greyscale heightmap with nothing but the standard library: hills, mountains in the north, a flat meadow in the middle and the
sea at the edges. The picture is what `Terrain.heightmap` reads (see docs/TERRAIN.md): black is the lowest ground, white the highest.

    python -I tools/make_heightmap.py out.png [--size 257] [--seed 7] [--meadow 0.42]

`--size` is the number of samples on each side (a cell is one step between two samples, so 257 samples make 256 cells; 2^n + 1 is a good
size and 2049 is the biggest that is drawn without skipping samples). `--meadow` is the height, as a fraction of the relief, of the
flat circle at the middle (0 for none). The same arguments always give the same picture, byte for byte.

This is for examples and tests, and for people who have no terrain program yet. Real work is made in Gaea, World Machine, WorldPainter,
Blender (a baked height texture), or anything that exports a 16-bit greyscale PNG.
"""
import argparse
import math
import struct
import zlib


def write_png16(path, rows):
    """A 16-bit greyscale PNG from rows of integers 0..65535 (all rows the same length)."""
    h, w = len(rows), len(rows[0])
    raw = bytearray()
    for r in rows:
        raw.append(0)  # filter: none
        raw += struct.pack(">%dH" % w, *r)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 16, 0, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def _hash(ix, iz, seed):
    n = (ix * 374761393 + iz * 668265263 + seed * 1442695041) & 0xFFFFFFFF
    n = ((n ^ (n >> 13)) * 1274126177) & 0xFFFFFFFF
    n ^= n >> 16
    return n / 4294967296.0


def _smooth(t):
    return t * t * (3.0 - 2.0 * t)


def value_noise(x, z, seed):
    ix, iz = math.floor(x), math.floor(z)
    fx, fz = _smooth(x - ix), _smooth(z - iz)
    a, b = _hash(ix, iz, seed), _hash(ix + 1, iz, seed)
    c, d = _hash(ix, iz + 1, seed), _hash(ix + 1, iz + 1, seed)
    return (a + (b - a) * fx) + ((c + (d - c) * fx) - (a + (b - a) * fx)) * fz


def fbm(x, z, seed, octaves=6, ridged=False):
    total, amp, freq, norm = 0.0, 1.0, 1.0, 0.0
    for o in range(octaves):
        v = value_noise(x * freq, z * freq, seed + o * 31)
        if ridged:
            v = 1.0 - abs(2.0 * v - 1.0)
            v *= v
        total += v * amp
        norm += amp
        amp *= 0.5
        freq *= 2.0
    return total / norm


def edge(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return _smooth(t)


def thermal(h, n, passes, talus):
    """Let steep slopes slump: each pass moves a part of the excess over `talus` to the lowest neighbour."""
    for _ in range(passes):
        for z in range(1, n - 1):
            row = h[z]
            for x in range(1, n - 1):
                here = row[x]
                best, bx, bz = 0.0, 0, 0
                for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    drop = here - h[z + dz][x + dx]
                    if drop > best:
                        best, bx, bz = drop, dx, dz
                if best > talus:
                    move = (best - talus) * 0.35
                    row[x] -= move
                    h[z + bz][x + bx] += move


def heights(n=257, seed=7, meadow=0.42):
    """The heightmap as rows of floats 0..1 (row 0 is the north edge, the top of the picture)."""
    h = []
    for j in range(n):
        v = j / (n - 1)
        row = []
        for i in range(n):
            u = i / (n - 1)
            base = fbm(u * 3.0, v * 3.0, seed)
            peaks = fbm(u * 3.2 + 9.0, v * 3.2, seed + 100, 6, ridged=True)
            north = edge(0.55, 0.05, v)
            d = math.hypot(u - 0.5, v - 0.5) * 2.0
            land = 1.0 - edge(0.7, 1.0, d)
            row.append((0.05 + 0.5 * base + 0.7 * north * peaks) * land)
        h.append(row)
    lo = min(min(r) for r in h)
    hi = max(max(r) for r in h)
    for r in h:
        for i in range(n):
            r[i] = (r[i] - lo) / (hi - lo)
    thermal(h, n, 12, 0.5 / (n - 1) * 3.0)
    if meadow > 0:
        c = (n - 1) / 2.0
        flat, blend = n * 0.09, n * 0.2
        for j in range(n):
            for i in range(n):
                d = math.hypot(i - c, j - c)
                w = 1.0 - edge(flat, blend, d)
                h[j][i] += (meadow - h[j][i]) * w
    return h


def samples(n=257, seed=7, meadow=0.42):
    return [[max(0, min(65535, round(v * 65535))) for v in row] for row in heights(n, seed, meadow)]


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("out")
    ap.add_argument("--size", type=int, default=257)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--meadow", type=float, default=0.42)
    a = ap.parse_args()
    if not 3 <= a.size <= 8193:
        raise SystemExit("--size must be 3 to 8193")
    write_png16(a.out, samples(a.size, a.seed, a.meadow))
    print(f"wrote {a.out}: {a.size} x {a.size} samples, 16 bit")
