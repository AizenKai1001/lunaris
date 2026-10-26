"""Compare two PNGs: mean absolute difference (levels of 255) over everything and over a band of rows, and how many channel values differ by more than a threshold.
    python3 tools/png_diff.py a.png b.png [y0 y1] [threshold]"""
import struct
import sys
import zlib


def read_png(p):
    d = open(p, "rb").read()
    pos, idat, w, h, ct = 8, b"", 0, 0, 2
    while pos < len(d):
        l = struct.unpack(">I", d[pos : pos + 4])[0]
        t, b = d[pos + 4 : pos + 8], d[pos + 8 : pos + 8 + l]
        pos += 12 + l
        if t == b"IHDR":
            w, h, _bd, ct = struct.unpack(">IIBB", b[:10])
        if t == b"IDAT":
            idat += b
    raw = zlib.decompress(idat)
    bpp = 4 if ct == 6 else 3
    stride = w * bpp
    out, prev, i = bytearray(), bytearray(stride), 0
    for _ in range(h):
        f = raw[i]
        i += 1
        line = bytearray(raw[i : i + stride])
        i += stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b_ = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b_) & 255
            elif f == 3:
                line[x] = (line[x] + ((a + b_) >> 1)) & 255
            elif f == 4:
                pa, pb, pc = abs(b_ - c), abs(a - c), abs(a + b_ - 2 * c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else (b_ if pb <= pc else c))) & 255
        out += line
        prev = line
    return w, h, bpp, out


a, b = read_png(sys.argv[1]), read_png(sys.argv[2])
y0, y1 = (int(sys.argv[3]), int(sys.argv[4])) if len(sys.argv) > 4 else (0, a[1])
thr = int(sys.argv[5]) if len(sys.argv) > 5 else 12
w, bpp = a[0], a[2]
tot = band = n = nb = off = 0
for y in range(a[1]):
    for x in range(w):
        i = (y * w + x) * bpp
        for k in range(3):
            d = abs(a[3][i + k] - b[3][i + k])
            tot += d
            n += 1
            if d > thr:
                off += 1
            if y0 <= y < y1:
                band += d
                nb += 1
print("mean abs diff %.3f levels (rows %d to %d: %.3f); values off by more than %d: %d of %d" % (tot / n, y0, y1, band / max(nb, 1), thr, off, n))
