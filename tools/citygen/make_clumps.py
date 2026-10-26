"""Foliage clumps: a picture of a dense bunch of the model's own leaves, to be laid on big cards instead of the million little ones.

    python tools/citygen/make_clumps.py citygen-data/city/props [SLUG ...]      (no slugs: the trees and shrubs)

A Poly Haven tree has a leaf atlas (a few leaves or twigs, each cut out by its alpha) and a million cards that each show one leaf. The simplifier cannot keep a crown of that with a few thousand triangles, so
`props::clump_cards` replaces the cards by a few hundred big ones, and this makes the picture they wear: the sprites of the atlas (the connected pieces of its alpha) scattered, turned and scaled on a 512 x 512 card,
a darker layer below for depth, the edge fading out so that the card is a bush and not a square. The picture is stored next to the atlas as `<name>_clump.png`; a second run changes nothing.
"""
import os
import random
import sys
import zlib

import numpy as np
from PIL import Image, ImageEnhance
from scipy import ndimage

TREES = ("tree", "pine", "fir_", "jacaranda", "shrub")


def sprites(rgba):
    a = np.array(rgba)[:, :, 3] > 128
    labels, n = ndimage.label(a)
    out = []
    for i, sl in enumerate(ndimage.find_objects(labels), start=1):
        if sl is None:
            continue
        h, w = sl[0].stop - sl[0].start, sl[1].stop - sl[1].start
        if h * w < 400:
            continue
        crop = rgba.crop((sl[1].start, sl[0].start, sl[1].stop, sl[0].stop)).copy()
        # only this component's pixels stay opaque (neighbouring leaves in the box are cut away)
        mask = Image.fromarray(((labels[sl] == i) * 255).astype(np.uint8))
        alpha = Image.fromarray(np.minimum(np.array(crop)[:, :, 3], np.array(mask)))
        crop.putalpha(alpha)
        out.append(crop)
    # an atlas may also hold bark and cones (brown): the clump is made of the green sprites only
    def greenish(im):
        a = np.array(im).astype(np.float32)
        m = a[:, :, 3] > 128
        return a[:, :, 1][m].mean() >= 0.95 * a[:, :, 0][m].mean()

    green = [s for s in out if greenish(s)]
    return green or out


def clump(rgba, rng, size=512):
    sp = sprites(rgba)
    if not sp:
        return None
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    # sprite scale: the largest sprite should be about a seventh of the card
    big = max(max(s.size) for s in sp)
    base = size / 7.0 / big
    for layer, (count, dark) in enumerate([(120, 0.55), (150, 0.85), (110, 1.0)]):
        for _ in range(count):
            s = rng.choice(sp)
            k = base * rng.uniform(0.7, 1.35) * (1.0 + 0.2 * layer)
            im = s.resize((max(2, int(s.size[0] * k)), max(2, int(s.size[1] * k))), Image.LANCZOS)
            im = im.rotate(rng.uniform(0, 360), expand=True, resample=Image.BICUBIC)
            if dark < 1.0:
                im = ImageEnhance.Brightness(im).enhance(dark)
            else:
                im = ImageEnhance.Brightness(im).enhance(rng.uniform(0.9, 1.12))
            # more leaves in the middle than at the edge
            while True:
                x, y = rng.uniform(0, size), rng.uniform(0, size)
                r = (((x - size / 2) / (size / 2)) ** 2 + ((y - size / 2) / (size / 2)) ** 2) ** 0.5
                if rng.random() < max(0.0, 1.15 - r):
                    break
            paste_clipped(canvas, im, int(x - im.size[0] / 2), int(y - im.size[1] / 2))
    # the edge fades: a card is a bush
    arr = np.array(canvas).astype(np.float32)
    yy, xx = np.mgrid[0:size, 0:size]
    r = np.sqrt(((xx - size / 2) / (size / 2)) ** 2 + ((yy - size / 2) / (size / 2)) ** 2)
    fade = np.clip((1.0 - r) / 0.28, 0.0, 1.0)
    arr[:, :, 3] = arr[:, :, 3] * fade
    # real leaves are dark (a tenth of the light); a crown in a game has no light bouncing through it, so the picture is brighter
    arr[:, :, :3] = np.minimum(arr[:, :, :3] * np.array([1.9 * 0.8, 1.9, 1.9 * 0.82], dtype=np.float32), 255.0)
    # the colour under transparent pixels is the colour of the leaves around it, so that mip levels do not darken the edge
    return Image.fromarray(arr.astype(np.uint8), "RGBA")


def paste_clipped(canvas, im, x, y):
    w, h = canvas.size
    x0, y0 = max(0, x), max(0, y)
    x1, y1 = min(w, x + im.size[0]), min(h, y + im.size[1])
    if x1 <= x0 or y1 <= y0:
        return
    canvas.alpha_composite(im.crop((x0 - x, y0 - y, x1 - x, y1 - y)), (x0, y0))


def main():
    root = sys.argv[1]
    slugs = sys.argv[2:] or sorted(d for d in os.listdir(root) if os.path.isdir(os.path.join(root, d)) and any(t in d for t in TREES))
    for slug in slugs:
        tex = os.path.join(root, slug, "textures")
        if not os.path.isdir(tex):
            continue
        for f in sorted(os.listdir(tex)):
            if not f.endswith("_diff_1k.png"):
                continue
            out = os.path.join(tex, f.replace("_diff_1k.png", "_clump.png"))
            if os.path.exists(out):
                continue
            rgba = Image.open(os.path.join(tex, f)).convert("RGBA")
            if np.array(rgba)[:, :, 3].min() > 250:
                continue
            c = clump(rgba, random.Random(zlib.crc32(slug.encode())), 512)
            if c is None:
                continue
            c.save(out, optimize=True)
            print(slug, f, "->", os.path.basename(out))


if __name__ == "__main__":
    main()
