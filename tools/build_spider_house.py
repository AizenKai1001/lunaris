"""Builds examples/spider_house.lunaris.json: a furnished house the spiders live in, with food in places they have to travel, climb or web their way to.
Run: python tools/build_spider_house.py [brain-file-name]"""
import json, math, sys

brain = sys.argv[1] if len(sys.argv) > 1 else "spider_brain_house.json"
SIZE = float(sys.argv[2]) if len(sys.argv) > 2 else 0.12   # how big the spiders are against the animal they were trained as
E = []
uid = [1000]


def add(name, comps, u=None):
    if u is None:
        u = uid[0]
        uid[0] += 1
    c = {"Name": {"value": name}}
    c.update(comps)
    E.append({"uid": u, "components": c})
    return u


def box(name, cx, cy, cz, sx, sy, sz, colour, visible=True, rough=0.85):
    comps = {"Transform": {"pos": [cx, cy, cz], "scale": [sx, sy, sz]}, "Collider": {"shape": 1, "size": [1, 1, 1]}}
    comps["MeshRenderer"] = {"mesh": "cube", "color": colour, "roughness": rough, "visible": visible}
    return add(name, comps)


add("Sky", {"Environment": {"sky": True, "sky_top": [0.14, 0.3, 0.6], "sky_horizon": [0.72, 0.8, 0.92], "ground": [0.2, 0.2, 0.22], "clouds": 0.25, "fog": 0.002, "fog_color": [0.7, 0.76, 0.86], "bloom": 0.25, "ssao": 1.0, "shadow_distance": 40}})
add("Sun", {"Transform": {"rot": [-0.55, -0.3, -0.25, 0.74]}, "Light": {"kind": 0, "color": [1.0, 0.95, 0.85], "intensity": 3.4}})
add("Floor", {"Transform": {"pos": [0, 0, 0], "scale": [13.0, 1, 9.0]}, "MeshRenderer": {"mesh": "plane", "color": [0.46, 0.33, 0.22, 1], "roughness": 0.7}})
wall = [0.78, 0.74, 0.66, 1]
# the outer walls (the south one is a cutaway: it is solid but not drawn) and a dividing wall with a doorway
box("North wall", 0, 1.5, -4.1, 12.4, 3.0, 0.2, wall)
box("South wall", 0, 1.5, 4.1, 12.4, 3.0, 0.2, wall, visible=False)
box("West wall", -6.1, 1.5, 0, 0.2, 3.0, 8.4, wall)
box("East wall", 6.1, 1.5, 0, 0.2, 3.0, 8.4, wall)
box("Divider A", 0, 1.5, -2.35, 0.2, 3.0, 3.3, wall)
box("Divider B", 0, 1.5, 2.35, 0.2, 3.0, 3.3, wall)
wood = [0.42, 0.27, 0.15, 1]
# table (left room)
box("Table top", -3.0, 0.75, -1.0, 1.6, 0.08, 0.9, wood)
for dx in (-0.7, 0.7):
    for dz in (-0.38, 0.38):
        box("Table leg", -3.0 + dx, 0.375, -1.0 + dz, 0.08, 0.75, 0.08, wood)
# bookcase on the north wall of the left room: sides and three shelves
box("Bookcase side", -5.25, 0.95, -3.75, 0.05, 1.9, 0.35, wood)
box("Bookcase side", -3.35, 0.95, -3.75, 0.05, 1.9, 0.35, wood)
for y in (0.6, 1.2, 1.8):
    box("Shelf", -4.3, y, -3.75, 1.9, 0.05, 0.35, wood)
