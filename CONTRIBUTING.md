# Contributing to BongoCat

[English](CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md)

Thank you for helping improve BongoCat. Please keep contributions focused, understandable, and
supported by repeatable verification.

## Before you start

- Search existing issues and pull requests before opening a new one.
- For a substantial behavior change, discuss the user need and intended result before implementation.
- Keep unrelated cleanup and formatting out of the same change.
- When changing a user- or contributor-facing document, update its English and Simplified Chinese
  counterpart in the same change.
- Keep public documentation focused on product behavior and workflows. Internal project labels,
  migration narratives, implementation-stack emphasis, and source filenames do not belong in public
  documentation unless they are essential to complete the task.

## Development setup

BongoCat is developed on Windows and macOS. Linux is not a release target and is not required for
routine validation.

Install `rustup` (the repository pins the required toolchain), `just`, and Python 3. Run all commands
from the repository root:

```text
just dev
just check
just build
just schema
```

`just dev` and `just dev-smoke` also prepare the official plugins. They use the
checkout named by `BONGOCAT_PLUGINS_ROOT`, or clone
`https://github.com/ayangweb/BongoCat-Plugins` under `target/plugin-dev` on the
first run and check out the SDK revision pinned in `Cargo.toml`, build only the
current host target, and start the application with the generated local
catalog. The automatic checkout must stay clean. Set the variable to work
against an existing plugin checkout, including uncommitted plugin changes:

```text
BONGOCAT_PLUGINS_ROOT=/path/to/BongoCat-Plugins just dev
```

Use `just plugin-tests` to run the workspace tests with the same plugin checkout
and real-capable plugin binaries. Without that command, tests that need the
independent checkout skip; this is useful for lightweight local checks but is
not the full plugin lifecycle verification. Set `BONGOCAT_PLUGINS_ROOT` to the
same checkout when testing uncommitted plugin changes.

The plugin build requires the same Rust toolchain and `just`; the orchestration
script requires Python 3 and Git. No plugin release, plugin signing key, or
application release is involved.

### Plugin repository and joint debugging

