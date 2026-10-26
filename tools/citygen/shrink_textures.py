"""Make the normal and roughness pictures of the photographed material sets smaller (the colour picture stays at 1024): they carry no detail the eye can tell at half the size, and the engine holds every picture it uses
in memory at 4 bytes a texel (a city meets all of its sets sooner or later).

    python tools/citygen/shrink_textures.py citygen-data/city/textures [SIDE]      (SIDE default 512)
"""
import os
import sys

from PIL import Image


def main():
    root = sys.argv[1]
    side = int(sys.argv[2]) if len(sys.argv) > 2 else 512
    n = 0
    before = after = 0
    for slug in sorted(os.listdir(root)):
        d = os.path.join(root, slug)
        if not os.path.isdir(d) or slug.startswith("_"):
            continue
        for name in ("nor.jpg", "arm.jpg"):
            p = os.path.join(d, name)
            if not os.path.exists(p):
                continue
            before += os.path.getsize(p)
            with Image.open(p) as im:
                if max(im.size) > side:
                    out = im.convert("RGB").resize((side, side), Image.LANCZOS)
                    out.save(p, quality=92)
                    n += 1
            after += os.path.getsize(p)
    print(f"{n} pictures halved: {before / 1048576:.0f} MB -> {after / 1048576:.0f} MB on disk")


if __name__ == "__main__":
    main()
