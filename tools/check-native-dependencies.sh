#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPOSITORY_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
EXPECTED_CARGO_DENY_VERSION="cargo-deny 0.20.2"
ACTUAL_CARGO_DENY_VERSION=$(cargo deny --version)

if [ "$ACTUAL_CARGO_DENY_VERSION" != "$EXPECTED_CARGO_DENY_VERSION" ]; then
    printf 'expected %s, found %s\n' "$EXPECTED_CARGO_DENY_VERSION" "$ACTUAL_CARGO_DENY_VERSION" >&2
    exit 1
fi

cd "$REPOSITORY_ROOT"

check_release_dependency_tree() {
    target=$1
    forbidden=$(cargo tree \
        --manifest-path native/Cargo.toml \
        --locked \
        --target "$target" \
        --edges normal,build \
        --prefix none \
        | awk '{print $1}' \
        | sed 's/\(.*\) v[0-9].*/\1/' \
        | awk '
            $0 == "tauri" || $0 ~ /^tauri-/ ||
            $0 == "wry" || $0 ~ /^webview/ ||
            $0 == "webview2" || $0 == "nodejs-sys" ||
            $0 == "neon" || $0 == "deno_core" ||
            $0 == "quickjs" || $0 == "javascriptcore" { print }
        ' \
        | sort -u)
    if [ -n "$forbidden" ]; then
        printf 'forbidden release dependency on %s: %s\n' "$target" "$forbidden" >&2
        exit 1
    fi
    printf 'release dependency tree clean: %s\n' "$target"
}

for target in \
    x86_64-pc-windows-msvc \
    aarch64-pc-windows-msvc \
    x86_64-apple-darwin \
    aarch64-apple-darwin; do
    check_release_dependency_tree "$target"
done

for manifest in native/Cargo.toml spikes/*/Cargo.toml tools/cubism-bindgen/Cargo.toml tools/legacy-config-inspector/Cargo.toml; do
    printf 'checking dependency policy: %s\n' "$manifest"
    cargo deny \
        --manifest-path "$manifest" \
        --locked \
        --config "$REPOSITORY_ROOT/deny.toml" \
        check licenses sources \
        --allow license-not-encountered
done
