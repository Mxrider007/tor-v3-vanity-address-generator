#!/usr/bin/env python3
"""Regenerate the application icons (requires Pillow: pip install pillow).

Writes gui/assets/icon.png (256 px, window icon), gui/assets/icon.ico
(Windows .exe icon) and packaging/macos/AppIcon.icns (macOS app bundle).
A keyhole on a teal-to-indigo rounded tile, drawn at 1024 px.
"""
from PIL import Image, ImageDraw

S = 1024
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
grad = Image.new("RGBA", (S, S))
px = grad.load()
c0, c1 = (20, 184, 166), (79, 70, 229)  # teal-500 -> indigo-600
for y in range(S):
    for x in range(S):
        t = (x + y) / (2 * (S - 1))
        px[x, y] = tuple(int(c0[i] + (c1[i] - c0[i]) * t) for i in range(3)) + (255,)
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle([40, 40, S - 40, S - 40], radius=220, fill=255)
img.paste(grad, (0, 0), mask)
d = ImageDraw.Draw(img)
cx, cy = S // 2, S // 2 - 10
for r, a in ((360, 60), (300, 90)):
    d.ellipse([cx - r, cy - r, cx + r, cy + r], outline=(255, 255, 255, a), width=26)
white = (255, 255, 255, 255)
d.ellipse([cx - 120, cy - 190, cx + 120, cy + 50], fill=white)
d.polygon([(cx - 70, cy), (cx + 70, cy), (cx + 115, cy + 250), (cx - 115, cy + 250)], fill=white)

small = img.resize((256, 256), Image.LANCZOS)
small.save("gui/assets/icon.png", optimize=True)
small.save("gui/assets/icon.ico", sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
img.save("packaging/macos/AppIcon.icns", sizes=[(s, s) for s in (16, 32, 64, 128, 256, 512, 1024)])
print("icons written")
