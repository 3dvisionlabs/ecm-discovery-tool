#!/usr/bin/env python3
"""Generate app-icon.png from the corporate SVG logo on a branded background."""

import os
import subprocess
from PIL import Image, ImageDraw

SIZE = 1024
CORNER_RADIUS = 180
LOGO_PADDING = 160  # padding around the logo inside the rounded rect

RICH_BLACK = (21, 28, 40)

PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SVG_PATH = os.path.join(PROJECT_ROOT, "references", "3dvisionlabs_Icon_Green.svg")
ICON_DIR = PROJECT_ROOT


def rounded_rect_mask(size, radius):
    mask = Image.new("L", size, 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle([0, 0, size[0] - 1, size[1] - 1], radius=radius, fill=255)
    return mask


def main():
    os.makedirs(ICON_DIR, exist_ok=True)

    # Render SVG to a large PNG using rsvg-convert
    logo_size = SIZE - 2 * LOGO_PADDING
    logo_png = os.path.join(ICON_DIR, "_logo_tmp.png")
    subprocess.run([
        "rsvg-convert", "-w", str(logo_size), "-h", str(logo_size),
        SVG_PATH, "-o", logo_png
    ], check=True)

    logo = Image.open(logo_png).convert("RGBA")

    # Create rounded rect background
    img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    bg = Image.new("RGBA", (SIZE, SIZE), RICH_BLACK + (255,))
    mask = rounded_rect_mask((SIZE, SIZE), CORNER_RADIUS)
    img.paste(bg, (0, 0), mask)

    # Center the logo on the background
    offset_x = (SIZE - logo.width) // 2
    offset_y = (SIZE - logo.height) // 2
    img.paste(logo, (offset_x, offset_y), logo)

    # Save 1024x1024 PNG; `npm run icons` derives the platform icons from it
    img.save(os.path.join(ICON_DIR, "app-icon.png"))
    print("Saved app-icon.png (1024x1024). Now run: npm run icons")

    # Cleanup
    os.remove(logo_png)


if __name__ == "__main__":
    main()
