"""Draws the pixel art of the "Depths" example (examples/depths/*.png) with nothing but the standard library: no copyrighted art, every picture is arithmetic.

  tiles_dungeon.png / tiles_cave.png / tiles_maze.png   19 x 1 sheets of 16 x 16 tiles: 0-2 three floors, 3-18 the 16 wall pieces of `autotile` mode "4"
                                                         (`base + N + 2 E + 4 S + 8 W`, each letter 1 when the neighbour on that side is wall). A wall whose south side
                                                         looks at the floor shows its brick face, the other exposed sides get a light rim.
  hero.png      6 frames (idle 0-1, walk 2-5)           grunt.png, skeleton.png: 4 walk frames           bat.png: 2 wing frames
  items.png     8 frames: coin spin 0-3, key 4, heart 5, locked door 6, stairs 7
  props.png     8 frames: wall torch 0, barrel 1, crate 2, bones 3, skull 4, pot 5, slash arcs 6-7 (16 x 16 each; the swing is a sprite that turns)

    python -I tools/make_depths_art.py
"""
import math
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "depths")
T = 16


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


CLEAR = (0, 0, 0, 0)


def canvas():
    return [[CLEAR] * T for _ in range(T)]


def put(cv, x, y, c):
    if 0 <= x < T and 0 <= y < T:
        cv[y][x] = c if len(c) == 4 else tuple(c) + (255,)


def rect(cv, x, y, w, h, c):
    for j in range(h):
        for i in range(w):
            put(cv, x + i, y + j, c)


def outline(cv, c=(18, 14, 24, 255)):
    """A one pixel dark edge round every drawn pixel, so a figure reads on any floor."""
    add = []
    for y in range(T):
        for x in range(T):
            if cv[y][x][3] == 0:
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < T and 0 <= ny < T and cv[ny][nx][3] != 0 and cv[ny][nx] != c:
                        add.append((x, y))
                        break
    for x, y in add:
        cv[y][x] = c
    return cv


def sheet(frames, path):
    rows = [[px for f in frames for px in f[y]] for y in range(T)]
    write_png(path, T * len(frames), T, rows)


# ------------------------------------------------------------------------------------------------ tiles
THEMES = {
    # floor, floor accent (joints), wall cap, wall face, brick joint, light rim
    "dungeon": dict(floor=(70, 66, 92), joint=-9, cap=(40, 36, 58), face=(30, 26, 44), mortar=(58, 50, 80), rim=(120, 106, 164), style="bricks"),
    "cave": dict(floor=(86, 68, 52), joint=0, cap=(52, 40, 34), face=(38, 28, 24), mortar=(66, 52, 44), rim=(140, 112, 84), style="rock"),
    "maze": dict(floor=(74, 82, 62), joint=-5, cap=(34, 70, 44), face=(24, 52, 34), mortar=(44, 88, 56), rim=(120, 176, 96), style="hedge"),
}


