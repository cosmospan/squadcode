"""Render Squad Code's app icon (a terminal window with Peek peeking over its top edge, lime on graphite) from squadcode.svg for every platform.

Run from this folder: python3 build.py (needs cairosvg and Pillow).

- macOS: squadcode.icns, from the inset artwork (Apple's grid leaves a margin round the tile).
- Windows: squadcode.ico, full-bleed so small sizes stay legible.
- Linux: png/squadcode-<size>.png for the hicolor theme.
- macOS menu bar: menubar/*.png, black template glyphs at 2x from menubar/*.svg.
"""

import io
import re

import cairosvg
from PIL import Image

SVG = open("squadcode.svg").read()
# Below 48px fine detail turns to mush, so small sizes drop the window dots and code line, bolden the prompt and grow Peek.
SMALL = open("squadcode-small.svg").read()
# The tile spans 100..924 of the 1024 canvas; this crops to it for full-bleed sizes and drops the macOS drop shadow round it.
FLAT = re.sub(r'<g id="drop">.*?</g>', "", SVG.replace('viewBox="0 0 1024 1024"', 'viewBox="96 96 832 832"'), flags=re.S)


def render(svg, size):
    png = cairosvg.svg2png(bytestring=svg.encode(), output_width=size, output_height=size)
    return Image.open(io.BytesIO(png)).convert("RGBA")


# Linux: inset art from 64px up, full-bleed below that, and the simpler art under 48.
for size in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
    art = SVG if size >= 64 else FLAT if size >= 48 else SMALL
    render(art, size).save(f"png/squadcode-{size}.png")

# Windows: each size drawn on its own, so the small ones get the simpler art.
big = render(FLAT, 256)
small = {s: render(SMALL if s < 48 else FLAT, s) for s in (16, 24, 32, 48, 64, 128)}
big.save("squadcode.ico", sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)], append_images=list(small.values()))

# macOS.
render(SVG, 1024).save("squadcode.icns")
# The macOS menu bar glyph: drawn at 18pt, so 36px for Retina; macOS tints it.
for name in ("squadcode-menubar", "squadcode-menubar-dot"):
    svg = open(f"menubar/{name}.svg").read()
    render(svg, 36).save(f"menubar/{name}.png")
print("icons written")
