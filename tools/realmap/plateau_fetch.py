r"""Download the parts of a PLATEAU ward's open data that the game needs, without the rest.

    python plateau_fetch.py OUT_FOLDER CODE [CODE ...]       CODE: 13101 .. 13123 (the wards of Tokyo), or `all`

For a ward, the dataset page of geospatial.jp (CKAN API) names the two archives; they are read with range requests and only these members come out (each a few hundred megabytes of the several gigabytes):
  3D Tiles: the buildings at LoD1 and LoD2 (with textures), road surfaces LoD3, city furniture LoD3, bridges, solitary trees and plant cover (LoD3), water bodies;
  CityGML: the relief (`udx/dem`).
They land in OUT_FOLDER/<code>/tiles/<kind>/ (a tileset each: tileset.json and data/) and OUT_FOLDER/<code>/dem/. A ward whose parts are all there is skipped; PARTS=dem (or tiles) fetches only the relief (a few megabytes) or only the tiles.

The data is (c) MLIT Japan, Project PLATEAU: 出典：国土交通省 PLATEAUウェブサイト (https://www.mlit.go.jp/plateau/). Free use, commercial use included.
"""
import glob
import json
import os
import re
import sys
import threading
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor

from remotezip import RemoteZip

WARDS = {
    13101: "chiyoda-ku", 13102: "chuo-ku", 13103: "minato-ku", 13104: "shinjuku-ku", 13105: "bunkyo-ku", 13106: "taito-ku", 13107: "sumida-ku", 13108: "koto-ku", 13109: "shinagawa-ku", 13110: "meguro-ku", 13111: "ota-ku",
    13112: "setagaya-ku", 13113: "shibuya-ku", 13114: "nakano-ku", 13115: "suginami-ku", 13116: "toshima-ku", 13117: "kita-ku", 13118: "arakawa-ku", 13119: "itabashi-ku", 13120: "nerima-ku", 13121: "adachi-ku", 13122: "katsushika-ku",
    13123: "edogawa-ku",
}
# the folders of the 3D Tiles archive that are wanted, by what they are called (the first match of a kind is the one: LoD2 with textures, not the `no_texture` copy)
TILE_KINDS = [
    ("bldg_lod1", re.compile(r"_bldg_3dtiles_(?:[^/]*_)?lod1/")),
    ("bldg_lod2", re.compile(r"_bldg_3dtiles_(?:[^/]*_)?lod2/")),
    ("tran", re.compile(r"_tran_3dtiles_lod3/")),
    ("frn", re.compile(r"_frn_3dtiles_lod3/")),
    ("brid", re.compile(r"_brid_3dtiles_lod2/")),
    ("veg_solitary", re.compile(r"_veg_SolitaryVegetationObject_3dtiles_lod3/")),
    ("veg_cover", re.compile(r"_veg_PlantCover_3dtiles_lod3/")),
    ("wtr", re.compile(r"_wtr_3dtiles_lod1/")),
]


def api(code):
    name = WARDS[code]
    url = f"https://www.geospatial.jp/ckan/api/3/action/package_show?id=plateau-{code}-{name}-2025"
    data = json.load(urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "lunaris-realmap"}), timeout=60))
    urls = {}
    for r in data["result"]["resources"]:
        u = r.get("url") or ""
        base = u.rsplit("/", 1)[-1]
        if not base.startswith(f"{code}_"):
            continue  # the use-case packages of some wards (uc25-..., a few areas only) are not the ward's data
        if "3dtiles_mvt" in base:
            urls["tiles"] = u
        elif "_citygml_" in base and base.endswith(".zip"):
            urls["citygml"] = u
    return urls


def extract_members(url, members, dest, workers=16):
    """Extract these members of a remote zip into dest (several at once: each is a request of its own)."""
    local = threading.local()

    def one(info_name):
        z = getattr(local, "z", None)
        if z is None:
            z = local.z = RemoteZip(url)
        for attempt in range(4):
            try:
                z.extract(info_name, dest)
                return
            except Exception:  # noqa: BLE001
                time.sleep(1 + attempt)
                local.z = z = RemoteZip(url)
        print("failed:", info_name, flush=True)

    with ThreadPoolExecutor(workers) as pool:
        list(pool.map(one, members))


def fetch_ward(out, code, parts=("dem", "tiles")):
    """The wanted parts of a ward that are not there yet: the relief (`dem`) and the 3D Tiles (`tiles`, done when fetched.json is written). Returns the tilesets found (or None when the tiles are not asked for)."""
    d = os.path.join(out, str(code))
    done = os.path.join(d, "fetched.json")
    os.makedirs(d, exist_ok=True)
    t0 = time.time()
    urls = None
    if "dem" in parts and not glob.glob(os.path.join(d, "dem", "udx", "dem", "*.gml")):
        urls = api(code)
        with RemoteZip(urls["citygml"]) as z:
            dem = [i.filename for i in z.infolist() if i.filename.startswith("udx/dem/") and i.filename.endswith(".gml")]
        extract_members(urls["citygml"], dem, os.path.join(d, "dem"), workers=2)
        print(code, "relief done", f"{time.time() - t0:.0f} s", flush=True)
    if "tiles" not in parts:
        return None
    if os.path.exists(done):
        return json.load(open(done))
    urls = urls or api(code)
    got = {}
    with RemoteZip(urls["tiles"]) as z:
        names = [i.filename for i in z.infolist() if not i.is_dir()]
    tiles_dir = os.path.join(d, "tiles")
    wanted = []
    for kind, pat in TILE_KINDS:
        members = [n for n in names if pat.search(n.split("/")[0] + "/") and "no_texture" not in n.split("/")[0]]
        if not members:
            continue
        top = members[0].split("/")[0]
        got[kind] = top
        wanted += members
    print(code, WARDS[code], "tiles:", {k: v[-40:] for k, v in got.items()}, len(wanted), "files", flush=True)
    extract_members(urls["tiles"], wanted, tiles_dir)
    print(code, "tiles done", f"{time.time() - t0:.0f} s", flush=True)
    meta = {"code": code, "ward": WARDS[code], "tilesets": got}
    json.dump(meta, open(done, "w"))
    return meta


def main():
    out = sys.argv[1]
    codes = sorted(WARDS) if sys.argv[2] == "all" else [int(c) for c in sys.argv[2:]]
    os.makedirs(out, exist_ok=True)
    parts = tuple(os.environ.get("PARTS", "dem,tiles").split(","))
    for c in codes:
        fetch_ward(out, c, parts)


if __name__ == "__main__":
    main()
