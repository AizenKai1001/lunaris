r"""The relief files of the wards (CityGML, 0.5 to 2 GB a ward) as small point files, the CityGML deleted.

    python plateau_dem_pts.py LAT0 LON0 H0 GML_FOLDER [GML_FOLDER ...]

Every `*_dem_*.gml` in the folders is read (plateau_dem.read_vertices), its corners are put into the local frame (exactly, through Earth-centred coordinates; the geoid GEOID (default 36.7 m) minus LOWER (0.3 m) added to the heights, as
plateau_dem_city.py does) and saved as `<name>.npy` (float32 x, y, z) beside it; then the .gml is removed unless KEEP=1. plateau_dem_city.py reads these files in place of the .gml.
The data is (c) MLIT Japan, Project PLATEAU (processed here).
"""
import glob
import multiprocessing
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from plateau_dem import read_vertices  # noqa: E402
from plateau_dem_city import to_local  # noqa: E402
from plateau_tiles import Frame  # noqa: E402


def one(job):
    path, frame_args, geoid, keep = job
    out = os.path.splitext(path)[0] + ".npy"
    if not os.path.exists(out):
        v = read_vertices(path)
        loc = to_local(Frame(*frame_args), v[:, 0], v[:, 1], v[:, 2] + geoid)
        np.save(out + ".part.npy", loc.astype(np.float32))
        os.replace(out + ".part.npy", out)
    if not keep:
        os.remove(path)
    return os.path.basename(path), os.path.getsize(out)


def main():
    frame_args = (float(sys.argv[1]), float(sys.argv[2]), float(sys.argv[3]))
    geoid = float(os.environ.get("GEOID", "36.7")) - float(os.environ.get("LOWER", "0.3"))
    keep = os.environ.get("KEEP") == "1"
    files = [f for folder in sys.argv[4:] for f in sorted(glob.glob(os.path.join(folder, "*.gml")))]
    print(len(files), "files", flush=True)
    with multiprocessing.Pool(int(os.environ.get("PROCS", "3"))) as pool:
        for k, (name, size) in enumerate(pool.imap_unordered(one, [(f, frame_args, geoid, keep) for f in files])):
            if k % 20 == 0:
                print(k, name, size // 1000, "kB", flush=True)
    print("done")


if __name__ == "__main__":
    main()
