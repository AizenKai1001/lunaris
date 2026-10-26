"""Write examples/outpost.lunaris.json: a small online survival scene that exercises the game server (docs/GAME_SERVER.md) end to end: spawn points, a hut with a door that swings, crates with loot,
items on the ground, a build tool, two patrolling armed drones, an avatar with a weapon, and the first-person controller of examples/first_person.lunaris.json with its buttons and commands wired to the server.

    lunaris server --scene examples/outpost.lunaris.json --players 16
    lunaris play   examples/outpost.lunaris.json --connect 127.0.0.1:7777 --name Ada

Everything in it is a greybox of the engine's own primitives (flat-coloured cubes and spheres): no models, textures or sounds; a real game replaces the prefabs named in `kinds` below with its own.
The one scene serves both sides: the server runs it, the player plays it and the server's things replace the scene's own copies when it connects.

    python3 tools/build_outpost.py        (from the repository root; it reads examples/first_person.lunaris.json for the walking systems)
"""
import json
import math

import os

OUT = os.environ.get("OUTPOST_OUT", "examples/outpost.lunaris.json")
fp = json.load(open("examples/first_person.lunaris.json", encoding="utf-8"))

entities = []


def spawn(uid, name, **components):
    entities.append({"uid": uid, "components": {"Name": {"value": name}, **components}})


def box(uid, name, pos, size, color, **more):
    spawn(uid, name, Transform={"pos": pos, "scale": size}, MeshRenderer={"mesh": "cube", "color": color + [1], "roughness": 0.8}, Collider={"shape": 1, "size": [1, 1, 1]}, **more)


def look_rot(px, pz):
    """A rotation that makes local -z point from (px, pz) to the middle."""
    yaw = math.atan2(px, pz)
    return [0, round(math.sin(yaw / 2), 5), 0, round(math.cos(yaw / 2), 5)]


# ---------------------------------------------------------------- the rules of the game (read by the server; harmless on the client)
spawn(6, "Loot tables", LootTable={"text": "table crate rolls=2-3 empty=0.15\n  bandage w=5 n=1-2\n  can w=4\n  water w=4\n  @rare w=1\n  @guns w=3\n  @ammo w=4\n"
                                    "table rare rolls=1\n  crowbar w=2\n  medkit w=2\n  flashlight w=2\n  goggles w=1\n"
                                    "table guns rolls=1\n  pistol w=4\n  smg w=3\n  shotgun w=3\n  rifle w=2\n  sniper w=1\n"
                                    "table ammo rolls=1\n  ammo9 w=4 n=12-30\n  shells w=3 n=4-10\n  ammo556 w=3 n=15-30\n  ammo762 w=1 n=5-10\n"})
# the items are numbered from 1 in the order of this list (the guns must be next to each other: the client's `net_hand` system below knows them by their numbers). `OUTPOST_ITEMS` (comma separated keys) gives another order:
# a game whose item icons are one sheet numbers the items in the sheet's order, so that an icon is found from the number (the survival example scene does).
ITEM_LINES = {
    "bandage": "Bandage | stack=10 weight=0.1", "can": "Canned food | stack=5 weight=0.4", "water": "Water bottle | stack=3 weight=0.5", "crowbar": "Crowbar | stack=1 weight=2",
    "pistol": "Pistol | stack=1 weight=1", "smg": "Submachine gun | stack=1 weight=2.5", "shotgun": "Shotgun | stack=1 weight=3.5", "rifle": "Assault rifle | stack=1 weight=3.5", "sniper": "Sniper rifle | stack=1 weight=5",
    "ammo9": "9 mm rounds | stack=60 weight=0.02", "shells": "Shotgun shells | stack=24 weight=0.04", "ammo556": "5.56 rounds | stack=90 weight=0.02", "ammo762": "7.62 rounds | stack=30 weight=0.03",
    "medkit": "Medical kit | stack=1 weight=1.2", "flashlight": "Flashlight | stack=1 weight=0.4", "goggles": "Hacking goggles | stack=1 weight=0.6",
}
GUN_KEYS = ["pistol", "smg", "shotgun", "rifle", "sniper"]
ITEMS = os.environ.get("OUTPOST_ITEMS", ",".join(ITEM_LINES)).split(",")
assert sorted(ITEMS) == sorted(ITEM_LINES), "OUTPOST_ITEMS must name every item once"
item_id = {k: i + 1 for i, k in enumerate(ITEMS)}
GUN_FIRST, GUN_LAST = min(item_id[k] for k in GUN_KEYS), max(item_id[k] for k in GUN_KEYS)
assert GUN_LAST - GUN_FIRST == len(GUN_KEYS) - 1, "the guns must have numbers next to each other"
# (`OUTPOST_ICONS` = "picture,columns,rows": the items are pictures of that sheet, item number n in cell n - 1; the interface's cards show them, docs/GAMEPLAY.md)
ICONS = os.environ.get("OUTPOST_ICONS")
table = {"text": "".join(f"item {k} | {ITEM_LINES[k]}" + (f" icon={item_id[k] - 1}" if ICONS else "") + "\n" for k in ITEMS)}
if ICONS:
    picture, cols, rows = ICONS.split(",")
    table.update({"icons": picture, "icon_cols": int(cols), "icon_rows": int(rows)})
