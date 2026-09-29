#!/usr/bin/env bash
set -euo pipefail

# collect-release-files.sh
# Collects the release files of the current platform from target/release into
# one directory, named so that the platform is obvious:
#   ecm-discovery_<version>_<os>_<arch>[_<variant>].<ext>
# and creates the portable archives (app + command line tool + LICENSE).
# Run after `npm run make` (release.yml: after the AppImage fix).
#
# Usage: ./scripts/collect-release-files.sh [out-dir]   (default: target/release/upload)

cd "$(dirname "$0")/.."
OUT="${1:-target/release/upload}"
VERSION=$(node -p "require('./package.json').version")
NAME="ecm-discovery_${VERSION}"
BIN=target/release
BUNDLE=$BIN/bundle

shopt -s nullglob
rm -rf "$OUT"
mkdir -p "$OUT"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT

# Copy the single file matching the pattern to $OUT/<name>
take() {
  local name="$1"
  shift
  [ $# -eq 1 ] || { echo "ERROR: expected exactly one file for $name, found $#: $*" >&2; exit 1; }
  cp "$1" "$OUT/$name"
}

case "$(uname -s)" in
  Linux)
    arch=$(uname -m)                                  # x86_64
    deb_arch=$([ "$arch" = x86_64 ] && echo amd64 || echo "$arch")
    take "${NAME}_linux_${deb_arch}.deb" "$BUNDLE"/deb/*.deb
    take "${NAME}_linux_${arch}.rpm" "$BUNDLE"/rpm/*.rpm
    take "${NAME}_linux_${arch}.AppImage" "$BUNDLE"/appimage/*.AppImage
    dir="${NAME}_linux_${arch}"
    mkdir "$STAGE/$dir"
    cp "$BIN/ecm-discovery" "$BIN/ecm-discovery-cli" LICENSE "$STAGE/$dir/"
    tar czf "$OUT/$dir.tar.gz" -C "$STAGE" "$dir"
    ;;
  Darwin)
    arch=$(uname -m)                                  # arm64
    take "${NAME}_macos_${arch}.dmg" "$BUNDLE"/dmg/*.dmg
    # The command line tool is inside the app (Contents/MacOS)
    ditto -c -k --keepParent "$BUNDLE/macos/Edge Camera Discovery.app" "$OUT/${NAME}_macos_${arch}.zip"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    take "${NAME}_windows_x64_setup.exe" "$BUNDLE"/nsis/*.exe
    cp "$BIN/ecm-discovery.exe" "$BIN/ecm-discovery-cli.exe" LICENSE "$STAGE/"
    (cd "$STAGE" && 7z a -tzip -bso0 "portable.zip" ecm-discovery.exe ecm-discovery-cli.exe LICENSE)
    mv "$STAGE/portable.zip" "$OUT/${NAME}_windows_x64_portable.zip"
    ;;
  *)
    echo "ERROR: unknown platform $(uname -s)" >&2
    exit 1
    ;;
esac

echo "Release files in $OUT:"
ls -1 "$OUT" | sed 's/^/  /'
