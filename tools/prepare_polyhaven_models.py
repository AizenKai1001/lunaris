"""Make the Poly Haven glTF models in examples/gfx_lab/models/<id>/ fit Lunaris's importer, which reads PNG and JPEG textures but shows one texture per model: a model with one textured material keeps its
picture; a model with several (a tree: bark, trunks, twigs) gets each material's plain colour set to the average colour of its picture (as linear light) and the picture left out. Every material is made
non-metallic and rough. Writes <id>/<id>_lunaris.gltf beside the original. Needs ffmpeg on the PATH (for the average colours).
    python -I tools/prepare_polyhaven_models.py [id ...]"""
import json
import os
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "gfx_lab", "models"))


def average_colour(path):
    """The mean colour of a picture in linear light (0..1), found by letting ffmpeg shrink it to one pixel."""
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-vf", "scale=1:1:flags=area", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], capture_output=True, check=True).stdout
    return [round((c / 255.0) ** 2.2, 4) for c in raw[:3]]


def to_png(src, dst):
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", src, dst], check=True)


def prepare(mid):
    folder = os.path.join(ROOT, mid)
    src = os.path.join(folder, f"{mid}_1k.gltf")
    g = json.load(open(src))
    images = g.get("images", [])
    textures = g.get("textures", [])

    def picture(tex_index):
        return os.path.join(folder, images[textures[tex_index]["source"]]["uri"].replace("/", os.sep))

    mats = g.get("materials", [])
    textured = [m for m in mats if "baseColorTexture" in m.get("pbrMetallicRoughness", {})]
    for m in mats:
        pbr = m.setdefault("pbrMetallicRoughness", {})
        pbr["metallicFactor"] = 0.0
        pbr["roughnessFactor"] = 0.9
        for key in ("metallicRoughnessTexture",):
            pbr.pop(key, None)
        m.pop("normalTexture", None)
        m.pop("occlusionTexture", None)
        if "baseColorTexture" not in pbr:
            continue
        pic = picture(pbr["baseColorTexture"]["index"])
        if len(textured) == 1:
            continue
        else:
            rgb = average_colour(pic)
            # the twig pictures are atlases with the needles stretched across them: their average is brown, the needles are a dark conifer green
            if "twig" in (m.get("name") or ""):
                rgb = [0.045, 0.085, 0.035]
            pbr["baseColorFactor"] = rgb + [1.0]
            del pbr["baseColorTexture"]
    out = os.path.join(folder, f"{mid}_lunaris.gltf")
    json.dump(g, open(out, "w"))
    print(mid, "ok", [round(c, 2) for m in mats for c in m["pbrMetallicRoughness"].get("baseColorFactor", [])[:3]][:12])


for mid in sys.argv[1:] or sorted(d for d in os.listdir(ROOT) if os.path.isdir(os.path.join(ROOT, d))):
    prepare(mid)