spawn(7, "Items", ItemTable=table)
# what each gun is (docs/LOADOUT.md): the one the player holds is its `Weapon`
GUNS = ("gun pistol  ammo=ammo9   damage=25 rate=300 mag=12 reload=1.4 auto=0 spread=0.3 range=120 noise=100 recoil=0.5\n"
        "gun smg     ammo=ammo9   damage=14 rate=800 mag=30 reload=2.0 auto=1 spread=1.2 bloom=0.35 range=90 noise=130 falloff=25 recoil=0.4\n"
        "gun shotgun ammo=shells  damage=9  pellets=8 rate=70 mag=6 reload=3.0 auto=0 spread=3.5 range=50 noise=170 falloff=10 falloff_min=0.2 recoil=1.4\n"
        "gun rifle   ammo=ammo556 damage=32 rate=640 mag=30 reload=2.4 auto=1 spread=0.5 bloom=0.25 range=220 noise=150 recoil=0.6\n"
        "gun sniper  ammo=ammo762 damage=90 rate=45 mag=5 reload=3.2 auto=0 spread=0.05 range=400 noise=190 head=2.5 falloff=200 falloff_min=0.8 recoil=2.0\n")


def bag_of(*stacks):
    """The 48 id and count slots of an Inventory with these (item, count) stacks from slot 0."""
    ids, counts = [0] * 48, [0] * 48
    for k, (item, n) in enumerate(stacks):
        ids[k], counts[k] = item_id[item], n
    return {"id": ids, "count": counts}


spawn(8, "Building rules", BuildRules={"text": "rules step=90 snap=1.5\npiece foundation size=4,0.5,4 hp=500 free=1 anchor=1 decay=0.02\n"
      "  socket n at=0,0.25,-2 yaw=0   type=edge mates=edge,wall_base carry=0.8,wall_base:0\n"
      "  socket s at=0,0.25,2  yaw=180 type=edge mates=edge,wall_base carry=0.8,wall_base:0\n"
      "  socket e at=2,0.25,0  yaw=-90 type=edge mates=edge,wall_base carry=0.8,wall_base:0\n"
      "  socket w at=-2,0.25,0 yaw=90  type=edge mates=edge,wall_base carry=0.8,wall_base:0\n"
      "piece wall size=4,3,0.2 hp=300 inset=0.12 decay=0.02\n"
      "  socket base at=0,-1.5,0 yaw=0 type=wall_base mates=edge carry=0.9 turn=0\n"})
spawn(9, "Server", NetHost={"player": "Avatar", "forward": "shot 150\nbuild_placed 120\ncorpse 120\ndoor_open 60\ndoor_close 60\ncontainer_open 40\npicked 40", "static_every": 15,
                            "save": "saves/outpost_world.json", "save_every": 60, "persist": "Transform Inventory Vitals HitPoints", "drop": "Pickup",
                            "uses": "use can eat=35\nuse water drink=40\nuse bandage heal=5 bandage=1\nuse medkit heal=60 bandage=1"})
spawn(5, "Environment", Environment={"sky": True, "sky_top": [0.25, 0.4, 0.75], "sky_horizon": [0.75, 0.82, 0.92], "ambient": 0.9, "ssao": 1.0, "bloom": 0.2, "fog": 0.003})
spawn(3, "Sun", Transform={"rot": [-0.5, 0.35, 0.15, 0.78]}, Light={"kind": 0, "intensity": 2.6})

# ---------------------------------------------------------------- the ground and the walls round it
box(10, "Ground", [0, -0.5, 0], [80, 1, 80], [0.45, 0.55, 0.4])
for i, (pos, size) in enumerate([([0, 2, -40.25], [80, 4, 0.5]), ([0, 2, 40.25], [80, 4, 0.5]), ([-40.25, 2, 0], [0.5, 4, 80]), ([40.25, 2, 0], [0.5, 4, 80])]):
    box(11 + i, f"Perimeter {i + 1}", pos, size, [0.6, 0.58, 0.55])

