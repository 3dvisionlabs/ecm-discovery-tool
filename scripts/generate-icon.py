#!/usr/bin/env python3
"""Generate app icon from corporate SVG logo on branded background."""

import os
import subprocess
from PIL import Image, ImageDraw

SIZE = 1024
CORNER_RADIUS = 180
LOGO_PADDING = 160  # padding around the logo inside the rounded rect

RICH_BLACK = (21, 28, 40)

PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SVG_PATH = os.path.join(PROJECT_ROOT, "references", "3dvisionlabs_Icon_Green.svg")
ICON_DIR = os.path.join(PROJECT_ROOT, "src", "icons")


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

    # Save 1024x1024 PNG
    img.save(os.path.join(ICON_DIR, "icon.png"))
    print("Saved icon.png (1024x1024)")

    # Generate ICO for Windows using ImageMagick (Pillow's ICO writer only saves one size)
    ico_path = os.path.join(ICON_DIR, "icon.ico")
    ico_sizes = "256,128,64,48,32,24,16"
    subprocess.run([
        "convert", os.path.join(ICON_DIR, "icon.png"),
        "-define", f"icon:auto-resize={ico_sizes}",
        ico_path,
    ], check=True)
    print(f"Saved icon.ico ({ico_sizes})")

    # Generate iconset PNGs for macOS (run iconutil -c icns on macOS)
    iconset_dir = os.path.join(ICON_DIR, "icon.iconset")
    os.makedirs(iconset_dir, exist_ok=True)
    icns_sizes = {
        "icon_16x16.png": 16, "icon_16x16@2x.png": 32,
        "icon_32x32.png": 32, "icon_32x32@2x.png": 64,
        "icon_128x128.png": 128, "icon_128x128@2x.png": 256,
        "icon_256x256.png": 256, "icon_256x256@2x.png": 512,
        "icon_512x512.png": 512, "icon_512x512@2x.png": 1024,
    }
    for filename, s in icns_sizes.items():
        img.resize((s, s), Image.LANCZOS).save(os.path.join(iconset_dir, filename))
    print("Saved iconset/ (run 'iconutil -c icns src/icons/icon.iconset -o src/icons/icon.icns' on macOS)")

    # Cleanup
    os.remove(logo_png)


if __name__ == "__main__":
    main()
