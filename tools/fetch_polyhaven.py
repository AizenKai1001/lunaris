"""Download CC0 PBR texture sets from Poly Haven (api.polyhaven.com) into examples/hollowlight/textures/<id>/: diffuse, DirectX normal (what Unreal reads), ARM (ambient occlusion, roughness, metallic packed in R, G, B) and displacement, at 2K, as jpg/png.
    python -I tools/fetch_polyhaven.py id [id ...]      (defaults to the Hollowlight set)
Every set is licensed CC0 by Poly Haven; the ids and sources are written to examples/hollowlight/CREDITS.md. The pictures are not committed (about 120 MB): run this to get them."""
import json
import os
import sys
import urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "hollowlight"))
DEFAULT = ["decrepit_wallpaper", "yellow_plaster", "dirty_carpet", "ceiling_interior", "white_plaster_02", "garage_floor", "hangar_concrete_floor", "concrete_wall_003", "smooth_concrete_floor",
           "long_white_tiles", "interior_tiles", "old_linoleum_flooring_01", "marble_01", "painted_concrete", "rebar_reinforced_concrete"]
WANT = {"Diffuse": "diff", "nor_dx": "nor_dx", "arm": "arm", "Displacement": "disp"}


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "Hollowlight-asset-fetch/1.0"})
    return urllib.request.urlopen(req, timeout=60).read()


def main():
    ids = sys.argv[1:] or DEFAULT
    credits = []
    for tid in ids:
        files = json.loads(get(f"https://api.polyhaven.com/files/{tid}"))
        out = os.path.join(ROOT, "textures", tid)
        os.makedirs(out, exist_ok=True)
        got = []
        for api, short in WANT.items():
            node = (files.get(api) or {}).get("2k") or {}
            pick = node.get("jpg") or node.get("png")
            if not pick:
                continue
            ext = "jpg" if "jpg" in node else "png"
            path = os.path.join(out, f"{tid}_{short}.{ext}")
            if not os.path.exists(path):
                open(path, "wb").write(get(pick["url"]))
            got.append(f"{short}:{os.path.getsize(path) // 1024}KB")
        print(tid, " ".join(got), flush=True)
        credits.append(f"- {tid}: https://polyhaven.com/a/{tid} (CC0)")
    with open(os.path.join(ROOT, "CREDITS.md"), "a", encoding="utf-8") as f:
        f.write("\n".join(credits) + "\n")


if __name__ == "__main__":
    main()
