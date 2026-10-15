"""Builds examples/moon_hopper.lunaris.json (a small 2D platformer) by calling Lunaris tools, like an agent would.

    python -I tools/make_tiles.py          # once: the pixel art
    python -I tools/build_moon_hopper.py

Run, jump (Space / W / Up) and collect every coin. The level is a tile map; its solid tiles also become static box colliders.
"""
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "clients", "python"))
from lunaris import Engine  # noqa: E402

ROOT = os.path.join(HERE, "..")
W, H = 40, 14  # tiles
GRASS, DIRT, STONE, WATER, BRICK, SAND, PLANK, COIN = range(8)
SOLID = {GRASS, DIRT, STONE, BRICK, SAND, PLANK}

grid = [[-1] * W for _ in range(H)]


def put(c, r, t):
    if 0 <= c < W and 0 <= r < H:
        grid[r][c] = t


for c in range(W):  # ground: grass over dirt, with two water gaps
    gap = 14 <= c <= 17 or 28 <= c <= 30
    for r in range(11, H):
        put(c, r, WATER if gap and r >= 12 else (-1 if gap else (GRASS if r == 11 else DIRT)))
for c, r, n in [(8, 8, 4), (20, 7, 4), (24, 5, 3), (33, 8, 5)]:  # floating planks
    for k in range(n):
        put(c + k, r, PLANK)
for k in range(4):  # stone steps and a brick wall at the end
    for h in range(k + 1):
        put(5 + k, 10 - h, STONE)
for r in range(5, 11):
    put(38, r, BRICK)
    put(39, r, BRICK)

coins = [(9.5, 8), (10.5, 8), (21.5, 7), (22.5, 7), (25.5, 5), (35, 8), (15.5, 6.2), (29.5, 6.0), (12.5, 11), (2.5, 11), (18.5, 11), (36.5, 11)]


def world(c, r):  # centre of a tile in world units (the map is centred on the origin, row 0 on top)
    return c - W / 2 + 0.5, H / 2 - r - 0.5


