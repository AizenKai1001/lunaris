"""A contact sheet of downloadable, attribution-licensed (CC BY or CC0) house models from Sketchfab's public search, to choose from by eye:

    python tools/realmap/sketchfab_candidates.py OUT.png "query one" "query two" ...

Prints the uid, name, author, licence and face count of each; the sheet has the thumbnails with their numbers. (Downloading a model needs a logged-in Sketchfab account: that part is done in the browser.)
"""
import io
import json
import sys
import urllib.parse
import urllib.request

from PIL import Image, ImageDraw


def search(q, lic):
    url = "https://api.sketchfab.com/v3/search?" + urllib.parse.urlencode({"type": "models", "q": q, "downloadable": "true", "license": lic, "count": 24, "sort_by": "-likeCount"})
    return json.load(urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "lunaris-realmap"}), timeout=30)).get("results", [])


def main():
    out, queries = sys.argv[1], sys.argv[2:]
    seen = {}
    for q in queries:
        for lic in ("by", "cc0"):
            try:
                for r in search(q, lic):
                    fc = r.get("faceCount") or 0
                    if r["uid"] in seen or fc < 800 or fc > 60000:
                        continue
                    imgs = sorted(r["thumbnails"]["images"], key=lambda i: abs(i["width"] - 256))
                    seen[r["uid"]] = {"name": r["name"], "faces": fc, "lic": lic, "user": r["user"]["username"], "thumb": imgs[0]["url"], "likes": r.get("likeCount") or 0}
            except Exception as e:  # noqa: BLE001
                print("search failed:", q, lic, e)
    rows = sorted(seen.items(), key=lambda kv: -kv[1]["likes"])[:48]
    cols, tw, th = 8, 240, 190
    sheet = Image.new("RGB", (cols * tw, ((len(rows) + cols - 1) // cols) * th), (30, 30, 30))
    d = ImageDraw.Draw(sheet)
    for k, (uid, r) in enumerate(rows):
        try:
            im = Image.open(io.BytesIO(urllib.request.urlopen(urllib.request.Request(r["thumb"], headers={"User-Agent": "lunaris-realmap"}), timeout=30).read())).convert("RGB")
            im.thumbnail((tw, th - 18))
            sheet.paste(im, ((k % cols) * tw, (k // cols) * th))
        except Exception as e:  # noqa: BLE001
            print("thumb failed", uid, e)
        d.rectangle([(k % cols) * tw, (k // cols) * th + th - 18, (k % cols) * tw + tw, (k // cols) * th + th], fill=(0, 0, 0))
        d.text(((k % cols) * tw + 3, (k // cols) * th + th - 16), f"{k} {r['name'][:30]} {r['faces'] // 1000}k", fill=(255, 255, 255))
        print(k, uid, r["name"].encode("ascii", "replace").decode(), "|", r["user"] if "user" in r else r["user"], "|", r["lic"], "|", r["faces"], "faces")
    sheet.save(out)
    print("sheet", out)


if __name__ == "__main__":
    main()
