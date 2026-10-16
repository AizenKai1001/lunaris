"""Makes the pictures and the map data of Caldera's island with nothing but the standard library: examples/caldera/world/ground.png (a 16-bit heightmap), splat.png (an RGBA map of
four materials: grass, rock, sand and ash), water.png (the surface heights of the sea and the lake), ash.png and lava.png (two tileable textures), shot_map.png (a shaded picture of
the whole island, for looking at it without the engine) and world.json (where the camp, the shards, the golems and the ruins are, with the height of the ground under each).

    python -I tools/make_caldera_world.py            (about ten seconds; the same bytes every time)

The ground is worked out in metres first (`ground_m`), then written as 16 bits (`RELIEF` metres from black to white) and read back, so that every height in world.json is what the engine stands on.
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
OUT = os.path.join(ROOT, "examples", "caldera", "world")
N, SIZE, RELIEF, SEED = 449, 448.0, 90.0, 11
CELL = SIZE / (N - 1)
SEA, LAKE = 5.0, 9.5  # the height of the sea's surface and of the lake's, in metres over the heightmap's black
CAMP = (45.0, 40.0)
VOLCANO = (-18.0, -70.0)
LAKE_AT, LAKE_R = (-72.0, 44.0), 20.0
HIGHLAND = (-102.0, -42.0)
ARENA = (VOLCANO[0], VOLCANO[1] + 21.0)
ARENA_H = None  # the height of the ground there before it is flattened (worked out below)
PLATFORMS = [(-52.0, -6.0, 9.0, 17.0), (HIGHLAND[0], HIGHLAND[1], 9.0, 17.0)]  # x, z, the radius that is flat, the radius where it has blended into the land
PLATFORM_H = []


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
    """The island's radius at an angle: a smooth wobble round 125 m."""
    c, s = math.cos(ang), math.sin(ang)
    return 146.0 + 20.0 * (mh.fbm(c * 1.6 + 5.0, s * 1.6 + 5.0, SEED + 3, 3) - 0.5) * 2.0


def ground_at_m(x, z, arena=True):
    """The ground's height in metres at world (x, z): sea floor, a coast with beaches, forest hills, a rocky plateau in the north-west, a volcano with a crater in the north."""
    r = math.hypot(x, z)
    ang = math.atan2(z, x)
    big = mh.fbm(x / 95.0 + 3.0, z / 95.0 + 7.0, SEED, 5)
    small = mh.fbm(x / 22.0, z / 22.0, SEED + 50, 3)
    land = 15.0 + 6.5 * (big - 0.45) * 2.0 + 1.4 * (small - 0.5)
    # a rocky plateau and ridges in the north-west
    hd = math.hypot(x - HIGHLAND[0], z - HIGHLAND[1])
    ridges = mh.fbm(x / 40.0 + 11.0, z / 40.0, SEED + 100, 5, ridged=True)
    land += 17.0 * smooth(75.0, 22.0, hd) * (0.65 + 0.55 * ridges)
    # the volcano: a cone with a crater, rim at 26 m from the middle
    vd = math.hypot(x - VOLCANO[0], z - VOLCANO[1])
    gully = 0.88 + 0.24 * mh.fbm(math.atan2(z - VOLCANO[1], x - VOLCANO[0]) * 2.5 + 20.0, vd / 38.0, SEED + 7, 4)
    cone = 42.0 * (smooth(88.0, 22.0, vd) ** 1.05) * gully
    crater = 22.0 * smooth(21.0, 7.0, vd)
    land += cone - crater
    # a flat arena on the south rim of the crater, where the boss waits
    ad = math.hypot(x - ARENA[0], z - ARENA[1])
    if arena and ARENA_H is not None:
        land += (ARENA_H - land) * smooth(15.0, 9.0, ad)
        # level ground for the ruin and the standing stones (walls and circles do not stand on a slope)
        for (px, pz, rf, rb), h in zip(PLATFORMS, PLATFORM_H):
            land += (h - land) * smooth(rb, rf, math.hypot(x - px, z - pz))
    # the camp's meadow and the lake's bowl
    cd = math.hypot(x - CAMP[0], z - CAMP[1])
    land += (13.0 - land) * smooth(26.0, 13.0, cd)
    ld = math.hypot(x - LAKE_AT[0], z - LAKE_AT[1]) / LAKE_R
    land -= 19.0 * math.exp(-1.6 * ld * ld) * smooth(2.4, 1.0, ld)
    # a little island in the lake, a stack of rock off the east coast
    land += 15.5 * math.exp(-((math.hypot(x - (LAKE_AT[0] + 3.0), z - (LAKE_AT[1] - 2.0)) / 4.4) ** 2))
    # the coast: land inside the radius, sea floor outside
    R = coast(ang)
    m = smooth(R + 6.0, R - 46.0, r)
    floor = 0.8 + 2.2 * mh.fbm(x / 30.0, z / 30.0, SEED + 9, 3)
    g = floor + (land - floor) * m ** 1.7
    # a sand bar and an islet off the east coast
    g += 5.5 * math.exp(-((math.hypot(x - 168.0, z - 92.0) / 5.5) ** 2))
    # the map's rim is sea floor
    e = min(SIZE / 2 - abs(x), SIZE / 2 - abs(z))
    g = g * smooth(0.0, 26.0, e) + floor * (1.0 - smooth(0.0, 26.0, e))
    return max(0.2, g)


