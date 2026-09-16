# Changelog

[English](CHANGELOG.md) | [简体中文](CHANGELOG.zh-CN.md)

BongoCat 2.0.0 is the first release recorded in this changelog.

## 2.0.0 - 2026-09-16

### ⚠️ Upgrade Notice

- This release uses entirely new configuration and model data. Settings, shortcuts, model selections, and installed-model lists from previous versions are not imported automatically. Reconfigure the app and import your models again after upgrading.

### ✨ Features

- Added a persistent model library for importing model folders or ZIP archives, with validation, progress, cancellation, and rejection of unsafe archives.
- The model library lets you rename, enable, and delete models and restores the last active model on startup. If a model cannot be activated, the currently working model remains available.
- Model details list the available motions and expressions, which can be previewed and assigned to global behavior shortcuts.
- Added a Diagnostics page for viewing runtime, rendering, and input status, exporting anonymous diagnostic reports, opening the backup folder, and restoring default settings when needed.
- Improved the update experience with a dedicated update window, download progress, signature verification, Markdown release notes, installation and restart states, and retry support. If an update fails, the current version remains installed.
- Added startup permission guidance when needed. On macOS, you can go directly to Input Monitoring settings; on Windows, BongoCat explains how administrator permissions affect global input. The model still starts when input permission is unavailable.

### ⚡️ Performance

- Improved model display to reduce unnecessary CPU/GPU usage and provide smoother scaling and continuous rendering.
- Added a configurable frame-rate limit from 15 to 240 FPS. Hidden models refresh less often to reduce background resource usage.

### 🐛 Bug Fixes

- Improved keyboard and mouse state recovery, with automatic resets after lock screen, sleep, device changes, or permission changes to reduce stuck keys and buttons.
- Settings are now backed up automatically and can recover from corruption, helping prevent lost configuration after an unexpected exit.

### 🎨 UI and Experience

- Redesigned settings navigation and status feedback, with more complete loading, empty, error, and retry states.
- Model scaling now supports 25%–400%, and opacity supports 1%–100%.
- Added dead-zone settings for gamepad sticks and triggers.

### 💻 Platform Changes

- Supported platforms are Windows 10 1903+ (x64), macOS 12+ (Intel), and macOS 12+ (Apple silicon).
- Windows ARM devices run the x64 build through system emulation. Windows x86 and native Windows ARM64 packages are no longer provided.
- Linux is not supported in this initial release.
- “Launch at login” on macOS requires macOS 13 or later.

### 🗑️ Removed or No Longer Supported

- Settings, shortcuts, model configuration, and installed models from previous versions are no longer imported automatically.
- Removed the hide-on-hover, hover-delay, and window corner-radius settings.
- Models can no longer be imported by dragging them into the window. Use the folder or ZIP import options in the model library instead.
- Removed Traditional Chinese, Portuguese, and Vietnamese language options. Available choices are System, Simplified Chinese, and English.
- The Windows key auto-release delay is now configured in milliseconds as a release fallback timeout instead of seconds.
