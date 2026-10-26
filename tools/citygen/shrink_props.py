"""Make the pictures of the prop models smaller, in place: a piece of furniture does not need 1024 x 1024 texels for each of its three maps (the engine holds every picture in memory as 4 bytes a texel, and one furnished
floor uses a hundred of them).

    python tools/citygen/shrink_props.py citygen-data/city/props [MAX_SIDE] [SMALL_SIDE]

Pictures larger than MAX_SIDE (default 512) are scaled down to it (SMALL_SIDE, default 256, for the props named in SMALL: little things seen close only when held). JPEGs are saved again at quality 90, PNGs as they are.
The pictures stay where they are with the same names, so the glTF files need no change. Run it once after tools/citygen/fetch_models.py (it fetches the 1k pictures); a second run changes nothing.
"""
import os
import sys

from PIL import Image

SMALL = ("frame", "clock", "mirror", "lamp", "light", "tea_set", "bottle", "vase", "kettle", "laptop", "basket", "book", "camera", "alarm", "register", "microwave", "television", "stool", "chair", "crate", "box", "cart", "can")


def main():
    root = sys.argv[1]
    big = int(sys.argv[2]) if len(sys.argv) > 2 else 512
    small = int(sys.argv[3]) if len(sys.argv) > 3 else 256
    changed = 0
    before = after = 0
    for slug in sorted(os.listdir(root)):
        d = os.path.join(root, slug)
        if not os.path.isdir(d):
            continue
        limit = small if any(k in slug.lower() for k in SMALL) else big
        for dirpath, _, files in os.walk(d):
            for f in files:
                if not f.lower().endswith((".jpg", ".jpeg", ".png")):
                    continue
                p = os.path.join(dirpath, f)
                size = os.path.getsize(p)
                before += size
                with Image.open(p) as im:
                    w, h = im.size
                    if max(w, h) <= limit:
                        after += size
                        continue
                    k = limit / max(w, h)
                    out = im.convert("RGBA" if im.mode in ("RGBA", "LA", "P") else "RGB").resize((max(1, round(w * k)), max(1, round(h * k))), Image.LANCZOS)
                if f.lower().endswith(".png"):
                    out.save(p, optimize=True)
                else:
                    out.convert("RGB").save(p, quality=90)
                after += os.path.getsize(p)
                changed += 1
    print(f"{changed} pictures made smaller; {before / 1048576:.0f} MB -> {after / 1048576:.0f} MB on disk")


if __name__ == "__main__":
    main()