ARENA_H = ground_at_m(*ARENA, arena=False)
PLATFORM_H = [ground_at_m(p[0], p[1], arena=False) for p in PLATFORMS]


def build():
    os.makedirs(OUT, exist_ok=True)
    ground_m = []
    for j in range(N):
        z = (j - (N - 1) / 2) * CELL
        ground_m.append([ground_at_m((i - (N - 1) / 2) * CELL, z) for i in range(N)])
    # a few passes of slumping so that cliffs have talus slopes
    mh.thermal(ground_m, N, 8, 0.85 * CELL)
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

    # ---- the water: the sea wherever the ground is under SEA, the lake in its bowl
    water16 = [[0] * N for _ in range(N)]
    for j in range(N):
        z = (j - (N - 1) / 2) * CELL
        for i in range(N):
            x = (i - (N - 1) / 2) * CELL
            g = gm[j][i]
            level = None
            if math.hypot(x - LAKE_AT[0], z - LAKE_AT[1]) < LAKE_R * 1.6 and g < LAKE - 0.02 and math.hypot(x, z) < 160.0:
                level = LAKE
            elif g < SEA - 0.02:
                level = SEA
            if level is not None:
                water16[j][i] = max(1, min(65535, round(level / RELIEF * 65535.0)))
    mh.write_png16(os.path.join(OUT, "water.png"), water16)

    # ---- the splat map: grass, rock, sand, ash (red, green, blue, alpha)
    rows = []
    sl_cache = [[slope_deg(i, j) for i in range(N)] for j in range(N)]
    for j in range(N):
        row = []
        for i in range(N):
            x, z = (i - (N - 1) / 2) * CELL, (j - (N - 1) / 2) * CELL
            g, sl = gm[j][i], sl_cache[j][i]
            wob = mh.fbm(i * 0.08, j * 0.08, SEED + 5, 3) - 0.5
            vd = math.hypot(x - VOLCANO[0], z - VOLCANO[1])
            sand = smooth(SEA + 5.2 + wob * 3.0, SEA + 2.6 + wob * 2.0, g) if math.hypot(x - LAKE_AT[0], z - LAKE_AT[1]) > LAKE_R * 1.3 or g < LAKE + 1.2 else 0.0
            sand = max(sand, smooth(LAKE + 1.6, LAKE + 0.2, g) * smooth(LAKE_R * 1.9, LAKE_R * 1.2, math.hypot(x - LAKE_AT[0], z - LAKE_AT[1])))
            ash = max(smooth(44.0 + wob * 12.0, 56.0 + wob * 8.0, g) * smooth(62.0, 40.0, vd), smooth(40.0, 26.0, vd + wob * 16.0))
            outcrop = smooth(0.58, 0.68, mh.fbm(i * 0.04 + 3.0, j * 0.04, SEED + 9, 4)) * smooth(10.0, 16.0, g) * 0.85
            rock = max(smooth(21.0, 33.0, sl + wob * 6.0), smooth(26.0, 40.0, g + wob * 12.0) * 0.8, outcrop)
            rock *= 1.0 - 0.9 * ash
            grass = max(0.0, 1.0 - sand - ash - rock)
            tot = grass + rock + sand + ash
            if tot <= 0:
                grass, tot = 1.0, 1.0
            row += [round(255 * grass / tot), round(255 * rock / tot), round(255 * sand / tot), round(255 * ash / tot)]
        rows.append(row)
    write_png8(os.path.join(OUT, "splat.png"), N, N, 4, rows)

    def weights(x, z):
        i, j = max(0, min(N - 1, round(x / CELL + (N - 1) / 2))), max(0, min(N - 1, round(z / CELL + (N - 1) / 2)))
        r = rows[j][4 * i: 4 * i + 4]
        return [c / 255.0 for c in r]

    # ---- two textures: ash (dark grey grit) and lava (black crust over orange)
    T = 128

    def tile_noise(x, y, period, seed):
        px, py = x * period / T, y * period / T
        ix, iy = math.floor(px), math.floor(py)
        fx, fy = px - ix, py - iy
        fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
        h = lambda a, b: mh._hash(a % period, b % period, seed)  # noqa: E731
        top = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * fx
        bot = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * fx
        return top + (bot - top) * fy

    ash, lava = [], []
    for y in range(T):
        arow, lrow = [], []
        for x in range(T):
            n = 0.45 * tile_noise(x, y, 8, 21) + 0.3 * tile_noise(x, y, 16, 22) + 0.25 * tile_noise(x, y, 32, 23)
            grit = tile_noise(x, y, 64, 24)
            v = 0.20 + 0.26 * n + 0.07 * grit
            arow += [round(255 * v * 1.06), round(255 * v), round(255 * v * 0.94)]
            c = 0.5 * tile_noise(x, y, 4, 31) + 0.3 * tile_noise(x, y, 8, 32) + 0.2 * tile_noise(x, y, 16, 33)
            crust = smooth(0.46, 0.58, c)
            glow = 1.0 - crust
            lrow += [round(255 * (0.07 + 0.93 * glow)), round(255 * (0.03 + 0.46 * glow * glow)), round(255 * (0.02 + 0.06 * glow))]
        ash.append(arow)
        lava.append(lrow)
    write_png8(os.path.join(OUT, "ash.png"), T, T, 3, ash)
    write_png8(os.path.join(OUT, "lava.png"), T, T, 3, lava)

    # ---- a shaded picture of the island for looking at it (the colour of the dominant material, lit from the north-west, the sea blue)
    shade = []
    cols = [(86, 120, 58), (118, 108, 98), (214, 196, 148), (48, 44, 48)]
    for j in range(N):
        row = []
        for i in range(N):
            if water16[j][i]:
                d = 0.35 + 0.55 * smooth(0.0, 6.0, SEA - gm[j][i]) if water16[j][i] and gm[j][i] < SEA else 0.5
                row += [round(30 * (1 - d) + 20), round(90 * (1 - d) + 40), round(150 * (1 - d) + 60)] if gm[j][i] < LAKE - 0.5 else [60, 130, 190]
                continue
            w = rows[j][4 * i: 4 * i + 4]
            c = [sum(w[k] * cols[k][ch] for k in range(4)) / 255.0 for ch in range(3)]
            gx = (gm[j][min(N - 1, i + 1)] - gm[j][max(0, i - 1)]) / (2 * CELL)
            gz = (gm[min(N - 1, j + 1)][i] - gm[max(0, j - 1)][i]) / (2 * CELL)
            lit = max(0.25, min(1.4, 0.8 + 3.2 * (gx * 0.6 + gz * 0.8) * -1.0 * 0.5 + 0.0))
            row += [round(min(255, c[0] * lit)), round(min(255, c[1] * lit)), round(min(255, c[2] * lit))]
        shade.append(row)
    write_png8(os.path.join(OUT, "shot_map.png"), N, N, 3, shade)

    return gm, ground, weights, sl_cache, water16


