"""Contact sheets that compare the free datasets at their own resolution (what each really shows), from the samples in realmap-data\\quality:

    python tools/realmap/quality_sheet.py

  imagery.png   the same size of ground (64 m) cut from each aerial photo at its native pixels (nearest neighbour: no smoothing, you see the real resolution)
  surface.png   hillshades of the bare-earth terrain and of the surface model (buildings and trees stand in it) at 0.5 m or 1 m
"""
import os

import numpy as np
from PIL import Image, ImageDraw

Image.MAX_IMAGE_PIXELS = None
Q = r"realmap-data\quality"
OUT = r"realmap-data\quality\sheets"
os.makedirs(OUT, exist_ok=True)


def label(img, text):
    d = ImageDraw.Draw(img)
    d.rectangle([0, 0, img.width, 22], fill=(0, 0, 0))
    d.text((6, 4), text, fill=(255, 255, 255))
    return img


def crop_m(im, gsd, cx_px, cy_px, metres, show=640):
    half = int(metres / gsd / 2)
    c = im.crop((cx_px - half, cy_px - half, cx_px + half, cy_px + half)).convert("RGB")
    return c.resize((show, show), Image.NEAREST), c.size[0]


def imagery():
    tiles = []
    # Stowe: NAIP mosaic 0.5 m (the village centre = the window centre)
    st = os.path.join(r"realmap-data\stowe")
    big = Image.new("RGB", (6000, 6000))
    for j in range(3):
        for i in range(3):
            big.paste(Image.open(os.path.join(st, f"naip_{i}_{j}.jpg")).convert("RGB"), (i * 2000, (2 - j) * 2000))
    img, px = crop_m(big, 0.5, 3000, 3000, 64)
    tiles.append(label(img, f"USA NAIP, Stowe VT: 0.5 m a pixel ({px} px for 64 m)"))
    # Zurich swissIMAGE 10 cm
    z = Image.open(os.path.join(Q, "ch", "swissimage-dop10_2025_2683-1247_0.1_2056.tif"))
    img, px = crop_m(z, 0.1, 5000, 5000, 64)
    tiles.append(label(img, f"Switzerland swissIMAGE, Zurich: 0.1 m ({px} px for 64 m)"))
    # Utrecht PDOK 8 cm: nine tiles of 2500 px (200 m); the centre tile
    u = Image.open(os.path.join(Q, "nl", "aerial_1_1.jpg"))
    img, px = crop_m(u, 200 / 2500, 1250, 1250, 64)
    tiles.append(label(img, f"Netherlands PDOK, Utrecht: 0.08 m ({px} px for 64 m)"))
    # Wellington 7.5 cm
    n = Image.open(os.path.join(Q, "nz", "BQ31_1000_2036.tiff"))
    img, px = crop_m(n, 0.075, 1500, 3200, 64)
    tiles.append(label(img, f"New Zealand LINZ, Wellington: 0.075 m ({px} px for 64 m)"))
    sheet = Image.new("RGB", (1280, 1280))
    for k, t in enumerate(tiles):
        sheet.paste(t, ((k % 2) * 640, (k // 2) * 640))
    sheet.save(os.path.join(OUT, "imagery.png"))
    print("imagery.png")


def hillshade(z, cell, az=315, alt=40):
    gy, gx = np.gradient(z.astype(np.float64), cell)
    slope = np.pi / 2 - np.arctan(np.hypot(gx, gy))
    aspect = np.arctan2(-gx, gy)
    a, l = np.radians(360 - az + 90), np.radians(alt)
    hs = np.sin(l) * np.sin(slope) + np.cos(l) * np.cos(slope) * np.cos(a - aspect)
    return np.clip(hs, 0, 1)


def read_f(path):
    im = Image.open(path)
    a = np.array(im, dtype=np.float32)
    a[a < -500] = np.nan
    a[a > 9000] = np.nan
    return a


def surface():
    panels = []

    def panel(arr, cell, name, size=640, metres=100):
        n = int(metres / cell)
        h, w = arr.shape
        c = arr[h // 2 - n // 2 : h // 2 + n // 2, w // 2 - n // 2 : w // 2 + n // 2]
        c = np.where(np.isnan(c), np.nanmean(c), c)
        hs = (hillshade(c, cell) * 255).astype(np.uint8)
        img = Image.fromarray(hs).convert("RGB").resize((size, size), Image.NEAREST)
        panels.append(label(img, name))

    # Stowe 3DEP 1 m bare earth (centre tile)
    st = np.array(Image.open(r"realmap-data\stowe\dem_1_1.tif"), dtype=np.float32)
    panel(st, 1.0, "USA 3DEP bare earth 1 m, Stowe (100 m square)")
    # Zurich swissALTI3D (DTM) and swissSURFACE3D (DSM) 0.5 m
    ch = Q + r"\ch"
    dtm = read_f(ch + r"\swissalti3d_2026_2683-1247_0.5_2056_5728.tif")
    dsm = read_f(ch + r"\swisssurface3d-raster_2024_2683-1247_0.5_2056_5728.tif")
    panel(dtm, 0.5, "Switzerland swissALTI3D terrain 0.5 m, Zurich")
    panel(dsm, 0.5, "Switzerland swissSURFACE3D surface 0.5 m, Zurich")
    # Utrecht AHN
    nl = Q + r"\nl"
    panel(read_f(nl + r"\dtm_05m.tif"), 0.5, "Netherlands AHN terrain 0.5 m, Utrecht")
    panel(read_f(nl + r"\dsm_05m.tif"), 0.5, "Netherlands AHN surface 0.5 m, Utrecht")
    sheet = Image.new("RGB", (1920, 1280))
    for k, t in enumerate(panels):
        sheet.paste(t, ((k % 3) * 640, (k // 3) * 640))
    sheet.save(os.path.join(OUT, "surface.png"))
    print("surface.png")


if __name__ == "__main__":
    imagery()
    surface()
