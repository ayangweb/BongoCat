# `just` defaults to `sh` on every platform, but Windows has no POSIX shell and
# Git for Windows only adds `cmd` (never `usr\bin`) to PATH, so every recipe
# failed with "could not find the shell `sh`". `cmd.exe` is always resolvable on
# Windows and propagates native exit codes, so a failing `cargo` still fails the
# recipe; a shell that reports success for a failed child command would let a
# broken build pass. Unix keeps the default `sh -cu`.
set windows-shell := ["cmd.exe", "/c"]

# List the available tasks.
default:
    @just --list

# Run the Development product until explicitly quit.
#
# The plugins are built and packed first, so a plugin's edit is on the model window
# after one launch rather than after a second command the author has to remember. A
# plugin whose sources have not moved since it was last packed is skipped, so this
# costs one file-time comparison per plugin rather than a compile and a zip.
dev: plugins
    cargo run --locked -p bongocat-app --release -- --run-seconds 0

# Exercise settings close, reopen, and runtime continuity.
dev-smoke: plugins
    cargo run --locked -p bongocat-app --release -- --run-seconds 4 --settings-window-smoke

# Run a deterministic Live2D diagnostic preview.
preview model="standard" seconds="30":
    cargo run --locked -p bongocat-overlay --release -- "{{model}}" "{{seconds}}"

# Print the single product version source resolved by Cargo.
version:
    @cargo run --locked -q -p bongocat-packaging -- --print-version

# Regenerate the checked-in configuration and window-state JSON Schemas.
schema:
    cargo run --locked -p bongocat-config --features schema-generation --bin generate_json_schemas

# Run the workspace tests.
test:
    cargo test --locked --workspace

# Run all default workspace quality gates.
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

# Build every plugin in `plugins/` whose sources changed since it was last packed, and
# pack it into <plugins>/build/<id>.zip. One plugin is named in the development catalog
# by that archive and in nothing else, so this is the whole of what a plugin author runs:
#
#   just plugins
#   just plugins --plugin-target x86_64-pc-windows-msvc
#
# The build runs in `plugins/`, which is its own Cargo workspace with its own lockfile,
# so nothing a plugin depends on reaches the product's dependency graph. `just dev` runs
# this first; install the result from Settings → Plugins, and the app performs the same
# unpack a release would.
plugins *args:
    cargo run --locked -p bongocat-packaging -- --pack-plugins {{args}}

# Build and pack one plugin by id, for when you want to hear about one:
#
#   just plugin pomodoro
#   just plugin pomodoro --plugin-target x86_64-pc-windows-msvc
#
# Same workspace, same lockfile and same archive as `plugins` above; the difference is
# that this names one, so a compile error in an unrelated plugin is not in the way.
plugin id *args:
    cargo run --locked -p bongocat-packaging -- --pack-plugin {{id}} {{args}}

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

# Compose this version's release notes from CHANGELOG.md and CHANGELOG.zh-CN.md into
# <file>, followed by the download links, model gallery and sponsors this tool knows.
# The release pipeline feeds that one file to both `release-manifest` and the
# GitHub release body, so the release page and the update window cannot disagree:
#
#   just release-notes release-notes.md
#
# The lookup is keyed on the product version `just version` prints, so it fails when the
# version was bumped without a matching changelog entry. Only the changelog entry is
# versioned; the trailing block is generated, so the wording, the gallery and the
# sponsor list are constants in `crates/bongocat-packaging`, not changelog text.
release-notes file:
    cargo run --locked -p bongocat-packaging -- --extract-release-notes {{file}}
