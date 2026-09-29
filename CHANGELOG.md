# Changelog

## Unreleased

### ✨ Features

- The model library gains a remote model library: entries published in the Awesome-BongoCat catalog can be previewed there, downloaded with a progress bar, and imported automatically.

## 2.0.0 - 2026-09-29

### ⚠️ Upgrade Notice

- BongoCat 2.0.0 uses a new native Rust architecture.
- Uninstall the previous version first, then download and install the new version manually.
- BongoCat is now licensed under the Apache License 2.0 instead of MIT.

### ✨ Features

- Mouse, keyboard and gamepad input can each be ignored separately, and each has a global shortcut.
- Gamepad stick and trigger dead zones can be adjusted.
- Models can switch automatically when a gamepad connects or disconnects, or you can pin the switch to a specific model.
- Model import captures a cover image automatically.
- BongoCat Mver apps can be imported and converted automatically.
- Built-in and imported models can be renamed and given a new cover.
- Random motions or expressions can be played, choosing expressions only, motions only, or both.
- Each model remembers the expression you last used on it, and restores it on the next launch or when you switch back to that model.
- Mouse tracking can also be flipped vertically.
- Logs have an adjustable level and retention, and old logs are cleaned up automatically.
- Window shortcuts and model-behaviour shortcuts have separate switches.

### 🐛 Bug Fixes

- Held keys and gamepad buttons no longer stay stuck after device, lock, sleep and permission changes.
- Switching models or resizing the window no longer leaves the model window black, transparent or flickering.

### 🗑️ Removals

- The "Key release timeout" setting is removed.
- Traditional Chinese, Portuguese and Vietnamese are no longer available.

### 💻 Support Changes

- Supported platforms are Windows 10 1903+ (x64) and macOS 12+ (Intel/Apple Silicon).
- Windows ARM runs the x64 build through emulation.
- **Windows x86, Windows ARM64 and Linux builds are not provided.**