# ---------------------------------------------------------------- where players start: a ring of eight near the walls, out of the drones' sight, facing the middle
for i in range(8):
    a = math.tau * i / 8
    x, z = 30 * math.cos(a), 30 * math.sin(a)
    spawn(100 + i, f"Spawn {i + 1}", Transform={"pos": [round(x, 2), 0, round(z, 2)], "rot": look_rot(x, z)}, SpawnPoint={"radius": 2})

# ---------------------------------------------------------------- the hut: walls with a doorway, a roof, a door that swings
hut_z = -14
box(20, "Hut back", [0, 1.5, hut_z - 3], [6, 3, 0.3], [0.7, 0.6, 0.5])
box(21, "Hut left", [-3, 1.5, hut_z], [0.3, 3, 6], [0.7, 0.6, 0.5])
box(22, "Hut right", [3, 1.5, hut_z], [0.3, 3, 6], [0.7, 0.6, 0.5])
box(23, "Hut front left", [-1.9, 1.5, hut_z + 3], [2.2, 3, 0.3], [0.7, 0.6, 0.5])
box(24, "Hut front right", [1.9, 1.5, hut_z + 3], [2.2, 3, 0.3], [0.7, 0.6, 0.5])
box(25, "Hut over the door", [0, 2.55, hut_z + 3], [1.6, 0.9, 0.3], [0.7, 0.6, 0.5])
box(26, "Hut roof", [0, 3.15, hut_z], [6.6, 0.3, 6.6], [0.45, 0.3, 0.25])
spawn(27, "Door", Transform={"pos": [0, 1.05, hut_z + 3], "scale": [1.1, 2.1, 0.12]}, MeshRenderer={"mesh": "cube", "color": [0.4, 0.28, 0.18, 1]}, Collider={"shape": 1, "size": [1, 1, 1]},
      Interactable={"kind": 1, "prompt": "Open"}, Door={"hinge": [-0.55, 0, 0], "angle_open": -100, "speed": 200}, NetEntity={"class": 3, "kind": 30})

# ---------------------------------------------------------------- crates with loot, inside and outside
crate_spots = [(-2.0, hut_z - 2.0), (2.0, hut_z - 2.0), (2.2, hut_z + 0.5), (-8, 6), (9, -3), (6, 12)]
for i, (x, z) in enumerate(crate_spots):
    spawn(30 + i, f"Crate {i + 1}", Transform={"pos": [x, 0.5, z]}, MeshRenderer={"mesh": "cube", "color": [0.55, 0.4, 0.2, 1]}, Collider={"shape": 1, "size": [1, 1, 1]},
          Interactable={"kind": 2, "prompt": "Search"}, Inventory={"size": 8}, LootContainer={"table": "crate", "key": 1000 + i, "refill": 900}, NetEntity={"class": 3, "kind": 31})

# ---------------------------------------------------------------- things on the ground
for i, (x, z, item) in enumerate([(-5, -4, "bandage 3"), (4, -6, "can 2"), (-10, 10, "shells 8"), (11, 8, "shotgun 1")]):
    spawn(50 + i, f"Pickup {i + 1}", Transform={"pos": [x, 0.3, z], "scale": [0.4, 0.4, 0.4]}, MeshRenderer={"mesh": "sphere", "color": [0.95, 0.85, 0.2, 1]},
          Interactable={"kind": 3, "prompt": "Take", "item": item, "radius": 0.7}, NetEntity={"class": 1, "kind": 32})

# ---------------------------------------------------------------- two drones that patrol and shoot what they see
patrols = ["-12 0 -12\n12 0 -12\n12 0 12\n-12 0 12\n", "0 0 -12\n0 0 12\n"]
for i in range(2):
    spawn(200 + i, f"Drone {i + 1}", Transform={"pos": [-12 if i == 0 else 0, 7, -12]}, Velocity={}, Team={"id": 2}, HitPoints={"value": 40, "max": 40},
          Collider={"shape": 0, "size": [0.9, 0.9, 0.9]}, MeshRenderer={"mesh": "sphere", "color": [0.85, 0.15, 0.15, 1], "emissive": 0.4},
          Perception={"engage": True, "chase": False, "sight_range": 18, "sight_fov": 360, "attack_range": 18, "hearing": 1.0, "shout": 25, "los": True, "sight_eye": 0.0},
          Weapon={"damage": 5, "rate": 150, "range": 40, "spread": 1.5, "infinite": True, "noise": 60, "recoil": [0, 0], "owner": 200 + i},
          FlyAgent={"perception": True, "patrol": patrols[i], "speed": 8, "range": 12, "orbit": 4, "hover": 7, "wait": 1.5}, Hackable={"difficulty": 3, "range": 25, "duration": 40, "speed": 8, "accel": 20}, NetEntity={"class": 4, "kind": 40})

