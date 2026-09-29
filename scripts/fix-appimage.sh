#!/usr/bin/env bash
set -euo pipefail

# fix-appimage.sh
# Removes files from a Tauri AppImage that break on newer distributions, and
# repacks it. The host provides both on every Linux desktop.
#
# 1. libwayland-*: the AppImage bundles WebKitGTK and libwayland from the build system
# (ubuntu-22.04), but Mesa (libEGL, libgbm) always comes from the host. On
# newer distributions (e.g. Fedora 44, Mesa 26) the old libwayland does not
# match Mesa: WebKitWebProcess aborts with "Could not create default EGL
# display: EGL_BAD_PARAMETER" and the window stays empty. Every desktop with
# GTK has libwayland installed, so the host's version is used instead.
#
# 2. usr/bin/xdg-open (xdg-utils 1.1 from ubuntu-22.04): does not know KDE
# Plasma 6 and calls the non-existent `kde-open6`, so "Open" silently does
# nothing. Without it the host's xdg-open is used.
#
# Usage: ./scripts/fix-appimage.sh <file.AppImage> [appimagetool]
#   appimagetool: path to appimagetool; downloaded if omitted

APPIMAGE="$(readlink -f "$1")"
TOOL="${2:-}"
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.0/appimagetool-x86_64.AppImage"

WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT
cd "$WORKDIR"

# --appimage-extract works without FUSE
"$APPIMAGE" --appimage-extract >/dev/null

removed=$(find squashfs-root \( -name 'libwayland-*.so*' -o -path 'squashfs-root/usr/bin/xdg-open' \) -print -delete)
[ -n "$removed" ] || { echo "Nothing to remove in $APPIMAGE."; exit 0; }
echo "Removed from AppImage:"
echo "$removed" | sed 's#^squashfs-root/#  #'

if [ -z "$TOOL" ]; then
  TOOL="$WORKDIR/appimagetool"
  curl -fsSL -o "$TOOL" "$APPIMAGETOOL_URL"
  chmod +x "$TOOL"
fi

# Extract-and-run: CI runners have no FUSE
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$TOOL" --no-appstream squashfs-root "$WORKDIR/fixed.AppImage" >/dev/null
mv "$WORKDIR/fixed.AppImage" "$APPIMAGE"
chmod +x "$APPIMAGE"
echo "Repacked $APPIMAGE"
