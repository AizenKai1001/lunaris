"""Renders a picture of every example scene (and the two bots' games) into docs/gallery/ and writes docs/gallery/index.html. The
reference pictures come from the deterministic CPU renderer; the `-high` ones from the GPU renderer's high-quality mode (they need a GPU,
and differ a little from one graphics card to the next).

    python -I tools/make_gallery.py

Editor screenshots (docs/gallery/editor-*.png) are added by hand with a browser; the page lists them when they exist.
"""
import html
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "clients", "python"))
from lunaris import Engine  # noqa: E402

ROOT = os.path.join(HERE, "..")
OUT = os.path.join(ROOT, "docs", "gallery")
os.makedirs(OUT, exist_ok=True)

SHOTS = [
    ("showcase", "examples/showcase.lunaris.json", {"width": 960, "height": 540, "supersample": 2}, 0, "Lit 3D meshes with a point light and a sun: the reference renderer, bit-identical on every machine."),
    ("models", "examples/models.lunaris.json", {"width": 960, "height": 540, "supersample": 2}, 0, "Imported glTF models: a textured crate with a plain-material pyramid, and a three-colour tower."),
    ("sprites", "examples/sprites.lunaris.json", {"width": 960, "height": 540}, 0, "2D sprites with textures, blending and a 3D cube among them (orthographic camera)."),
    ("orb_dash", "examples/orb_dash.lunaris.json", {"width": 960, "height": 540}, 120, "Orb Dash, a 3D game made of data: components, graph systems, HUD text. Roll with WASD."),
    ("moon_hopper", "examples/moon_hopper.lunaris.json", {"width": 960, "height": 540}, 0, "Moon Hopper, a 2D pixel-art platformer: tile map, sprite sheet animation, contact systems."),
    ("clockwork", "examples/clockwork.lunaris.json", {"width": 960, "height": 540, "supersample": 2}, 600, "Clockwork: planets, moons and a moon of a moon, all attached entities (Parent + LocalTransform) under spinning pivots; no orbit code at all. Built by tools/build_clockwork.py through tool calls."),
    ("coin_chase", "examples/coin_chase.lunaris.json", {"width": 960, "height": 540}, 0, "Coin Chase, the tutorial game (docs/TUTORIAL.md): the camera is attached to the ball with Parent + LocalTransform."),
    ("particles", "examples/particles.lunaris.json", {"width": 960, "height": 540}, 150, "Particles: a fountain, a campfire and fireworks, all `Emitter` entities (docs/AI_GUIDE.md); a small graph system times the bursts."),
    ("animation", "examples/animation.lunaris.json", {"width": 960, "height": 540}, 100, "Skeletal animation: one skinned glTF model (`examples/models/swayer.glb`, four joints) playing its two clips at different speeds; an `Animator` component holds the clip name and the clock (`tools/build_animation.py`). The reference renderer draws it too."),
    ("walker", "examples/walker.lunaris.json", {"width": 960, "height": 540}, 40, "Walker: a `CharacterBody` hero (WASD and Space) with stairs, a wall, a platform to jump onto or fall off; the engine moves, collides, steps and falls, a three-statement graph system turns keys into `walk` and `jump`. The camera rides on the hero (`tools/build_walker.py`)."),
    ("pile", "examples/pile.lunaris.json", {"width": 960, "height": 540, "eye": [14, 10, 22], "target": [0, 3, 0], "fov": 50}, 240, "Physics: spheres and boxes settling in a walled box (sequential impulses, identical on every run)."),
]
HIGH_SHOTS = [
    ("vista", "examples/vista.lunaris.json", {"width": 1280, "height": 720}, 0, "Vista: a procedural evening sky with clouds, sun shadows, ambient occlusion, bloom on the lanterns, fog, cobbles, bricks and grass from built-in material textures, metal and plastic spheres (`tools/build_vista.py`)."),
    ("showcase", "examples/showcase.lunaris.json", {"width": 960, "height": 540}, 0, "The showcase scene through the high-quality mode."),
    ("models", "examples/models.lunaris.json", {"width": 960, "height": 540}, 0, "Imported glTF models, textured and shadowed."),
    ("orb_dash", "examples/orb_dash.lunaris.json", {"width": 960, "height": 540}, 120, "Orb Dash with shadows and soft light."),
    ("particles", "examples/particles.lunaris.json", {"width": 960, "height": 540}, 150, "Particles: the glow is bloom."),
    ("animation", "examples/animation.lunaris.json", {"width": 960, "height": 540}, 100, "The animated models with sun shadows, grass and a sky."),
    ("depth_of_field", "examples/depth_of_field.lunaris.json", {"width": 960, "height": 540}, 0, "Depth of field: the middle sphere is in focus, the near and far ones and the floor blur with distance (`Environment.dof`, `focus`, `focus_range`)."),
    ("orchard", "examples/orchard.lunaris.json", {"width": 960, "height": 540}, 300, "Prefabs: forty trees placed as instances of one prefab, balls raining from a `spawn_prefab` statement (`tools/build_orchard.py`)."),
    ("maze", "examples/maze.lunaris.json", {"width": 960, "height": 540}, 240, "Navigation: a Seeker patrolling four flags through a maze by itself (`NavAgent`, `tools/build_maze.py`)."),
    ("duel", "examples/duel.lunaris.json", {"width": 960, "height": 540}, 5, "The two-player lockstep game: two heroes, 24 orbs (`lunaris host`, `lunaris play --join`; `tools/build_duel.py`)."),
    ("menu", "examples/menu.lunaris.json", {"width": 960, "height": 540}, 2, "The `Ui` component: a health bar, buttons that react to the mouse, a title panel (`tools/build_menu.py`)."),
]
cards = []
high_cards = []
with Engine(root=ROOT) as eng:
    for name, scene, args, ticks, caption in SHOTS:
        eng.reset(seed=1)
        eng.load_scene(path=scene)
        if ticks:
            eng.step(ticks=ticks)
        frame = eng.render(quality="reference", **args)
        with open(os.path.join(OUT, f"{name}.png"), "wb") as f:
            f.write(frame["png"])
        cards.append((name, caption, f"{frame['width']}x{frame['height']}, reference renderer, picture hash {frame['hash']}"))
        print("rendered", name)
    # the same worlds through the GPU renderer's high-quality mode
    for name, scene, args, ticks, caption in HIGH_SHOTS:
        eng.reset(seed=1)
        eng.load_scene(path=scene)
        if ticks:
            eng.step(ticks=ticks)
        frame = eng.render(quality="high", **args)
        if not frame["renderer"].startswith("gpu"):
            print("no GPU here: skipped", name)
            continue
        with open(os.path.join(OUT, f"{name}-high.png"), "wb") as f:
            f.write(frame["png"])
        high_cards.append((f"{name}-high", caption, f"{frame['width']}x{frame['height']}, {frame['renderer']}"))
        print("rendered", name, "(high)")
    # a text map, the way a language model sees the world
    eng.reset(seed=1)
    eng.load_scene(path="examples/orb_dash.lunaris.json")
    player = eng.query(with_=["Player"], select=[])["entities"][0]["uid"]
    m = eng.minimap(around=player, radius=16, cells=25)
    minimap = "\n".join(m["rows"]) + "\n\nlegend: " + ", ".join(f"{k} = {v}" for k, v in m["legend"].items())