# ---------------------------------------------------------------- two crawlers on the ground: low machines that notice a player within 25 m, walk to them round the obstacles (a shut door is a wall to them) and bite
for i, (cx, cz) in enumerate([(-16, 4), (16, 6)]):
    spawn(240 + i, f"Crawler {i + 1}", Transform={"pos": [cx, 0, cz]}, CharacterBody={"radius": 0.5, "height": 0.9}, Team={"id": 2}, HitPoints={"value": 60, "max": 60},
          NavAgent={"speed": 5, "radius": 0.5, "height": 0.9, "arrive": 1.0}, Perception={"engage": True, "sight_range": 25, "sight_fov": 360, "attack_range": 2.2, "hearing": 1.0, "shout": 20, "notice_time": 0.6, "lose_time": 6, "sight_eye": 0.6, "weapon": 250 + i},
          Hackable={"difficulty": 3, "range": 25, "duration": 40, "speed": 5, "accel": 30}, NetEntity={"class": 4, "kind": 42})
    # (the bite is a weapon on a child at the height of its mouth, which the crawler holds the trigger of)
    spawn(250 + i, f"Crawler {i + 1} mouth", Parent={"target": 240 + i}, LocalTransform={"pos": [0, 0.5, 0]}, Transform={"pos": [cx, 0.5, cz]},
          Weapon={"damage": 12, "rate": 120, "range": 2.4, "spread": 0, "infinite": True, "noise": 0, "recoil": [0, 0], "owner": 240 + i})

# ---------------------------------------------------------------- a helicopter in the open: use it (E) to sit in the pilot's seat, fly it with the keys, land it to get out (docs/SEATS.md)
spawn(220, "Helicopter", Transform={"pos": [22, 1.0, 14], "scale": [2.4, 2.0, 4.5]}, MeshRenderer={"mesh": "cube", "color": [0.3, 0.35, 0.4, 1], "roughness": 0.6}, Collider={"shape": 1, "size": [1, 1, 1]},
      RigidBody={"mass": 2000}, Velocity={}, AngularVelocity={"damping": 0}, HitPoints={"value": 1500, "max": 1500, "last": 1500},
      Helicopter={"engine": False, "spool": 0, "spool_up": 4, "noise": 160, "heat": 0.5}, Interactable={"kind": 0, "prompt": "Fly"},
      Seat={"offset": [0, 0.6, 0.4], "exit": [3, -0.7, 0], "drive": 1, "max_exit_altitude": 3}, NetEntity={"class": 4, "kind": 60})

# ---------------------------------------------------------------- a family car near the south-west corner (out of the drones' sight, which is 18 m from their patrol; the seated player stands up above the greybox car's low body, so they could shoot it): E to sit in the driver's seat, W S throttle and reverse, A D steer, Ctrl brake, Space handbrake; stop to get out
WHEELS = "defaults: radius=0.35 rest=0.3 travel=0.2 spring=35000 damper=3200 grip=1.2\nfl: -0.8 -0.15 -1.3 steer=1 brake=1\nfr: 0.8 -0.15 -1.3 steer=1 brake=1\nrl: -0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1\nrr: 0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1"
spawn(221, "Car", Transform={"pos": [-33, 0.74, 28]}, Collider={"shape": 1, "size": [1.8, 0.7, 4.2]}, RigidBody={"mass": 1200, "friction": 0.3}, Velocity={}, AngularVelocity={"damping": 0.05},
      HitPoints={"value": 800, "max": 800, "last": 800}, Vehicle={"wheels": WHEELS, "handbrake": 1}, Interactable={"kind": 0, "prompt": "Drive"},
      Seat={"offset": [-0.4, 0.45, 0.2], "exit": [-2.2, -0.3, 0], "drive": 2, "max_exit_speed": 4}, NetEntity={"class": 4, "kind": 61})

# ---------------------------------------------------------------- the lighthouse: a tower in the middle with a beam that sweeps the yard; a player it catches in the light is detected, the director
# raises their heat and sends hunters (docs/AI.md). The hut roof is cover from it.
TILT = math.radians(66)  # the beam looks down this far: it meets the ground 9 to 23 m from the tower (the spawn ring at 30 m is out of it)
LAMP = 35  # the lamp is high above the tower's top (30 m) so that the beam leaves the tower's footprint before it could meet the tower's roof
tilt_rot = [round(-math.sin(TILT / 2), 5), 0, 0, round(math.cos(TILT / 2), 5)]
box(230, "Tower", [0, 15, 0], [2, 30, 2], [0.5, 0.5, 0.55])
spawn(229, "Mast", Transform={"pos": [0, 32.5, 0], "scale": [0.4, 5, 0.4]}, MeshRenderer={"mesh": "cube", "color": [0.4, 0.4, 0.45, 1]})  # (no collider: it would be in the beam's way)
spawn(231, "Lighthouse", Transform={"pos": [0, LAMP, 0], "rot": tilt_rot}, Team={"id": 2}, Searchlight={"range": 44, "angle": 9, "sweep": 18, "hold": 0.8}, NetEntity={"class": 4, "kind": 70})
spawn(232, "Director", Director={"prefab": "Hunter", "threshold": 1.0, "light_heat": 1.2, "max_alive": 4, "min_dist": 22, "max_dist": 32, "height": 7, "cooldown": 20, "cleanup": 60})

