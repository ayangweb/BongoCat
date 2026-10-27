# ADR-0100: Retire the Phase 0 spike workspaces

## Status

Accepted (2026-10-27).

## Context

Phase 0 validated the ownership models with independent prototypes under
`spikes/`. Each was its own Cargo workspace with its own lockfile, Dependabot
entry and CI job (`contract-spikes`, `model-package-platforms`, `macos-spikes`,
`windows-input-spike`, `windows-gpui-spikes`). ADR-0011 let the product
workspace proceed while those prototypes stayed as separate oracles.

By the 2.4.0 product the same behaviour lives in the product crates: runtime,
config, model store, input, UI and overlay own the contracts and are covered by
`cargo test --workspace`, and the `product-smoke`, `packaging-smoke` and
`windows-platform-smoke` jobs exercise the real binaries on both shipped
platforms. The spikes' only remaining role was duplicate maintenance — every
dependency bump, config field and key-image rename had to be mirrored — plus
four CI jobs that compiled and ran a second copy of the same code.

## Decision

Delete `spikes/` and all of its wiring:

- the `contract-spikes`, `model-package-platforms`, `macos-spikes`,
  `windows-input-spike` and `windows-gpui-spikes` jobs in `verify.yml`;
- the spike directories in `.github/dependabot.yml`;
- the `spikes/*/Cargo.toml` loop in `tools/check-dependencies.sh`;
- the spike references in the fixture READMEs and the tool contract tests.

## Consequences

- The shared fixtures are now checked only by the product crates
  (`bongocat-runtime`'s `shared_input_fixtures` test and the `bongocat-model` /
  `bongocat-model-store` fixture contracts) plus the Python oracles
  `tools/run-input-fixtures.py` and `tools/validate-fixtures.py`.
- `preset-model3-index.json` stays a frozen snapshot; regenerating it now means
  updating the product parser's snapshot test, not the removed spike parser.
- No job runs on Linux any more. Linux was never a shipped target and the
  product workspace does not build there; the platform-neutral contracts are
  covered by the `workspace` job on macOS and Windows.
- Older ADRs (0009, 0038, 0039, 0052, 0054, 0067, 0070, 0074, 0079, 0085) still
  name the retired paths as the record of the decisions taken at the time.
