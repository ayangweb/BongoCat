# `just` defaults to `sh` on every platform, but Windows has no POSIX shell and
# Git for Windows only adds `cmd` (never `usr\bin`) to PATH, so every recipe
# failed with "could not find the shell `sh`". `cmd.exe` is always resolvable on
# Windows and propagates native exit codes, so a failing `cargo` still fails the
# recipe; a shell that reports success for a failed child command would let a
# broken build pass. Unix keeps the default `sh -cu`.
set windows-shell := ["cmd.exe", "/c"]

# List the available Native Rewrite tasks.
default:
    @just --list

# Run the Development product until explicitly quit.
dev:
    cargo run --locked -p bongocat-app --release -- --run-seconds 0

# Exercise settings close, reopen, and runtime continuity.
dev-smoke:
    cargo run --locked -p bongocat-app --release -- --run-seconds 4 --settings-window-smoke

# Run a deterministic Live2D diagnostic preview.
preview model="standard" seconds="30":
    cargo run --locked -p bongocat-overlay --release -- "{{model}}" "{{seconds}}"

# Print the single product version source resolved by Cargo.
version:
    @cargo run --locked -q -p bongocat-packaging -- --print-version

# Run the Native workspace tests.
test:
    cargo test --locked --workspace

# Run all default Native workspace quality gates.
check:
    cargo fmt --all -- --check
    cargo clippy --locked --workspace --all-targets --all-features --exclude bongocat-app -- -D warnings
    cargo clippy --locked -p bongocat-app --all-targets --features storage-test-injection -- -D warnings
    cargo clippy --locked -p bongocat-app --all-targets --features production -- -D warnings
    cargo test --locked --workspace
    cargo check --locked --workspace --release

# `--target`, `--environment` and `--formats` are forwarded to the packaging
# entry point, which is the single source of truth for targets and artifacts:
#
#   just build
#   just build --target x86_64-apple-darwin
#   just build --environment development --formats app
#
# Build the Production product and package the release artifacts.
build *args:
    cargo run --locked -p bongocat-packaging -- {{args}}

# Generate the Minisign key pair that signs update payloads (one-time, offline).
keygen file:
    cargo run --locked -p bongocat-packaging -- --generate-signing-key {{file}}

# Each `just build` writes one manifest fragment per target, but the updater requests a
# single shared manifest, so a multi-target release merges the fragments once before
# publishing:
#
#   just manifest target/package target/package/macos-aarch64.json ...
#   just release-manifest target/package NOTES.md target/package/*.json
#
# Merge the per-target manifest fragments into the shared release manifest.
manifest directory *fragments:
    cargo run --locked -p bongocat-packaging -- --merge-manifests {{directory}} {{fragments}}

# Merge the fragments and announce the release changelog read from <notes>, which the
# update window shows. The release pipeline uses this one; `manifest` is the same merge
# without notes.
release-manifest directory notes *fragments:
    cargo run --locked -p bongocat-packaging -- --merge-manifests {{directory}} --release-notes {{notes}} {{fragments}}
