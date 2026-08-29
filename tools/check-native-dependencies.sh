#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPOSITORY_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
EXPECTED_CARGO_DENY_VERSION="cargo-deny 0.18.3"
ACTUAL_CARGO_DENY_VERSION=$(cargo deny --version)

if [ "$ACTUAL_CARGO_DENY_VERSION" != "$EXPECTED_CARGO_DENY_VERSION" ]; then
    printf 'expected %s, found %s\n' "$EXPECTED_CARGO_DENY_VERSION" "$ACTUAL_CARGO_DENY_VERSION" >&2
    exit 1
fi

cd "$REPOSITORY_ROOT"

for manifest in spikes/*/Cargo.toml tools/legacy-config-inspector/Cargo.toml; do
    printf 'checking dependency policy: %s\n' "$manifest"
    cargo deny \
        --manifest-path "$manifest" \
        --locked \
        check licenses sources \
        --config "$REPOSITORY_ROOT/deny.toml" \
        --allow license-not-encountered
done
