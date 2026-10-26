"""Netherlands samples (central Utrecht, 500 m square): PDOK aerial photo at 8 cm, AHN terrain and surface model at 0.5 m, 3D BAG buildings (LoD 1.2 / 1.3 / 2.2 from the AHN laser survey).
All open data (CC BY 4.0 for the photos; AHN and 3D BAG open). Files go to realmap-data\quality\nl\.  The 3D BAG host serves a certificate chain that Python's trust store
refuses ('expired') but curl accepts, so that one is fetched with curl (verification stays on)."""
import json
import os
import subprocess
import urllib.parse
import urllib.request

OUT = r"realmap-data\quality\nl"
UA = {"User-Agent": "lunaris-realmap/0.1"}
X0, Y0, S = 136300, 455700, 500


def get(url, t=180):
    return urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=t).read()


def curl(url, params=None):
    cmd = ["curl", "-s", "-m", "120", "-A", "lunaris-realmap/0.1", "-G", url]
    for k, v in (params or {}).items():
        cmd += ["--data-urlencode", f"{k}={v}"]
    return subprocess.run(cmd, capture_output=True, check=True).stdout


def main():
    os.makedirs(OUT, exist_ok=True)
    # aerial photo: nine tiles of 200 m at 2500 px (8 cm a pixel; the service takes at most 2500 px), covering 600 m round the sample square
    for j in range(3):
        for i in range(3):
            p = os.path.join(OUT, f"aerial_{i}_{j}.jpg")
            if os.path.exists(p) and os.path.getsize(p) > 5000:
                continue
            bx, by = X0 - 50 + i * 200, Y0 - 50 + j * 200
            u = "https://service.pdok.nl/hwh/luchtfotorgb/wms/v1_0?" + urllib.parse.urlencode({"service": "WMS", "version": "1.3.0", "request": "GetMap", "layers": "Actueel_orthoHR", "styles": "", "crs": "EPSG:28992", "bbox": f"{bx},{by},{bx + 200},{by + 200}", "width": 2500, "height": 2500, "format": "image/jpeg"})
            b = get(u)
            open(p, "wb").write(b)
            print("aerial", i, j, len(b) // 1024, "KB")
    for cov in ("dtm_05m", "dsm_05m"):
        p = os.path.join(OUT, cov + ".tif")
        if os.path.exists(p):
            continue
        u = "https://service.pdok.nl/rws/ahn/wcs/v1_0?" + urllib.parse.urlencode({"SERVICE": "WCS", "VERSION": "1.0.0", "REQUEST": "GetCoverage", "COVERAGE": cov, "CRS": "EPSG:28992", "BBOX": f"{X0},{Y0},{X0 + S},{Y0 + S}", "WIDTH": 1000, "HEIGHT": 1000, "FORMAT": "image/tiff"})
        b = get(u)
        open(p, "wb").write(b)
        print(cov, len(b) // 1024, "KB")
    p = os.path.join(OUT, "bag.json")
    if not os.path.exists(p):
        feats = []
        url = "https://api.3dbag.nl/collections/pand/items"
        params = {"bbox": f"{X0},{Y0},{X0 + S},{Y0 + S}", "limit": "50"}
        raw = json.loads(curl(url, params))
        meta = raw.get("metadata")
        while True:
            feats += raw["features"]
            nxt = next((l["href"] for l in raw.get("links", []) if l.get("rel") == "next"), None)
            if not nxt:
                break
            raw = json.loads(subprocess.run(["curl", "-s", "-m", "120", "-A", "lunaris-realmap/0.1", nxt], capture_output=True, check=True).stdout)
        json.dump({"features": feats, "metadata": meta}, open(p, "w"))
        print("3D BAG buildings:", len(feats))


if __name__ == "__main__":
    main()
