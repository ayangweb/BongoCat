# Changelog

## Unreleased

### ✨ Features

- On macOS, the Input Monitoring prompt now opens a guided panel: it takes you to the right System Settings page and shows how to drag BongoCat into the authorization list. The prompt clears BongoCat's Input Monitoring permission first, so the authorization always starts from scratch, and it no longer carries the paragraph that told you to remove and re-add the entry by hand.

### 🐛 Bug Fixes

- On Windows, BongoCat no longer freezes and quits when you click a link that opens an external website, such as "Report an issue" under Settings → About. The window also keeps painting while the browser starts.
- On macOS, the menu bar icon no longer disappears after updating to a new version.
- On Windows, analog sticks and triggers on controllers that Windows reads as raw HID devices no longer report wrong values: a stick at rest read as full deflection in one direction, half its travel was clamped there, and an analog trigger at rest sat exactly on the press threshold so it could flicker. This affected Switch-mode, DS4-mode and other non-Xbox controller modes.

### 🌍 Localization

Korean is now available in Settings → Appearance & language. Existing configurations are unaffected and keep the language they already had.

## 2.0.1 - 2026-09-29

### 🐛 Bug Fixes

- Fixed some models failing to import with an "invalid model package" message.

### 🌍 Localization

Arabic, Vietnamese, Traditional Chinese, and Portuguese are now available.

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
