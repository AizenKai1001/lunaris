"""The Lunaris scene of a real place (see build.py and build_lidar.py):

    python tools/realmap/scene.py NAME [FOCUS_X FOCUS_Z TREE_RADIUS]

Paths in the scene are relative to a workspace root in which the folder `NAME/` holds the files of `realmap-data\\NAME\\scene\\` and `models/` the tree models.
Trees are the measured tree tops of the lidar survey (position, height, crown) drawn with a model of the engine's library scaled to the measured size; only those within
TREE_RADIUS metres of the focus are placed (a scene of every tree of a county is the job of the engine's streaming and GPU scatter, not of a list of entities).
"""
import json
import math
import os
import sys

ROOT = r"realmap-data"
TREE_MODELS = [("models/pine_sapling_small/pine_sapling_small_lunaris.gltf", 2.7, 1.2), ("models/fir_sapling/fir_sapling_lunaris.gltf", 2.6, 1.1)]


def quat(yaw, pitch):
    sy, cy = math.sin(math.radians(yaw) / 2), math.cos(math.radians(yaw) / 2)
    sp, cp = math.sin(math.radians(pitch) / 2), math.cos(math.radians(pitch) / 2)
    return [cy * sp, sy * cp, -sy * sp, cy * cp]


def sky_and_camera(add, fx, fz):
    """Sun, sky, atmosphere, exposure, bounced light and the camera: the same look for every real-place scene (and the streamed world of world.py). Returns the camera's uid."""
    add({"Environment": {"sky": True, "clouds": 0.3, "shadows": True, "shadow_distance": 220, "ssao": 1.0, "bloom": 0.08, "vignette": 0.12, "tonemap": 1, "ambient": float(os.environ.get("AMBIENT", "1.4")), "ground": [0.5, 0.48, 0.42]}})
    add({"Light": {"kind": 0, "intensity": float(os.environ.get("SUN", "2.6")), "color": [1.0, 0.93, 0.82]}, "Transform": {"rot": quat(215, -38)}})
    add({"Atmosphere": {"turbidity": 2.4, "intensity": 1.0}})
    add({"Shadows": {"softness": 0.02}})
    add({"AutoExposure": {"min": 0.3, "max": 2.5, "speed": 3.0, "target": float(os.environ.get("EXPOSURE", "0.26"))}})
    if os.environ.get("GI", "1") != "0":
        add({"GlobalIllumination": {"intensity": 1.0, "radius": 10.0}})
    return add({"Camera": {"fov_y": 50, "active": True, "far": 8000.0}, "Transform": {"pos": [fx, 60, fz + 120]}})


def main():
    name = sys.argv[1]
    fx = float(sys.argv[2]) if len(sys.argv) > 2 else 0.0
    fz = float(sys.argv[3]) if len(sys.argv) > 3 else 0.0
    radius = float(sys.argv[4]) if len(sys.argv) > 4 else 250.0
    d = os.path.join(ROOT, name, "scene")
    tint = float(os.environ.get("TINT", "0.42"))
    info = json.load(open(os.path.join(d, "info.json")))
    size = float(info["size"])
    relief = float(info["relief"])
    ents = []
    uid = [0]

    def add(comps):
        uid[0] += 1
        ents.append({"uid": uid[0], "components": comps})
        return uid[0]

    sky_and_camera(add, fx, fz)
    tiles = info.get("tiles")
    if tiles:
        layers = os.environ.get("LAYERS")
        for t in tiles:
            comps = {"Name": {"value": f"Ground {t['i']},{t['j']}"}, "Transform": {"pos": [t["cx"], 0, t["cz"]]}, "Terrain": {"heightmap": f"{name}/{t['heightmap']}", "size": [t["size"], t["size"]], "height": relief, "step": 2, "color": [tint, tint, tint, 1], "texture": f"{name}/{t['texture']}", "uv_scale": [1, 1], "friction": 0.9}}
            if layers and t.get("splat"):
                # fine detail under the photograph's colour: real PBR ground photographs (Poly Haven, CC0), chosen by what the photo and the slope say the ground is
                lines = ["grass textures/sparse_grass/sparse_grass_diff.jpg uv_scale=450 tint=0.9,1.0,0.8", "rock textures/rock_face_03/rock_face_03_diff.jpg uv_scale=260", "forest textures/grass_ground/grass_ground_diff.jpg uv_scale=380 tint=0.7,0.85,0.6", "bare textures/coast_sand_01/coast_sand_01_diff.jpg uv_scale=320 tint=0.9,0.85,0.8"]
                comps["TerrainLayers"] = {"splat": f"{name}/{t['splat']}", "layers": chr(10).join(lines)}
                comps["Terrain"]["color"] = [1, 1, 1, 1]
            tid = add(comps)
            only = os.environ.get("GRASS_ONLY")
            if layers and os.environ.get("GRASS") and (not only or only == f"{t['i']},{t['j']}"):
                add({"Name": {"value": f"Grass {t['i']},{t['j']}"}, "GrassField": {"terrain": tid, "radius": 90, "density": 20, "mask_layer": 1, "height": [0.10, 0.34], "seed": 1.0 + 3 * t["i"] + 7 * t["j"]}})
    else:
        add({"Name": {"value": "Ground"}, "Transform": {"pos": [0, 0, 0]}, "Terrain": {"heightmap": f"{name}/heightmap.png", "size": [size, size], "height": relief, "step": 2, "color": [tint, tint, tint, 1], "texture": f"{name}/terrain.jpg", "uv_scale": [1, 1], "friction": 0.9}})
    add({"Name": {"value": "Water"}, "Transform": {"pos": [0, 0, 0]}, "WaterBody": {"heightmap": f"{name}/water.png", "size": [size, size], "relief": relief, "step": 2, "color": [0.05, 0.22, 0.28, 0.85], "wave_height": 0.05, "wave_length": 3.0}})
    for bfile in ("buildings.gltf", "lidar_buildings.glb"):
        if os.path.exists(os.path.join(d, bfile)):
            add({"Name": {"value": "Buildings (real roofs)"}, "Transform": {"pos": [0, 0, 0]}, "MeshRenderer": {"mesh": f"{name}/{bfile}", "roughness": 0.85}})
            break
    if not os.environ.get("NO_ROADS"):
        add({"Name": {"value": "Roads"}, "Transform": {"pos": [0, 0, 0]}, "MeshRenderer": {"mesh": f"{name}/roads.glb", "roughness": 0.9, "cast_shadow": False}})
    trees = json.load(open(os.path.join(d, "trees.json"))) if os.path.exists(os.path.join(d, "trees.json")) else []
    placed = 0
    for i, t in enumerate(trees):
        if math.hypot(t["x"] - fx, t["z"] - fz) > radius:
            continue
        model, mh, mr = TREE_MODELS[(i * 7 + int(abs(t["x"]) * 3)) % 2]
        sy = t["h"] / mh
        sxz = max(0.6, min(8.0, t["r"] / mr * 1.0))
        yaw = (i * 137.5) % 360
        add({"Transform": {"pos": [t["x"], t["y"] - 0.1, t["z"]], "rot": [0, math.sin(math.radians(yaw) / 2), 0, math.cos(math.radians(yaw) / 2)], "scale": [sxz, sy, sxz]}, "MeshRenderer": {"mesh": model, "roughness": 0.8}})
        placed += 1
    print(f"scene: {len(ents)} entities, {placed} of {len(trees)} measured trees")
    json.dump({"lunaris": 1, "entities": ents}, open(os.path.join(d, f"{name}.lunaris.json"), "w"))


if __name__ == "__main__":
    main()