extras = [(f, c) for f, c in [("editor-game.png", "The browser editor playing Orb Dash: entity list, viewport, inspector, Systems and Tool calls tabs."), ("editor-nodes.png", "A graph system drawn as nodes (right-click to edit; Apply sends it to the engine)."), ("editor-hierarchy.png", "Attached entities nested under their parent in the list (Parent + LocalTransform, set with Attach...); the inspector shows the selected moon's local placement. Made by tools/editor_shots.mjs."), ("editor-new-menu.png", "The + menu: ten kinds of entity, each one an ordinary spawn call.")] if os.path.exists(os.path.join(OUT, f))]
page = ["<!doctype html><meta charset=utf-8><title>Lunaris gallery</title><style>body{font:15px system-ui;max-width:1040px;margin:24px auto;padding:0 16px;background:#14161b;color:#d6dae3}img{max-width:100%;border:1px solid #303644;border-radius:6px}figure{margin:0 0 28px}figcaption{margin-top:6px}small{color:#8b93a6}pre{background:#0f1115;padding:12px;border-radius:6px;overflow:auto}</style>", "<h1>Lunaris engine gallery</h1>", "<p>Every picture below was produced by the engine itself (<code>tools/make_gallery.py</code>); the scenes are in <code>examples/</code>.</p>"]
for name, caption, info in cards:
    page.append(f'<figure><img src="{name}.png" alt="{name}"><figcaption><b>{html.escape(name)}</b>: {html.escape(caption)} <small>{html.escape(info)}</small></figcaption></figure>')
for name, caption, info in high_cards:
    page.append(f'<figure><img src="{name}.png" alt="{name}"><figcaption><b>{html.escape(name)}</b>: {html.escape(caption)} <small>{html.escape(info)}</small></figcaption></figure>')
for f, c in extras:
    page.append(f'<figure><img src="{f}" alt="{f}"><figcaption>{html.escape(c)}</figcaption></figure>')
page.append("<h2>What a language model sees</h2><p>The <code>minimap</code> tool: a text map around the player in Orb Dash (<code>@</code> is the player, <code>O</code> an orb, up is -z).</p>")
page.append("<pre>" + html.escape(minimap) + "</pre>")
open(os.path.join(OUT, "index.html"), "w", encoding="utf-8").write("\n".join(page))
print("wrote docs/gallery/index.html")
