# Changelog

[English](CHANGELOG.md) | [简体中文](CHANGELOG.zh-CN.md)

BongoCat 2.0.0 is the first release recorded in this changelog.

## 2.0.0 - 2026-09-16

### ⚠️ Upgrade Notice

- This release uses entirely new configuration and model data. Settings, shortcuts, model selections, and installed-model lists from previous versions are not imported automatically. Reconfigure the app and import your models again after upgrading.

### ✨ Features

- Added a persistent model library for importing model folders or ZIP archives, with validation, progress, cancellation, and rejection of unsafe archives.
- BongoCatMver models can be imported directly. Each input mode the source carries (standard, keyboard, or gamepad) becomes a separate model named after the source and its mode, so the converter tool is no longer needed.
- The model library lets you rename, enable, and delete models and restores the last active model on startup. If a model cannot be activated, the currently working model remains available.
- Model details list the available motions and expressions, which can be previewed and assigned to global behavior shortcuts.
- Added a Diagnostics page for viewing runtime, rendering, and input status, exporting anonymous diagnostic reports, opening the backup folder, and restoring default settings when needed.
- Improved the update experience with a dedicated update window, download progress, signature verification, Markdown release notes, installation and restart states, and retry support. If an update fails, the current version remains installed.
- Added startup permission guidance when needed. On macOS, you can go directly to Input Monitoring settings; on Windows, BongoCat explains how administrator permissions affect global input. The model still starts when input permission is unavailable.
- Models can draw F1 to F24 with their own `F1.png` to `F24.png` images. A function key whose dedicated image is missing falls back to the model's shared `Fn.png`, so a model that ships only `Fn.png` keeps the previous behavior.

### ⚡️ Performance

- Improved model display to reduce unnecessary CPU/GPU usage and provide smoother scaling and continuous rendering.
- Added a configurable frame-rate limit from 15 to 240 FPS. Hidden models refresh less often to reduce background resource usage.

### 🐛 Bug Fixes

- Improved keyboard and mouse state recovery, with automatic resets after lock screen, sleep, device changes, or permission changes to reduce stuck keys and buttons.
- Settings are now backed up automatically and can recover from corruption, helping prevent lost configuration after an unexpected exit.
- The left and right Alt keys are now drawn as the two separate keys they are. Pressing right Alt used to show the left Alt image and a model's own right-Alt artwork (`AltGr.png`) was never used. Models that still name the two keys `Alt` and `AltGr` are converted to the new names automatically on import, and already installed models keep drawing the right artwork.
- The main Enter key and the keypad Enter key are now named and drawn correctly. The main key's artwork is looked up as `Enter` (the old `Return` name is converted automatically on import, and already installed models keep drawing their image), and the keypad Enter key falls back to the main Enter image when a model does not ship a dedicated `KpEnter.png`; pressing the keypad Enter used to show nothing at all.
- The numeric keypad is now drawn instead of silently showing nothing. Every keypad key that duplicates a main keyboard key — `Num 1` to `Num 9`, `Num 0`, keypad Enter, and keypad `/` — falls back to that key's artwork, so a model that only draws the main keyboard reacts to the keypad too. The five keypad keys with no main keyboard counterpart (`Num Lock`, `*`, `-`, `+`, `.`) still draw nothing, because no model has ever shipped artwork for them. This also makes the keypad Enter fallback described above reachable for the first time.
- Every key of the standard keyboard now has a name, including the ones no model has ever drawn: the punctuation keys (`.`, `,`, `-`, `=`, `[`, `]`, `\`, `;`, `'`), PrintScreen, Scroll Lock, Pause, the navigation cluster (Insert, Home, Page Up, Delete, End, Page Down), the ISO extra key, Apps, and keypad `=`. A model that ships an image for any of them now draws it with no further change. The Delete key's image has shipped with the built-in models all along but could never be drawn, and now works.

### 🎨 UI and Experience

- Redesigned settings navigation and status feedback, with more complete loading, empty, error, and retry states.
- Model scaling now supports 25%–400%, and opacity supports 1%–100%.
- The model window corner radius can be set from 0% to 50% of its width and height. 0% keeps square corners and 50% clips the window to a full ellipse.
- Added a "Hide on pointer hover" setting that fades the model window out and lets pointer input pass through while the pointer rests on it, restoring it when the pointer leaves. The hover delay is configurable from 0 to 60 seconds, where 0 hides immediately.
- Added dead-zone settings for gamepad sticks and triggers.

### 💻 Platform Changes

- Supported platforms are Windows 10 1903+ (x64), macOS 12+ (Intel), and macOS 12+ (Apple silicon).
- Windows ARM devices run the x64 build through system emulation. Windows x86 and native Windows ARM64 packages are no longer provided.
- Linux is not supported in this initial release.
- “Launch at login” on macOS requires macOS 13 or later.

### 🗑️ Removed or No Longer Supported

- Settings, shortcuts, model configuration, and installed models from previous versions are no longer imported automatically.
- Models can no longer be imported by dragging them into the window. Use the folder or ZIP import options in the model library instead.
- Removed Traditional Chinese, Portuguese, and Vietnamese language options. Available choices are System, Simplified Chinese, and English.
- The Windows key auto-release delay is now configured in milliseconds as a release fallback timeout instead of seconds.
