r"""The real material pictures of the procedural city: CC0 photographed PBR sets from Poly Haven (https://polyhaven.com, public domain, no attribution needed).

    python tools/citygen/fetch_textures.py OUT_FOLDER [SLUG ...]            (no slugs: the whole list below)

For every slug: OUT/textures/<slug>/diff.jpg, nor.jpg (DirectX normals: green down, what the engine reads) and arm.jpg (occlusion, roughness, metallic), 1k each, and OUT/textures/materials.json with the real-world size
in metres that one repeat of the pictures covers (Poly Haven's `dimensions`), which the generator uses to lay them on walls at their true scale. Nothing is drawn or made by hand: the pictures are photographs.
"""
import json
import os
import sys
import urllib.request

SLUGS = [
    # concrete and plaster walls
    "concrete_wall_007", "concrete_slab_wall_02", "concrete_panels", "rough_concrete", "concrete_tile_facade", "patterned_concrete_wall", "exterior_wall_cladding_03", "concrete_wall_003",
    "grey_plaster_03", "beige_wall_001", "plastered_wall_05", "painted_plaster_wall", "plaster_grey_04", "yellow_plaster_02",
    # brick
    "brick_wall_001", "brown_brick_02", "large_red_bricks", "brick_wall_003", "yellow_brick", "painted_brick", "dark_brick_wall", "brick_wall_09", "church_bricks_02",
    # metal cladding, shutters, doors
    "box_profile_metal_sheet", "painted_metal_shutter", "metal_plate_02", "factory_wall", "rusty_metal_02",
    # roofs
    "bitumen", "grey_roof_01", "grey_roof_tiles", "clay_roof_tiles_03", "roof_slates_03",
    # interiors: floors and walls
    "laminate_floor_02", "laminate_floor_03", "wood_floor", "herringbone_parquet", "floor_tiles_08", "floor_tiles_06", "terrazzo_tiles", "rubber_tiles", "marble_01", "linoleum_brown",
    "white_plaster_02", "plastered_wall_04", "white_stucco", "wood_planks_grey",
    # ground: roads, pavements, kerbs
    "asphalt_06", "asphalt_04", "road_damaged_clean", "clean_asphalt", "concrete_pavement_02", "concrete_tiles_02", "pavement_03", "concrete_floor_03", "patterned_paving", "concrete_pavers",
    # parks: lawn, paths, a playground
    "leafy_grass", "sparse_grass", "grass_ground", "park_dirt", "playground_sand", "stony_dirt_path", "gravel_floor_02",
]
MAPS = [("Diffuse", "diff"), ("nor_dx", "nor"), ("arm", "arm")]


def get(url):
    return json.load(urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "lunaris-citygen"}), timeout=60))


def main():
    out = sys.argv[1]
    slugs = sys.argv[2:] or SLUGS
    root = os.path.join(out, "textures")
    os.makedirs(root, exist_ok=True)
    meta_path = os.path.join(root, "materials.json")
    meta = json.load(open(meta_path)) if os.path.exists(meta_path) else {}
    for slug in slugs:
        d = os.path.join(root, slug)
        os.makedirs(d, exist_ok=True)
        files = get(f"https://api.polyhaven.com/files/{slug}")
        if not all(key in files for key, _ in MAPS):
            print(slug, "skipped: it has", sorted(files)[:8], flush=True)
            continue
        for key, name in MAPS:
            dst = os.path.join(d, name + ".jpg")
            if os.path.exists(dst):
                continue
            url = files[key]["1k"]["jpg"]["url"]
            req = urllib.request.Request(url, headers={"User-Agent": "lunaris-citygen"})
            with urllib.request.urlopen(req, timeout=120) as r, open(dst + ".part", "wb") as f:
                f.write(r.read())
            os.replace(dst + ".part", dst)
        info = get(f"https://api.polyhaven.com/info/{slug}")
        dim = info.get("dimensions") or [2000, 2000]
        meta[slug] = {"size_m": [round(dim[0] / 1000.0, 3), round(dim[1] / 1000.0, 3)], "tags": info.get("tags", [])[:8], "authors": list((info.get("authors") or {}).keys())}
        print(slug, meta[slug]["size_m"], flush=True)
        json.dump(meta, open(meta_path, "w"), indent=1)


if __name__ == "__main__":
    main()
