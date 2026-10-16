"""The items, tools, foods and recipes of Wyrdwood: ONE table, from which the inventory scripts (`tools/wyrdwood_inv.py`), the icon atlas (`tools/make_wyrdwood_icons.py`) and the interface are made.

The shape of the progression is the one the reference game teaches (hands first: stones and sticks off the ground, a stone axe and a club and a hammer from nothing; then a workbench, which is built; then better tools
from what the first region gives; the first pickaxe waits for the first Warden), and every name, number and picture here is our own.
"""

# key, display name, short name for the icon, stack size, class, power, durability, (colour of the icon), food: (health, stamina, seconds)
# class: res (a resource), food, axe, pick, hammer, weapon, torch, trophy
ITEMS = [
    # id 0 is "nothing"
    ("wood", "Wood", 50, "res", 0, 0, (0.55, 0.38, 0.2), None),
    ("stone", "Stone", 50, "res", 0, 0, (0.62, 0.62, 0.66), None),
    ("flint", "Flint", 30, "res", 0, 0, (0.3, 0.36, 0.42), None),
    ("resin", "Resin", 50, "res", 0, 0, (0.92, 0.62, 0.18), None),
    ("leather", "Leather scraps", 30, "res", 0, 0, (0.58, 0.42, 0.3), None),
    ("hide", "Deer hide", 30, "res", 0, 0, (0.66, 0.48, 0.3), None),
    ("bone", "Bone fragments", 30, "res", 0, 0, (0.9, 0.88, 0.8), None),
    ("coal", "Coal", 50, "res", 0, 0, (0.14, 0.14, 0.17), None),
    ("fine", "Fine wood", 50, "res", 0, 0, (0.72, 0.5, 0.24), None),
    ("berries", "Berries", 20, "food", 0, 0, (0.78, 0.14, 0.3), (7, 14, 600)),
    ("mushroom", "Mushroom", 20, "food", 0, 0, (0.86, 0.76, 0.6), (9, 5, 600)),
    ("meat", "Raw meat", 20, "res", 0, 0, (0.78, 0.26, 0.26), None),
    ("cooked", "Cooked meat", 20, "food", 0, 0, (0.56, 0.26, 0.16), (16, 11, 1200)),
    ("honey", "Honey", 20, "food", 0, 0, (0.95, 0.74, 0.2), (8, 20, 900)),
    ("torch", "Torch", 1, "torch", 1, 400, (0.9, 0.55, 0.2), None),
    ("stone_axe", "Stone axe", 1, "axe", 7, 100, (0.62, 0.62, 0.66), None),
    ("club", "Club", 1, "weapon", 14, 100, (0.55, 0.38, 0.2), None),
    ("hammer", "Hammer", 1, "hammer", 1, 100, (0.6, 0.6, 0.64), None),
    ("flint_axe", "Flint axe", 1, "axe", 11, 120, (0.3, 0.36, 0.42), None),
    ("flint_knife", "Flint knife", 1, "weapon", 17, 100, (0.3, 0.36, 0.42), None),
    ("trophy_moss", "Moss heart", 5, "trophy", 0, 0, (0.4, 0.75, 0.4), None),
    ("horn_pick", "Horn pick", 1, "pick", 9, 120, (0.85, 0.82, 0.7), None),
    ("bow", "Bow", 1, "weapon", 12, 100, (0.6, 0.42, 0.22), None),
    ("arrow", "Arrow", 100, "res", 0, 0, (0.7, 0.55, 0.3), None),
    ("wood_shield", "Round shield", 1, "weapon", 0, 120, (0.6, 0.44, 0.24), None),
    ("rag_tunic", "Rag tunic", 1, "armor", 2, 200, (0.6, 0.5, 0.4), None),
]
ID = {it[0]: i + 1 for i, it in enumerate(ITEMS)}  # key -> number
BY_ID = {i + 1: it for i, it in enumerate(ITEMS)}

# (result key, how many, station, [(ingredient key, count)])  station: "" (by hand), "workbench"
RECIPES = [
    ("stone_axe", 1, "", [("wood", 5), ("stone", 4)]),
    ("club", 1, "", [("wood", 6)]),
    ("hammer", 1, "", [("wood", 3), ("stone", 2)]),
    ("torch", 1, "", [("wood", 2), ("resin", 3)]),
    ("flint_axe", 1, "workbench", [("wood", 4), ("flint", 6)]),
    ("flint_knife", 1, "workbench", [("wood", 2), ("flint", 4), ("leather", 2)]),
    ("bow", 1, "workbench", [("wood", 10), ("leather", 4)]),
    ("arrow", 20, "workbench", [("wood", 8), ("flint", 2)]),
    ("wood_shield", 1, "workbench", [("wood", 8), ("leather", 2)]),
    ("rag_tunic", 1, "workbench", [("leather", 6)]),
    ("horn_pick", 1, "workbench", [("wood", 10), ("trophy_moss", 1)]),
]

# what the hammer can build is in `tools/wyrdwood_build.py`

# the base of the character, and what food adds to it (the reference game's first rule: no hunger bar, three foods that set your health and stamina)
BASE_HEALTH, BASE_STAMINA = 25.0, 50.0
FOOD_SLOTS = 3
INV_SLOTS = 32  # 8 x 4, the first row is the hotbar
HOT_SLOTS = 8
