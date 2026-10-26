"""The words on the shop signs: readable names of small businesses on transparent pictures, in white and in black (a sign is dark or light, the lettering goes the other way).

    python tools/citygen/make_signs.py citygen-data/city/textures [FONT.ttf]

Writes OUT/_signs/text_w_<k>.png and text_b_<k>.png (1024 x 128, the letters opaque, the rest of the picture transparent) for the names below, and atlas_w.png / atlas_b.png with all of them in a grid (what the city uses). The font is Liberation Sans Bold (SIL Open Font License), found in the
`fonts` folder beside the workspace or given on the command line. A second run changes nothing.
"""
import os
import sys

from PIL import Image, ImageDraw, ImageFont

NAMES = [
    "BAKERY", "PHARMACY", "CAFE LUNA", "BOOKS & CO", "PIZZA", "HARDWARE", "BARBER SHOP", "LAUNDRY", "DELI", "FLOWERS", "OPTICIAN", "TAILOR", "NOODLE BAR", "BOTTLE SHOP", "GROCERY", "TOY SHOP",
    "SHOES", "VIDEO", "TOBACCO", "PET SUPPLIES", "HAIR STUDIO", "DINER", "TAVERN", "PAWN SHOP", "ELECTRONICS", "FISH & CHIPS", "CHEMIST", "BUTCHER", "FRUIT MARKET", "THE CORNER", "GALLERY", "JEWELLERS",
    "BANK", "POST OFFICE", "INSURANCE", "TRAVEL", "REAL ESTATE", "DENTIST", "LAW OFFICE", "COPY CENTRE", "PHONE REPAIR", "GYM", "YOGA", "ART SUPPLY", "WINE BAR", "ICE CREAM", "DONUTS", "OPEN 24 HOURS",
]
STREETS = [
    "MAPLE", "OAK", "PINE", "CEDAR", "ELM", "BIRCH", "WILLOW", "CHERRY", "WALNUT", "CHESTNUT", "SPRUCE", "ASH", "POPLAR", "HICKORY", "SYCAMORE", "MAGNOLIA",
    "LINCOLN", "WASHINGTON", "JEFFERSON", "ADAMS", "MADISON", "MONROE", "JACKSON", "FRANKLIN", "CHURCH", "MARKET", "MILL", "PARK", "LAKE", "RIVER", "HILL", "VALLEY",
    "SUNSET", "HIGHLAND", "MEADOW", "FOREST", "STATION", "BRIDGE", "MAIN", "HIGH", "KING", "QUEEN", "PRINCE", "UNION", "LIBERTY", "HARBOR", "GARDEN", "ORCHARD",
    "RIDGE", "SPRING", "SUMMER", "WINTER", "FAIRVIEW", "LAKEVIEW", "GLENWOOD", "KINGSTON", "WINDSOR", "OXFORD", "CAMBRIDGE", "DOVER", "YORK", "ALBANY", "HUDSON", "CONCORD",
]
W, H = 1024, 128


def main():
    out = os.path.join(sys.argv[1], "_signs")
    font_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(os.path.abspath(sys.argv[1].rstrip("/\\"))), "fonts", "LiberationSans-Bold.ttf")
    os.makedirs(out, exist_ok=True)
    for k, name in enumerate(NAMES):
        for tag, rgb in (("w", (245, 245, 240)), ("b", (18, 18, 20))):
            path = os.path.join(out, f"text_{tag}_{k}.png")
            if os.path.exists(path):
                continue
            # the biggest letters that fit in 92% of the width and 72% of the height
            size = 120
            while size > 12:
                font = ImageFont.truetype(font_path, size)
                box = font.getbbox(name)
                if box[2] - box[0] <= W * 0.92 and box[3] - box[1] <= H * 0.72:
                    break
                size -= 4
            img = Image.new("RGBA", (W, H), rgb + (0,))
            d = ImageDraw.Draw(img)
            box = d.textbbox((0, 0), name, font=font)
            x = (W - (box[2] - box[0])) // 2 - box[0]
            y = (H - (box[3] - box[1])) // 2 - box[1]
            d.text((x, y), name, font=font, fill=rgb + (255,))
            img.save(path, optimize=True)
    # the atlases: all the names of a colour in one picture, 8 across (`materials::SIGN_COLUMNS`), each cell 512 x 64 (the names above, halved): `atlas_w.png`, `atlas_b.png`
    cols, cw, ch = 8, 512, 64
    rows = (len(NAMES) + cols - 1) // cols
    for tag in ("w", "b"):
        path = os.path.join(out, f"atlas_{tag}.png")
        if os.path.exists(path):
            continue
        atlas = Image.new("RGBA", (cols * cw, rows * ch), (0, 0, 0, 0))
        for k in range(len(NAMES)):
            cell = Image.open(os.path.join(out, f"text_{tag}_{k}.png")).convert("RGBA").resize((cw, ch), Image.LANCZOS)
            atlas.paste(cell, ((k % cols) * cw, (k // cols) * ch))
        atlas.save(path, optimize=True)
    # street name plates: white on dark green, `street_<m>.png` with m = 2 * name + (1 for an avenue)
    for m in range(len(STREETS) * 2):
        path = os.path.join(out, f"street_{m}.png")
        if os.path.exists(path):
            continue
        text = f"{STREETS[m // 2]} {'AVE' if m % 2 else 'ST'}"
        pw, ph = 1024, 256
        img = Image.new("RGB", (pw, ph), (12, 92, 62))
        d = ImageDraw.Draw(img)
        d.rectangle([10, 10, pw - 11, ph - 11], outline=(240, 240, 235), width=8)
        size = 160
        while size > 20:
            font = ImageFont.truetype(font_path, size)
            box = d.textbbox((0, 0), text, font=font)
            if box[2] - box[0] <= pw * 0.84 and box[3] - box[1] <= ph * 0.5:
                break
            size -= 4
        d.text(((pw - (box[2] - box[0])) // 2 - box[0], (ph - (box[3] - box[1])) // 2 - box[1]), text, font=font, fill=(245, 245, 240))
        img.save(path, optimize=True)
    print(len(NAMES), "shop names and", len(STREETS) * 2, "street plates written to", out)


if __name__ == "__main__":
    main()
