"""Write examples/terrain_layers.lunaris.json, the pictures it reads in examples/terrain_layers/ (a 16-bit heightmap, an RGBA splat map, a 16-bit map of the water's heights and a tileable snow
texture) and the playtest examples/tests/terrain_layers.test.json: a valley painted with four materials (`TerrainLayers`, docs/TERRAIN.md "Layers"), a lake and a river that steps down
the valley to it (`WaterBody`, "Water bodies"), a hero who can wade into the lake (A and D walk, W and S, Space jumps: `InWater` slows it down) and a few things that float on it (`Buoyant`).

    python -I tools/build_terrain_layers.py

Everything is made from the standard library, from a seed, so the same bytes come out every time. The ground is made in metres (`ground`), the river runs down a valley that is carved to
it (and filled up to it where the land is lower), the splat map is painted from the height and the slope of the ground (sand at the water, rock on the steep, snow on the high ground, grass
elsewhere), and the water picture marks every sample whose ground is under the surface of the lake or of the river at that place.
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
OUT = os.path.join(ROOT, "examples", "terrain_layers")
N, SIZE, RELIEF, SEED = 257, 256.0, 60.0, 5  # samples a side, metres across, metres from black to white, the seed
CELL = SIZE / (N - 1)
LAKE_AT, LAKE_R, LAKE_LEVEL = (-30.0, 28.0), 24.0, 4.0  # the lake's middle (x, z), the size of the bowl, the surface's height
# the river: where it runs (x, z) and the height of its surface at the start; it ends at the shore, at the lake's level
RIVER = [(22.0, -66.0), (30.0, -48.0), (14.0, -32.0), (24.0, -14.0), (6.0, 0.0), (-6.0, 8.0), (-14.0, 13.0)]
RIVER_START = 17.0
RIVER_HALF, VALLEY = 3.3, 14.0  # how wide the water is (half), how far the valley reaches from the river's middle


def smooth(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3.0 - 2.0 * t)


def png_chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def write_png8(path, w, h, channels, pixels):
    """An 8-bit PNG (3 channels RGB, 4 RGBA) from a flat list of rows of bytes."""
    raw = bytearray()
    for row in pixels:
        raw.append(0)
        raw += bytes(row)
    colour = {3: 2, 4: 6}[channels]
    png = b"\x89PNG\r\n\x1a\n" + png_chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, colour, 0, 0, 0)) + png_chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + png_chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


os.makedirs(OUT, exist_ok=True)

# ---- the river as a list of points with the surface's height at each (falling evenly with the length)
pts = []
length = 0.0
for k, p in enumerate(RIVER):
    if k:
        length += math.hypot(p[0] - RIVER[k - 1][0], p[1] - RIVER[k - 1][1])
    pts.append((p[0], p[1], length))
LENGTH = length


def river_level(s):
    return RIVER_START + (LAKE_LEVEL - RIVER_START) * s / LENGTH


def near_river(x, z):
    """(distance to the river's middle line in metres, the surface's height there)."""
    best = (1e9, 0.0)
    for k in range(len(pts) - 1):
        (ax, az, sa), (bx, bz, sb) = pts[k], pts[k + 1]
        dx, dz = bx - ax, bz - az
        t = max(0.0, min(1.0, ((x - ax) * dx + (z - az) * dz) / (dx * dx + dz * dz)))
        d = math.hypot(x - (ax + dx * t), z - (az + dz * t))
        if d < best[0]:
            best = (d, river_level(sa + (sb - sa) * t))
    return best


# ---- the ground in metres: rolling hills, mountains in the north, a bowl for the lake, a valley for the river
ground_m = []
for j in range(N):
    row = []
    v = j / (N - 1)
    for i in range(N):
        u = i / (N - 1)
        x, z = (i - (N - 1) / 2) * CELL, (j - (N - 1) / 2) * CELL
        hills = mh.fbm(u * 3.2, v * 3.2, SEED)
        peaks = mh.fbm(u * 3.0 + 7.0, v * 3.0, SEED + 100, 6, ridged=True)
        north = mh.edge(0.46, 0.04, v)
        g = 9.0 + 24.0 * (hills - 0.5) + 38.0 * north * (0.3 + peaks)
        # the lake's bowl
        d = math.hypot(x - LAKE_AT[0], z - LAKE_AT[1]) / LAKE_R
        g -= 9.5 * math.exp(-1.7 * d * d)
        # the river's valley: the bed 0.9 m under the surface, banks 2.6 m over it, blended into the land round about
        rd, level = near_river(x, z)
        if rd < VALLEY:
            target = level - 0.9 + 3.5 * mh.edge(0.0, 10.0, rd)
            g += (target - g) * (1.0 - smooth(6.0, VALLEY, rd))
        row.append(max(0.4, g))
    ground_m.append(row)
# the edges of the map are low (the picture is a landscape, not a table): a soft fall to the sea level over the last 12 m
for j in range(N):
    for i in range(N):
        e = min(i, j, N - 1 - i, N - 1 - j) * CELL
        ground_m[j][i] = max(0.4, ground_m[j][i] * (0.25 + 0.75 * smooth(0.0, 18.0, e)) if e < 18.0 else ground_m[j][i])
heights16 = [[max(0, min(65535, round(g / RELIEF * 65535.0))) for g in row] for row in ground_m]
mh.write_png16(os.path.join(OUT, "ground.png"), heights16)
# from here on the ground is what the 16-bit picture says (what the engine stands on)
ground_m = [[h / 65535.0 * RELIEF for h in row] for row in heights16]


def ground(x, z):
    """The height of the ground at world (x, z), the way the engine works it out: two triangles to a cell, the diagonal from (i+1, j) to (i, j+1)."""
    fx, fz = (x + SIZE / 2) / CELL, (z + SIZE / 2) / CELL
    i, j = max(0, min(N - 2, int(math.floor(fx)))), max(0, min(N - 2, int(math.floor(fz))))
    u, w = fx - i, fz - j
    p00, p10, p01, p11 = ground_m[j][i], ground_m[j][i + 1], ground_m[j + 1][i], ground_m[j + 1][i + 1]
    return p00 + (p10 - p00) * u + (p01 - p00) * w if u + w <= 1.0 else p11 + (p01 - p11) * (1 - u) + (p10 - p11) * (1 - w)


def slope_deg(i, j):
    gx = (ground_m[j][min(N - 1, i + 1)] - ground_m[j][max(0, i - 1)]) / (2 * CELL)
    gz = (ground_m[min(N - 1, j + 1)][i] - ground_m[max(0, j - 1)][i]) / (2 * CELL)
    return math.degrees(math.atan(math.hypot(gx, gz)))


# ---- the water: every sample whose ground is under the lake's surface (near the lake) or under the river's (in its channel)
water16 = [[0] * N for _ in range(N)]
wet = 0
for j in range(N):
    for i in range(N):
        x, z = (i - (N - 1) / 2) * CELL, (j - (N - 1) / 2) * CELL
        g = ground_m[j][i]
        level = None
        if math.hypot(x - LAKE_AT[0], z - LAKE_AT[1]) < LAKE_R * 1.5 and g < LAKE_LEVEL - 0.02:
            level = LAKE_LEVEL
        else:
            rd, rl = near_river(x, z)
            if rd < RIVER_HALF + 1.2 and g < rl - 0.02:
                level = rl
        if level is not None:
            water16[j][i] = max(1, min(65535, round(level / RELIEF * 65535.0)))
            wet += 1
mh.write_png16(os.path.join(OUT, "water.png"), water16)

# ---- the splat map: grass, rock, sand, snow (red, green, blue, alpha) by the height and the slope of the ground
rows = []
for j in range(N):
    row = []
    for i in range(N):
        x, z = (i - (N - 1) / 2) * CELL, (j - (N - 1) / 2) * CELL
        g, sl = ground_m[j][i], slope_deg(i, j)
        wobble = 2.2 * (mh.fbm(i * 0.09, j * 0.09, SEED + 5, 3) - 0.5)
        near_water = min(math.hypot(x - LAKE_AT[0], z - LAKE_AT[1]) - LAKE_R * 0.95, near_river(x, z)[0] * 1.6 - 3.0)
        sand = smooth(1.6, 0.0, near_water) * smooth(7.0, 5.0, g + wobble * 0.5) if near_water < 1.6 else 0.0
        snow = smooth(27.0, 34.0, g + wobble * 2.0) * (1.0 - smooth(34.0, 52.0, sl))
        outcrop = smooth(0.60, 0.70, mh.fbm(i * 0.035 + 3.0, j * 0.035, SEED + 9, 4)) * smooth(7.0, 12.0, g) * 0.9
        rock = max(smooth(22.0, 36.0, sl + wobble * 3.0), smooth(36.0, 46.0, g + wobble * 2.0) * 0.55, outcrop)
        rock *= 1.0 - snow * 0.8
        grass = max(0.0, 1.0 - sand - snow - rock)
        total = grass + rock + sand + snow
        row += [round(255 * grass / total), round(255 * rock / total), round(255 * sand / total), round(255 * snow / total)]
    rows.append(row)
write_png8(os.path.join(OUT, "splat.png"), N, N, 4, rows)

# ---- a snow texture (the layers may be pictures as well as built-in materials): bluish white grain that tiles
T = 128


def tile_noise(x, y, period, seed):
    px, py = x * period / T, y * period / T
    ix, iy = math.floor(px), math.floor(py)
    fx, fy = px - ix, py - iy
    fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    h = lambda a, b: mh._hash(a % period, b % period, seed)
    top = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * fx
    bottom = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * fx
    return top + (bottom - top) * fy


snow = []
for y in range(T):
    row = []
    for x in range(T):
        n = 0.5 * tile_noise(x, y, 8, 1) + 0.3 * tile_noise(x, y, 16, 2) + 0.2 * tile_noise(x, y, 32, 3)
        row += [round(214 + 36 * n), round(224 + 28 * n), round(238 + 17 * n)]
    snow.append(row)
write_png8(os.path.join(OUT, "snow.png"), T, T, 3, snow)

# ---- the scene
entities = []


def add(name, **components):
    entities.append({"uid": len(entities) + 1, "components": {"Name": {"value": name}, **components}})
    return len(entities)


def weights_at(x, z):
    """The splat weights (grass, rock, sand, snow) at world (x, z), nearest sample: for placing things where they belong."""
    i, j = max(0, min(N - 1, round(x / CELL + (N - 1) / 2))), max(0, min(N - 1, round(z / CELL + (N - 1) / 2)))
    r = rows[j][4 * i: 4 * i + 4]
    return [c / 255.0 for c in r]


def slope_at(x, z):
    gx, gz = (ground(x + 1, z) - ground(x - 1, z)) / 2, (ground(x, z + 1) - ground(x, z - 1)) / 2
    return math.degrees(math.atan(math.hypot(gx, gz)))


add("Environment", Environment={"sky": True, "sky_top": [0.17, 0.4, 0.82], "sky_horizon": [0.86, 0.9, 0.96], "ground": [0.3, 0.32, 0.26], "clouds": 0.42, "fog": 0.0018, "fog_color": [0.78, 0.86, 0.95],
                                "bloom": 0.2, "ssao": 1.0, "exposure": 1.05, "vignette": 0.12})
add("Sun", Transform={"rot": [-0.46, 0.34, 0.14, 0.81]}, Light={"kind": 0, "color": [1.0, 0.94, 0.82], "intensity": 2.5})
layers = "grass grass uv_scale=52 bump=0.05\nrock concrete uv_scale=38 tint=0.66,0.6,0.55\nsand sand uv_scale=60\nsnow examples/terrain_layers/snow.png uv_scale=45"
add("Valley", Transform={"pos": [0, 0, 0]}, Terrain={"heightmap": "examples/terrain_layers/ground.png", "size": [SIZE, SIZE], "height": RELIEF, "step": 1, "color": [1, 1, 1, 1], "friction": 0.9},
    TerrainLayers={"splat": "examples/terrain_layers/splat.png", "layers": layers})
add("Water", Transform={"pos": [0, 0, 0]}, WaterBody={"heightmap": "examples/terrain_layers/water.png", "size": [SIZE, SIZE], "relief": RELIEF, "color": [0.05, 0.3, 0.42, 0.84], "wave_height": 0.16, "wave_length": 4.5, "speed": 1.1})

# things that float on the lake: wooden crates and a red ball
floaters = [("Crate 1", "cube", [-24.0, 30.0], 1.3, 450, [0.8, 0.8, 0.8, 1], "wood"), ("Crate 2", "cube", [-33.0, 24.0], 1.0, 600, [0.8, 0.8, 0.8, 1], "wood"),
            ("Crate 3", "cube", [-38.0, 33.0], 1.6, 350, [0.8, 0.8, 0.8, 1], "wood"), ("Buoy", "sphere", [-27.0, 21.0], 0.9, 250, [0.95, 0.2, 0.15, 1], "")]
for name, mesh, (x, z), size, density, colour, tex in floaters:
    mr = {"mesh": mesh, "color": colour, "roughness": 0.45}
    if tex:
        mr["texture"] = tex
        mr["uv_scale"] = [1, 1]
    add(name, Transform={"pos": [x, LAKE_LEVEL + 2.5, z]}, Velocity={}, Collider={"shape": 1 if mesh == "cube" else 0, "size": [size, size, size]}, RigidBody={"mass": 1, "restitution": 0.2, "friction": 0.6},
        Buoyant={"density": density, "drag": 4.0}, MeshRenderer=mr)

# pines on the grass: gentle ground above the shore, away from the hero's start and from the water
import random  # noqa: E402

rng = random.Random(23)
trees = []
HERO = (2.0, 28.0)
for _ in range(9000):
    if len(trees) >= 150:
        break
    x, z = rng.uniform(-118, 118), rng.uniform(-118, 118)
    g = ground(x, z)
    w = weights_at(x, z)
    if w[0] < 0.7 or g < 6.0 or g > 26.0 or slope_at(x, z) > 24.0 or math.hypot(x - HERO[0], z - HERO[1]) < 14.0 or near_river(x, z)[0] < 9.0:
        continue
    if any((x - a) ** 2 + (z - b) ** 2 < 36 for a, b, _ in trees):
        continue
    trees.append((x, z, g))
for k, (x, z, g) in enumerate(trees):
    big = 0.8 + 0.7 * rng.random()
    add(f"Pine {k + 1} trunk", Transform={"pos": [round(x, 3), round(g + 1.2 * big, 3), round(z, 3)], "scale": [0.45 * big, 2.4 * big, 0.45 * big]}, MeshRenderer={"mesh": "cylinder", "color": [0.36, 0.25, 0.16, 1], "roughness": 0.9})
    add(f"Pine {k + 1} top", Transform={"pos": [round(x, 3), round(g + 3.6 * big, 3), round(z, 3)], "scale": [2.3 * big, 3.6 * big, 2.3 * big]}, MeshRenderer={"mesh": "sphere", "color": [0.1, 0.32, 0.16, 1], "roughness": 0.8})
# boulders where the splat map says rock
rocks = []
for _ in range(9000):
    if len(rocks) >= 36:
        break
    x, z = rng.uniform(-118, 118), rng.uniform(-118, 118)
    if weights_at(x, z)[1] < 0.6 or ground(x, z) < 5.5 or near_river(x, z)[0] < 5.0:
        continue
    rocks.append((x, z))
for k, (x, z) in enumerate(rocks):
    s, a = 0.8 + 1.6 * rng.random(), rng.random() * math.pi
    add(f"Boulder {k + 1}", Transform={"pos": [round(x, 3), round(ground(x, z) + 0.25 * s, 3), round(z, 3)], "rot": [0, round(math.sin(a), 4), 0, round(math.cos(a), 4)], "scale": [1.3 * s, 0.9 * s, 1.1 * s]},
        MeshRenderer={"mesh": "sphere", "color": [0.55, 0.53, 0.5, 1], "roughness": 0.95, "texture": "concrete", "uv_scale": [2, 2]})

# the hero: a capsule-ish body on the grass east of the lake; the camera rides on it
hero_y = ground(*HERO)
hero = add("Hero", Transform={"pos": [HERO[0], hero_y + 0.3, HERO[1]]}, CharacterBody={"radius": 0.35, "height": 1.8, "step_height": 0.4})
add("Hero body", Transform={"pos": [HERO[0], hero_y + 1.2, HERO[1]], "scale": [0.7, 1.8, 0.7]}, Parent={"target": hero}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.7, 1.8, 0.7]},
    MeshRenderer={"mesh": "cylinder", "color": [0.95, 0.5, 0.25, 1], "roughness": 0.5})
add("Hero head", Transform={"pos": [HERO[0], hero_y + 2.25, HERO[1]], "scale": [0.55, 0.55, 0.55]}, Parent={"target": hero}, LocalTransform={"pos": [0, 1.95, 0], "scale": [0.55, 0.55, 0.55]},
    MeshRenderer={"mesh": "sphere", "color": [1.0, 0.85, 0.7, 1], "roughness": 0.45})
add("Camera", Transform={"pos": [HERO[0], hero_y + 4.5, HERO[1] + 8], "rot": [-0.2, 0, 0, 0.98]}, Parent={"target": hero}, LocalTransform={"pos": [0, 4.2, 8], "rot": [-0.2, 0, 0, 0.98]}, Camera={"fov_y": 55, "far": 700, "clear": [0.5, 0.65, 0.85, 1]})
add("Hint", Text={"value": "WASD walk   Space jump   wade into the lake", "pos": [0.5, 0.04], "size": 0.04, "align": 1, "color": [1, 1, 1, 1], "shadow": True})

doc = {"lunaris": 1, "entities": entities, "systems": [
    {"name": "walk_input", "doc": "Keys into the character controller: A/D and W/S set the walking speed (6 m/s), Space asks for a jump (6.5 m/s up) when on the ground.",
     "each": ["CharacterBody"], "do": [
         {"set": "CharacterBody.walk[0]", "to": {"mul": [{"sub": [{"res": "input.d"}, {"res": "input.a"}]}, 6]}},
         {"set": "CharacterBody.walk[1]", "to": {"mul": [{"sub": [{"res": "input.s"}, {"res": "input.w"}]}, 6]}},
         {"if": {"gt": [{"res": "input.space"}, 0.5]}, "then": [{"set": "CharacterBody.jump", "to": 6.5}]},
     ]},
    {"name": "wade", "doc": "The engine puts InWater on a character whose feet are under the water and takes it off when it climbs out: only those are visited here. The deeper the water, the slower the walk (down to 30% at 1.5 m).",
     "each": ["CharacterBody", "InWater"], "order": 150, "do": [
         {"let": "k", "be": {"sub": [1.0, {"mul": [0.7, {"clamp": [{"div": [{"get": "InWater.depth"}, 1.5]}, 0, 1]}]}]}},
         {"set": "CharacterBody.walk[0]", "to": {"mul": [{"get": "CharacterBody.walk[0]"}, {"var": "k"}]}},
         {"set": "CharacterBody.walk[1]", "to": {"mul": [{"get": "CharacterBody.walk[1]"}, {"var": "k"}]}},
         {"res_set": "hero.wet", "to": 1},
     ]},
    {"name": "dry", "doc": "The resource hero.wet is 1 while the hero is in the water (written by `wade`) and 0 otherwise.",
     "each": ["CharacterBody"], "order": 140, "do": [{"res_set": "hero.wet", "to": 0}]},
]}
path = os.path.join(ROOT, "examples", "terrain_layers.lunaris.json")
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(doc, f, indent=1)
    f.write("\n")
print(f"wrote {path}: {len(entities)} entities ({len(trees)} pines, {len(rocks)} boulders), {wet} wet samples, river {LENGTH:.0f} m long, hero is uid {hero}, ground under it {hero_y:.2f} m")


# ---- the playtest: what the numbers of the pictures say the world should do
def cap_fraction(h, r):
    """The share of a ball's volume under the water when its bottom is h below the surface (a spherical cap)."""
    return h * h * (3 * r - h) / (4 * r * r * r)


