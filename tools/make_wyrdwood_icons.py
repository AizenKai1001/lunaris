"""Paints the icons of Wyrdwood's items into one picture, `examples/wyrdwood/icons.png`: an 8 x 8 atlas of 64 x 64 cells, the item with number n (tools/wyrdwood_items.py, starting at 1) in cell n (row n // 8, column n % 8).
A `Ui` with a `UiStyle.image_uv` picks its cell, so a slot shows whatever item is in it. Everything is drawn from polygons with the standard library: the same bytes every time.

    python -I tools/make_wyrdwood_icons.py
"""
import math
import os
import struct
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import wyrdwood_items as I  # noqa: E402

CELL, COLS = 64, 8
SS = 3  # samples per pixel side


class Canvas:
    def __init__(self):
        self.px = [[(0.0, 0.0, 0.0, 0.0)] * CELL for _ in range(CELL)]

    def shape(self, inside, color, bbox):
        x0, y0, x1, y1 = [max(0, int(v)) for v in (bbox[0] - 1, bbox[1] - 1, bbox[2] + 1, bbox[3] + 1)]
        x1, y1 = min(CELL - 1, x1), min(CELL - 1, y1)
        for y in range(y0, y1 + 1):
            row = self.px[y]
            for x in range(x0, x1 + 1):
                hit = 0
                for sy in range(SS):
                    for sx in range(SS):
                        if inside(x + (sx + 0.5) / SS, y + (sy + 0.5) / SS):
                            hit += 1
                if hit:
                    a = hit / (SS * SS) * color[3]
                    r, g, b, ba = row[x]
                    out_a = a + ba * (1 - a)
                    if out_a > 0:
                        row[x] = ((color[0] * a + r * ba * (1 - a)) / out_a, (color[1] * a + g * ba * (1 - a)) / out_a, (color[2] * a + b * ba * (1 - a)) / out_a, out_a)

    def poly(self, pts, color):
        xs, ys = [p[0] for p in pts], [p[1] for p in pts]

        def inside(x, y):
            c = False
            j = len(pts) - 1
            for i in range(len(pts)):
                xi, yi, xj, yj = pts[i][0], pts[i][1], pts[j][0], pts[j][1]
                if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
                    c = not c
                j = i
            return c

        self.shape(inside, color, (min(xs), min(ys), max(xs), max(ys)))

    def circle(self, cx, cy, r, color, sx=1.0, sy=1.0):
        self.shape(lambda x, y: ((x - cx) / sx) ** 2 + ((y - cy) / sy) ** 2 <= r * r, color, (cx - r * sx, cy - r * sy, cx + r * sx, cy + r * sy))

    def line(self, a, b, w, color):
        dx, dy = b[0] - a[0], b[1] - a[1]
        n = math.hypot(dx, dy) or 1.0
        nx, ny = -dy / n * w / 2, dx / n * w / 2
        self.poly([(a[0] + nx, a[1] + ny), (b[0] + nx, b[1] + ny), (b[0] - nx, b[1] - ny), (a[0] - nx, a[1] - ny)], color)


def shade(c, k):
    return (max(0, min(1, c[0] * k)), max(0, min(1, c[1] * k)), max(0, min(1, c[2] * k)), 1.0)


def outlined(cv, pts, col):
    """A polygon with a dark edge and a light top."""
    cx = sum(p[0] for p in pts) / len(pts)
    cy = sum(p[1] for p in pts) / len(pts)
    big = [(cx + (p[0] - cx) * 1.12, cy + (p[1] - cy) * 1.12) for p in pts]
    cv.poly(big, shade(col, 0.4))
    cv.poly(pts, shade(col, 1.0))
    small = [(cx + (p[0] - cx) * 0.55, cy + (p[1] - cy) * 0.55 - 2) for p in pts]
    cv.poly(small, shade(col, 1.18))


def blob(cx, cy, r, n, seed, wob=0.22):
    pts = []
    for k in range(n):
        a = k / n * math.tau
        v = 1 + wob * math.sin(a * 3 + seed) * 0.8 + wob * 0.5 * math.sin(a * 5 + seed * 1.7)
        pts.append((cx + math.cos(a) * r * v, cy + math.sin(a) * r * v))
    return pts


WOOD = (0.52, 0.34, 0.18)


def handle(cv, a=(18, 54), b=(46, 12), w=6):
    cv.line((a[0] + 1, a[1] + 1), (b[0] + 1, b[1] + 1), w + 2, shade(WOOD, 0.4))
    cv.line(a, b, w, shade(WOOD, 1.0))
    cv.line((a[0] - 1, a[1] - 1), (b[0] - 1, b[1] - 1), 2, shade(WOOD, 1.3))


