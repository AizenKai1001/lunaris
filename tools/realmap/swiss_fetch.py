"""The best open data there is, Switzerland (swisstopo, free for any use with the credit "Federal Office of Topography swisstopo"): for a window of 2 x 2 km round a place

    python tools/realmap/swiss_fetch.py NAME LAT LON

  swissALTI3D        bare-earth terrain, 0.5 m                (1 km tiles, ~16 MB each)
  swisssurface3d     surface model (buildings, trees), 0.5 m   (1 km tiles, ~15 MB each)
  swissIMAGE         aerial photo, 10 cm                       (1 km tiles, ~35 MB each)
  swissBUILDINGS3D   3D buildings with real roofs (CityGML)    (map sheets, ~7 MB each)
  OpenStreetMap      roads, water, paths (official API, ODbL; the Swiss official vector map swissTLM3D is a 2.5 GB download for the whole country)

Files go to realmap-data\\NAME\\raw\\. The local frame is the Swiss grid LV95 (E, N metres); `meta.json` names the window.
"""
import json
import math
import os
import sys
import time
import urllib.request

ROOT = r"realmap-data"
UA = {"User-Agent": "lunaris-realmap/0.1 (game-engine feasibility)"}
STAC = "https://data.geo.admin.ch/api/stac/v0.9/collections"


def lv95(lat, lon):
    """swisstopo's approximate WGS84 -> LV95 formulas (about a metre)."""
    p = (lat * 3600 - 169028.66) / 10000
    l = (lon * 3600 - 26782.5) / 10000
    e = 2600072.37 + 211455.93 * l - 10938.51 * l * p - 0.36 * l * p * p - 44.54 * l**3
    n = 1200147.07 + 308807.95 * p + 3745.25 * l * l + 76.63 * p * p - 194.56 * l * l * p + 119.79 * p**3
    return e, n


def wgs84(e, n):
    """swisstopo's approximate LV95 -> WGS84 formulas (the inverse of `lv95`, about a metre): (lat, lon)."""
    y, x = (e - 2600000) / 1e6, (n - 1200000) / 1e6
    lon = 2.6779094 + 4.728982 * y + 0.791484 * y * x + 0.1306 * y * x * x - 0.0436 * y**3
    lat = 16.9023892 + 3.238272 * x - 0.270978 * y * y - 0.002528 * x * x - 0.0447 * y * y * x - 0.0140 * x**3
    return lat * 100 / 36, lon * 100 / 36


def get(url, t=120):
    for k in range(4):
        try:
            return urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=t).read()
        except Exception as e:  # noqa: BLE001
            print("  retry", k, str(e)[:80])
            time.sleep(2 + 3 * k)
    raise RuntimeError(url)