def walk_report(gm, water16, pois):
    """Which of the land can be walked to from the camp: a flood over the heightmap that does not climb ground steeper than 46 degrees or enter water deeper than a metre. Writes walk_map.png
    (green reachable, red land that is not, blue water) and returns the share of the land that is reachable and, for each place, whether it is."""
    import collections
    limit = math.tan(math.radians(46.0))

    def cell_of(x, z):
        return max(0, min(N - 1, round(z / CELL + (N - 1) / 2))), max(0, min(N - 1, round(x / CELL + (N - 1) / 2)))

    start = cell_of(*CAMP)
    seen = {start}
    q = collections.deque([start])
    while q:
        j, i = q.popleft()
        for dj, di in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)):
            nj, ni = j + dj, i + di
            if not (0 <= nj < N and 0 <= ni < N) or (nj, ni) in seen:
                continue
            step = math.hypot(dj, di) * CELL
            if abs(gm[nj][ni] - gm[j][i]) > limit * step:
                continue
            wet = water16[nj][ni] / 65535.0 * RELIEF - gm[nj][ni] if water16[nj][ni] else 0.0
            if wet > 1.0:
                continue
            seen.add((nj, ni))
            q.append((nj, ni))
    rows = []
    land = ok = 0
    for j in range(N):
        row = []
        for i in range(N):
            wet = water16[j][i] / 65535.0 * RELIEF - gm[j][i] if water16[j][i] else 0.0
            if wet > 1.0:
                row += [40, 90, 160]
            elif gm[j][i] < SEA:
                row += [30, 60, 120]
            else:
                land += 1
                if (j, i) in seen:
                    ok += 1
                    row += [80, 170, 80]
                else:
                    row += [200, 60, 50]
        rows.append(row)
    for name, (x, z) in pois.items():
        j, i = cell_of(x, z)
        for dj in range(-2, 3):
            for di in range(-2, 3):
                if 0 <= j + dj < N and 0 <= i + di < N:
                    rows[j + dj][3 * (i + di): 3 * (i + di) + 3] = [255, 255, 255] if (j, i) in seen else [255, 255, 0]
    write_png8(os.path.join(OUT, "walk_map.png"), N, N, 3, [sum(([a, b, c] for a, b, c in zip(r[0::3], r[1::3], r[2::3])), []) for r in rows])
    return ok / max(1, land), {name: cell_of(x, z) in seen for name, (x, z) in pois.items()}