def floor_tile(th, variant):
    cv = canvas()
    for y in range(T):
        for x in range(T):
            d = (noise(x, y, 3 + variant) - 128) // 22
            if th["style"] == "bricks" and (x % 8 == 0 or y % 8 == 0):
                d += th["joint"]
            put(cv, x, y, shade(th["floor"], d))
    if th["style"] == "bricks":
        if variant == 1:  # a crack
            for i in range(6):
                put(cv, 3 + i, 9 + i // 2, shade(th["floor"], -22))
        if variant == 2:  # a worn flagstone
            rect(cv, 9, 2, 4, 3, shade(th["floor"], 8))
    elif th["style"] == "rock":
        for k in range(3 + variant * 2):  # pebbles
            px, py = noise(k, variant, 5) % 14 + 1, noise(variant, k, 9) % 14 + 1
            put(cv, px, py, shade(th["floor"], 22))
            put(cv, px + 1, py + 1, shade(th["floor"], -24))
    else:
        for k in range(5 + variant * 3):  # tufts of grass on the path
            px, py = noise(k, variant, 7) % 14 + 1, noise(variant, k, 13) % 14 + 1
            put(cv, px, py, (96, 140, 70, 255))
            put(cv, px, py - 1, (120, 168, 84, 255))
    return cv


def wall_tile(th, mask):
    north, east, south, west = mask & 1, mask >> 1 & 1, mask >> 2 & 1, mask >> 3 & 1
    cv = canvas()
    for y in range(T):
        for x in range(T):
            d = (noise(x, y, 11) - 128) // 26
            c = shade(th["cap"], d)
            if th["style"] == "hedge":
                if noise(x // 2, y // 2, 4) % 5 == 0:
                    c = shade(th["cap"], 22)
            elif th["style"] == "rock" and noise(x // 3, y // 3, 8) % 4 == 0:
                c = shade(th["cap"], -10)
            put(cv, x, y, c)
    # the face of a wall that looks south at the floor: bricks, a little darker
    if not south:
        for y in range(10, T):
            for x in range(T):
                d = (noise(x, y, 17) - 128) // 30
                c = shade(th["face"], d)
                if th["style"] == "bricks":
                    if y % 4 == 2 or (x + (y // 4 % 2) * 4) % 8 == 0:
                        c = th["mortar"] + (255,)
                elif th["style"] == "rock":
                    if (y == 12 and x % 5 != 0) or (x + y) % 9 == 0:
                        c = th["mortar"] + (255,)
                else:
                    if noise(x, y, 21) % 4 == 0:
                        c = shade(th["face"], 14)
                put(cv, x, y, c)
        for x in range(T):  # the lit edge between the cap and the face
            put(cv, x, 9, th["rim"] + (255,))
        for x in range(T):
            put(cv, x, 15, shade(th["face"], -14))
    # a light rim on the other exposed sides
    for exposed, dist_of in ((not north, lambda x, y: y), (not west, lambda x, y: x), (not east, lambda x, y: 15 - x)):
        if exposed:
            for y in range(0, 10 if not south else T):
                for x in range(T):
                    dist = dist_of(x, y)
                    if dist == 0:
                        put(cv, x, y, th["rim"] + (255,))
                    elif dist == 1:
                        put(cv, x, y, shade(th["rim"], -64))
    return cv


def make_tiles():
    for name, th in THEMES.items():
        tiles = [floor_tile(th, v) for v in range(3)] + [wall_tile(th, m) for m in range(16)]
        sheet(tiles, os.path.join(OUT, f"tiles_{name}.png"))


# ------------------------------------------------------------------------------------------------ characters
SKIN = (240, 200, 160, 255)
STEEL = (170, 180, 196, 255)
STEEL_D = (108, 116, 136, 255)
BLUE = (52, 96, 200, 255)
BLUE_D = (34, 62, 142, 255)
BROWN = (120, 78, 44, 255)
RED = (214, 52, 56, 255)


def hero_frame(frame):
    """frame 0-1 idle (a breath), 2-5 walk. The figure faces the viewer; it is flipped by the game (negative x scale) to face left."""
    cv = canvas()
    walking = frame >= 2
    step = (frame - 2) % 4 if walking else 0
    bob = 1 if (walking and step in (1, 3)) or (not walking and frame == 1) else 0
    ly = {0: (0, 0), 1: (-1, 1), 2: (0, 0), 3: (1, -1)}[step] if walking else (0, 0)
    # legs and boots
    rect(cv, 5, 12 + ly[0] + bob, 3, 2, BLUE_D)
    rect(cv, 8, 12 + ly[1] + bob, 3, 2, BLUE_D)
    rect(cv, 5, 14 + ly[0] + bob - (1 if ly[0] < 0 else 0), 3, 1, BROWN)
    rect(cv, 8, 14 + ly[1] + bob - (1 if ly[1] < 0 else 0), 3, 1, BROWN)
    # tunic, belt, arms
    rect(cv, 4, 7 + bob, 8, 5, BLUE)
    rect(cv, 4, 11 + bob, 8, 1, BROWN)
    rect(cv, 7, 11 + bob, 2, 1, (230, 200, 90, 255))
    rect(cv, 3, 8 + bob, 1, 3, STEEL_D)
    rect(cv, 12, 8 + bob, 1, 3, STEEL_D)
    rect(cv, 3, 11 + bob, 1, 1, SKIN)
    rect(cv, 12, 11 + bob, 1, 1, SKIN)
    # helmet, visor, plume
    rect(cv, 5, 2 + bob, 6, 5, STEEL)
    rect(cv, 5, 2 + bob, 6, 1, (206, 214, 228, 255))
    rect(cv, 6, 4 + bob, 4, 1, (26, 22, 38, 255))
    rect(cv, 6, 5 + bob, 4, 1, SKIN)
    rect(cv, 7, 1 + bob, 2, 1, RED)
    rect(cv, 7, 0 + bob, 1, 1, RED)
    # the sword in the right hand
    rect(cv, 13, 4 + bob, 1, 6, (226, 232, 244, 255))
    rect(cv, 12, 10 + bob, 3, 1, (230, 200, 90, 255))
    return outline(cv)


def grunt_frame(frame):
    cv = canvas()
    bob = frame % 2
    ly = [(0, 0), (-1, 1), (0, 0), (1, -1)][frame]
    G, GD = (92, 168, 70, 255), (56, 112, 48, 255)
    rect(cv, 5, 12 + bob + ly[0], 2, 2, GD)
    rect(cv, 9, 12 + bob + ly[1], 2, 2, GD)
    rect(cv, 4, 7 + bob, 8, 5, (176, 62, 44, 255))   # red tunic
    rect(cv, 4, 11 + bob, 8, 1, BROWN)
    rect(cv, 2, 8 + bob, 2, 3, G)
    rect(cv, 12, 8 + bob, 2, 3, G)
    rect(cv, 13, 5 + bob, 1, 4, BROWN)  # club
    rect(cv, 12, 4 + bob, 3, 2, (150, 104, 60, 255))
    rect(cv, 4, 2 + bob, 8, 5, G)
    rect(cv, 2, 3 + bob, 2, 2, G)      # ears
    rect(cv, 12, 3 + bob, 2, 2, G)
    rect(cv, 5, 3 + bob, 2, 2, (250, 232, 80, 255))  # yellow eyes
    rect(cv, 9, 3 + bob, 2, 2, (250, 232, 80, 255))
    put(cv, 6, 4 + bob, (30, 20, 20, 255))
    put(cv, 10, 4 + bob, (30, 20, 20, 255))
    rect(cv, 6, 6 + bob, 4, 1, (240, 240, 230, 255))  # fangs
    return outline(cv)


def skeleton_frame(frame):
    cv = canvas()
    bob = frame % 2
    ly = [(0, 0), (-1, 1), (0, 0), (1, -1)][frame]
    B, BD = (232, 228, 214, 255), (170, 164, 150, 255)
    rect(cv, 5, 11 + bob + ly[0], 2, 3, B)
    rect(cv, 9, 11 + bob + ly[1], 2, 3, B)
    rect(cv, 5, 7 + bob, 6, 4, BD)  # ribs
    for y in (7, 9):
        rect(cv, 4, y + bob, 8, 1, B)
    rect(cv, 7, 7 + bob, 2, 5, B)
    rect(cv, 3, 7 + bob, 1, 4, B)
    rect(cv, 12, 7 + bob, 1, 4, B)
    rect(cv, 13, 3 + bob, 1, 8, (150, 150, 170, 255))  # rusty blade
    rect(cv, 12, 10 + bob, 3, 1, BROWN)
    rect(cv, 4, 1 + bob, 8, 6, B)   # skull
    rect(cv, 5, 3 + bob, 2, 2, (30, 20, 30, 255))
    rect(cv, 9, 3 + bob, 2, 2, (30, 20, 30, 255))
    put(cv, 6, 4 + bob, (255, 70, 60, 255))
    put(cv, 10, 4 + bob, (255, 70, 60, 255))
    rect(cv, 5, 6 + bob, 6, 1, BD)
    for x in (6, 8, 10):
        put(cv, x, 6 + bob, (40, 30, 40, 255))
    return outline(cv)


def bat_frame(frame):
    cv = canvas()
    P, PD = (120, 70, 150, 255), (74, 40, 100, 255)
    up = frame == 0
    rect(cv, 6, 6, 4, 5, PD)     # body
    rect(cv, 6, 4, 4, 3, P)      # head
    put(cv, 6, 3, PD)
    put(cv, 9, 3, PD)
    put(cv, 7, 5, (255, 80, 70, 255))
    put(cv, 9, 5, (255, 80, 70, 255))
    wy = 2 if up else 7
    for i in range(5):  # wings: a fan of dark triangles either side
        h = 5 - i if up else 2 + i // 2
        rect(cv, 5 - i, wy + (i if up else 0), 1, h, P if i % 2 == 0 else PD)
        rect(cv, 10 + i, wy + (i if up else 0), 1, h, P if i % 2 == 0 else PD)
    rect(cv, 7, 7, 2, 1, (255, 255, 255, 255))  # a fang
    return outline(cv)


# ------------------------------------------------------------------------------------------------ items and props
GOLD, GOLD_D, GOLD_L = (252, 208, 56, 255), (200, 144, 20, 255), (255, 244, 170, 255)


def coin_frame(frame):
    cv = canvas()
    w = [8, 6, 2, 6][frame]
    x0 = 8 - w // 2
    for y in range(4, 12):
        r = 1 if y in (4, 11) else 0
        rect(cv, x0 + r, y, w - 2 * r, 1, GOLD)
    rect(cv, x0, 5, w, 6, GOLD)
    if w >= 6:
        rect(cv, x0 + 1, 5, w - 2, 1, GOLD_L)
        rect(cv, x0 + 1, 10, w - 2, 1, GOLD_D)
        rect(cv, 7, 6, 2, 4, GOLD_D)
    return outline(cv)


def key_frame():
    cv = canvas()
    rect(cv, 3, 3, 5, 5, GOLD)   # bow
    rect(cv, 4, 4, 3, 3, (0, 0, 0, 0))
    rect(cv, 7, 5, 6, 2, GOLD)   # shaft
    rect(cv, 11, 7, 2, 3, GOLD)  # teeth
    rect(cv, 9, 7, 1, 2, GOLD_D)
    rect(cv, 3, 3, 5, 1, GOLD_L)
    put(cv, 10, 5, GOLD_L)
    return outline(cv)


def heart_frame():
    cv = canvas()
    R, RD, RL = (230, 50, 70, 255), (160, 24, 44, 255), (255, 150, 160, 255)
    pat = ["..XX..XX..", ".XXXXXXXX.", "XXXXXXXXXX", "XXXXXXXXXX", ".XXXXXXXX.", "..XXXXXX..", "...XXXX...", "....XX...."]
    for y, row in enumerate(pat):
        for x, ch in enumerate(row):
            if ch == "X":
                put(cv, 3 + x, 4 + y, R if (x + y) % 5 else RD)
    put(cv, 5, 5, RL)
    put(cv, 6, 5, RL)
    return outline(cv)


def door_frame():
    cv = canvas()
    for y in range(T):
        for x in range(T):
            d = (noise(x, y, 31) - 128) // 30
            c = shade((126, 82, 46), d)
            if x % 4 == 3:
                c = shade((92, 58, 30), d)
            put(cv, x, y, c)
    for y in (3, 11):  # iron bands
        rect(cv, 0, y, T, 2, (84, 90, 104, 255))
        rect(cv, 0, y, T, 1, (130, 138, 156, 255))
    rect(cv, 6, 6, 4, 5, (84, 90, 104, 255))   # lock plate
    rect(cv, 7, 7, 2, 2, GOLD)
    rect(cv, 7, 9, 2, 1, (24, 20, 28, 255))
    for x in (0, 15):
        rect(cv, x, 0, 1, T, (40, 26, 16, 255))
    return cv


def stairs_frame():
    cv = canvas()
    for i in range(6):
        c = shade((80, 76, 100), -i * 11)
        rect(cv, 2 + i, 2 + i * 2, 12 - 2 * i, 2, c)
        rect(cv, 2 + i, 4 + i * 2, 12 - 2 * i, 1, shade(c, -26))
    rect(cv, 1, 1, 14, 1, (150, 140, 190, 255))
    rect(cv, 1, 1, 1, 14, (150, 140, 190, 255))
    rect(cv, 14, 1, 1, 14, (150, 140, 190, 255))
    return cv


def torch_frame():
    cv = canvas()
    rect(cv, 7, 7, 2, 7, BROWN)
    rect(cv, 6, 6, 4, 2, (84, 90, 104, 255))
    rect(cv, 5, 5, 1, 2, (84, 90, 104, 255))
    rect(cv, 10, 5, 1, 2, (84, 90, 104, 255))
    rect(cv, 7, 13, 2, 1, (60, 40, 24, 255))
    return outline(cv)


def barrel_frame():
    cv = canvas()
    for y in range(3, 14):
        w = 10 - (1 if y in (3, 13) else 0)
        rect(cv, 8 - w // 2, y, w, 1, shade((136, 88, 48), (noise(y, 2, 3) - 128) // 30))
    for y in (5, 11):
        rect(cv, 3, y, 10, 1, (84, 90, 104, 255))
    rect(cv, 4, 3, 8, 1, (176, 124, 72, 255))
    return outline(cv)


def crate_frame():
    cv = canvas()
    rect(cv, 2, 2, 12, 12, (150, 104, 60, 255))
    rect(cv, 2, 2, 12, 1, (190, 140, 84, 255))
    for x in (2, 13):
        rect(cv, x, 2, 1, 12, (96, 62, 34, 255))
    for i in range(10):
        put(cv, 3 + i, 3 + i, (110, 72, 40, 255))
        put(cv, 12 - i, 3 + i, (110, 72, 40, 255))
    return outline(cv)


def bones_frame():
    cv = canvas()
    B = (220, 214, 196, 255)
    rect(cv, 3, 9, 8, 1, B)
    rect(cv, 2, 8, 2, 3, B)
    rect(cv, 10, 8, 2, 3, B)
    rect(cv, 6, 5, 6, 1, B)
    rect(cv, 5, 4, 2, 3, B)
    rect(cv, 11, 4, 2, 3, B)
    rect(cv, 4, 12, 7, 1, (180, 174, 160, 255))
    return outline(cv)


def skull_frame():
    cv = canvas()
    B, BD = (232, 228, 214, 255), (150, 146, 134, 255)
    rect(cv, 4, 4, 8, 6, B)
    rect(cv, 5, 10, 6, 2, BD)
    rect(cv, 5, 6, 2, 2, (30, 20, 30, 255))
    rect(cv, 9, 6, 2, 2, (30, 20, 30, 255))
    put(cv, 8, 9, (30, 20, 30, 255))
    return outline(cv)


def pot_frame():
    cv = canvas()
    for y in range(4, 14):
        w = [4, 6, 8, 10, 10, 10, 8, 8, 6, 6][y - 4]
        rect(cv, 8 - w // 2, y, w, 1, shade((170, 90, 60), (noise(y, 4, 5) - 128) // 30))
    rect(cv, 5, 4, 6, 1, (210, 130, 90, 255))
    rect(cv, 4, 7, 8, 1, (110, 50, 34, 255))
    return outline(cv)


def slash_frame(step):
    """A crescent of light: 0 a thin arc, 1 a wide one. It is drawn facing right and turned by the game."""
    cv = canvas()
    import math
    for y in range(T):
        for x in range(T):
            dx, dy = x - 3.5, y - 7.5
            r = math.hypot(dx, dy)
            a = math.degrees(math.atan2(dy, dx))
            if (9.5 - step * 1.5) <= r <= (11.5 if step else 10.8) and -62 <= a <= 62:
                t = 1 - abs(a) / 62
                put(cv, x, y, (255, 255, 255, 255) if t > 0.55 else (190, 230, 255, 255) if t > 0.2 else (120, 180, 255, 255))
    return cv


def npc_frame(frame):
    """The old warden: a hooded robe, a white beard, a staff with a glowing tip (two frames: the glow breathes)."""
    cv = canvas()
    bob = frame
    ROBE, ROBE_D, HOOD = (96, 84, 150, 255), (64, 54, 110, 255), (120, 108, 176, 255)
    rect(cv, 4, 7 + bob, 8, 7 - bob, ROBE)
    rect(cv, 4, 12, 8, 2, ROBE_D)
    rect(cv, 3, 8 + bob, 1, 4, ROBE_D)
    rect(cv, 12, 8 + bob, 1, 4, ROBE_D)
    rect(cv, 5, 1 + bob, 6, 6, HOOD)            # the hood
    rect(cv, 6, 3 + bob, 4, 3, SKIN)
    rect(cv, 6, 5 + bob, 4, 4, (236, 236, 244, 255))   # the beard
    put(cv, 6, 3 + bob, (30, 20, 30, 255))
    put(cv, 9, 3 + bob, (30, 20, 30, 255))
    rect(cv, 13, 3, 1, 11, BROWN)               # the staff
    rect(cv, 12, 1 + frame, 3, 2, (150, 230, 255, 255) if frame else (110, 190, 240, 255))
    return outline(cv)


def make_sprites():
    sheet([npc_frame(i) for i in range(2)], os.path.join(OUT, "npc.png"))
    sheet([hero_frame(i) for i in range(6)], os.path.join(OUT, "hero.png"))
    sheet([grunt_frame(i) for i in range(4)], os.path.join(OUT, "grunt.png"))
    sheet([skeleton_frame(i) for i in range(4)], os.path.join(OUT, "skeleton.png"))
    sheet([bat_frame(i) for i in range(2)], os.path.join(OUT, "bat.png"))
    sheet([coin_frame(i) for i in range(4)] + [key_frame(), heart_frame(), door_frame(), stairs_frame()], os.path.join(OUT, "items.png"))
    sheet([torch_frame(), barrel_frame(), crate_frame(), bones_frame(), skull_frame(), pot_frame(), slash_frame(0), slash_frame(1)], os.path.join(OUT, "props.png"))


def make_lantern():
    """lantern.png: a 256 x 256 mask, clear in the middle and dark towards the edge, that the game lays over the level round the hero (the dungeon is dark beyond the lantern)."""
    n = 256
    rows = []
    for y in range(n):
        row = []
        for x in range(n):
            r = math.hypot((x + 0.5) / n * 2 - 1, (y + 0.5) / n * 2 - 1)
            t = min(1.0, max(0.0, (r - 0.14) / (0.40 - 0.14)))
            t = t * t * (3 - 2 * t)
            row.append((5, 3, 11, int(round(t * 0.93 * 255))))
        rows.append(row)
    write_png(os.path.join(OUT, "lantern.png"), n, n, rows)


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    make_tiles()
    make_sprites()
    make_lantern()
    print("wrote", ", ".join(sorted(f for f in os.listdir(OUT) if f.endswith(".png"))))
