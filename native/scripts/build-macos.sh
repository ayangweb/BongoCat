#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
NATIVE_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
PACKAGE_DIR="$NATIVE_DIR/target/package"
APP_PATH="$PACKAGE_DIR/BongoCat.app"
DMG_PATH="$PACKAGE_DIR/BongoCat.dmg"

if ! command -v hdiutil >/dev/null 2>&1; then
    printf '%s\n' 'macOS packaging requires hdiutil' >&2
    exit 1
fi

APP_PATH=$("$SCRIPT_DIR/package-macos.sh")
rm -f "$DMG_PATH"
hdiutil create \
    -volname "BongoCat" \
    -srcfolder "$APP_PATH" \
    -ov \
    -format UDZO \
    "$DMG_PATH" >/dev/null

if [ ! -s "$DMG_PATH" ]; then
    printf 'macOS installer was not created: %s\n' "$DMG_PATH" >&2
    exit 1
fi

printf '%s\n' 'Build completed successfully!' '' 'Installer:' "  $DMG_PATH"