#!/bin/sh

set -eu

if [ -z "${BONGOCAT_BUILD_ENV+x}" ]; then
    printf '%s\n' 'BONGOCAT_BUILD_ENV must be explicitly set to development or production' >&2
    exit 1
fi
case "$BONGOCAT_BUILD_ENV" in
    development|production) ;;
    *)
        printf 'invalid BONGOCAT_BUILD_ENV: %s\n' "$BONGOCAT_BUILD_ENV" >&2
        exit 1
        ;;
esac

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
NATIVE_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
APP_PATH="$NATIVE_DIR/target/package/BongoCat.app"
CONTENTS_PATH="$APP_PATH/Contents"
MACOS_PATH="$CONTENTS_PATH/MacOS"
RESOURCES_PATH="$CONTENTS_PATH/Resources"
EXECUTABLE_NAME="bongocat-app"
EXPECTED_BUNDLE_ID="com.ayangweb.bongo-cat"
EXPECTED_MINIMUM_SYSTEM_VERSION="12.0"

cargo build --release --locked --manifest-path "$NATIVE_DIR/Cargo.toml" -p bongocat-app

rm -rf "$APP_PATH"
mkdir -p "$MACOS_PATH" "$RESOURCES_PATH"
cp "$NATIVE_DIR/macos/Info.plist" "$CONTENTS_PATH/Info.plist"
cp "$NATIVE_DIR/target/release/$EXECUTABLE_NAME" "$MACOS_PATH/$EXECUTABLE_NAME"
cp -R "$NATIVE_DIR/resources/models" "$RESOURCES_PATH/models"
python3 "$NATIVE_DIR/../tools/record-native-provenance.py" \
    --workspace "$NATIVE_DIR" \
    --output "$RESOURCES_PATH/build-provenance.json" \
    --target "$(rustc -vV | awk -F': ' '$1 == "host" { print $2; exit }')" \
    --profile release \
    --features default \
    --environment "$BONGOCAT_BUILD_ENV"

ACTUAL_BUNDLE_ID=$(plutil -extract CFBundleIdentifier raw -o - "$CONTENTS_PATH/Info.plist")
if [ "$ACTUAL_BUNDLE_ID" != "$EXPECTED_BUNDLE_ID" ]; then
    printf 'unexpected bundle id: %s\n' "$ACTUAL_BUNDLE_ID" >&2
    exit 1
fi
if [ "$(plutil -extract LSMinimumSystemVersion raw -o - "$CONTENTS_PATH/Info.plist")" != "$EXPECTED_MINIMUM_SYSTEM_VERSION" ]; then
    printf '%s\n' 'LSMinimumSystemVersion must be 12.0' >&2
    exit 1
fi
if [ "$(plutil -extract LSMultipleInstancesProhibited raw -o - "$CONTENTS_PATH/Info.plist")" != "true" ]; then
    printf '%s\n' 'LSMultipleInstancesProhibited must be true' >&2
    exit 1
fi
for model in standard keyboard gamepad; do
    if [ ! -d "$RESOURCES_PATH/models/$model" ]; then
        printf 'bundled preset model is missing: %s\n' "$model" >&2
        exit 1
    fi
done

# Ad-hoc signing makes bundle integrity testable. Distribution signing,
# hardened runtime and notarization remain separate release gates.
codesign --force --sign - --timestamp=none "$APP_PATH"
codesign --verify --deep --strict "$APP_PATH"

printf '%s\n' "$APP_PATH"
