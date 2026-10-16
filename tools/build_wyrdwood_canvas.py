"""Write the Wyrdwood plan as a canvas (the JSON Canvas format: the Lunaris Editor's Canvas tab opens it, and so does Obsidian).

    python tools/build_wyrdwood_canvas.py [OUT.canvas] [--links]      (default: examples/wyrdwood/wyrdwood.canvas)

`--links` writes `[[wiki links]]` to notes of the maker's own vault; without it the same words are plain text.
"""
import json
import os
import sys

ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
LINKS = "--links" in sys.argv
OUT = ARGS[0] if ARGS else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "wyrdwood", "wyrdwood.canvas")
nodes, edges = [], []


def text(id, x, y, w, h, body, color=None):
    n = {"id": id, "type": "text", "text": body, "x": x, "y": y, "width": w, "height": h}
    if color:
        n["color"] = color
    nodes.append(n)


def group(id, x, y, w, h, label, color=None):
    n = {"id": id, "type": "group", "label": label, "x": x, "y": y, "width": w, "height": h}
    if color:
        n["color"] = color
    nodes.append(n)


def edge(a, b, label=None, fa="right", tb="left", color=None):
    e = {"id": f"e_{a}_{b}", "fromNode": a, "fromSide": fa, "toNode": b, "toSide": tb}
    if label:
        e["label"] = label
    if color:
        e["color"] = color
    edges.append(e)


# ------------------------------------------------------------------ the middle
text("core", -320, -90, 640, 250,
     "# Wyrdwood\nAn **original** 3D survival and building game for 1 to 10 players, built on **Lunaris** to find what the engine lacks (like Caldera did).\n\nSeeded archipelago, five Wardens, bases that must stand, stamina fights, boats, weather.\n\nPlan file: `examples/wyrdwood/PLAN.md`" + (" · notes: [[Wyrdwood - Overview]] · engine: [[Lunaris - Project Hub]]" if LINKS else ""), "6")

text("status", 400, -90, 560, 250,
     "## Where it stands (2026-10-15)\n- Plan written and committed.\n- **Character motion done**: swim, wade, roll, ladders, ledge pull-up (`CharacterMotion`, `Climbable`) and the three Caldera traps fixed (characters report contacts, a `Steer` walks a character, a `Probe` gives scripts the ground and a ray). 12 new tests; every old test, the 51 golden scenes and the 51 playtests unchanged.\n- Next: foot IK, then destruction, then the M1 island.\n- **This board is the editor's Canvas tab** (`lunaris-editor --canvas examples/wyrdwood/wyrdwood.canvas`): it reads Obsidian's canvas files and follows the file when it is saved, so the plan stays on screen while the work goes on.", "4")

