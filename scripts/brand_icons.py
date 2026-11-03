#!/usr/bin/env python3
"""Xirius brand icons (UI-349): build assets/brand/xirius.ico for Windows.

  uv run --no-project --with pillow scripts/brand_icons.py

PIL draws the rounded-square face and the star of app-icon-win.svg (36 px and up)
and app-icon-win-small.svg (16-32 px, optically thickened) at a high supersample
factor and shrinks each frame with a BOX filter, then packs every size Windows
asks for (16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256) into one
ico as PNG frames.

The SVGs are the source of truth; the geometry below mirrors them so the script
needs no SVG rasterizer. If an SVG changes, update FACE and the star polygons to
match.
"""
from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "brand" / "xirius.ico"

CANVAS = 1024.0  # viewBox side shared by app-icon-win*.svg
SS = 8  # supersample factor; the face/star are flat, so 8 is ample

FACE = (32.0, 32.0, 992.0, 992.0)  # rect x=32 y=32 width=960 height=960
FACE_RX = 214.0
FACE_FILL = "#222329"
STAR_SCALE = 1.934  # svg: translate(512 512) scale(1.934) translate(-256 -256)
STAR_DARK = "#5665cd"
STAR_LIGHT = "#9eabde"

# The two <path> polygons of the star, in paint order (dark then light).
STAR_NORMAL = (
    (
        (240, 88), (272, 88), (300, 184), (320, 204), (464, 240), (464, 272),
        (320, 308), (300, 328), (272, 424), (240, 424), (212, 328), (192, 308),
        (48, 272), (48, 240), (192, 204), (212, 184),
    ),
    (
        (240, 88), (272, 88), (300, 184), (314, 198), (198, 314), (192, 308),
        (48, 272), (48, 240), (192, 204), (212, 184),
    ),
)
STAR_SMALL = (
    (
        (232, 88), (280, 88), (300, 184), (320, 204), (464, 232), (464, 280),
        (320, 308), (300, 328), (280, 424), (232, 424), (212, 328), (192, 308),
        (48, 280), (48, 232), (192, 204), (212, 184),
    ),
    (
        (232, 88), (280, 88), (300, 184), (314, 198), (198, 314), (192, 308),
        (48, 280), (48, 232), (192, 204), (212, 184),
    ),
)

SMALL_MAX = 32
SIZES = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256]


def render(size: int) -> Image.Image:
    side = max(size * SS, int(CANVAS))
    f = side / CANVAS
    img = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    x0, y0, x1, y1 = (v * f for v in FACE)
    d.rounded_rectangle([x0, y0, x1 - 1, y1 - 1], radius=FACE_RX * f, fill=FACE_FILL)
    star = STAR_SMALL if size <= SMALL_MAX else STAR_NORMAL
    for poly, fill in zip(star, (STAR_DARK, STAR_LIGHT)):
        d.polygon(
            [
                ((512 + STAR_SCALE * (px - 256)) * f, (512 + STAR_SCALE * (py - 256)) * f)
                for px, py in poly
            ],
            fill=fill,
        )
    return img.resize((size, size), Image.Resampling.BOX)


def main() -> None:
    frames = {s: render(s) for s in SIZES}
    frames[SIZES[-1]].save(
        OUT,
        format="ICO",
        sizes=[(s, s) for s in SIZES],
        append_images=[frames[s] for s in SIZES[:-1]],
    )
    print(f"wrote {OUT} ({len(SIZES)} sizes: {', '.join(map(str, SIZES))})")


if __name__ == "__main__":
    main()
