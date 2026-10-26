r"""The aerial orthophotographs of PLATEAU Tokyo (23 wards, 2020), downloaded sheet by sheet and kept in a form that is a tenth of the size.

    python plateau_ortho_fetch.py OUT_FOLDER [SHEET ...]        SHEET: 533925 .. 533957 (default: all fourteen)

The archive (https://www.geospatial.jp/ckan/dataset/plateau-tokyo23ku, "GeoTIFF") holds one uncompressed GeoTIFF of 1.26 GB per sheet (0.5 m a pixel, JGD2011 longitude/latitude). Each sheet is fetched alone, rewritten as a tiled
JPEG-compressed GeoTIFF (quality 95, in strips so that memory stays small) as OUT_FOLDER/<sheet>.tif, and the original deleted at once: the fourteen sheets take about two gigabytes instead of eighteen.
plateau_ortho.py reads them like the originals. The data is (c) MLIT Japan, Project PLATEAU (processed here: recompressed).
"""
import os
import sys
import time

import rasterio
from rasterio.windows import Window
from remotezip import RemoteZip

URL = "https://gsic-opendata.s3.ap-northeast-1.amazonaws.com/national-gov/mlit/city-bureau/3d-city-model/2020/plateau-tokyo23ku/ortho/13100_tokyo23-ku_2020_ortho_2_op.zip"
SHEETS = ["533925", "533926", "533934", "533935", "533936", "533937", "533944", "533945", "533946", "533947", "533954", "533955", "533956", "533957"]
STRIP = 2048


def convert(src, dst):
    with rasterio.open(src) as ds:
        profile = ds.profile.copy()
        profile.update(driver="GTiff", compress="jpeg", photometric="ycbcr", jpeg_quality=95, tiled=True, blockxsize=512, blockysize=512, count=3)
        profile.pop("nodata", None)
        with rasterio.open(dst + ".part", "w", **profile) as out:
            for r in range(0, ds.height, STRIP):
                w = Window(0, r, ds.width, min(STRIP, ds.height - r))
                out.write(ds.read([1, 2, 3], window=w), window=w)
    os.replace(dst + ".part", dst)


def main():
    out = sys.argv[1]
    sheets = sys.argv[2:] or SHEETS
    os.makedirs(out, exist_ok=True)
    tmp = os.path.join(out, "_raw")
    os.makedirs(tmp, exist_ok=True)
    for s in sheets:
        dst = os.path.join(out, f"{s}.tif")
        if os.path.exists(dst):
            continue
        t0 = time.time()
        member = f"13100_tokyo23-ku_2020_ortho_2_op/images/{s}.tif"
        with RemoteZip(URL) as z:
            z.extract(member, tmp)
        raw = os.path.join(tmp, member)
        print(s, "downloaded", f"{time.time() - t0:.0f} s", flush=True)
        convert(raw, dst)
        os.remove(raw)
        print(s, "done", os.path.getsize(dst) // 1000000, "MB", f"{time.time() - t0:.0f} s", flush=True)


if __name__ == "__main__":
    main()