def sink_depth(density, size, ball):
    """How far the bottom of a body of this density and size is under the surface when it floats."""
    share = density / 1000.0
    if not ball:
        return share * size
    lo, hi, r = 0.0, size, size / 2
    for _ in range(60):
        mid = (lo + hi) / 2
        lo, hi = (mid, hi) if cap_fraction(mid, r) < share else (lo, mid)
    return (lo + hi) / 2


# where the hero first gets its feet wet walking west along z = HERO.z: the ground falls under the lake's level
shore_x = HERO[0]
while ground(shore_x, HERO[1]) >= LAKE_LEVEL and shore_x > -60:
    shore_x -= 0.01
peak = max(((ground_m[j][i], (i - (N - 1) / 2) * CELL, (j - (N - 1) / 2) * CELL) for j in range(N) for i in range(N)), key=lambda t: t[0])
steps = [
    {"expect": {"check": "errors", "eq": 0}},
    {"expect": {"count": {"with": ["InWater"]}, "eq": 0, "message": "nobody is in the water yet"}},
    {"step": 480},
    {"expect": {"name": "Hero", "field": "CharacterBody.grounded", "eq": True, "message": "the hero stands on the ground"}},
    {"expect": {"name": "Hero", "field": "Transform.pos[1]", "approx": [round(hero_y, 3), 0.05], "message": "with its feet on the grass"}},
    {"expect": {"count": {"with": ["InWater"]}, "eq": 0, "message": "dry on the bank"}},
]
for name, mesh, (x, z), size, density, colour, tex in floaters:
    ball = mesh == "sphere"
    sunk = sink_depth(density, size, ball)
    y = LAKE_LEVEL - sunk + size / 2
    steps.append({"expect": {"name": name, "field": "Transform.pos[1]", "approx": [round(y, 3), 0.12], "message": f"{name} floats: density {density} of 1000, so {sunk:.2f} m of it is under the surface of the lake at {LAKE_LEVEL} m"}})