# ---------------------------------------------------------------- the prefabs: the server's (the avatar, the corpse, the dropped thing) and the clients' (what the replicated things look like)
prefabs = {
    "Avatar": {
        "components": {"Name": {"value": "Avatar"}, "Transform": {}, "CharacterBody": {"radius": 0.35, "height": 1.8, "gravity_scale": 0}, "HitPoints": {"value": 100, "max": 100, "last": 100},
                       "Vitals": {}, "Heat": {}, "Hacker": {"range": 30, "speed": 2, "trace_heat": 0.4, "aim": True}, "Stealth": {}, "Respawn": {"delay": 5, "avoid": 20, "protect": 8, "corpse": "Corpse", "corpse_time": 300},
                       # (a pistol and two magazines of rounds: all it starts with; the other guns are in the crates)
                       "Inventory": {"size": 12, **bag_of(("pistol", 1), ("ammo9", 24))},
                       "NetAvatar": {}},
        # (the weapon is whatever gun is in the hand: `Loadout` makes it, from the bag)
        "children": [{"components": {"Name": {"value": "Head"}, "LocalTransform": {"pos": [0, 1.6, 0]},
                                     "Weapon": {"active": False, "spread": 0, "noise": 0},
                                     "Loadout": {"guns": GUNS},
                                     "BuildTool": {"reach": 8}, "Interactor": {"reach": 3, "look": False}}}],
    },
    # (what the dead player carried is on the corpse: a container anybody can search, with a box to aim at)
    "Corpse": {"components": {"Name": {"value": "Corpse"}, "Collider": {"shape": 1, "size": [1.8, 0.4, 0.7]}, "Inventory": {"size": 12}, "Interactable": {"kind": 2, "prompt": "Search"}, "NetEntity": {"class": 3, "kind": 9}}},
    # what the director sends: a flying machine like the drones, already alert to where the player stands
    "Hunter": {"components": {"Name": {"value": "Hunter"}, "Transform": {}, "Velocity": {}, "Team": {"id": 2}, "HitPoints": {"value": 40, "max": 40}, "Collider": {"shape": 0, "size": [0.9, 0.9, 0.9]},
                              "Perception": {"engage": True, "chase": False, "sight_range": 30, "sight_fov": 360, "attack_range": 22, "hearing": 1.0, "shout": 25, "los": True, "sight_eye": 0.0},
                              "Weapon": {"damage": 5, "rate": 150, "range": 40, "spread": 1.5, "infinite": True, "noise": 60, "recoil": [0, 0]},
                              "FlyAgent": {"perception": True, "speed": 9, "range": 14, "orbit": 4, "hover": 7, "wait": 1.5}, "Hackable": {"difficulty": 3, "range": 25, "duration": 40, "speed": 9, "accel": 20}, "NetEntity": {"class": 4, "kind": 40}}},
    "Pickup": {"components": {"Name": {"value": "Pickup"}, "Transform": {"scale": [0.4, 0.4, 0.4]}, "Interactable": {"kind": 3, "prompt": "Take", "radius": 0.7}, "NetEntity": {"class": 1, "kind": 32}}},
}


def model(name, color, mesh="cube", scale=None, solid=False, extra=None):
    comps = {"Name": {"value": name}, "Transform": {"scale": scale or [1, 1, 1]}, "MeshRenderer": {"mesh": mesh, "color": color + [1]}, "NetPuppet": {}}
    if solid:
        comps["Collider"] = {"shape": 1, "size": [1, 1, 1]}  # a box collider is scaled by the entity
    comps.update(extra or {})
    prefabs[name] = {"components": comps}


