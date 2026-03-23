#!/usr/bin/env bash
set -euo pipefail

# Build script for macOS — run this on a Mac to produce .dmg / .zip artifacts
# Usage: ./scripts/build-mac.sh

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "==> Edge Camera Discovery — macOS build"

# --- Prerequisites ---
if ! command -v node &>/dev/null; then
  echo "ERROR: Node.js is not installed or not in PATH." >&2
  echo ""
  echo "Install Node.js using one of these methods:"
  echo "  brew install node"
  echo "  Or download from https://nodejs.org/"
  echo ""
  echo "After installing, open a new terminal and re-run this script."
  exit 1
fi

if ! command -v iconutil &>/dev/null; then
  echo "ERROR: 'iconutil' not found. This script must be run on macOS." >&2
  exit 1
fi

echo "    Node $(node -v) | npm $(npm -v)"

cd "$PROJECT_DIR"

# --- Install dependencies ---
echo "==> Installing dependencies..."
npm install

# --- Generate .icns icon ---
ICONSET="src/icons/icon.iconset"
ICNS="src/icons/icon.icns"

if [ ! -d "$ICONSET" ]; then
  echo "ERROR: Icon set not found at $ICONSET" >&2
  exit 1
fi

echo "==> Generating $ICNS from $ICONSET..."
iconutil -c icns "$ICONSET" -o "$ICNS"

# --- Build ---
echo "==> Running electron-forge make for macOS..."
npm run make:mac

# --- Output ---
echo ""
echo "==> Build complete. Artifacts:"
find out/make -type f \( -name "*.dmg" -o -name "*.zip" \) 2>/dev/null | while read -r f; do
  echo "    $f"
done
