# Changelog

## Unreleased

### ✨ Features

- On macOS, the Input Monitoring prompt now opens a guided panel: it takes you to the right System Settings page and shows how to drag BongoCat into the authorization list. The prompt clears BongoCat's Input Monitoring permission first, so the authorization always starts from scratch, and it no longer carries the paragraph that told you to remove and re-add the entry by hand.
- Settings has a Plugins page that lists every plugin available to this version, with one click to install, update, remove, or show its panel.
- A plugin adds a panel to the model window — a focus timer, a clock, a counter — and its buttons are pressed on the model window itself, without opening another window.
- Plugins are downloaded when you install them, so they are not part of the BongoCat installer.
- A plugin is now its own program, installed from a download and started when you switch it on. A plugin that stops responding costs its own card and nothing else, and adding a plugin never changes the BongoCat installer.
- Every plugin has its own settings, on its own page section, with its own labels in your language — a plugin that is not running cannot be configured, and a plugin nobody is running changes nothing.
- The first plugin is a Pomodoro timer: pick how long a round lasts, what follows it, and whether the next round starts itself. When a round ends the cat reacts and says so.
- A second plugin, Key Display, shows the keys you are holding on the model window as keycaps, in the order you pressed them. It can show the mouse buttons too, and can get out of the way when you are not typing.
- A third plugin, Typing Sound, gives the cat a voice while you type: each key plays one of your model's own motions, and the model's own sound for it. You choose the motion, the shortest gap between two sounds, and whether a held key counts once or many times.
- A fourth plugin, Key Stats, counts the keys you press and how far the pointer travels, per day, and keeps the tally in its own files so it survives a restart. Distance is shown in screen widths, which are exact, or in centimetres, which assume a 24-inch screen and say so.

### 🐛 Bug Fixes

- On Windows, BongoCat no longer freezes and quits when you click a link that opens an external website, such as "Report an issue" under Settings → About. The window also keeps painting while the browser starts.
- On macOS, the menu bar icon no longer disappears after updating to a new version.
- On Windows, an Xbox-mode controller no longer stops responding while another window has focus. BongoCat was reading controllers through a Windows gaming input interface that Windows only delivers to the app that currently owns the foreground window, so gamepad input only arrived while a BongoCat window was focused. Controllers Windows reads as raw HID devices, such as DS4 and Switch modes, were not affected.
- On Windows, analog sticks and triggers on controllers that Windows reads as raw HID devices no longer report wrong values: a stick at rest read as full deflection in one direction, half its travel was clamped there, and an analog trigger at rest sat exactly on the press threshold so it could flicker. This affected Switch-mode, DS4-mode and other non-Xbox controller modes.
- A plugin panel's buttons now respond to clicks on the Windows model window.
- The plugin catalog is no longer read with the download timeout, so an unreachable mirror no longer stalls the model window.
- A plugin you install now shows its panel straight away, instead of waiting for you to switch it on.
- A plugin installed from an archive built on another operating system now starts, instead of failing with no explanation.

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
