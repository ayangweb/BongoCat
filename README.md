# BongoCat

[English](README.md) | [简体中文](README.zh-CN.md)

BongoCat is a desktop companion for Windows and macOS. It responds to keyboard, mouse, and
gamepad input, brings Live2D models to life, and provides a movable, always-on-top model window.

## Status

BongoCat is under active development for:

- Windows 10 1903+ (x64)
- macOS 12+ (Intel and Apple silicon)

A stable release has not been published yet.

## Build from source

Install `rustup` (the repository pins the required toolchain), `just`, and Python 3, then run
commands from the repository root:

```text
just dev
just check
just build
```

`just build` creates a release package: a `.app` and `.dmg` on macOS, or an x64 installer on
Windows. Run `just version` to print the product version.

## Documentation

- [Contributing guide](CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)
- [License](LICENSE)

## Earlier versions

The previous implementation remains available on the protected
[`pre-refactor-tauri`](https://github.com/ayangweb/BongoCat/tree/pre-refactor-tauri) branch for
historical reference.
