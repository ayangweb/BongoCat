#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SPIKE_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
APP_PATH="$SPIKE_DIR/target/package/BongoCat GPUI Spike.app"
CONTENTS_PATH="$APP_PATH/Contents"
MACOS_PATH="$CONTENTS_PATH/MacOS"
EXECUTABLE_NAME="bongocat-gpui-settings-spike"

cargo build --release --locked --manifest-path "$SPIKE_DIR/Cargo.toml"

rm -rf "$APP_PATH"
mkdir -p "$MACOS_PATH"
cp "$SPIKE_DIR/macos/Info.plist" "$CONTENTS_PATH/Info.plist"
cp "$SPIKE_DIR/target/release/$EXECUTABLE_NAME" "$MACOS_PATH/$EXECUTABLE_NAME"

# Ad-hoc signing makes bundle integrity testable. Release signing is a separate gate.
codesign --force --sign - --timestamp=none "$APP_PATH"

printf '%s\n' "$APP_PATH"
