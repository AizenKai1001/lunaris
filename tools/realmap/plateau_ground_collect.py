r"""What one ward of PLATEAU says about the ground, as a small file, so that its raw tiles can be deleted: roads, building bases and the road surfaces that are not at ground level.

    python plateau_ground_collect.py DEM_FOLDER WARD_FOLDER OUT_NPZ ELEV_OUT LAT0 LON0 H0

WARD_FOLDER is `<fetch>/<code>` (plateau_fetch.py): its `fetched.json` names the tilesets. For the ward this does what plateau_ground.py does for a whole city, up to the burning of the cells:
- the triangles of the road surfaces (LoD3 tileset `tran`; a ward without any gives none), each with its plane, class (1 roadway, 2 pavement, 3 island) and whether it lies at ground level on the relief of DEM_FOLDER;
- the ground faces of the LoD2 buildings (`bldg_lod2`, or `bldg_lod1` where there is no LoD2) with their planes;
- written to OUT_NPZ (plateau_ground_burn.py reads one for every ward); the road surfaces that are NOT at ground level (stilts, overpasses, tunnels) become glTF tiles in ELEV_OUT with the shared pictures asphalt.jpg and pavement.jpg
  (they must be in that folder).
The data is (c) MLIT Japan, Project PLATEAU (processed here).
"""
import json
import os
import sys
import time

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import plateau_ground as G  # noqa: E402
from plateau_tiles import Frame  # noqa: E402


def main():
    folder, ward, out_npz, elev_out = sys.argv[1:5]
    frame = Frame(float(sys.argv[5]), float(sys.argv[6]), float(sys.argv[7]))
    t0 = time.time()
    meta = json.load(open(os.path.join(ward, "fetched.json")))["tilesets"]
    tiles = os.path.join(ward, "tiles")
    _, first = G.prepare(folder)
    dem = G.dem_arrays(folder, first)
    dem_fn = lambda x, z: G.dem_at(dem, x, z)  # noqa: E731
    del first
    empty2 = np.zeros((0, 3, 2), dtype=np.float32)
    r_tri, r_pl, r_cat, r_ground = empty2, np.zeros((0, 3)), np.zeros(0, dtype=np.uint8), np.zeros(0, dtype=bool)
    n_elev = 0
    if "tran" in meta:
        tran_dir = os.path.join(tiles, meta["tran"])
        r_tri, r_pl, r_cat, r_ground, refs = G.collect_roads(tran_dir, frame, dem_fn)
        r_tri = r_tri.astype(np.float32)
        print(f"roads: {len(r_tri)} triangles, {int(r_ground.sum())} at ground level, {time.time() - t0:.0f} s", flush=True)
        n_elev = G.write_elevated(tran_dir, elev_out, frame, r_ground, refs)
        print(f"{n_elev} elevated road tiles, {time.time() - t0:.0f} s", flush=True)
    b_tri, b_pl = empty2, np.zeros((0, 3))
    bkey = "bldg_lod2" if "bldg_lod2" in meta else "bldg_lod1" if "bldg_lod1" in meta else None
    if bkey:
        b_tri, b_pl = G.collect_building_bases(os.path.join(tiles, meta[bkey]), frame, dem_fn)
        b_tri = b_tri.astype(np.float32)
        print(f"buildings: {len(b_tri)} ground faces, {time.time() - t0:.0f} s", flush=True)
    os.makedirs(os.path.dirname(os.path.abspath(out_npz)), exist_ok=True)
    np.savez(out_npz + ".part.npz", r_tri=r_tri, r_pl=r_pl, r_cat=r_cat, r_ground=r_ground, b_tri=b_tri, b_pl=b_pl)
    os.replace(out_npz + ".part.npz", out_npz)
    print(f"{ward}: written {out_npz}, {time.time() - t0:.0f} s")


if __name__ == "__main__":
    main()
