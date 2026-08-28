# Legacy Config Inspector

This isolated Phase 0 archaeology tool reads the five JSON stores produced by
the historical Tauri Pinia persistence plugin and prints a deterministic risk
report. It never writes the source directory. Native Rewrite does not import
legacy configuration, so this tool must not become a product dependency.

The report contains only normalized non-secret settings, inventory counts, store
states, and stable diagnostic codes. It never prints the input directory, model
paths or ids, shortcut values, pressed keys, or raw JSON fragments.

## Usage

```sh
cargo run --locked -- --input ../../shared/config/legacy-pinia/default
```

Exit code `0` means all five stores were inspectable. Exit code `2` means the
report is blocked by a missing, unreadable, invalid, or non-object store. No exit
code authorizes writing or deleting old data.

The inspector applies only provisional safety ranges needed to expose legacy
data hazards in Phase 0. These ranges and its `report_version` are unrelated to
the Native Rewrite `schema_version` contract.

## Dependencies

- `serde 1.0.228` and `serde_json 1.0.149` are exact-pinned in this isolated
  workspace and locked in `Cargo.lock`.
- Both are maintained Rust ecosystem serialization crates under MIT OR Apache-2.0.
- Their types remain private implementation details; the tool can replace them
  without changing the legacy store or future product configuration contracts.

## Verification

```sh
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo check --release --locked
```