Official plugins and the plugin SDK are maintained in the independent
[`BongoCat-Plugins`](https://github.com/ayangweb/BongoCat-Plugins) repository.
When changing only plugin behavior, plugin UI, or plugin business logic, create a
branch and pull request in that repository. Do not copy plugin source back into
this repository, and do not update this repository's SDK revision.

Maintainers with write access may clone `ayangweb/BongoCat-Plugins` directly.
Other developers must first fork `BongoCat-Plugins` on GitHub, clone their fork,
and add the upstream repository as a remote:

```text
git clone https://github.com/<your-name>/BongoCat-Plugins.git
cd BongoCat-Plugins
git remote add upstream https://github.com/ayangweb/BongoCat-Plugins.git
git fetch upstream
git switch -c fix/plugin-topic upstream/master
```

After validating the change, push the branch to the fork and open a pull request
against the upstream repository:

```text
git push -u origin fix/plugin-topic
```

On GitHub, open a pull request from `<your-name>:fix/plugin-topic` to
`ayangweb/BongoCat-Plugins:master`. Do not push commits directly to the upstream
repository or use a pull request to modify upstream `master`.

The application repository follows the same access rules: maintainers may clone
the upstream directly, while other developers must fork `ayangweb/BongoCat`,
branch from upstream `master`, push to their fork, and open a pull request
against the upstream repository.

#### Plugin-only changes

These changes do not require modifying application dependencies. Point the
application's plugin path at your plugin fork checkout to develop and run the
full plugin tests with uncommitted plugin changes:

```text
BONGOCAT_PLUGINS_ROOT=/path/to/BongoCat-Plugins just dev
BONGOCAT_PLUGINS_ROOT=/path/to/BongoCat-Plugins just plugin-tests
```

An explicitly named plugin checkout may contain uncommitted changes. The
automatic `target/plugin-dev/BongoCat-Plugins` checkout managed by `just dev`
must stay clean and remains pinned to the SDK revision declared in `Cargo.toml`.

#### SDK, protocol, or host-interface changes

These changes affect both repositories and require two pull requests, with the
plugin-repository change merged first. Until the plugin pull request is merged,
you may temporarily point the application's SDK dependency at the local plugin
checkout to verify that both sides agree on the protocol:

```toml
[workspace.dependencies]
bongocat-plugin-sdk = { path = "../BongoCat-Plugins/crates/bongocat-plugin-sdk" }
```

The path above is an example relative to the application repository; adjust it
for your checkout. You may also temporarily use a full commit from your fork.
Such dependency changes and the corresponding `Cargo.lock` changes are for local
integration testing only and must not be committed to the application pull
request.

After the plugin-repository change is merged, restore the dependency to the
upstream repository and pin `rev` to the merged full 40-character commit SHA:

```toml
[workspace.dependencies]
bongocat-plugin-sdk = { git = "https://github.com/ayangweb/BongoCat-Plugins", rev = "<full-upstream-commit-sha>" }
```

Then run `cargo update --workspace` to synchronize `Cargo.lock`, run `just check`,
and run `just plugin-tests` against the merged plugin checkout. The application
must not merge a commit from a contributor's fork; it must use a full revision
merged in the upstream repository. The application consumes the SDK through a
Git revision, so this step does not require a plugin Tag or Release; plugin
releases remain independent in the plugin repository.

When building on Windows, ensure you use the MSVC toolchain (of the form
`stable-x86_64-pc-windows-**msvc**`, not `stable-x86_64-pc-windows-**gnu**`). This project depends
on Live2D Cubism Core, whose Windows distribution is only available in MSVC format (`.lib`), so MSVC
is mandatory on Windows. Use `rustup show` to check the active toolchain. If it is not MSVC, switch
to it with `rustup override set`.

Building on macOS also needs an Xcode Swift toolchain, because the guided Input Monitoring flow
links a Swift static library (ADR-0078). SwiftPM refuses to load a package whose manifest declares a
newer tools version than the installed toolchain, and that package asks for 6.1. The Rust toolchain
file does not cover it, so keep Xcode up to date — the failure otherwise appears as a `swift build`
error inside `permission-flow`'s build script.

Use `just dev-smoke` to exercise the settings-window lifecycle. Run `just schema` after changing the
configuration or window-state types; it regenerates the checked-in JSON Schema contracts.
Platform-specific changes must also be verified on every affected operating system; a build or test
on another host is not a substitute for platform smoke coverage.

## Validation

Before opening a pull request:

- Run `just check` for code changes.
- Run focused tests while iterating, then use the full check before submission.
- Add or update automated tests for changed behavior whenever practical.
- Include screenshots for visible UI changes and note the window size, scale, theme, and platform
  used to capture them.
- Update both language versions when user-facing text changes.

Platform features must not be declared complete from compilation alone. Record any unrun platform
checks and the remaining risk in the pull request.

## Build and packaging

`just build` is the only supported build and packaging entry point. Do not add a parallel packaging
path to another tool, workflow, or script. Do not commit build artifacts, user data, local
environment files, or signing keys.

## Commits and pull requests

Use [Conventional Commits](https://www.conventionalcommits.org/) for commit messages, for example
`feat: add model import validation`.

A pull request should explain:

- the user-visible problem and intended behavior;
- the scope of the change;
- the validation commands and results;
- affected platforms, permissions, and any checks that could not be run;
- screenshots or recordings when behavior is visual.

If a change affects user-visible behavior, defaults, compatibility, supported platforms, or upgrade
guidance, update both `CHANGELOG.md` and `CHANGELOG.zh-CN.md` unless maintainers decide otherwise.

## Privacy and security

Never include real user paths, key sequences, clipboard contents, private model data, credentials, or
signing keys in issues, pull requests, logs, screenshots, or test fixtures.

## Earlier versions

The previous implementation remains available on the protected
[`pre-refactor-tauri`](https://github.com/ayangweb/BongoCat/tree/pre-refactor-tauri) branch. Use it
only as a historical reference; do not modify or reintroduce that implementation.