def draw(key, col):
    cv = Canvas()
    if key in ("wood", "fine"):
        base = col
        for (x, y) in ((8, 36), (10, 24), (6, 12)):
            cv.line((x, y + 6), (x + 44, y + 6), 11, shade(base, 0.4))
            cv.line((x, y + 6), (x + 44, y + 6), 9, shade(base, 1.0))
            cv.line((x + 2, y + 3), (x + 42, y + 3), 2, shade(base, 1.3))
            cv.circle(x, y + 6, 5.5, shade(base, 1.35), 0.7, 1.0)
            cv.circle(x, y + 6, 2.5, shade(base, 0.85), 0.7, 1.0)
    elif key == "stone":
        outlined(cv, blob(32, 34, 20, 9, 1.0), col)
        outlined(cv, blob(46, 44, 9, 7, 2.0), shade(col, 0.9))
    elif key == "flint":
        outlined(cv, [(12, 44), (24, 14), (40, 10), (54, 30), (46, 50), (26, 54)], col)
        cv.line((26, 18), (40, 44), 2, shade(col, 1.6))
    elif key == "resin":
        outlined(cv, [(32, 8), (46, 34), (44, 48), (32, 54), (20, 48), (18, 34)], col)
        cv.circle(26, 38, 4, (1, 0.9, 0.55, 0.9))
    elif key in ("leather", "hide"):
        outlined(cv, blob(32, 32, 24 if key == "hide" else 19, 11, 0.5 if key == "hide" else 2.5, 0.28), col)
        for k in range(4):
            cv.line((18 + k * 8, 24), (22 + k * 8, 44), 2, shade(col, 0.7))
    elif key == "bone":
        cv.line((14, 50), (50, 14), 7, shade(col, 0.5))
        cv.line((14, 50), (50, 14), 5, col + (1.0,))
        for (x, y) in ((12, 46), (18, 52), (46, 12), (52, 18)):
            cv.circle(x, y, 5, shade(col, 0.55))
            cv.circle(x, y, 4, col + (1.0,))
    elif key == "coal":
        for (x, y, r, s) in ((24, 38, 14, 0.4), (42, 42, 11, 1.3), (36, 22, 10, 2.2)):
            outlined(cv, blob(x, y, r, 7, s, 0.18), col)
    elif key == "berries":
        cv.line((32, 22), (22, 8), 3, (0.2, 0.5, 0.2, 1))
        cv.poly([(30, 14), (46, 6), (44, 20)], (0.25, 0.62, 0.25, 1))
        for (x, y) in ((22, 38), (38, 40), (30, 28), (44, 28), (28, 48)):
            cv.circle(x, y, 9, shade(col, 0.45))
            cv.circle(x, y, 8, col + (1.0,))
            cv.circle(x - 3, y - 3, 2.5, (1, 0.7, 0.8, 0.9))
    elif key == "mushroom":
        cv.poly([(26, 34), (38, 34), (40, 54), (24, 54)], (0.9, 0.86, 0.76, 1))
        cv.circle(32, 34, 20, shade(col, 0.5), 1.0, 0.8)
        cv.circle(32, 34, 19, (0.78, 0.5, 0.3, 1), 1.0, 0.8)
        cv.poly([(10, 34), (54, 34), (54, 42), (10, 42)], (0.9, 0.86, 0.76, 1))
        for (x, y) in ((22, 22), (36, 18), (44, 28)):
            cv.circle(x, y, 3, (1, 0.95, 0.85, 1))
    elif key in ("meat", "cooked"):
        outlined(cv, blob(30, 34, 20, 10, 1.3, 0.25), col)
        cv.line((44, 22), (56, 10), 5, (0.9, 0.88, 0.8, 1))
        cv.circle(57, 9, 5, (0.9, 0.88, 0.8, 1))
    elif key == "honey":
        outlined(cv, [(20, 22), (44, 22), (48, 52), (16, 52)], col)
        cv.poly([(18, 14), (46, 14), (44, 22), (20, 22)], (0.55, 0.38, 0.2, 1))
    elif key == "torch":
        cv.line((22, 58), (36, 22), 7, shade(WOOD, 0.45))
        cv.line((22, 58), (36, 22), 5, WOOD + (1.0,))
        cv.poly([(36, 4), (46, 18), (44, 28), (36, 32), (28, 28), (26, 18)], (1.0, 0.45, 0.1, 1))
        cv.poly([(36, 12), (42, 22), (40, 28), (36, 30), (32, 26), (31, 20)], (1.0, 0.85, 0.3, 1))
    elif key in ("stone_axe", "flint_axe"):
        handle(cv, (16, 56), (44, 10))
        outlined(cv, [(30, 8), (58, 14), (52, 34), (36, 30)], col)
    elif key == "club":
        cv.poly([(10, 58), (18, 60), (34, 28), (50, 8), (56, 20), (46, 38), (26, 52)], shade(WOOD, 0.4))
        cv.poly([(13, 56), (19, 57), (35, 27), (49, 11), (53, 19), (44, 36), (26, 49)], WOOD + (1.0,))
        cv.line((20, 52), (46, 14), 2, shade(WOOD, 1.35))
    elif key == "hammer":
        handle(cv, (16, 58), (42, 20))
        outlined(cv, [(24, 8), (56, 8), (56, 28), (24, 28)], col)
    elif key == "flint_knife":
        outlined(cv, [(30, 6), (46, 10), (40, 40), (28, 38)], col)
        cv.line((34, 38), (22, 58), 8, shade(WOOD, 0.45))
        cv.line((34, 38), (22, 58), 6, WOOD + (1.0,))
    elif key == "trophy_moss":
        outlined(cv, [(32, 54), (8, 28), (10, 14), (22, 10), (32, 20), (42, 10), (54, 14), (56, 28)], col)
        cv.circle(32, 30, 6, (0.9, 1.0, 0.7, 0.8))
    elif key == "horn_pick":
        handle(cv, (14, 58), (44, 18))
        cv.poly([(8, 22), (22, 10), (50, 12), (60, 28), (52, 22), (36, 18), (18, 24)], shade(col, 0.45))
        cv.poly([(10, 22), (23, 12), (49, 14), (57, 26), (50, 22), (36, 20), (19, 26)], col + (1.0,))
    elif key == "bow":
        for k in range(20):
            a0, a1 = -1.2 + k * 0.12, -1.2 + (k + 1) * 0.12
            cv.line((14 + 36 * math.cos(a0) * 0.9, 32 + 30 * math.sin(a0)), (14 + 36 * math.cos(a1) * 0.9, 32 + 30 * math.sin(a1)), 5, WOOD + (1.0,))
        cv.line((20, 8), (20, 56), 1.5, (0.95, 0.93, 0.85, 1))
    elif key == "arrow":
        cv.line((10, 54), (50, 14), 3, WOOD + (1.0,))
        cv.poly([(50, 8), (58, 6), (56, 14), (52, 18), (46, 16)], (0.7, 0.72, 0.76, 1))
        cv.poly([(8, 56), (4, 50), (12, 52)], (0.9, 0.9, 0.85, 1))
        cv.poly([(8, 56), (14, 60), (14, 52)], (0.9, 0.9, 0.85, 1))
    elif key == "wood_shield":
        cv.circle(32, 32, 26, shade(WOOD, 0.4))
        cv.circle(32, 32, 24, WOOD + (1.0,))
        for r in (18, 12):
            cv.circle(32, 32, r, shade(WOOD, 0.9 if r == 18 else 1.15))
        cv.circle(32, 32, 7, (0.7, 0.7, 0.74, 1))
    elif key == "rag_tunic":
        outlined(cv, [(20, 8), (44, 8), (58, 22), (50, 30), (44, 24), (44, 56), (20, 56), (20, 24), (14, 30), (6, 22)], col)
    else:
        cv.circle(32, 32, 18, col + (1.0,))
    return cv


def main():
    n = CELL * COLS
    atlas = [[(0, 0, 0, 0)] * n for _ in range(n)]
    for num, it in I.BY_ID.items():
        key, col = it[0], it[6]
        cv = draw(key, col)
        cx, cy = (num % COLS) * CELL, (num // COLS) * CELL
        for y in range(CELL):
            for x in range(CELL):
                r, g, b, a = cv.px[y][x]
                atlas[cy + y][cx + x] = (round(r * 255), round(g * 255), round(b * 255), round(a * 255))
    raw = bytearray()
    for row in atlas:
        raw.append(0)
        for p in row:
            raw += bytes(p)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "wyrdwood", "icons.png")
    open(out, "wb").write(png)
    print("wrote", os.path.normpath(out), len(I.ITEMS), "icons")


if __name__ == "__main__":
    main()
