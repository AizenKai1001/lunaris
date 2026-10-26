"""A still scene of converted PLATEAU tiles (plateau_tiles.py) for looking at the data: the full-detail tiles within RADIUS metres of a point, a flat ground, the usual sky and sun.

    python3 tokyo_scene.py WORKSPACE_ROOT TILES_FOLDER(s, relative to the root, comma separated) OUT_SCENE [RADIUS [X Z]]

Tiles are the leaves of the tile tree. Frame as plateau_tiles.py: x east, y up, z south, the origin at the Shibuya Scramble crossing unless the converter was given another.
"""
import json
import math
import os
import sys


def quat(yaw, pitch):
    sy, cy = math.sin(math.radians(yaw) / 2), math.cos(math.radians(yaw) / 2)
    sp, cp = math.sin(math.radians(pitch) / 2), math.cos(math.radians(pitch) / 2)
    return [cy * sp, sy * cp, -sy * sp, cy * cp]


def main():
    root, folder, out = sys.argv[1], sys.argv[2], sys.argv[3]
    radius = float(sys.argv[4]) if len(sys.argv) > 4 else 1200.0
    fx = float(sys.argv[5]) if len(sys.argv) > 5 else 0.0
    fz = float(sys.argv[6]) if len(sys.argv) > 6 else 0.0
    folders = folder.split(",")
    indexes = [(f, json.load(open(os.path.join(root, f, "index.json")))) for f in folders]
    ents = []

    def add(c):
        ents.append({"uid": len(ents) + 1, "components": c})
        return len(ents)

    add({"Environment": {"sky": True, "clouds": 0.25, "shadows": True, "shadow_distance": 400, "ssao": 1.0, "bloom": 0.08, "vignette": 0.1, "tonemap": 1, "ambient": 1.4, "ground": [0.45, 0.45, 0.43]}})
    add({"Light": {"kind": 0, "intensity": 2.6, "color": [1.0, 0.95, 0.86]}, "Transform": {"rot": quat(215, -42)}})
    add({"Atmosphere": {"turbidity": 3.0, "intensity": 1.0}})
    add({"Shadows": {"softness": 0.02}})
    add({"AutoExposure": {"min": 0.3, "max": 2.5, "speed": 3.0, "target": 0.26}})
    add({"GlobalIllumination": {"intensity": 1.0, "radius": 10.0}})
    add({"Camera": {"fov_y": 55, "active": True, "far": 6000.0}, "Transform": {"pos": [fx, 40, fz + 80]}})
    # a flat ground a little under the lowest building bottoms of the tiles near the point (the terrain comes with the DEM)
    n = 0
    tris = 0
    low = 1e9
    for folder, index in indexes:
        for t in index["tiles"]:
            if not t["leaf"]:
                continue
            d = math.hypot(t["centre"][0] - fx, t["centre"][2] - fz)
            if d > radius + t["radius"]:
                continue
            add({"Transform": {"pos": [0, 0, 0]}, "MeshRenderer": {"mesh": f"{folder}/{t['file']}", "roughness": 0.85}})
            n += 1
            tris += t["triangles"]
            if folder.endswith("bldg"):
                low = min(low, t["min"][1])
    add({"Transform": {"pos": [fx, low - 0.5, fz], "scale": [radius * 3, 1, radius * 3]}, "MeshRenderer": {"mesh": "plane", "color": [0.28, 0.28, 0.27, 1], "roughness": 0.95}})
    json.dump({"lunaris": 1, "entities": ents}, open(out, "w"), separators=(",", ":"))
    print(f"{n} tiles, {tris} triangles, ground at y={low - 0.5:.1f} -> {out}")


main()