# sofa and cabinet (right room), a high ledge on the east wall
box("Sofa", 3.2, 0.25, -2.2, 2.0, 0.5, 0.9, [0.3, 0.38, 0.5, 1])
box("Cabinet", 3.6, 0.8, 3.3, 1.2, 1.6, 0.6, wood)
box("Ledge", 5.7, 2.3, 0.0, 0.4, 0.1, 2.0, wood)
# two piles of cushions in the open (heightfield mounds made by target/patch_house_terrain_hills.py's recipe: a flat top and 40 and 45 degree sides, in examples/house_hill.png and house_mound.png): the spiders can walk up from any side, with food on top
add("Cushion hill", {"Transform": {"pos": [-1.8, 0.0, 1.0]}, "Terrain": {"heightmap": "house_hill.png", "size": [1.2, 1.2], "height": 0.30, "color": [0.65, 0.3, 0.27, 1], "friction": 1.2}})
add("Cushion mound", {"Transform": {"pos": [2.3, 0.0, 1.2]}, "Terrain": {"heightmap": "house_mound.png", "size": [1.6, 1.6], "height": 0.45, "color": [0.3, 0.5, 0.36, 1], "friction": 1.2}})
# the nest: a rug in the left room's corner, where the spiders live
add("Nest rug", {"Transform": {"pos": [-4.9, 0.006, 2.9], "scale": [1.8, 0.012, 1.5]}, "MeshRenderer": {"mesh": "cube", "color": [0.2, 0.12, 0.1, 1], "roughness": 1.0}})

H = 0.27 * SIZE   # the body of a spider stands about 0.27 of its size above what it walks on: that is where food goes
spots = [
    # the floor of both rooms
    (-4.5, H, -2.0), (-2.0, H, 2.4), (-5.2, H, 0.6), (2.6, H, -3.0), (4.8, H, 1.2), (1.8, H, 3.1),
    # on the furniture
    (-3.0, 0.79 + H, -1.0), (-3.4, 0.79 + H, -1.2), (3.2, 0.5 + H, -2.2),
    # on top of the cushions
    (-1.8, 0.30 + H, 1.0), (-1.75, 0.30 + H, 1.05), (2.3, 0.45 + H, 1.2), (2.35, 0.45 + H, 1.15),
    (-4.3, 0.625 + H, -3.7), (-4.3, 1.225 + H, -3.7), (-4.3, 1.825 + H, -3.7),
    (3.6, 1.6 + H, 3.3), (5.55, 2.35 + H, 0.0),
    # on the walls
    (-5.93, 1.4, -0.5), (-5.93, 2.2, 1.6), (5.93, 1.5, -2.5),
]
for i, p in enumerate(spots):
    add(f"Food spot {i}", {"Transform": {"pos": list(p)}, "FoodSpot": {}})
foods = [(-4.5, H, -2.0), (-3.0, 0.79 + H, -1.0), (-4.3, 1.225 + H, -3.7), (3.6, 1.6 + H, 3.3), (-1.8, 0.30 + H, 1.0), (2.3, 0.45 + H, 1.2)]
for i, p in enumerate(foods):
    add(f"Food {i}", {"Transform": {"pos": list(p), "scale": [0.3 * SIZE, 0.3 * SIZE, 0.3 * SIZE]}, "MeshRenderer": {"mesh": "sphere", "color": [1.0, 0.55, 0.15, 1], "emissive": 2.0}, "Food": {}}, u=100 + i)
starts = [(-5.0, 0.0, 2.5), (-4.5, 0.0, 3.2), (-4.0, 0.0, 2.6)]
for k, p in enumerate(starts):
    yaw = 0.6 * k
    add(f"Spider {k}", {"Transform": {"pos": list(p), "rot": [0, math.sin(yaw / 2), 0, math.cos(yaw / 2)]}, "Creature": {"brain": brain, "genome": k, "body_seed": 3 + 5 * k, "forage": True, "size": SIZE}}, u=20 + k)
add("Camera", {"Transform": {"pos": [-4.5, 0.6, 3.5], "rot": [-0.1, 0.0, 0.0, 0.99]}, "Camera": {"fov_y": 58, "far": 300, "clear": [0.05, 0.06, 0.09, 1]}, "CameraRig": {"target": 20, "mode": 0, "offset": [-0.5 * SIZE / 0.25, 0.45 * SIZE / 0.25 + 0.12, 0.9 * SIZE / 0.25], "look_at": True, "look_offset": [0.0, 0.05 * SIZE / 0.25, 0.0], "follow_speed": 3.0}}, u=900)
add("Title", {"Text": {"value": "the spiders' house: they go and get the food", "pos": [0.02, 0.03], "size": 0.035, "color": [1, 1, 1, 1], "shadow": True}})
json.dump({"lunaris": 1, "entities": E}, open(r"examples\spider_house.lunaris.json", "w"), indent=1)
print(len(E), "entities")
