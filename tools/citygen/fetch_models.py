r"""Real models for the procedural city's streets: CC0 photographed props and trees from Poly Haven (https://polyhaven.com, public domain), 1k pictures.

    python tools/citygen/fetch_models.py OUT_FOLDER [SLUG ...]            (no slugs: the list below)

For every slug OUT/city/props/<slug>/<slug>.gltf with its .bin and textures/. The pictures the glTF names are Poly Haven's OpenGL normal maps (`nor_gl`); the engine reads DirectX ones, so the
`nor_dx` picture is downloaded in their place and the glTF made to name it. Nothing is made by hand: they are scanned or sculpted assets someone else released to the public domain.
"""
import json
import os
import sys
import urllib.request

SLUGS = [
    # street furniture
    "street_lamp_01", "street_lamp_02", "fire_hydrant", "utility_box_01", "utility_box_02", "metal_trash_can", "water_manhole_cover", "security_light", "exterior_aircon_unit",
    "standing_chalkboard_01", "planter_box_01", "planter_box_02", "concrete_road_barrier", "rollershutter_door", "modular_street_seating", "outdoor_table_chair_set_01", "modular_fire_escape",
    "modular_electricity_poles",
    # furniture, fittings and decoration for the interiors
    "dining_chair_02", "modern_arm_chair_01", "modern_coffee_table_01", "modern_coffee_table_02", "modern_wooden_cabinet", "modern_ceiling_lamp_01", "sofa_02", "sofa_03", "sofa_01", "mid_century_lounge_chair",
    "side_table_01", "side_table_tall_01", "small_wooden_table_01", "round_wooden_table_01", "round_wooden_table_02", "wooden_table_02", "dining_table", "steel_frame_shelves_01", "steel_frame_shelves_02",
    "steel_frame_shelves_03", "Shelf_01", "painted_wooden_shelves", "painted_wooden_nightstand", "drawer_cabinet", "metal_office_desk", "SchoolDesk_01", "SchoolChair_01", "Television_01", "television_02",
    "electric_stove", "potted_plant_01", "potted_plant_02", "potted_plant_04", "calathea_orbifolia_01", "pachira_aquatica_01", "wall_clock", "hanging_picture_frame_01", "hanging_picture_frame_02",
    "hanging_picture_frame_03", "standing_picture_frame_01", "decorative_book_set_01", "book_encyclopedia_set_01", "ceramic_vase_01", "ceramic_vase_02", "tea_set_01", "wicker_basket_01", "classic_laptop",
    "desk_lamp_arm_01", "wooden_bookshelf_worn", "wooden_display_shelves_01", "CashRegister_01", "CoffeeCart_01", "vintage_day_bed", "old_bed_frame", "GothicBed_01", "ClassicNightstand_01", "ArmChair_01",
    "WoodenChair_01", "painted_wooden_chair_01", "painted_wooden_table", "bar_chair_round_01", "metal_stool_02", "plastic_monobloc_chair_01", "painted_wooden_cabinet", "vintage_cabinet_01",
    "hanging_industrial_lamp", "caged_hanging_light", "mounted_fluorescent_lights", "projector_screen", "security_camera_01", "ornate_mirror_01", "wine_bottles_01", "long_life_food", "plastic_crate_01",
    "cardboard_box_01", "hand_truck", "tool_cart", "wheelchair_01", "fire_alarm", "ceiling_fan", "vintage_microwave", "vintage_electric_kettle",
    # a street that has been left alone, and a park
    "covered_car", "korean_public_payphone_01", "wooden_picnic_table", "painted_wooden_bench", "trashbag", "compost_bags", "Barrel_01", "barrel_03", "wooden_crate_02", "old_tyre", "WetFloorSign_01",
    "planter_box_03", "power_box_01", "portable_generator", "concrete_road_barrier_02", "modular_chainlink_fence", "metal_jerrycan", "propane_tank",
    # trees and plants
    "jacaranda_tree", "island_tree_01", "island_tree_02", "island_tree_03", "tree_small_02", "fir_tree_01", "pine_tree_01", "shrub_01", "shrub_02", "shrub_03", "shrub_04",
]


def get(url):
    return json.load(urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "lunaris-citygen"}), timeout=60))


def fetch(url, dst):
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    if os.path.exists(dst):
        return
    req = urllib.request.Request(url, headers={"User-Agent": "lunaris-citygen"})
    with urllib.request.urlopen(req, timeout=180) as r, open(dst + ".part", "wb") as f:
        f.write(r.read())
    os.replace(dst + ".part", dst)


def main():
    out = sys.argv[1]
    slugs = sys.argv[2:] or SLUGS
    for slug in slugs:
        d = os.path.join(out, "city", "props", slug)
        try:
            files = get(f"https://api.polyhaven.com/files/{slug}")
            one = files["gltf"]["1k"]["gltf"]
        except Exception as e:  # noqa: BLE001
            print(slug, "skipped:", repr(e)[:80], flush=True)
            continue
        text = urllib.request.urlopen(urllib.request.Request(one["url"], headers={"User-Agent": "lunaris-citygen"}), timeout=60).read().decode("utf-8")
        try:
            for rel, info in one["include"].items():
                if "nor_gl" in rel and "nor_dx" in files:
                    dx = rel.replace("nor_gl", "nor_dx")
                    fetch(files["nor_dx"]["1k"]["jpg"]["url"], os.path.join(d, dx))
                    text = text.replace(rel, dx)
                else:
                    fetch(info["url"], os.path.join(d, rel))
        except Exception as e:  # noqa: BLE001
            print(slug, "failed:", repr(e)[:100], flush=True)
            continue
        open(os.path.join(d, slug + ".gltf"), "w", encoding="utf-8").write(text)
        print(slug, "ok", flush=True)


if __name__ == "__main__":
    main()
