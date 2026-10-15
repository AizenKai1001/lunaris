"""Make the little JPEG pictures the texture tests use, and print what Pillow reads at a few pixels (the tests compare the engine's decoder with it).
Needs Pillow (a Linux machine has it): python3 tools/make_test_jpegs.py tmp_jpg
"""
import os
import sys

from PIL import Image, ImageDraw

out = sys.argv[1] if len(sys.argv) > 1 else "tmp_jpg"
os.makedirs(out, exist_ok=True)
img = Image.new("RGB", (64, 48), (200, 30, 30))
d = ImageDraw.Draw(img)
d.rectangle([32, 0, 63, 47], fill=(30, 30, 200))
d.rectangle([0, 20, 63, 27], fill=(30, 200, 40))
d.ellipse([10, 4, 22, 16], fill=(250, 240, 30))
img.save(os.path.join(out, "jpg_color.jpg"), quality=92, subsampling=2)
img.convert("L").save(os.path.join(out, "jpg_gray.jpg"), quality=92)
img.save(os.path.join(out, "jpg_prog.jpg"), quality=90, progressive=True, subsampling=0)
points = [(4, 4), (16, 10), (50, 5), (8, 23), (50, 24), (4, 40), (50, 40), (33, 40)]
for name in ("jpg_color.jpg", "jpg_gray.jpg", "jpg_prog.jpg"):
    im = Image.open(os.path.join(out, name)).convert("RGB")
    print(name, [im.getpixel(p) for p in points])