def main():
    gm, ground, weights, sl, water16 = build()

    def beach(angle_deg, inland=5.0):
        """A point on the sand: walk in from the sea along an angle until the ground is a metre and a half over the sea, then a few steps inland."""
        a = math.radians(angle_deg)
        r = 200.0
        while r > 0 and ground(math.cos(a) * r, math.sin(a) * r) < SEA + 1.3:
            r -= 0.5
        r -= inland
        return math.cos(a) * r, math.sin(a) * r

    def at(x, z, **extra):
        return {"x": round(x, 2), "z": round(z, 2), "y": round(ground(x, z), 3), **extra}

    info = {
        "size": SIZE, "n": N, "relief": RELIEF, "sea": SEA, "lake": LAKE,
        "camp": at(*CAMP), "volcano": at(*VOLCANO), "lake_at": at(*LAKE_AT), "highland": at(*HIGHLAND), "arena": at(*ARENA),
        "max_height": round(max(max(r) for r in gm), 2),
    }
    for name, ang in (("start", 82.0), ("wreck", 18.0), ("bones", 112.0)):
        bx, bz = beach(ang)
        info[name] = at(bx, bz)
    pois = {"start": (info["start"]["x"], info["start"]["z"]), "wreck": (info["wreck"]["x"], info["wreck"]["z"]), "bones": (info["bones"]["x"], info["bones"]["z"]), "forest ruin": (-52.0, -6.0),
            "lake shore": (LAKE_AT[0] + 14.0, LAKE_AT[1] + 8.0), "highland stones": HIGHLAND, "arena": ARENA, "shard 6": (ARENA[0], ARENA[1] - 7.5), "crater floor": VOLCANO}
    share, flags = walk_report(gm, water16, pois)
    print(f"  walkable from the camp: {share * 100:.0f}% of the land;", ", ".join(f"{k} {'yes' if v else 'NO'}" for k, v in flags.items()))
    json.dump(info, open(os.path.join(OUT, "world.json"), "w"), indent=1)
    print("wrote", sorted(os.listdir(OUT)))
    print("camp", info["camp"], "volcano", info["volcano"], "max", info["max_height"])
    for name, (x, z) in (("camp", CAMP), ("beach start", beach(82.0)), ("wreck", beach(18.0)), ("bones", beach(112.0)), ("forest ruin", (-52, -6)), ("lake island", (LAKE_AT[0] + 3, LAKE_AT[1] - 2)),
                         ("highland", HIGHLAND), ("rim south", (VOLCANO[0], VOLCANO[1] + 22)), ("crater floor", VOLCANO)):
        print(f"  {name:12} ({x:6.1f}, {z:6.1f}) ground {ground(x, z):5.1f} m  weights {[round(w, 2) for w in weights(x, z)]}")


if __name__ == "__main__":
    main()
