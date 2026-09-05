#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SPIKE_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
APP_PATH="$SPIKE_DIR/target/package/BongoCat GPUI Overlay Spike.app"
CONTENTS_PATH="$APP_PATH/Contents"
MACOS_PATH="$CONTENTS_PATH/MacOS"
EXECUTABLE_NAME="bongocat-gpui-overlay-macos-spike"
EXPECTED_BUNDLE_ID="com.ayangweb.bongo-cat"

cargo build --release --locked --manifest-path "$SPIKE_DIR/Cargo.toml"

rm -rf "$APP_PATH"
mkdir -p "$MACOS_PATH"
cp "$SPIKE_DIR/macos/Info.plist" "$CONTENTS_PATH/Info.plist"
cp "$SPIKE_DIR/target/release/$EXECUTABLE_NAME" "$MACOS_PATH/$EXECUTABLE_NAME"

ACTUAL_BUNDLE_ID=$(plutil -extract CFBundleIdentifier raw -o - "$CONTENTS_PATH/Info.plist")
if [ "$ACTUAL_BUNDLE_ID" != "$EXPECTED_BUNDLE_ID" ]; then
  printf 'unexpected bundle id: %s\n' "$ACTUAL_BUNDLE_ID" >&2
  exit 1
fi

codesign --force --sign - --timestamp=none "$APP_PATH"
printf '%s\n' "$APP_PATH"
