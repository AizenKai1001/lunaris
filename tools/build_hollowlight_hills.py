"""Hollowlight, the Fields: a very large, empty, smooth landscape of lone hills under a pale sky with the wind going through the grass (examples/hollowlight/hills.lunaris.json), built only by calling the engine's tools.

A 4 km square of smooth hills (a heightmap made here with only the standard library) that rise into a ring of low mountains so the edge is never seen, painted with four CC0 ground textures by a splat map, with tufts of grass
(a mesh of three crossed cards and a grass picture, both made here) scattered round the player, each swaying with `Sway` and given an offset by where it stands so gusts travel across the field. The player is the body camera of
`tools/hollowlight_rig.py`.

    python3 tools/build_hollowlight_hills.py
"""
import json
import math
import os
import random
import struct
import subprocess
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import hollowlight_rig as R  # noqa: E402

BIN = os.environ.get("LUNARIS_BIN", "target/release/lunaris")
D = "examples/hollowlight"
H = f"{D}/hills"
T = f"{D}/textures"
OUT = sys.argv[1] if len(sys.argv) > 1 else f"{D}/hills.lunaris.json"
SHOTS = sys.argv[2] if len(sys.argv) > 2 else "hills"
SIZE, RELIEF, N = 4000.0, 140.0, 1025
os.makedirs(H, exist_ok=True)


# ---------------------------------------------------------------------------------------------- pictures made here
def png_chunk(tag, data):
    c = struct.pack(">I", len(data)) + tag + data
    return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


def write_png(path, w, h, depth, color, rows):
    raw = b"".join(b"\x00" + r for r in rows)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + png_chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, color, 0, 0, 0)) + png_chunk(b"IDAT", zlib.compress(raw, 6)) + png_chunk(b"IEND", b""))


