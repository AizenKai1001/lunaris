"""Real-world data for one place, from open sources only (no Google, no Bing):

    python tools/realmap/fetch.py NAME LAT LON [SIZE_M=3000]

  * elevation: USGS 3DEP bare-earth DEM (public domain), 1 m where lidar exists, as a 32-bit float GeoTIFF in UTM metres
  * imagery:   USGS NAIP orthophoto (public domain), 4 bands, tiled in squares of NAIP_TILE_M metres
  * vectors:   OpenStreetMap through its official API (ODbL: the credit "(c) OpenStreetMap contributors" has to be shown wherever the map is)

Everything goes into realmap-data\\NAME\\ and a meta.json says where the window is (UTM zone, the south-west corner, the size). This is a feasibility tool for the real-world map
game: the product version belongs in the engine's own geo crate.
"""
import json
import math
import os
import sys
import time
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.dirname(__file__))
import geo

ROOT = r"realmap-data"
UA = "lunaris-realmap/0.1 (game-engine feasibility; contact: owner of the Lunaris project)"
DEM_URL = "https://elevation.nationalmap.gov/arcgis/rest/services/3DEPElevation/ImageServer/exportImage"
NAIP_URL = "https://imagery.nationalmap.gov/arcgis/rest/services/USGSNAIPImagery/ImageServer/exportImage"
OVERPASS = "https://overpass-api.de/api/interpreter"
NAIP_TILE_M = 1000
NAIP_PX = 2000  # 0.5 m a pixel (the sensor's own is 0.6 m)


def get(url, params=None, data=None, retries=3):
    if params:
        url = url + "?" + urllib.parse.urlencode(params)
    for k in range(retries):
        try:
            req = urllib.request.Request(url, data=data, headers={"User-Agent": UA, "Accept": "*/*"})
            with urllib.request.urlopen(req, timeout=180) as r:
                return r.read()
        except Exception as e:  # noqa: BLE001
            print("  retry", k, e)
            time.sleep(3 + 4 * k)
    raise RuntimeError("download failed: " + url[:120])


def main():
    name, lat, lon = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
    size = float(sys.argv[4]) if len(sys.argv) > 4 else 3000.0
    out = os.path.join(ROOT, name)
    os.makedirs(out, exist_ok=True)
    zone = geo.zone_of(lon)
    epsg = geo.epsg_of(lat, lon)
    ce, cn = geo.forward(lat, lon, zone)
    # a window aligned to 100 m so that tiles of neighbouring windows line up
    x0 = math.floor((ce - size / 2) / 100.0) * 100.0
    y0 = math.floor((cn - size / 2) / 100.0) * 100.0
    meta = {"name": name, "lat": lat, "lon": lon, "zone": zone, "epsg": epsg, "x0": x0, "y0": y0, "size_m": size, "sources": {"dem": "USGS 3DEP (public domain)", "imagery": "USGS NAIP (public domain)", "vector": "OpenStreetMap contributors (ODbL)"}}
    # ---- elevation: tiles of 1 km at one metre a pixel (the service refuses big requests)
    dem_tiles = []
    n = int(math.ceil(size / NAIP_TILE_M))
    for j in range(n):
        for i in range(n):
            tx, ty = x0 + i * NAIP_TILE_M, y0 + j * NAIP_TILE_M
            path = os.path.join(out, f"dem_{i}_{j}.tif")
            dem_tiles.append({"i": i, "j": j, "x0": tx, "y0": ty, "size_m": NAIP_TILE_M, "px": NAIP_TILE_M, "file": os.path.basename(path)})
            if os.path.exists(path):
                continue
            print("elevation tile", i, j)
            b = get(DEM_URL, {"bbox": f"{tx},{ty},{tx + NAIP_TILE_M},{ty + NAIP_TILE_M}", "bboxSR": epsg, "imageSR": epsg, "size": f"{NAIP_TILE_M},{NAIP_TILE_M}", "format": "tiff", "pixelType": "F32", "f": "image"})
            open(path, "wb").write(b)
            print("  ", len(b) // 1024, "KB")
    meta["dem_tiles"] = dem_tiles
    # ---- imagery in tiles
    tiles = []
    n = int(math.ceil(size / NAIP_TILE_M))
    for j in range(n):
        for i in range(n):
            tx, ty = x0 + i * NAIP_TILE_M, y0 + j * NAIP_TILE_M
            path = os.path.join(out, f"naip_{i}_{j}.jpg")
            tiles.append({"i": i, "j": j, "x0": tx, "y0": ty, "size_m": NAIP_TILE_M, "file": os.path.basename(path)})
            if os.path.exists(path):
                continue
            print("imagery tile", i, j)
            b = get(NAIP_URL, {"bbox": f"{tx},{ty},{tx + NAIP_TILE_M},{ty + NAIP_TILE_M}", "bboxSR": epsg, "imageSR": epsg, "size": f"{NAIP_PX},{NAIP_PX}", "format": "jpg", "bandIds": "0,1,2", "interpolation": "RSP_BilinearInterpolation", "f": "image"})
            open(path, "wb").write(b)
            print("  ", len(b) // 1024, "KB")
    meta["imagery_tiles"] = tiles
    # ---- OpenStreetMap through the official API (the public Overpass servers were refusing us: expired certificates), in boxes of 0.0075 degrees: the API takes at most 50,000 nodes a request
    corners = [geo.inverse(x0 + dx, y0 + dy, zone, lat >= 0) for dx in (-50, size + 50) for dy in (-50, size + 50)]
    s_, w_ = min(c[0] for c in corners), min(c[1] for c in corners)
    n_, e_ = max(c[0] for c in corners), max(c[1] for c in corners)
    step = 0.0075
    osm_files = []
    ni, nj = int(math.ceil((e_ - w_) / step)), int(math.ceil((n_ - s_) / step))
    for j in range(nj):
        for i in range(ni):
            bw, bs = w_ + i * step, s_ + j * step
            path = os.path.join(out, f"osm_{i}_{j}.xml")
            osm_files.append(os.path.basename(path))
            if os.path.exists(path):
                continue
            print("openstreetmap", i, j)
            b = get("https://api.openstreetmap.org/api/0.6/map", {"bbox": f"{bw:.6f},{bs:.6f},{min(bw + step, e_):.6f},{min(bs + step, n_):.6f}"})
            open(path, "wb").write(b)
            print("  ", len(b) // 1024, "KB")
            time.sleep(1.5)
    meta["osm_files"] = osm_files
    json.dump(meta, open(os.path.join(out, "meta.json"), "w"), indent=1)
    print("done:", out)


if __name__ == "__main__":
    main()
