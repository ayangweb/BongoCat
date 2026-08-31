set working-directory := "native"

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

# Run the Native workspace tests.
test:
    cargo test --locked --workspace

# Run all default Native workspace quality gates.
check:
    cargo fmt --all -- --check
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    cargo test --locked --workspace
    cargo check --locked --workspace --release

# Build the product with the immutable Production environment.
[env("BONGOCAT_BUILD_ENV", "production")]
build-production:
    cargo build --locked -p bongocat-app --release