steps += [
    {"call": {"tool": "water_depth", "args": {"x": LAKE_AT[0], "z": LAKE_AT[1]}}},
    {"call": {"tool": "terrain_layer", "args": {"x": HERO[0], "z": HERO[1]}}},
    {"note": "west into the lake"},
    {"input": {"a": True}},
    {"until": {"count": {"with": ["InWater"]}, "eq": 1, "max_ticks": 900}},
    {"expect": {"name": "Hero", "field": "Transform.pos[0]", "approx": [round(shore_x, 2), 0.6], "message": "the water closed over its feet where the ground falls under the lake's surface"}},
    {"step": 2},
    {"expect": {"resource": "hero.wet", "eq": 1, "message": "the wade system saw it"}},
    {"expect": {"name": "Hero", "field": "InWater.surface", "approx": [LAKE_LEVEL, 0.02]}},
    {"expect": {"name": "Hero", "field": "InWater.depth", "between": [0.0, 0.5], "message": "just wet"}},
    {"step": 120},
    {"expect": {"name": "Hero", "field": "InWater.depth", "between": [0.5, 3.0], "message": "deeper in"}},
    {"expect": {"name": "Hero", "field": "Transform.pos[0]", "between": [round(shore_x - 9.0, 2), round(shore_x - 2.0, 2)], "message": "wading slowed it: two seconds at 6 m/s would be 12 m"}},
    {"note": "and back to the bank"},
    {"input": {"a": False, "d": True}},
    {"until": {"count": {"with": ["InWater"]}, "eq": 0, "max_ticks": 1500}},
    {"step": 2},
    {"expect": {"resource": "hero.wet", "eq": 0, "message": "the hero is out of the water"}},
    {"expect": {"name": "Hero", "field": "CharacterBody.grounded", "eq": True}},
    {"expect": {"name": "Hero", "field": "Transform.pos[0]", "between": [round(shore_x - 2.0, 2), round(shore_x + 2.0, 2)], "message": "out where it went in"}},
]
test = {"name": "Terrain layers and water: crates and a ball float on the lake at the depth their density says, the hero wades in (InWater appears with its depth, and the wade system slows it) and out again", "scene": "examples/terrain_layers.lunaris.json",
        "seed": 1, "replay_check": True, "steps": steps}
tpath = os.path.join(ROOT, "examples", "tests", "terrain_layers.test.json")
with open(tpath, "w", encoding="utf-8", newline="\n") as f:
    json.dump(test, f, indent=2)
    f.write("\n")
print(f"wrote {tpath}: the hero gets wet at x = {shore_x:.2f} (z = {HERO[1]}); the highest ground is {peak[0]:.1f} m at ({peak[1]:.0f}, {peak[2]:.0f}); the lake's middle is at {LAKE_AT}")