with Engine(root=ROOT) as eng:
    eng.reset(seed=3)
    eng.define_component(name="Hero", doc="The player.", fields=[{"name": "speed", "type": "f32", "default": 5.5}, {"name": "jump", "type": "f32", "default": 7.5}, {"name": "grounded", "type": "f32"}, {"name": "sx", "type": "f32"}, {"name": "sy", "type": "f32"}])
    eng.define_component(name="Ground", doc="A static solid the hero can stand on.", fields=[{"name": "n", "type": "u32"}])
    eng.define_component(name="Coin", doc="A collectable; `y0` is where it bobs around.", fields=[{"name": "y0", "type": "f32"}])
    eng.define_component(name="Banner", doc="The text that appears when every coin is collected.", fields=[{"name": "n", "type": "u32"}])

    def named(name, **components):
        return eng.spawn(components={"Name": {"value": name}, **components})

    sky = [0.07, 0.09, 0.2, 1]
    named("Camera", Transform={"pos": [-(W / 2 - 8.8), 0, 10]}, Camera={"ortho": True, "ortho_height": 10, "clear": sky})
    rng = 12345
    for i in range(40):  # stars
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        x = (rng % 6000) / 100 - 30
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        y = (rng % 1000) / 100 - 4
        named(f"Star {i}", Transform={"pos": [x, y, -3]}, Sprite={"size": [0.08, 0.08], "texture": "circle", "color": [1, 1, 0.85, 0.8]})
    named("Moon", Transform={"pos": [8, 3.2, -4]}, Sprite={"size": [2.2, 2.2], "texture": "circle", "color": [0.9, 0.92, 1, 1]})
    data = "\n".join(" ".join(str(t) for t in row) for row in grid)
    named("Level", Transform={"pos": [0, 0, 0]}, Tilemap={"texture": "examples/tiles/tileset.png", "sheet": [4, 2], "size": [W, H], "tile": 1, "data": data})
    # the solid tiles become static colliders: one box per horizontal run
    runs = 0
    for r in range(H):
        c = 0
        while c < W:
            if grid[r][c] in SOLID:
                start = c
                while c < W and grid[r][c] in SOLID:
                    c += 1
                n = c - start
                x, y = world(start, r)
                named("Solid", Transform={"pos": [x + (n - 1) / 2, y, 0]}, Collider={"shape": 1, "size": [n, 1, 1]}, Ground={})
                runs += 1
            else:
                c += 1
    spawn = [-17, -2.5]
    named("Hero", Transform={"pos": [spawn[0], spawn[1], 0]}, Velocity={}, Hero={"sx": spawn[0], "sy": spawn[1]},
          Collider={"shape": 1, "size": [0.7, 1.0, 1]}, RigidBody={"mass": 1, "restitution": 0, "friction": 0.0, "gravity_scale": 1},
          Sprite={"size": [1, 1], "texture": "examples/tiles/hero.png", "sheet": [4, 1]})
    for i, (cx, cy) in enumerate(coins):
        x, y = cx - W / 2, H / 2 - cy - 1 + 0.5 + 0.0
        y = H / 2 - cy - 0.5
        named(f"Coin {i + 1}", Transform={"pos": [x, y + 0.9, 0]}, Coin={"y0": y + 0.9}, Collider={"shape": 0, "size": [0.6, 0.6, 0.6], "trigger": True},
              Sprite={"size": [0.7, 0.7], "texture": "examples/tiles/tileset.png", "sheet": [4, 2], "frame": COIN})
    named("Score", Text={"value": f"Coins {{score}} / {len(coins)}", "pos": [0.02, 0.03], "size": 0.06})
    named("Hint", Text={"value": "A D or arrows: run   Space W or Up: jump", "pos": [0.98, 0.04], "size": 0.035, "align": 2, "color": [0.8, 0.85, 1, 0.8]})
    named("Win", Banner={}, Text={"value": "ALL COINS!", "pos": [0.5, 0.4], "size": 0.14, "align": 1, "color": [1, 0.85, 0.3, 1], "visible": False})
    named("Sfx jump", Sound={"name": "jump", "wave": 0, "freq": 300, "freq_end": 700, "duration": 0.15, "volume": 0.25})
    named("Sfx coin", Sound={"name": "coin", "wave": 1, "freq": 880, "freq_end": 1320, "duration": 0.12, "volume": 0.22})
    named("Sfx hit", Sound={"name": "hit", "wave": 4, "freq": 600, "freq_end": 60, "duration": 0.3, "volume": 0.3})
    named("Sfx win", Sound={"name": "win", "wave": 3, "freq": 440, "freq_end": 880, "duration": 0.6, "volume": 0.3})
    eng.set_resource(name="score", value=0)

    key = lambda *names: {"max": [{"res": f"input.{n}"} for n in names]}
    dir_expr = {"sub": [key("d", "arrowright"), key("a", "arrowleft")]}
    eng.define_system(name="hero_move", order=100, doc="Keys to motion, jumping, facing, walking animation, falling back to the start.", each=["Hero", "Transform", "Velocity", "Sprite"], do=[
        {"let": "dir", "be": dir_expr},
        {"let": "was_grounded", "be": {"gt": [{"get": "Hero.grounded"}, 0.5]}},
        {"set": "Velocity.lin[0]", "to": {"mul": [{"var": "dir"}, {"get": "Hero.speed"}]}},
        {"if": {"and": [{"var": "was_grounded"}, {"gt": [key("space", "w", "arrowup"), 0.5]}]}, "then": [
            {"set": "Velocity.lin[1]", "to": {"get": "Hero.jump"}}, {"emit": "jump"}]},
        {"if": {"and": [{"gt": [{"get": "Velocity.lin[1]"}, 0.5]}, {"lt": [key("space", "w", "arrowup"), 0.5]}]}, "then": [
            {"set": "Velocity.lin[1]", "to": {"mul": [{"get": "Velocity.lin[1]"}, 0.55]}}]},  # let go early: a shorter hop
        {"set": "Hero.grounded", "to": 0},
        {"set": "Transform.scale[0]", "to": {"select": [{"gt": [{"var": "dir"}, 0.1]}, 1, {"select": [{"lt": [{"var": "dir"}, -0.1]}, -1, {"get": "Transform.scale[0]"}]}]}},
        {"set": "Sprite.frame", "to": {"select": [{"and": [{"var": "was_grounded"}, {"gt": [{"abs": {"var": "dir"}}, 0.5]}]}, {"mod": [{"floor": {"mul": [{"tick": True}, 0.17]}}, 4]}, 0]}},
        {"if": {"lt": [{"get": "Transform.pos[1]"}, -8]}, "then": [
            {"set": "Transform.pos[0]", "to": {"get": "Hero.sx"}}, {"set": "Transform.pos[1]", "to": {"get": "Hero.sy"}},
            {"set": "Velocity.lin[0]", "to": 0}, {"set": "Velocity.lin[1]", "to": 0}, {"emit": "hit"}]},
        {"res_set": "player.x", "to": {"get": "Transform.pos[0]"}},
        {"res_set": "player.y", "to": {"get": "Transform.pos[1]"}},
    ])
    eng.define_system(name="hero_ground", order=110, doc="Standing on something solid: the contact pushes the hero up, so the normal from hero to ground points down.", on_contact={"a": ["Hero"], "b": ["Ground"], "triggers": "solid"}, do=[
        {"if": {"lt": [{"contact": "ny"}, -0.5]}, "then": [{"set": "a.Hero.grounded", "to": 1}]},
    ])
    eng.define_system(name="coin_pickup", order=120, doc="Touch a coin, score, sound, gone.", on_contact={"a": ["Hero"], "b": ["Coin"], "triggers": "only"}, do=[
        {"res_add": "score", "by": 1}, {"emit": "coin"}, {"despawn": "b"},
    ])
    eng.define_system(name="coin_bob", order=125, each=["Coin", "Transform"], do=[
        {"set": "Transform.pos[1]", "to": {"add": [{"get": "Coin.y0"}, {"mul": [0.08, {"sin": {"add": [{"mul": [{"tick": True}, 0.1]}, {"uid": True}]}}]}]}},
    ])
    eng.define_system(name="camera_follow", order=130, doc="Ease towards the hero, staying inside the level.", each=["Camera", "Transform"], do=[
        {"set": "Transform.pos[0]", "to": {"clamp": [{"lerp": [{"get": "Transform.pos[0]"}, {"res": "player.x"}, 0.1]}, -(W / 2 - 8.8), W / 2 - 8.8]}},
    ])
    eng.define_system(name="win_banner", order=140, doc="All coins: show the banner once and play the fanfare.", each=["Banner", "Text"], do=[
        {"set": "Text.visible", "to": {"ge": [{"res": "score"}, len(coins)]}},
        {"if": {"and": [{"ge": [{"res": "score"}, len(coins)]}, {"eq": [{"res": "won_flag"}, 0]}]}, "then": [{"res_set": "won_flag", "to": 1}, {"emit": "win"}]},
    ])
    eng.save_scene(path="examples/moon_hopper.lunaris.json")
    print(f"saved examples/moon_hopper.lunaris.json - {eng.describe()['entities']} entities, {runs} collider boxes, {len(eng.systems()['graph'])} graph systems")
