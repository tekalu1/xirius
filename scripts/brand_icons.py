#!/usr/bin/env python3
"""Xirius brand icons (UI-349): build assets/brand/xirius.ico for Windows.

  uv run --no-project --with "pillow>=9.1" scripts/brand_icons.py

PIL draws the rounded-square face and the star of app-icon-win.svg (36 px and up)
and app-icon-win-small.svg (16-32 px, optically thickened) at a high supersample
factor and shrinks each frame with a BOX filter, then packs every size Windows
asks for (16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256) into one
ico as PNG frames. Every Pillow since 9.1 (Image.Resampling) gives identical
pixels; the committed ico was written with Pillow 10.4.0 (the PNG encoding, and
so the bytes, can differ in other versions).

The SVGs are the source of truth; the geometry below mirrors them so the script
needs no SVG rasterizer. check_svg reads the SVGs and refuses to run when the
constants have drifted from them, and after writing, the saved ico is opened
again and every frame is compared with the rendered image.
"""
from __future__ import annotations

import re
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "assets" / "brand"
OUT = BRAND / "xirius.ico"

CANVAS = 1024.0  # viewBox side shared by app-icon-win*.svg
# Each frame is drawn at max(size * SS, 1024) and shrunk with a BOX filter, so
# sizes under 128 are in fact rendered at 1024 (a higher factor than SS).
SS = 8

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


def check_svg() -> None:
    """Refuse to run when the constants above no longer match app-icon-win*.svg."""
    for name, star in (("app-icon-win.svg", STAR_NORMAL), ("app-icon-win-small.svg", STAR_SMALL)):
        text = (BRAND / name).read_text(encoding="utf-8")
        errors = []
        rect = re.search(
            r'<rect x="(\d+)" y="(\d+)" width="(\d+)" height="(\d+)" rx="(\d+)" fill="(#[0-9a-f]+)"',
            text,
        )
        if rect is None:
            errors.append("no face <rect> found")
        else:
            x, y, w, h = (float(v) for v in rect.groups()[:4])
            if (x, y, x + w, y + h) != FACE:
                errors.append("rect does not match FACE")
            if float(rect.group(5)) != FACE_RX:
                errors.append("rect rx does not match FACE_RX")
            if rect.group(6) != FACE_FILL:
                errors.append("rect fill does not match FACE_FILL")
        scale = re.search(r"scale\((\d+(?:\.\d+)?)\)", text)
        if scale is None or float(scale.group(1)) != STAR_SCALE:
            errors.append("star scale does not match STAR_SCALE")
        paths = re.findall(r'<path fill="(#[0-9a-f]+)" d="([^"]+)"', text)
        polys = tuple(
            tuple((int(x), int(y)) for x, y in re.findall(r"(\d+) (\d+)", d))
            for _, d in paths
        )
        if [fill for fill, _ in paths] != [STAR_DARK, STAR_LIGHT] or polys != star:
            errors.append("star paths do not match the star polygons")
        if errors:
            raise SystemExit(f"{name}: {'; '.join(errors)}; update the SVG or the script")


def verify(frames: dict[int, Image.Image]) -> None:
    """Open the saved ico and check every frame against the rendered image."""
    im = Image.open(OUT)
    got = sorted(im.ico.sizes())
    want = [(s, s) for s in SIZES]
    if got != want:
        raise SystemExit(f"{OUT}: sizes {got} != {want}")
    for s in SIZES:
        im.size = (s, s)
        im.load()
        if im.convert("RGBA").tobytes() != frames[s].tobytes():
            raise SystemExit(f"{OUT}: frame {s} differs from the rendered image")


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
    check_svg()
    frames = {s: render(s) for s in SIZES}
    frames[SIZES[-1]].save(
        OUT,
        format="ICO",
        sizes=[(s, s) for s in SIZES],
        append_images=[frames[s] for s in SIZES[:-1]],
    )
    verify(frames)
    print(f"wrote {OUT} ({len(SIZES)} sizes: {', '.join(map(str, SIZES))})")


if __name__ == "__main__":
    main()
