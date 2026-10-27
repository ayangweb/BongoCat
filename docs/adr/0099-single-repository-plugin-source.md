# ADR-0099: Keep plugin source and publication in BongoCat

## Status

Accepted.

## Context

The plugin process protocol isolates plugins behind versioned JSON messages,
and the host builds plugins as separate executables from the same workspace.
Keeping source, packaging and publication together gives protocol changes one
review and verification path.

## Decision

`ayangweb/BongoCat` is the only source repository for the plugin SDK, official
plugins, packaging, tests and release tooling.

- The SDK and all three official plugins are workspace members. They are built
  as separate executables and are never linked into the application.
- `just dev` and `just dev-smoke` build the host platform packages from
  `plugins/` under `target/plugin-dev`.
- Application tags remain `v*`. Plugin tags use `plugins-v*`, and plugin
  Releases are explicitly non-Latest so `releases/latest` continues to identify
  the application update manifest.
- Plugin packages belong to immutable plugin Releases. Production requests the
  signed catalog from the dedicated `plugin-catalog` branch, whose only current
  payload is `plugins.json` with absolute HTTPS asset URLs.
- The plugin signing key and public key remain separate from the application
  update key. Their public halves are versioned in
  `release-trust/plugin.pub` and `release-trust/app.pub`, embedded
  at compile time, and never read from a mutable runtime path. The plugin
  workflow updates the catalog branch only after it has verified the published
  package bytes, state and signature.
- The first plugin Release is `plugins-v1.0.0` and is built entirely from the
  current workspace. Every later release uses the normal assembler and keeps
  the immutable history check, so reusing a version after any future change is
  rejected.

## Consequences

- Plugin and host protocol changes can be reviewed, tested and released from
  one checkout and one pull request.
- Application releases and plugin releases remain independently versioned and
  independently installable; neither workflow rebuilds the other artifact.
- `plugins-v*` must always be published with `--latest=false`. A failed Latest
  assertion stops publication rather than silently breaking application update
  discovery.
- The plugin catalog branch is a publication surface, not durable release
  history. Deleting or losing it can be recovered from the signed
  `plugins.json` asset attached to the newest plugin Release.

## Verification

- `just check` covers the combined workspace, including the SDK, plugins and
  plugin packaging crate.
- `just plugin-tests` builds the host plugin executables and exercises real
  process installation, lifecycle and UI paths.
- `tools/tests/test_plugin_distribution_contract.py` pins the tag namespaces,
  fixed catalog URL, signing-key separation, in-repository development flow and
  non-Latest release rule.
- The plugin release workflow verifies the signed draft, immutable package
  digests and absence of a `plugins-v*` repository Latest before updating the
  catalog branch.
