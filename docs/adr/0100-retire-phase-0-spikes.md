# ADR-0100: Retire the Phase 0 spike workspaces and their fixture snapshots

## Status

Accepted (2026-10-27).

## Context

Phase 0 validated the ownership models with independent prototypes under
`spikes/`. Each was its own Cargo workspace with its own lockfile, dependency
maintenance entry and CI job (`contract-spikes`, `model-package-platforms`, `macos-spikes`,
`windows-input-spike`, `windows-gpui-spikes`). ADR-0011 let the product
workspace proceed while those prototypes stayed as separate oracles.

By the 2.4.0 product the same behaviour lives in the product crates: runtime,
config, model store, input, UI and overlay own the contracts and are covered by
`cargo test --workspace`, and the `product-smoke` and
`windows-platform-smoke` jobs exercise the real binaries on both shipped
platforms. The spikes' only remaining role was duplicate maintenance — every
dependency bump, config field and key-image rename had to be mirrored — plus
four CI jobs that compiled and ran a second copy of the same code.

Two model fixture snapshots were produced by the spike parser rather than a
product test. Once that parser was retired, no Rust or Python test read
`preset-model3-index.json` or `preset-models.json`; they only remained as
historical snapshots and documentation references.

## Decision

Delete `spikes/` and all of its wiring:

- the `contract-spikes`, `model-package-platforms`, `macos-spikes`,
  `windows-input-spike` and `windows-gpui-spikes` jobs in `verify.yml`;
- the spike directories from the former Dependabot configuration, and that
  configuration itself;
- the `spikes/*/Cargo.toml` loop in `tools/check-dependencies.sh`;
- the spike references in fixture READMEs and tool contract tests.

Also delete `shared/fixtures/model-fixtures/preset-model3-index.json` and
`preset-models.json`. The product tests now prepare and validate the three
tracked preset packages directly from `resources/models/`, so maintaining a
second frozen index has no current consumer.

## Consequences

- Shared fixtures are checked only by the product crates
  (`bongocat-runtime`'s `shared_input_fixtures` test and the `bongocat-model` /
  `bongocat-model-store` fixture contracts) plus the Python oracles
  `tools/run-input-fixtures.py` and `tools/validate-fixtures.py`.
- The three preset packages remain covered by product parser, catalog and
  Cubism binding tests. If a future regression needs a frozen index, generate
  it from the product parser and add a test that consumes it in the same
  change.
- No job runs on Linux any more. Linux was never a shipped target and the
  product workspace does not build there; platform-neutral contracts are
  covered by the `workspace` job on macOS and Windows.
- The no-device WGI startup/shutdown evidence that ran inside
  `windows-input-spike` via `bongocat-platform --example gilrs_gamepad_smoke`
  moved to `windows-platform-smoke`, on the same Windows runner.
- Older ADRs (0009, 0038, 0039, 0052, 0054, 0067, 0070, 0074, 0079, 0085) still
  name the retired paths as the record of the decisions taken at the time.