model("Mate", [0.2, 0.5, 0.95], scale=[0.7, 1.8, 0.7])
model("CorpseModel", [0.4, 0.1, 0.1], scale=[1.8, 0.4, 0.7])
model("DoorLeaf", [0.4, 0.28, 0.18], scale=[1.1, 2.1, 0.12], solid=True, extra={"Door": {"hinge": [-0.55, 0, 0], "angle_open": -100, "speed": 200}})
model("CrateModel", [0.55, 0.4, 0.2], solid=True)
model("PickupModel", [0.95, 0.85, 0.2], mesh="sphere", scale=[0.4, 0.4, 0.4])
model("DroneModel", [0.85, 0.15, 0.15], mesh="sphere", scale=[0.9, 0.9, 0.9], extra={"MeshRenderer": {"mesh": "sphere", "color": [0.85, 0.15, 0.15, 1], "emissive": 0.4}})
model("FoundationModel", [0.6, 0.6, 0.62], scale=[4, 0.5, 4], solid=True)
model("WallModel", [0.7, 0.62, 0.5], scale=[4, 3, 0.2], solid=True)
model("HeliModel", [0.3, 0.35, 0.4], scale=[2.4, 2.0, 4.5], solid=True)
model("CarModel", [0.7, 0.15, 0.12], scale=[1.8, 0.7, 4.2], solid=True)
model("CrawlerModel", [0.12, 0.12, 0.14], scale=[1.1, 0.5, 1.4], solid=True)
# the beam: the lighthouse's puppet turns about the vertical with the real one, the tilt and the length are the model's own
prefabs["BeamModel"] = {"components": {"Name": {"value": "BeamModel"}, "Transform": {}, "NetPuppet": {}},
                        "children": [{"components": {"Name": {"value": "Beam"},
                                                     "LocalTransform": {"pos": [0, round(-19 * math.sin(TILT), 3), round(-19 * math.cos(TILT), 3)], "rot": tilt_rot, "scale": [0.6, 0.6, 38]},
                                                     "MeshRenderer": {"mesh": "cube", "color": [1.0, 0.95, 0.6, 1], "emissive": 1.0, "cast_shadow": False}}}]}

# ---------------------------------------------------------------- the player: the first-person body of examples/first_person with the network in it
kinds = "3 30 DoorLeaf\n3 31 CrateModel\n3 9 CorpseModel\n3 1 FoundationModel\n3 2 WallModel\n1 32 PickupModel\n4 40 DroneModel\n4 60 HeliModel\n4 61 CarModel\n4 42 CrawlerModel\n4 70 BeamModel"
acts = "1 build:foundation\n2 build:wall\n3 place\n4 inv_drop\n5 inv_move\n6 inv_use"
spawn(40, "Player", Hero={}, Transform={"pos": [0, 0, 20]}, CharacterBody={"radius": 0.35, "height": 1.8}, Inventory={"size": 12}, Heat={},
      NetPlayer={"puppet": "Mate", "kinds": kinds, "acts": acts, "camera": 41, "eye": [0, 1.62, 0]})
spawn(41, "Eyes", Parent={"target": 40}, LocalTransform={"pos": [0, 1.62, 0]}, Transform={"pos": [0, 1.62, 20]}, Camera={"fov_y": 72}, MouseLook={"sensitivity": 0.0022})
spawn(42, "Input", InputMap={"bindings": "move_x = d, a:-1, gamepad.lx\nmove_y = w, s:-1, gamepad.ly\njump = space, gamepad.a\nfire = mouse_left, gamepad.rt\nuse = e, gamepad.x\n"
                                         "crouch = control\nsprint = shift\nbuild1 = 1\nbuild2 = 2\nplace = f\ndrop = g\nhack = h\nreload = r\nuse_item = u\nnext_gun = q\nroll_l = z\nroll_r = c\ndeadzone = 0.15"})
spawn(43, "Open container", NetContainer={}, Inventory={"size": 8})
spawn(44, "Crosshair", Text={"value": "+", "pos": [0.5, 0.47], "size": 0.05, "align": 1, "color": [1, 1, 1, 0.85], "shadow": True})
spawn(45, "Hint", Text={"value": "WASD walk  mouse look  click shoot  R reload  Q next gun in the bag  E use (doors, crates, the helicopter, the car)  Ctrl crouch  1 2 choose foundation or wall, F build  G drop what is in the hand  helicopter: W S pitch, A D turn, Space Ctrl up down, Z C roll  car: W S A D, Ctrl brake, Space handbrake  H hold on a drone: hack it, then WASD Space Ctrl fly it, click fires, R lets go  Esc quit", "pos": [0.5, 0.95], "size": 0.026, "align": 1,
                        "color": [1, 1, 1, 0.8], "shadow": True})

