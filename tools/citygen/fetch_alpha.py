"""Give the leaves their shapes. Poly Haven's glTF downloads of plants and trees name a JPEG for the colour of every leaf atlas (a leaf photographed on black), and keep the opacity in a separate picture that the glTF does
not use, so that every leaf card is a solid rectangle. This fetches the opacity map of every prop that has one and stores the colour and the opacity together as an RGBA PNG (`..._diff_1k.png`), points the glTF at it
and makes the material a cut-out (`alphaMode` MASK).

    python tools/citygen/fetch_alpha.py citygen-data/city/props [SLUG ...]      (no slugs: every prop folder)

Run it after tools/citygen/fetch_models.py and before tools/citygen/shrink_props.py (which then makes the pictures smaller); a second run changes nothing.
"""
import io
import json
import os
import sys
import urllib.request

from PIL import Image


def get(url):
    return urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "lunaris-citygen"}), timeout=120).read()


def main():
    root = sys.argv[1]
    slugs = sys.argv[2:] or sorted(d for d in os.listdir(root) if os.path.isdir(os.path.join(root, d)))
    for slug in slugs:
        gltf_path = os.path.join(root, slug, slug + ".gltf")
        if not os.path.exists(gltf_path):
            continue
        files = json.loads(get(f"https://api.polyhaven.com/files/{slug}"))
        alphas = {k: v for k, v in files.items() if "alpha" in k.lower() and isinstance(v, dict) and "1k" in v}
        if not alphas:
            continue
        doc = json.load(open(gltf_path))
        changed = False
        for key, entry in alphas.items():
            # `twig_alpha` -> the picture of the `twig` set; `Alpha` -> the main picture of the model
            part = key[: -len("_alpha")] if key.lower().endswith("_alpha") else ""
            stem = f"{slug}_{part}_diff_1k" if part else f"{slug}_diff_1k"
            tex_dir = os.path.join(root, slug, "textures")
            jpg = os.path.join(tex_dir, stem + ".jpg")
            png = os.path.join(tex_dir, stem + ".png")
            if not os.path.exists(jpg) and not os.path.exists(png):
                print(slug, key, "no colour picture", stem)
                continue
            if not os.path.exists(png):
                fmt = entry["1k"].get("png") or entry["1k"].get("jpg")
                alpha = Image.open(io.BytesIO(get(fmt["url"]))).convert("L")
                colour = Image.open(jpg).convert("RGB")
                alpha = alpha.resize(colour.size, Image.LANCZOS)
                rgba = colour.copy()
                rgba.putalpha(alpha)
                rgba.save(png, optimize=True)
                print(slug, key, "->", os.path.basename(png), rgba.size)
            rel_jpg, rel_png = f"textures/{stem}.jpg", f"textures/{stem}.png"
            for i, im in enumerate(doc.get("images", [])):
                if im.get("uri") == rel_jpg:
                    im["uri"] = rel_png
                    changed = True
            # the materials that wear it become cut-outs
            image_of = {i for i, im in enumerate(doc.get("images", [])) if im.get("uri") == rel_png}
            tex_of = {i for i, t in enumerate(doc.get("textures", [])) if t.get("source") in image_of}
            for m in doc.get("materials", []):
                t = (m.get("pbrMetallicRoughness") or {}).get("baseColorTexture")
                if t is not None and t.get("index") in tex_of and m.get("alphaMode") != "MASK":
                    m["alphaMode"] = "MASK"
                    m["alphaCutoff"] = 0.4
                    m["doubleSided"] = True
                    changed = True
        if changed:
            json.dump(doc, open(gltf_path, "w"), indent=1)


if __name__ == "__main__":
    main()
