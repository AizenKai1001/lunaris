r'''A contact sheet scene of the house models of a folder, each scaled to the same biggest side and standing on the ground, in a row (to see what each one is before it is fitted):

    python tools/realmap/model_sheet.py MODELS_FOLDER_UNDER_ROOT OUT_SCENE.json [SIDE_M]

MODELS_FOLDER_UNDER_ROOT is a folder of .glb files relative to realmap-data (for example `models/chalets`).
'''
import glob
import json
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import glb_info as G  # noqa: E402

ROOT = r"realmap-data"


def main():
    folder, out = sys.argv[1], sys.argv[2]
    side = float(sys.argv[3]) if len(sys.argv) > 3 else 12.0
    files = sorted(glob.glob(os.path.join(ROOT, folder, "*.glb")))
    ents = [
        {"uid": 1, "components": {"Environment": {"sky": True, "clouds": 0.2, "shadows": True, "shadow_distance": 120, "ssao": 1.0, "tonemap": 1, "ambient": 2.0, "ground": [0.5, 0.48, 0.42]}}},
        {"uid": 2, "components": {"Light": {"kind": 0, "intensity": 3.6, "color": [1.0, 0.93, 0.82]}, "Transform": {"rot": [-0.45, 0.55, 0.2, 0.68]}}},
        {"uid": 3, "components": {"Atmosphere": {"turbidity": 2.4, "intensity": 1.0}}},
        {"uid": 4, "components": {"Transform": {"pos": [40.0, -0.05, 20.0]}, "MeshRenderer": {"mesh": "cube", "color": [0.3, 0.42, 0.22, 1]}}},
    ]
    ents[3]["components"]["Transform"]["scale"] = [140.0, 0.1, 140.0]
    info = []
    for k, f in enumerate(files):
        j, _ = G.load_glb(f)
        lo, hi, tris = G.bounds(j)
        size = hi - lo
        s = side / max(size[0], size[2])
        centre = (lo + hi) / 2
        pos = [(k % 5) * 20.0 - s * centre[0], -s * lo[1], (k // 5) * 20.0 - s * centre[2]]
        ents.append({"uid": 10 + k, "components": {"Name": {"value": os.path.basename(f)}, "Transform": {"pos": pos, "scale": [s, s, s]}, "MeshRenderer": {"mesh": os.path.relpath(f, ROOT).replace(os.sep, "/"), "roughness": 0.85}}})
        info.append((os.path.basename(f), round(s, 4), np.round(size * s, 1).tolist()))
    json.dump({"lunaris": 1, "entities": ents}, open(out, "w"))
    for i in info:
        print(i)


if __name__ == "__main__":
    main()
