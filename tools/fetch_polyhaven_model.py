"""Download CC0 glTF models from Poly Haven (api.polyhaven.com) into examples/gfx_lab/models/<id>/ (the .gltf, its .bin and its textures, at 1k), for the renderer's showcase pictures.
    python -I tools/fetch_polyhaven_model.py id [id ...]      (defaults to the showcase set: two young conifers, rocks and a dead trunk)
Every model is licensed CC0 by Poly Haven; the ids are written to examples/gfx_lab/models/CREDITS.md. The files are not committed (about 70 MB): run this to get them. Poly Haven's models are made
for offline renderers (a sapling is 400,000 triangles): they are meant for pictures, not for a game's trees."""
import json
import os
import sys
import urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "gfx_lab", "models"))
DEFAULT = ["pine_sapling_small", "fir_sapling", "rock_07", "rock_09", "boulder_01", "dead_tree_trunk_02"]


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "Lunaris-asset-fetch/1.0"})
    return urllib.request.urlopen(req, timeout=120).read()


def main():
    ids = sys.argv[1:] or DEFAULT
    os.makedirs(ROOT, exist_ok=True)
    credits = []
    for mid in ids:
        files = json.loads(get(f"https://api.polyhaven.com/files/{mid}"))
        node = files["gltf"]["1k"]["gltf"]
        out = os.path.join(ROOT, mid)
        os.makedirs(out, exist_ok=True)
        todo = [(os.path.basename(node["url"]), node["url"])] + [(rel, v["url"]) for rel, v in node["include"].items()]
        for rel, url in todo:
            dest = os.path.join(out, rel)
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            if not os.path.exists(dest):
                with open(dest, "wb") as f:
                    f.write(get(url))
        info = json.loads(get(f"https://api.polyhaven.com/info/{mid}"))
        authors = ", ".join(f"{a} ({r})" for a, r in (info.get("authors") or {}).items())
        credits.append(f"* {info.get('name', mid)}: https://polyhaven.com/a/{mid}, CC0, {authors}")
        print(mid, "ok", sum(os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(out) for f in fs) // 1000, "kB")
    with open(os.path.join(ROOT, "CREDITS.md"), "w", encoding="utf-8") as f:
        f.write("# Models of the showcase pictures\n\nAll from Poly Haven (https://polyhaven.com), licensed CC0 (public domain).\n\n" + "\n".join(credits) + "\n")


main()