# ---------------------------------------------------------------- the HUD: what the server tells the player about itself (`NetPlayer.health` ... `reserve`, docs/GAME_SERVER.md) in `Ui` bars and a counter
HUD_PARTS = ["HudHealth", "HudFood", "HudWater", "HudStamina", "HudHeat", "HudAmmo", "HudBleeding"]
BAR = {"size": [0.18, 0.016], "back_color": [0.03, 0.04, 0.05, 0.7], "border": 0.002, "border_color": [0.5, 0.9, 1.0, 0.55], "layer": 5}
for k, (part, label, color, y) in enumerate([("HudHealth", "health", [0.85, 0.25, 0.22, 0.95], 0.835), ("HudFood", "food", [0.95, 0.6, 0.2, 0.95], 0.862),
                                                ("HudWater", "water", [0.3, 0.6, 0.95, 0.95], 0.889), ("HudStamina", "stamina", [0.9, 0.85, 0.35, 0.95], 0.916)]):
    spawn(300 + k, f"HUD {label}", **{part: {}}, Ui={"pos": [0.02, y], "color": color, "fill": 1.0, **BAR})
spawn(304, "HUD AI attention", HudHeat={}, Ui={"pos": [0.80, 0.04], "size": [0.18, 0.016], "color": [1.0, 0.7, 0.15, 0.95], "back_color": [0.03, 0.04, 0.05, 0.7], "fill": 0.0, "border": 0.002, "border_color": [0.5, 0.9, 1.0, 0.55], "layer": 5})
spawn(305, "HUD ammunition", HudAmmo={}, Ui={"pos": [0.84, 0.90], "size": [0.14, 0.06], "color": [0.03, 0.04, 0.05, 0.6], "border": 0.002, "border_color": [0.5, 0.9, 1.0, 0.55], "text": "{hud.mag} / {hud.reserve}", "text_size": 0.04, "text_color": [0.9, 0.97, 1.0, 1], "layer": 5, "visible": False})
spawn(306, "HUD bleeding", HudBleeding={}, Ui={"pos": [0.205, 0.835], "size": [0.06, 0.03], "color": [0.8, 0.1, 0.1, 0.9], "text": "BLEEDING", "text_size": 0.014, "text_color": [1, 1, 1, 1], "layer": 5, "visible": False})
spawn(307, "HUD label", Text={"value": "AI ATTENTION", "pos": [0.89, 0.025], "size": 0.014, "align": 1, "color": [0.6, 0.9, 1.0, 0.8]})

systems = list(fp["systems"])
systems.append({"name": "net_look", "doc": "What the server needs of the look: the pitch.", "each": ["MouseLook"], "order": 11, "do": [{"res_set": "look.pitch", "to": {"get": "MouseLook.pitch"}}], "enabled": True})
systems.append({
    "name": "net_buttons", "doc": "The buttons and the build keys, as the network says them.", "each": ["NetPlayer"], "order": 12,
    "do": [
        {"set": "NetPlayer.pitch", "to": {"res": "look.pitch"}},
        {"set": "NetPlayer.flags", "to": {"add": [{"add": [{"res": "action.fire"}, {"mul": [{"res": "action.sprint"}, 4]}]}, {"add": [{"add": [{"mul": [{"res": "action.use"}, 8]}, {"mul": [{"res": "action.crouch"}, 16]}]}, {"add": [{"mul": [{"res": "action.hack"}, 32]}, {"mul": [{"res": "action.reload"}, 2]}]}]}]}},
        {"if": {"gt": [{"res": "action.build1.pressed"}, 0.5]}, "then": [{"set": "NetPlayer.act", "to": 1}]},
        {"if": {"gt": [{"res": "action.build2.pressed"}, 0.5]}, "then": [{"set": "NetPlayer.act", "to": 2}]},
        {"if": {"gt": [{"res": "action.place.pressed"}, 0.5]}, "then": [{"set": "NetPlayer.act", "to": 3}]},
        # (drop what is in the hand: the slot the player says it holds, `NetPlayer.extra`)
        # (U uses what is in the hand: eat, drink, bandage; the server decides what that does, NetHost.uses)
        {"if": {"gt": [{"res": "action.use_item.pressed"}, 0.5]}, "then": [{"set": "NetPlayer.cmd_args[0]", "to": {"get": "NetPlayer.extra"}}, {"set": "NetPlayer.act", "to": 6}]},
        {"if": {"gt": [{"res": "action.drop.pressed"}, 0.5]}, "then": [{"set": "NetPlayer.cmd_args[0]", "to": {"get": "NetPlayer.extra"}}, {"set": "NetPlayer.cmd_args[1]", "to": 1}, {"set": "NetPlayer.act", "to": 4}]},
    ],
    "enabled": True,
})

# the hand: `NetPlayer.extra` is the slot of the bag that is in the hand (the server's `Loadout` makes it the gun there, docs/LOADOUT.md); Q takes the next slot of the bag that holds a gun (items GUN_FIRST to GUN_LAST).
# The bag the server tells the player is the `Inventory` of the player, so the system reads the slots; each of the 12 is written out (a field index is a constant).
def gun_in(slot):
    item = {"get": f"Inventory.id[{slot}]"}
    return {"and": [{"ge": [item, GUN_FIRST]}, {"le": [item, GUN_LAST]}]}


