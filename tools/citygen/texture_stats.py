r"""Average colours of the photographed materials: city/textures/materials.json gets, for every set, `avg` (linear light red, green, blue) so that the generator can colour distant buildings like the walls they have.

    python tools/citygen/texture_stats.py citygen-data/city
"""
import json
import os
import sys

from PIL import Image


def srgb_to_linear(c):
    return ((c + 0.055) / 1.055) ** 2.4 if c > 0.04045 else c / 12.92


def main():
    root = os.path.join(sys.argv[1], "textures")
    path = os.path.join(root, "materials.json")
    meta = json.load(open(path))
    for slug, info in meta.items():
        f = os.path.join(root, slug, "diff.jpg")
        if not os.path.exists(f):
            continue
        im = Image.open(f).convert("RGB").resize((16, 16), Image.BOX)
        px = list(im.getdata())
        rgb = [sum(p[k] for p in px) / len(px) / 255.0 for k in range(3)]
        info["avg"] = [round(srgb_to_linear(c), 4) for c in rgb]
    json.dump(meta, open(path, "w"), indent=1)
    print(len(meta), "sets")


if __name__ == "__main__":
    main()