def download(url, path):
    if os.path.exists(path) and os.path.getsize(path) > 1000:
        return
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=300) as r, open(path + ".part", "wb") as f:
        while True:
            b = r.read(1 << 20)
            if not b:
                break
            f.write(b)
    os.replace(path + ".part", path)
    print("  ", os.path.basename(path), os.path.getsize(path) // 1024, "KB", flush=True)


def main():
    name, lat, lon = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
    e, n = lv95(lat, lon)
    # a window of 2 x 2 km aligned to whole kilometres: the 4 tiles round the nearest grid corner
    ce, cn = round(e / 1000) * 1000, round(n / 1000) * 1000
    x0, y0 = ce - 1000, cn - 1000
    out = os.path.join(ROOT, name, "raw")
    os.makedirs(out, exist_ok=True)
    print(f"{name}: LV95 E {e:.0f} N {n:.0f}; window E {x0}..{x0 + 2000}, N {y0}..{y0 + 2000}")
    # the tile keys of the window (a tile is named by its south-west corner in km)
    keys = [(x0 // 1000 + i, y0 // 1000 + j) for i in range(2) for j in range(2)]
    dlat, dlon = 0.0135, 0.0200
    bbox = f"{lon - dlon:.5f},{lat - dlat:.5f},{lon + dlon:.5f},{lat + dlat:.5f}"
    files = {"dtm": [], "dsm": [], "image": [], "buildings": []}
    spec = [("dtm", "ch.swisstopo.swissalti3d", "_0.5_2056_5728.tif"), ("dsm", "ch.swisstopo.swisssurface3d-raster", "_0.5_2056_5728.tif"), ("image", "ch.swisstopo.swissimage-dop10", "_0.1_2056.tif")]
    for kind, cid, suffix in spec:
        items = json.loads(get(f"{STAC}/{cid}/items?bbox={bbox}&limit=100"))["features"]
        best = {}
        for it in items:
            # id: <name>_<year>_<E>-<N>
            parts = it["id"].rsplit("_", 2)
            year, key = int(parts[-2]), parts[-1]
            if key not in best or year > best[key][0]:
                best[key] = (year, it)
        for (kx, ky) in keys:
            k = f"{kx}-{ky}"
            if k not in best:
                print("  missing", kind, k)
                continue
            year, it = best[k]
            asset = next(a for an, a in it["assets"].items() if an.endswith(suffix))
            path = os.path.join(out, f"{kind}_{kx}_{ky}.tif")
            print(f"{kind} {k} ({year})", flush=True)
            download(asset["href"], path)
            files[kind].append({"kx": kx, "ky": ky, "year": year, "file": os.path.basename(path)})
    # buildings: the map sheets that touch the window, the newest year that has a CityGML file
    items = json.loads(get(f"{STAC}/ch.swisstopo.swissbuildings3d_3_0/items?bbox={bbox}&limit=100"))["features"]
    sheets = {}
    for it in items:
        parts = it["id"].rsplit("_", 2)
        if len(parts) < 3 or not parts[-2].isdigit():
            continue
        year, sheet = int(parts[-2]), parts[-1]
        a = next((a for an, a in it["assets"].items() if an.endswith(".citygml.zip")), None)
        if a and (sheet not in sheets or year > sheets[sheet][0]):
            sheets[sheet] = (year, a["href"])
    for sheet, (year, href) in sorted(sheets.items()):
        path = os.path.join(out, f"buildings_{sheet}.citygml.zip")
        print(f"buildings sheet {sheet} ({year})", flush=True)
        download(href, path)
        files["buildings"].append({"sheet": sheet, "year": year, "file": os.path.basename(path)})
    # OpenStreetMap in boxes of 0.0075 degrees through the official API
    osm = []
    w, s_, e_, n_ = lon - dlon, lat - dlat, lon + dlon, lat + dlat
    step = 0.0075
    ni, nj = int(math.ceil((e_ - w) / step)), int(math.ceil((n_ - s_) / step))
    for j in range(nj):
        for i in range(ni):
            bw, bs = w + i * step, s_ + j * step
            path = os.path.join(out, f"osm_{i}_{j}.xml")
            osm.append(os.path.basename(path))
            if os.path.exists(path) and os.path.getsize(path) > 100:
                continue
            print("openstreetmap", i, j, flush=True)
            b = get("https://api.openstreetmap.org/api/0.6/map?bbox=" + f"{bw:.6f},{bs:.6f},{min(bw + step, e_):.6f},{min(bs + step, n_):.6f}")
            open(path, "wb").write(b)
            time.sleep(1.5)
    meta = {"name": name, "country": "CH", "lat": lat, "lon": lon, "x0": x0, "y0": y0, "size_m": 2000, "files": files, "osm_files": osm, "sources": {"terrain/surface/photo/buildings": "Federal Office of Topography swisstopo (open government data)", "vector": "OpenStreetMap contributors (ODbL)"}}
    json.dump(meta, open(os.path.join(ROOT, name, "meta.json"), "w"), indent=1)
    print("done:", os.path.join(ROOT, name))


if __name__ == "__main__":
    main()
