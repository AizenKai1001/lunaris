"""A copy of a scene with some resources set, for trying a state of the game without playing to it (a rain, a night, a full stock):

    python3 tools/citygen/make_variant.py city.lunaris.json city_rain.lunaris.json wx_target=2 wx=0.95 rain=1
"""
import json
import sys


def main():
    src, dst, pairs = sys.argv[1], sys.argv[2], sys.argv[3:]
    scene = json.load(open(src, encoding="utf-8"))
    res = scene.setdefault("resources", {})
    for p in pairs:
        k, v = p.split("=")
        res[k] = float(v)
    json.dump(scene, open(dst, "w", encoding="utf-8"), separators=(",", ":"))
    print(dst, {k: res[k] for k in (p.split("=")[0] for p in pairs)})


if __name__ == "__main__":
    main()
