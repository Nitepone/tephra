#!/usr/bin/env python3
"""Draws the Local Forecast app's weather icons (apps/forecast/icons) with Pillow.

They are drawn in the panel's four grays (black, 85, 170, white) with flat two-tone shading, so
they need no dithering: the app draws them with `dither = false`, and only their antialiased
edges fall between the grays.

Each icon is drawn on each background it is shown on, because tephra flattens transparency onto
white: `dark` (85, the forecast panel with 4 grays), `black` (0, that panel with 2 grays) and
`white` (the "now" column). Run it again after changing a shape:

    python3 scripts/forecast-icons.py
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

S = 512  # drawn at 4x, then reduced
OUT = 128
INK = 0  # outlines
LIGHT, DARK = 170, 85
OUTLINE = 13
BACKGROUNDS = {"dark": 85, "black": 0, "white": 255}


def mask_of(draw_fn):
    m = Image.new("L", (S, S), 0)
    draw_fn(ImageDraw.Draw(m))
    return m


def outlined(img, mask, fill, width=OUTLINE):
    """Paints `mask` with `fill` (a gray, or an image) inside an outline of INK."""
    grown = mask.filter(ImageFilter.MaxFilter(width * 2 + 1))
    img.paste(INK, (0, 0), grown)
    img.paste(fill, (0, 0), mask)


def sun(img, cx, cy, r):
    import math

    def rays(dr):
        for i in range(8):
            t = math.pi * i / 4 + math.pi / 8
            tip = (cx + math.cos(t) * r * 1.62, cy + math.sin(t) * r * 1.62)
            a, b = t - 0.2, t + 0.2
            dr.polygon((tip, (cx + math.cos(a) * r * 1.12, cy + math.sin(a) * r * 1.12), (cx + math.cos(b) * r * 1.12, cy + math.sin(b) * r * 1.12)), 255)

    outlined(img, mask_of(rays), LIGHT, width=9)
    m = mask_of(lambda d: d.ellipse((cx - r, cy - r, cx + r, cy + r), 255))
    outlined(img, m, 255)


def moon(img, cx, cy, r):
    full = mask_of(lambda d: d.ellipse((cx - r, cy - r, cx + r, cy + r), 255))
    bite = mask_of(lambda d: d.ellipse((cx - r * 0.35, cy - r * 1.2, cx + r * 1.45, cy + r * 0.6), 255))
    m = ImageChops.subtract(full, bite)
    outlined(img, m, 255)


def cloud_mask(x, y, w):
    """A cloud `w` wide with its bottom-left corner at (x, y)."""
    k = w / 400

    def d(dr):
        for cx, cy, r in ((90, -95, 80), (190, -150, 115), (300, -100, 85)):
            dr.ellipse((x + (cx - r) * k, y + (cy - r) * k, x + (cx + r) * k, y + (cy + r) * k), 255)
        dr.rounded_rectangle((x + 10 * k, y - 110 * k, x + 390 * k, y), radius=55 * k, fill=255)

    return mask_of(d)


def cloud(img, x, y, w, top=255, bottom=LIGHT):
    """A cloud lit from above: `top` over its upper part, `bottom` under it."""
    fill = Image.new("L", (S, S), top)
    ImageDraw.Draw(fill).rectangle((0, y - 55 * w / 400, S, S), bottom)
    outlined(img, cloud_mask(x, y, w), fill)


def drops(img, points, size=34):
    def d(dr):
        for px, py in points:
            dr.ellipse((px - size * 0.55, py, px + size * 0.55, py + size * 1.1), 255)
            dr.polygon(((px - size * 0.5, py + size * 0.45), (px, py - size * 0.75), (px + size * 0.5, py + size * 0.45)), 255)

    outlined(img, mask_of(d), 255, width=8)


def flakes(img, points, size=30):
    import math

    def arms(dr, width):
        for px, py in points:
            for a in range(3):
                t = math.pi * a / 3
                dx, dy = math.cos(t) * size, math.sin(t) * size
                dr.line((px - dx, py - dy, px + dx, py + dy), fill=255, width=width)

    outer = mask_of(lambda d: arms(d, 26))
    inner = mask_of(lambda d: arms(d, 10))
    img.paste(INK, (0, 0), outer)
    img.paste(255, (0, 0), inner)


def bolt(img, x, y, k=1.0):
    pts = [(x + px * k, y + py * k) for px, py in ((40, 0), (100, 0), (70, 70), (115, 70), (25, 200), (50, 100), (5, 100))]
    outlined(img, mask_of(lambda d: d.polygon(pts, 255)), 255, width=10)


def bars(img, y0):
    def d(dr):
        for i, (x0, x1) in enumerate(((90, 420), (60, 380), (110, 450))):
            yy = y0 + i * 50
            dr.rounded_rectangle((x0, yy, x1, yy + 22), radius=11, fill=255)

    outlined(img, mask_of(d), 255, width=9)


def icon(kind, bg):
    img = Image.new("L", (S, S), bg)
    if kind == "sun":
        sun(img, 256, 256, 130)
    elif kind == "moon":
        moon(img, 256, 256, 160)
    elif kind == "partly":
        sun(img, 185, 185, 100)
        cloud(img, 110, 440, 360)
    elif kind == "partly_night":
        moon(img, 200, 190, 120)
        cloud(img, 110, 430, 360)
    elif kind == "cloudy":
        cloud(img, 160, 300, 300, LIGHT, DARK)
        cloud(img, 50, 420, 380)
    elif kind == "showers":
        sun(img, 175, 150, 80)
        cloud(img, 70, 340, 380)
        drops(img, ((170, 380), (260, 400), (350, 380)))
    elif kind == "rain":
        cloud(img, 60, 320, 390, LIGHT, DARK)
        drops(img, ((130, 365), (220, 395), (310, 365), (400, 395)))
    elif kind == "storm":
        cloud(img, 60, 300, 390, DARK, 0)
        bolt(img, 200, 270, 1.0)
        drops(img, ((120, 360), (360, 360)))
    elif kind == "snow":
        cloud(img, 60, 320, 390)
        flakes(img, ((140, 400), (256, 430), (372, 400)))
    elif kind == "sleet":
        cloud(img, 60, 320, 390, 255, LIGHT)
        drops(img, ((150, 370), (330, 370)))
        flakes(img, ((240, 420),))
    elif kind == "fog":
        cloud(img, 70, 290, 370, 255, LIGHT)
        bars(img, 320)
    return img.resize((OUT, OUT), Image.Resampling.LANCZOS)


# The radar view's overlays, transparent. The ring is drawn over a square view whose half-width
# is RING_OF_VIEW times the ring's radius; the app sizes the view to match.
RING_OF_VIEW = 25 / 60


def ring():
    import math

    n = 1024
    img = Image.new("LA", (n, n), (0, 0))
    d = ImageDraw.Draw(img)
    r = n / 2 * RING_OF_VIEW
    c = n / 2
    for i in range(36):  # dashed
        a0, a1 = i * 10, i * 10 + 6
        d.arc((c - r, c - r, c + r, c + r), a0, a1, fill=(0, 255), width=7)
    return img.resize((512, 512), Image.Resampling.LANCZOS)


def marker():
    n = 256
    img = Image.new("LA", (n, n), (0, 0))
    d = ImageDraw.Draw(img)
    c = n / 2
    d.ellipse((c - 120, c - 120, c + 120, c + 120), fill=(255, 255))  # halo, to show on rain
    d.ellipse((c - 92, c - 92, c + 92, c + 92), fill=(0, 255))
    d.ellipse((c - 62, c - 62, c + 62, c + 62), fill=(255, 255))
    d.ellipse((c - 34, c - 34, c + 34, c + 34), fill=(0, 255))
    return img.resize((64, 64), Image.Resampling.LANCZOS)


KINDS = ("sun", "moon", "partly", "partly_night", "cloudy", "showers", "rain", "storm", "snow", "sleet", "fog")

if __name__ == "__main__":
    out = Path(__file__).resolve().parent.parent / "apps" / "forecast" / "icons"
    for name, bg in BACKGROUNDS.items():
        (out / name).mkdir(parents=True, exist_ok=True)
        for kind in KINDS:
            icon(kind, bg).save(out / name / f"{kind}.png", optimize=True)
    ring().save(out / "ring.png", optimize=True)
    marker().save(out / "marker.png", optimize=True)
    print(f"wrote {len(KINDS) * len(BACKGROUNDS) + 2} icons to {out}")
