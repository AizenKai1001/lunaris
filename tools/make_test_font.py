"""Builds crates/lunaris-render/testdata/pixel.ttf: a TrueType font whose letters are the engine's own 5x8 bitmap glyphs (crates/lunaris-render/src/font.rs)
drawn as square outlines, so it is original work with no licence to carry. 800 units to the em, 100 units to a font pixel, baseline above the last row.

    python -I tools/make_test_font.py

Needs fontTools (pip install fonttools).
"""
import os
import re
import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "..", "crates", "lunaris-render", "src", "font.rs")
OUT = os.path.join(HERE, "..", "crates", "lunaris-render", "testdata", "pixel.ttf")

PIXEL = 100
ASCENT = 700
ADVANCE = 600


def read_glyphs():
    rows = []
    for line in open(SRC, encoding="utf-8"):
        m = re.match(r"\s*\[((?:0x[0-9A-Fa-f]{2},?\s*){5})\],", line)
        if m:
            rows.append([int(x, 16) for x in re.findall(r"0x[0-9A-Fa-f]{2}", m.group(1))])
    if len(rows) != 95:
        sys.exit(f"expected 95 glyphs in font.rs, found {len(rows)}")
    return rows


def glyph_for(cols):
    """One rectangle per run of lit font pixels in a row (bit 0 is the top row, bit 7 the one below the baseline)."""
    pen = TTGlyphPen(None)
    for row in range(8):
        col = 0
        while col < 5:
            if cols[col] >> row & 1:
                start = col
                while col < 5 and cols[col] >> row & 1:
                    col += 1
                x0, x1 = start * PIXEL, col * PIXEL
                y1 = ASCENT - row * PIXEL
                y0 = y1 - PIXEL
                # clockwise: the outside of a TrueType glyph
                pen.moveTo((x0, y0))
                pen.lineTo((x0, y1))
                pen.lineTo((x1, y1))
                pen.lineTo((x1, y0))
                pen.closePath()
            else:
                col += 1
    return pen.glyph()


def main():
    rows = read_glyphs()
    names = [".notdef"] + [f"uni{c:04X}" for c in range(32, 127)]
    glyphs = {}
    box = TTGlyphPen(None)
    for pts in ([(0, 0), (0, 700), (500, 700), (500, 0)], [(100, 100), (400, 100), (400, 600), (100, 600)]):
        box.moveTo(pts[0])
        for p in pts[1:]:
            box.lineTo(p)
        box.closePath()
    glyphs[".notdef"] = box.glyph()
    for i, cols in enumerate(rows):
        glyphs[names[i + 1]] = glyph_for(cols)
    fb = FontBuilder(800, isTTF=True)
    fb.setupGlyphOrder(names)
    fb.setupCharacterMap({c: f"uni{c:04X}" for c in range(32, 127)})
    fb.setupGlyf(glyphs)
    table = fb.font["glyf"]
    fb.setupHorizontalMetrics({n: (ADVANCE, getattr(table[n], "xMin", 0)) for n in names})
    fb.setupHorizontalHeader(ascent=ASCENT, descent=-PIXEL)
    fb.setupNameTable({"familyName": "Lunaris Pixel", "styleName": "Regular"})
    fb.setupOS2(sTypoAscender=ASCENT, sTypoDescender=-PIXEL, usWinAscent=ASCENT, usWinDescent=PIXEL, achVendID="LUNA")
    fb.setupPost()
    fb.save(OUT)
    print("wrote", os.path.normpath(OUT), os.path.getsize(OUT), "bytes")


main()
