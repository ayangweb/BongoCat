# ADR-0097: Integrate the independent plugin repository

## Status

Accepted.

## Context

ADR-0090 through ADR-0096 established the plugin protocol, host lifecycle,
Plugin Center and three official plugins. Those plugins and the plugin SDK were
initially built from workspace members of the application repository, and a
master-branch workflow uploaded plugin assets to the application's latest
release.

That arrangement made a plugin change wait for application CI, tied plugin
release history to an application release, and put host and plugin signing in
one workflow. The plugin SDK is already a versioned process protocol rather than
a Rust ABI, so the host does not need plugin source in its workspace.

## Decision

The canonical plugin development and distribution repository is
`https://github.com/ayangweb/BongoCat-Plugins`.

- The SDK, official plugin implementations, plugin packaging, catalog assembly,
  plugin signing and plugin changelogs live there.
- The application consumes the SDK as a Git dependency pinned to a full commit
  revision. It does not track a branch or consume a plugin release at build
  time.
- The application repository contains only host-side protocol consumption,
  installation, lifecycle, storage, Plugin Center UI and overlay rendering.
- Official plugins are never workspace members of the application.
- The application has no plugin release workflow, plugin release scripts, plugin
  changelog or plugin catalog packager.

Development builds read an optional plugin directory named by
`BONGOCAT_PLUGIN_DEV_DIRECTORY`. `just dev` and `just dev-smoke` prepare that
directory through `tools/dev_plugins.py`: it uses `BONGOCAT_PLUGINS_ROOT` when
set, otherwise clones `BongoCat-Plugins` under `target/plugin-dev`, runs the
repository's host-platform `catalog dev` command, and sets the generated
directory for the application. The automatic clone is detached at the same
full revision as the application's SDK dependency; an explicitly named checkout
remains available for testing unreleased plugin work. The application itself
still never builds or vendors official plugins. Production ignores local
directories and reads the signed `latest.json` from the latest release of
`BongoCat-Plugins`.

Plugin releases are triggered only by `v*` tags in `BongoCat-Plugins`. The
workflow validates the tag, builds every supported platform, verifies package
contents, signs the catalog with the plugin-owned Minisign key, creates a Release
and verifies every published URL and digest. There is no `workflow_run` trigger,
workflow dispatch to an application release, or upload to the application's
Release assets.

The plugin repository identity and release signing key are separate constants
from the application update identity. The embedded plugin public key is the
base64 Minisign public-key box from the plugin repository's
`config/plugin-release.pub`; `tools/tests/test_plugin_distribution_contract.py`
checks the separation and `deny.toml` allows only the pinned plugin Git source.

## Consequences

- A plugin can be published without rebuilding, versioning or publishing the
  application, and an application release never mutates plugin assets.
- Plugin Tags and application Tags share a naming convention but no workflow,
  concurrency group, credential, Release namespace or release ordering.
- Changing the SDK still requires updating the application's pinned revision and
  running application verification. This is intentional: protocol changes must
  be reviewed by the host.
- Publishing the first plugin Release is an external prerequisite. Until then,
  Production shows the normal retryable discovery failure; Development can be
  pointed at locally built packages in the plugin repository.
- Official plugin tests and packaging no longer run in application CI. They
  must pass in `BongoCat-Plugins` before its Tag release workflow publishes.

## Verification

- `cargo metadata --locked` resolves exactly one `bongocat-plugin-sdk` from the
  pinned plugin repository.
- `tools/tests/test_plugin_distribution_contract.py` enforces the repository
  split, revision pin, environment isolation and signing-key separation.
- The application verification pipeline continues to cover host installation,
  lifecycle, Plugin Center and overlay behavior without plugin source.