# ------------------------------------------------------------------ pillars
group("g_pillars", -1480, -420, 1000, 640, "The five pillars", "5")
pillars = [
    ("p1", "**1. Build anywhere, and it has to stand**\nPieces snap; wood and stone carry weight; a wall with nothing under it falls. A base is a place you made."),
    ("p2", "**2. Stamina fights**\nSwing, block, parry, roll. Every blow costs breath. A big animal teaches you to wait."),
    ("p3", "**3. The land is the progression**\nNo levels. A region is a wall until you own the tool that opens it."),
    ("p4", "**4. A world that looks like weather**\nFog in valleys, a low red sun, rain on water, firelight on timber. The GPU renderer is the art direction."),
    ("p5", "**5. Together, loosely**\n1 to 10 players who may come and go; alone is a full game."),
]
for i, (id, body) in enumerate(pillars):
    text(id, -1450 + (i % 2) * 480, -360 + (i // 2) * 200, 450, 170, body, "5")

# ------------------------------------------------------------------ the world
group("g_world", -300, -1250, 2780, 560, "The world: regions gate your tools (a Warden sleeps in each)", "4")
regions = [
    ("r1", "**Meadow**\ngreen hills, birches, berries, deer, boars\n+ wood, stone, flint, hide\n! boars, crows\nWarden: the **Moss Colossus**"),
    ("r2", "**Gloomwood**\ndense old fir, blue light, mist\n+ bog ore, resin, strong timber\n! wolves, bark-skinned stalkers\nWarden: the **Thornmother**"),
    ("r3", "**Moor**\nswamp, rain, rotten water, ruins\n+ peat iron, reeds, bones\n! leeches, mire hounds, sickness\nWarden: the **Drowned Jarl**"),
    ("r4", "**Crag**\nmountain, snow, cold that hurts\n+ silver, ice stone\n! frost wolves, cold, rockfall\nWarden: the **Rime Wyrm**"),
    ("r5", "**Barrens**\nash plains, cinder wind\n+ black iron, fire stone\n! raiders, ash storms\nWarden: the **Cinder King**"),
    ("r6", "**Sea**\nopen water between the islands\n+ fish, wrecks, new islands\n! storms, sea beasts\nno Warden"),
]
for i, (id, body) in enumerate(regions):
    text(id, -270 + i * 455, -1180, 430, 230, body, "4")
for a, b in [("r1", "r2"), ("r2", "r3"), ("r3", "r4"), ("r4", "r5")]:
    edge(a, b, "Warden beaten opens")
text("r_end", 520, -930, 1200, 150,
     "**The end**: light all five beacons and the road home appears.   **How the world is made**: Tellus (our terrain tool) writes the height tiles, splat pictures, water and forests; Lunaris loads them with `Terrain` + `TerrainLayers` + `WaterBody` + `scatter`. One seed, the same world on every machine.", "4")
edge("core", "r1", "starts here", fa="top", tb="bottom")

# ------------------------------------------------------------------ the loop
text("loop", -1480, -1250, 1000, 700,
     "## The loop\n\n**gather** (wood, stone, fibre, berries)\n↓\n**craft** at a station\n↓\n**build** a base, cook, rest\n↓\n**explore** a new region\n↓\n**meet its Warden**\n↓\nbetter tools → next region", "6")
edge("loop", "r1", None, fa="right", tb="left")

# ------------------------------------------------------------------ systems
group("g_sys", -1480, 330, 3000, 900, "Systems and where the engine stands today", None)
cols = [
    ("have", "Have (in the engine now)", "4", -1450, [
        "Terrain, materials, water, floating (`Terrain`, `TerrainLayers`, `WaterBody`, `Buoyant`)",
        "**Swim, wade, roll, ladders, ledge pull-up** (`CharacterMotion`, `Climbable`: new, 2026-10-15)",
        "Third-person camera with collision (`CameraRig`, `CameraCollide`)",
        "Creature parts: `Steer`, `Sensor`, `BehaviorTree`, navigation, spawners",
        "Day and night, fog, weather look (`TimeOfDay`, `HeightFog`, GPU high)",
        "UI kit v3: bars, drag-and-drop slots, menus",
        "Audio mixer; tools: playtests, golden hashes, `profile`",
    ]),
    ("part", "Part (exists, not enough)", "3", -450, [
        "Big world: `StreamZone` streams prefabs; **terrain tile streaming is new**",
        "Forests at a distance: instanced meshes exist; occlusion and LOD to measure",
        "Combat: `HitPoints`, `Sensor`, contacts; the rules are game content",
        "Items and inventory: UI slots exist; item data and rules new",
        "Boats: floating exists; a boat controller is new",
        "Characters you see: skinned `.glb` works; retargeting and morph targets new",
        "Multiplayer: lockstep and rollback to 16 players; **no drop-in**",
        "Saves: scenes and snapshots; world-changes save new",
    ]),
    ("new", "New (to build)", "1", 550, [
        "**Building with a support graph** (the core of the game)",
        "**Destruction**: fell a tree, break a rock, pieces and sound",
        "Crafting and stations (spatial query + recipes)",
        "Survival loop: food buffs, rest, cold and wet",
        "**Snapshot join** for the network (state at tick N + the log)",
        "Foot IK on slopes (presentation only)",
        "Runtime world generation from a seed (Tellus tiles)",
    ]),
]
for id, title, color, x, items in cols:
    body = f"## {title}\n" + "\n".join(f"- {t}" for t in items)
    text("sys_" + id, x, 380, 940, 800, body, color)

# ------------------------------------------------------------------ milestones
group("g_ms", -1480, 1280, 4040, 600, "Milestones: each ends with something you can play, a playtest, and screenshots", "6")
ms = [
    ("m1", "**M1 Shore**\n400 m island; walk, run, swim, roll; day and night; fell trees, mine rocks; hotbar, inventory; workbench, axe, hammer; one boar", "1"),
    ("m2", "**M2 Hearth**\nBuild a house from snapping pieces that has to stand; door, fire, bed; rest and cook; save and load", "1"),
    ("m3", "**M3 Blood**\nStamina combat with sword, shield, block, parry; boars, wolves, deer; hurt, heal, eat; the first Warden", "1"),
    ("m4", "**M4 Wood**\nThe whole island with Meadow and Gloomwood from a seed; thousands of trees; streaming; the second Warden", "1"),
    ("m5", "**M5 Together**\n2 to 4 players on a host, one joining mid-game", "1"),
    ("m6", "**M6 Sail**\nBuild a boat, sail to a second island with a Moor", "1"),
    ("m7", "**M7 Mountain and ash**\nCrag and Barrens, the last three Wardens, the beacon ending", "1"),
    ("m8", "**M8 Polish**\nMenus, settings, controller, sound, map, 60 fps on the weakest machine tested", "1"),
]
for i, (id, body, color) in enumerate(ms):
    text(id, -1450 + i * 500, 1460, 470, 220, body, color)
for (a, _, _), (b, _, _) in zip(ms, ms[1:]):
    edge(a, b)
text("m0", -1450, 1330, 3900, 100,
     "**M0 (done)**: the plan. **First target = M1 to M3, the game's first hour.** M4 and M5 are what make it a real engine test (scale and multiplayer); the rest is content on a finished foundation.", "4")

# ------------------------------------------------------------------ engine work order
group("g_eng", -1480, 1930, 4040, 560, "Engine work, in order, and which milestone waits for it", "2")
eng = [
    ("w1", "**1. Character toolkit**\nswim, wade, roll, ladders, ledges and the three Caldera traps (contacts, `Steer`, `Probe`): DONE.\nLeft: foot IK (presentation only)", "3"),
    ("w2", "**2. Destruction**\nfelling, breaking rocks, pieces, sound. Cloth waits (sails can be vertex-animated)", "1"),
    ("w3", "**3. 3D performance + tile streaming**\ninstancing at forest scale, occlusion, LOD, measured with `profile`", "1"),
    ("w4", "**4. Building support graph**\nas game scripts first; into the engine if 2,000 pieces measure too slow", "1"),
    ("w5", "**5. Snapshot join**\nfor the network: a player joins mid-game", "1"),
    ("w6", "**6. Morph targets + retargeting**\nwhen equipment on bodies is wanted", "1"),
    ("w7", "**7. gpu `high.rs` hang**\nwhenever it blocks", "1"),
]
for i, (id, body, color) in enumerate(eng):
    text(id, -1450 + i * 560, 1990, 530, 250, body, color)
for (a, _, _), (b, _, _) in zip(eng, eng[1:]):
    edge(a, b)
for w, m, label in [("w1", "m1", "needs"), ("w2", "m1", "needs"), ("w4", "m2", "needs"), ("w3", "m4", "needs"), ("w5", "m5", "needs")]:
    edge(w, m, label, fa="top", tb="bottom", color="2")

text("rules", -1450, 2270, 3900, 160,
     "**What I will not claim**: that the Wardens, regions or two kilometres exist before they are played and photographed; that combat or building feels good because it works (every playtest note goes in `docs/DOGFOOD_WYRDWOOD.md`, bad parts included); that multiplayer works until two real windows on two machines finish a session with the same state hash.", "2")

edge("core", "sys_part", "what it needs", fa="bottom", tb="top")

os.makedirs(os.path.dirname(os.path.abspath(OUT)), exist_ok=True)
json.dump({"nodes": nodes, "edges": edges}, open(OUT, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
print("wrote", os.path.normpath(OUT), len(nodes), "nodes", len(edges), "edges")