def smoothstep(a, b, x):
    t = max(0.0, min(1.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


rng = random.Random(7)
HILLS = []
while len(HILLS) < 11:
    cx, cz = rng.uniform(-1500, 1500), rng.uniform(-1500, 1500)
    if math.hypot(cx, cz) < 420:  # keep the start clear
        continue
    HILLS.append((cx, cz, rng.uniform(260, 620), rng.uniform(0.22, 0.62)))


def height(x, z):
    """0 to 1: broad hills, a gentle swell, and a rising rim far out so the edge is a wall of low mountains."""
    h = 0.07 + 0.04 * math.sin(x / 830.0 + 0.7) * math.cos(z / 610.0) + 0.03 * math.sin((x + z) / 420.0)
    for cx, cz, sg, amp in HILLS:
        d2 = ((x - cx) ** 2 + (z - cz) ** 2) / (sg * sg)
        if d2 < 20:
            h += amp * math.exp(-d2 * 0.5)
    r = math.hypot(x, z) / (SIZE / 2)
    h += 0.7 * smoothstep(0.62, 1.05, r) ** 1.6
    near = math.hypot(x, z)
    if near < 300:  # a flat place to start, blending into the land
        k = smoothstep(60, 300, near)
        h = 0.075 * (1 - k) + h * k
    return max(0.0, min(1.0, h))


def make_ground():
    rows = []
    for j in range(N):
        z = (j / (N - 1) - 0.5) * SIZE
        row = bytearray()
        for i in range(N):
            x = (i / (N - 1) - 0.5) * SIZE
            row += struct.pack(">H", int(height(x, z) * 65535))
        rows.append(bytes(row))
    write_png(f"{H}/ground.png", N, N, 16, 0, rows)


def make_splat(M=512):
    """Four layers by red, green, blue, alpha: grass_ground, sparse_grass, withered_grass, rocky moss. Which one is where comes from height, slope and slow noise."""
    rows = []
    for j in range(M):
        z = (j / (M - 1) - 0.5) * SIZE
        row = bytearray()
        for i in range(M):
            x = (i / (M - 1) - 0.5) * SIZE
            hh = height(x, z)
            dx = (height(x + 20, z) - height(x - 20, z)) * RELIEF / 40.0
            dz = (height(x, z + 20) - height(x, z - 20)) * RELIEF / 40.0
            slope = math.hypot(dx, dz)
            n1 = 0.5 + 0.5 * math.sin(x / 190.0 + math.sin(z / 260.0) * 2.0)
            n2 = 0.5 + 0.5 * math.sin(z / 150.0 - x / 330.0 + 1.3)
            rock = smoothstep(0.35, 0.8, slope) + 0.4 * smoothstep(0.75, 1.0, hh)
            dry = smoothstep(0.55, 0.92, n1) * smoothstep(0.22, 0.6, hh) * 0.8
            sparse = smoothstep(0.35, 0.8, n2) * 0.55 * (1 - dry)
            w = [max(0.3, 1.0 - dry - sparse), sparse, dry, rock * 0.8]
            tot = sum(w)
            row += bytes(int(255 * c / tot + 0.5) for c in w)
        rows.append(bytes(row))
    write_png(f"{H}/splat.png", M, M, 8, 6, rows)


def make_grass_picture(S=256):
    """A tuft of blades with alpha: dark green at the foot, pale gold at the tip, each blade a leaning taper."""
    r2 = random.Random(5)
    blades = []
    for _ in range(42):
        bx = max(0.1, min(0.9, r2.gauss(0.5, 0.16)))
        ht = r2.uniform(0.5, 1.0) * (1.05 - 1.1 * abs(bx - 0.5))
        lean = r2.uniform(-0.28, 0.28)
        curve = r2.uniform(-0.22, 0.22)
        wd = r2.uniform(0.012, 0.022)
        tint = r2.uniform(0.0, 1.0)
        blades.append((bx, ht, lean, curve, wd, tint))
    cover = [[(0.0, 0, 0, 0)] * S for _ in range(S)]
    for y in range(S):
        v = 1.0 - (y + 0.5) / S  # 0 at the foot (bottom row), 1 at the top
        for bx, ht, lean, curve, wd, tint in blades:
            if v > ht:
                continue
            t = v / ht
            cx = bx + lean * t + curve * t * t
            half = wd * (1.0 - t) ** 0.7 + 0.0025
            x0, x1 = int((cx - half) * S - 1), int((cx + half) * S + 1)
            for x in range(max(0, x0), min(S - 1, x1) + 1):
                d = abs((x + 0.5) / S - cx)
                a = max(0.0, min(1.0, (half - d) * S + 0.5))
                if a > cover[y][x][0]:
                    g = 0.28 + 0.5 * t + 0.1 * tint
                    cover[y][x] = (a, int(60 + 70 * t + 25 * tint), int(g * 190 + 20), int(26 + 30 * t))
    rows = []
    for y in range(S):
        row = bytearray()
        for x in range(S):
            a, r, g, b = cover[y][x]
            row += bytes((r, g, b, int(a * 255)))
        rows.append(bytes(row))
    write_png(f"{H}/grass.png", S, S, 8, 6, rows)


def make_grass_mesh():
    """Three cards crossed at 60 degrees, 0.9 m wide and 0.7 m tall, both faces, normals up (so the shade matches the ground), glTF uv (v runs down the picture)."""
    pos, nrm, uvs, idx = [], [], [], []
    w, h = 0.38, 0.62
    for k in range(3):
        a = k * math.pi / 3
        dx, dz = math.cos(a) * w, math.sin(a) * w
        base = len(pos)
        for (px, py, u, v) in ((-dx, 0.0, 0.0, 1.0), (dx, 0.0, 1.0, 1.0), (dx, h, 1.0, 0.0), (-dx, h, 0.0, 0.0)):  # glTF's v runs down the picture
            pos.append((px, py, -dz if px < 0 else dz))
            nrm.append((0.0, 1.0, 0.0))
            uvs.append((u, v))
        idx += [base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]
    blob = bytearray()

    def add(data):
        off = len(blob)
        blob.extend(data)
        blob.extend(b"\x00" * (-len(blob) % 4))
        return off, len(data)

    po, pl = add(b"".join(struct.pack("<3f", *p) for p in pos))
    no, nl_ = add(b"".join(struct.pack("<3f", *n) for n in nrm))
    uo, ul = add(b"".join(struct.pack("<2f", *u) for u in uvs))
    io, il = add(b"".join(struct.pack("<H", i) for i in idx))
    png = open(f"{H}/grass.png", "rb").read()
    mo, ml = add(png)
    doc = {
        "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}, "indices": 3, "material": 0}]}],
        "materials": [{"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0.0, "roughnessFactor": 0.9}, "alphaMode": "MASK", "alphaCutoff": 0.5, "doubleSided": True}],
        "textures": [{"source": 0, "sampler": 0}], "samplers": [{"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}], "images": [{"bufferView": 4, "mimeType": "image/png"}],
        "buffers": [{"byteLength": len(blob)}],
        "bufferViews": [{"buffer": 0, "byteOffset": po, "byteLength": pl}, {"buffer": 0, "byteOffset": no, "byteLength": nl_}, {"buffer": 0, "byteOffset": uo, "byteLength": ul}, {"buffer": 0, "byteOffset": io, "byteLength": il}, {"buffer": 0, "byteOffset": mo, "byteLength": ml}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": len(pos), "type": "VEC3", "min": [min(p[i] for p in pos) for i in range(3)], "max": [max(p[i] for p in pos) for i in range(3)]},
            {"bufferView": 1, "componentType": 5126, "count": len(nrm), "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": len(uvs), "type": "VEC2"},
            {"bufferView": 3, "componentType": 5123, "count": len(idx), "type": "SCALAR"},
        ],
    }
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    b = bytes(blob)
    total = 12 + 8 + len(js) + 8 + len(b)
    with open(f"{H}/grass.glb", "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, total) + struct.pack("<I4s", len(js), b"JSON") + js + struct.pack("<I4s", len(b), b"BIN\x00") + b)


if not os.path.exists(f"{H}/ground.png"):
    make_ground()
if not os.path.exists(f"{H}/splat.png"):
    make_splat()
if not os.path.exists(f"{H}/grass.png"):
    make_grass_picture()
make_grass_mesh()

# ---------------------------------------------------------------------------------------------- the scene
proc = subprocess.Popen([BIN, "serve", "--root", "."], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
n_req = 0
failed = 0


def call(name, args=None):
    global n_req, failed
    n_req += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n_req, "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + "\n")
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line:
            raise SystemExit(f"the engine stopped during {name}")
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("id") == n_req:
            break
    res = m.get("result") or {}
    if "error" in m or res.get("isError"):
        failed += 1
        print("request", n_req, name, "failed:", json.dumps(m.get("error") or res)[:600])
        return {}
    for c in res.get("content") or []:
        if c.get("type") == "text":
            try:
                return json.loads(c["text"])
            except ValueError:
                return {"text": c["text"]}
    return {}


uid = [100]


def spawn(name, **c):
    u = uid[0]
    uid[0] += 1
    call("spawn", {"uid": u, "components": {"Name": {"value": name}, **c}})
    return u


call("reset")
spawn("Environment", Environment={"sky": True, "sky_top": [0.22, 0.42, 0.78], "sky_horizon": [0.8, 0.86, 0.92], "ground": [0.32, 0.34, 0.27], "clouds": 0.6, "fog": 0.00006, "fog_color": [0.72, 0.8, 0.88],
                                  "bloom": 0.25, "bloom_threshold": 1.0, "ssao": 1.0, "exposure": 0.8, "vignette": 0.5, "shadows": True, "shadow_distance": 450, "saturation": 0.95, "ambient": 0.75})
spawn("Sun", Transform={"rot": [-0.31, 0.35, 0.11, 0.88]}, Light={"kind": 0, "color": [1.0, 0.93, 0.8], "intensity": 2.0})
spawn("Haze", HeightFog={"density": 0.0004, "falloff": 0.04, "height": 20, "color": [0.76, 0.83, 0.9]})
spawn("Picture effects", PostFx={"shafts": 0.35, "contrast": 1.08, "grain": 0.5, "tint": [0.98, 1.0, 1.0]})
spawn("Eyes", AutoExposure={"min": 0.6, "max": 1.6, "speed": 1.2, "target": 0.3})
LAYERS = chr(10).join([f"grass {T}/sparse_grass/sparse_grass_diff.jpg uv_scale=900 tint=0.5,1.0,0.38", f"mixed {T}/grass_ground/grass_ground_diff.jpg uv_scale=700 tint=0.48,0.95,0.34", f"withered {T}/withered_grass/withered_grass_diff.jpg uv_scale=800 tint=0.7,0.85,0.5", f"mossy {T}/aerial_grass_rock/aerial_grass_rock_diff.jpg uv_scale=600 tint=0.5,0.85,0.4"])
ground = spawn("Ground", Transform={"pos": [0, 0, 0]}, Terrain={"heightmap": f"{H}/ground.png", "size": [SIZE, SIZE], "height": RELIEF, "step": 2, "color": [1, 1, 1, 1], "friction": 0.9}, TerrainLayers={"splat": f"{H}/splat.png", "layers": LAYERS})
call("step", {"ticks": 1})
gy = call("terrain_height", {"x": 0.0, "z": 0.0}).get("height", 5.0)
print("ground at the start:", gy)

script = R.install(call, spawn, 0.0, gy + 0.1, 0.0, date="2026-10-17", start_seconds=15 * 3600 + 42 * 60 + 18, camera_name="BODYCAM 07")

# ---- sound: the wind
SND = f"{D}/sounds"
spawn("Wind", Transform={"pos": [0, gy + 2, 0]}, AudioSource={"file": f"{SND}/wind.wav", "looped": True, "playing": True, "spatial": False, "volume": 0.7})

# ---- the grass: a `GrassField`, made on the card round the camera (no entities: a field of a few hundred thousand tufts is one entity and a few numbers)
spawn("Grass", GrassField={"terrain": ground, "mesh": f"{H}/grass.glb", "radius": 320.0, "density": 10.0, "size": [0.6, 1.25], "wind_amount": 0.24, "wind_wavelength": 38.0, "wind_speed": 0.55, "wind_dir": 0.45,
                           "color_a": [0.8, 1.0, 0.62], "color_b": [1.2, 1.08, 0.78], "seed": 3.0})


r = call("define_script", {"source": script})
print("script:", json.dumps(r)[:140])
call("step", {"ticks": 3})
rc = call("check")
print("check:", json.dumps({k: rc.get(k) for k in ("errors", "warnings")}), [p.get("message") for p in rc.get("problems", [])][:3])
for name, eye, target in (("view", [0.0, gy + 1.34, 0.0], [0.0, gy + 6.0, 900.0]), ("high", [0.0, gy + 160.0, -1300.0], [0.0, gy + 10.0, 600.0]), ("grass", [6.0, gy + 0.9, 4.0], [14.0, gy + 0.6, 22.0])):
    rr = call("render", {"quality": "high", "width": 1600, "height": 900, "eye": eye, "target": target, "fov": 92, "path": f"{SHOTS}_{name}.png", "image": False})
    print(name, rr.get("renderer"), rr.get("warnings"))
saved = call("save_scene", {"path": OUT})
print("saved", OUT, saved, "failed requests:", failed)
proc.stdin.close()
proc.wait(timeout=300)
