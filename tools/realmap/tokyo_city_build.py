r"""The whole of Tokyo from PLATEAU, ward by ward, to the folder the engine reads. Run on the machine that has the disk and the cores (a Linux test machine).

    python tokyo_city_build.py WARD_CODE [WARD_CODE ...]        or `all`

For every ward, once (each step leaves a marker in FETCH/<code>/ and is skipped when it is there):
  1. plateau_fetch.py       the 3D Tiles of the ward (the next ward's are fetched while this one is processed);
  2. plateau_cells.py       the buildings as units with exclusive near (LoD2) and far (LoD1) meshes -> ROOT/bldg;
  3. plateau_ground_collect roads and building bases for the ground, elevated roads as glTF -> ROOT/ground/<code>.npz, ROOT/tran_elev/<code>;
  4. plateau_tiles.py       the leaf tiles of city furniture, bridges, water and solitary trees -> ROOT/frn_<code>, brid_<code>, wtr_<code>, vegs_<code>;
  5. the ward's raw tiles are deleted (they are several times the size of what is kept).
The relief (ROOT/dem: plateau_dem_city.py) and its photographs (plateau_ortho.py) must be there already; plateau_ground_burn.py then makes the ground of all the wards and tokyo23_world.py the scene.
Env: ROOT (default ~/realmap/tokyo23), FETCH (default ~/tokyo/city), ORIGIN ("35.675 139.74 40"), PROCS.
The data is (c) MLIT Japan, Project PLATEAU (processed here).
"""
import concurrent.futures
import json
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import plateau_fetch as F  # noqa: E402

ROOT = os.path.expanduser(os.environ.get("ROOT", "~/realmap/tokyo23"))
FETCH = os.path.expanduser(os.environ.get("FETCH", "~/tokyo/city"))
ORIGIN = os.environ.get("ORIGIN", "35.675 139.74 40").split()
PY = sys.executable
# the inner wards first (the origin is between Chiyoda and Minato), so that a world of the middle of the city can be tried early
ORDER = [13101, 13103, 13104, 13113, 13102, 13105, 13106, 13110, 13109, 13116, 13114, 13107, 13108, 13115, 13117, 13118, 13119, 13120, 13112, 13111, 13121, 13122, 13123]
OTHER_KINDS = [("frn", "frn"), ("brid", "brid"), ("wtr", "wtr"), ("veg_solitary", "vegs")]


def run(args, env=None, log=None):
    t0 = time.time()
    e = dict(os.environ)
    e.update(env or {})
    r = subprocess.run(args, env=e, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    tail = "\n".join(r.stdout.strip().splitlines()[-4:])
    print(f"  {os.path.basename(args[1])} {'ok' if r.returncode == 0 else 'FAILED ' + str(r.returncode)} {time.time() - t0:.0f} s\n    " + tail.replace("\n", "\n    "), flush=True)
    if log:
        open(log, "a").write(r.stdout)
    if r.returncode != 0:
        raise RuntimeError(f"{args[1]} failed")


def marker(code, name):
    return os.path.join(FETCH, str(code), f"done_{name}")


def process(code):
    ward = os.path.join(FETCH, str(code))
    meta = json.load(open(os.path.join(ward, "fetched.json")))["tilesets"]
    tiles = os.path.join(ward, "tiles")
    log = os.path.join(ward, "build.log")
    t0 = time.time()
    if not os.path.exists(marker(code, "cells")):
        run([PY, os.path.join(HERE, "plateau_cells.py"), os.path.join(ROOT, "bldg"), str(code), tiles, *ORIGIN], {"ORTHO_DEM": os.path.join(ROOT, "dem")}, log)
        open(marker(code, "cells"), "w").write("ok")
    if not os.path.exists(marker(code, "ground")):
        elev = os.path.join(ROOT, "tran_elev", str(code))
        os.makedirs(elev, exist_ok=True)
        for pic in ("asphalt.jpg", "pavement.jpg"):
            shutil.copy(os.path.join(ROOT, "tran_elev", pic), os.path.join(elev, pic))
        run([PY, os.path.join(HERE, "plateau_ground_collect.py"), os.path.join(ROOT, "dem"), ward, os.path.join(ROOT, "ground", f"{code}.npz"), elev, *ORIGIN], None, log)
        open(marker(code, "ground"), "w").write("ok")
    for kind, short in OTHER_KINDS:
        if kind not in meta or os.path.exists(marker(code, short)):
            continue
        run([PY, os.path.join(HERE, "plateau_tiles.py"), os.path.join(tiles, meta[kind]), os.path.join(ROOT, f"{short}_{code}"), *ORIGIN], {"LEAVES": "1"}, log)
        open(marker(code, short), "w").write("ok")
    shutil.rmtree(tiles, ignore_errors=True)
    open(marker(code, "raw_deleted"), "w").write("ok")
    print(f"{code} {F.WARDS[code]} done in {time.time() - t0:.0f} s", flush=True)


def main():
    codes = ORDER if sys.argv[1] == "all" else [int(c) for c in sys.argv[1:]]
    os.makedirs(os.path.join(ROOT, "bldg"), exist_ok=True)
    os.makedirs(os.path.join(ROOT, "ground"), exist_ok=True)
    codes = [c for c in codes if not os.path.exists(marker(c, "raw_deleted"))]
    with concurrent.futures.ThreadPoolExecutor(1) as fetcher:
        pending = {}

        def fetch(c):
            t0 = time.time()
            F.fetch_ward(FETCH, c, ("tiles",))
            print(f"{c} fetched in {time.time() - t0:.0f} s", flush=True)

        for k, c in enumerate(codes):
            if c not in pending:
                pending[c] = fetcher.submit(fetch, c)
            if k + 1 < len(codes) and codes[k + 1] not in pending:
                pending[codes[k + 1]] = fetcher.submit(fetch, codes[k + 1])
            pending[c].result()
            process(c)


if __name__ == "__main__":
    main()