steps_to_next = {"min": [{"select": [gun_in(c), {"mod": [{"sub": [c + 23, {"var": "held"}]}, 12]}, 99]} for c in range(12)]}
systems.append({
    "name": "net_hand", "doc": "Q: the hand takes the next gun of the bag.", "each": ["NetPlayer", "Inventory"], "order": 14,
    "do": [
        {"let": "held", "be": {"clamp": [{"get": "NetPlayer.extra"}, 0, 11]}},
        {"let": "steps", "be": steps_to_next},
        {"if": {"and": [{"gt": [{"res": "action.next_gun.pressed"}, 0.5]}, {"lt": [{"var": "steps"}, 99]}]},
         "then": [{"set": "NetPlayer.extra", "to": {"mod": [{"add": [{"var": "held"}, {"var": "steps"}, 1]}, 12]}}]},
    ],
    "enabled": True,
})

# in the helicopter the keys are the stick: W/S pitch, A/D the pedals, Space/Ctrl the collective, Z/C roll (the walking is overridden by the engine while the player rides); with a machine taken by the goggles
# (H held on it) W/S are forward and back, A/D left and right, Space/Ctrl up and down, in the way the player looks, and R gives it back
riding = {"min": [{"add": [{"get": "NetPlayer.riding"}, {"get": "NetPlayer.viewing"}]}, 1]}


def hand(expr):
    return {"mul": [expr, riding]}


def diff(a, b):
    return {"add": [{"res": a}, {"mul": [{"res": b}, -1]}]}


systems.append({
    "name": "net_axes", "doc": "The keys as the helicopter's stick while the player rides (the engine says so in NetPlayer.riding).", "each": ["NetPlayer"], "order": 13,
    "do": [
        {"set": "NetPlayer.axes[0]", "to": hand(diff("action.roll_r", "action.roll_l"))},
        {"set": "NetPlayer.axes[1]", "to": hand(diff("action.jump", "action.crouch"))},
        {"set": "NetPlayer.axes[2]", "to": hand({"res": "action.move_y"})},
        {"set": "NetPlayer.axes[3]", "to": hand({"res": "action.move_x"})},
    ],
    "enabled": True,
})

# the HUD: the player's own numbers into resources, and each part of the HUD reads its own
systems.append({
    "name": "net_hud_feed", "doc": "What the server told the player about itself, as resources for the HUD.", "each": ["NetPlayer"], "order": 15,
    "do": [{"res_set": f"hud.{k}", "to": {"get": f"NetPlayer.{k}"}} for k in ("health", "food", "water", "stamina", "heat", "mag", "gun")]
          + [{"res_set": "hud.reserve", "to": {"select": [{"ge": [{"get": "NetPlayer.reserve"}, 65535]}, 999, {"get": "NetPlayer.reserve"}]}},
             {"res_set": "hud.bleeding", "to": {"select": [{"get": "NetPlayer.bleeding"}, 1, 0]}}],
    "enabled": True,
})
for part, resource, scale in (("HudHealth", "hud.health", 1), ("HudFood", "hud.food", 1), ("HudWater", "hud.water", 1), ("HudStamina", "hud.stamina", 1), ("HudHeat", "hud.heat", 0.1)):
    systems.append({"name": "hud_" + part[3:].lower(), "each": [part, "Ui"], "order": 16, "do": [{"set": "Ui.fill", "to": {"mul": [{"res": resource}, scale]}}], "enabled": True})
systems.append({"name": "hud_ammo", "each": ["HudAmmo", "Ui"], "order": 16, "do": [{"set": "Ui.visible", "to": {"gt": [{"res": "hud.gun"}, 0.5]}}], "enabled": True})
systems.append({"name": "hud_bleeding", "each": ["HudBleeding", "Ui"], "order": 16, "do": [{"set": "Ui.visible", "to": {"gt": [{"res": "hud.bleeding"}, 0.5]}}], "enabled": True})

scene = {
    "lunaris": 1,
    "components": fp["components"] + [{"name": n, "doc": "marks a part of the HUD (`net_hud` systems)", "fields": []} for n in HUD_PARTS],
    "next_uid": 400,
    "entities": entities,
    "resources": {"cursor.lock": 1, "look.fz": -1, "look.rx": 1, "look.pitch": 0},
    "systems": systems,
    "prefabs": prefabs,
}
with open(OUT, "w", encoding="utf-8", newline="\n") as f:
    json.dump(scene, f, indent=1)
    f.write("\n")
print(f"wrote {OUT}: {len(entities)} entities, {len(prefabs)} prefabs, {len(systems)} systems")
