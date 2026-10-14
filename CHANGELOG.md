# Changelog

## Unreleased

### ✨ Features

- Added a plugin system: plugins run as separate processes and extend the model window through a versioned protocol.
- Added a plugin center under Settings that lists, enables, disables and configures discovered plugins.
- Plugins are distributed and installed as ZIP packages, and the plugin center can now install a package from disk, uninstall a plugin, and reset its data separately.
- The Pomodoro plugin now shows its countdown in the model window, staying in sync with starting, pausing and resetting the timer.
- Plugins keep running when the settings window is closed.

### 🎨 Interface

- Added a "Plugin Center" destination to the settings sidebar, with localized plugin-list messages.
- Renamed the focus timer plugin to "Pomodoro".
- The Pomodoro panel now shows the session length before the timer starts, so the duration is visible before counting down begins.
- Model-window plugin panels now use the same button size, corner radius and colors as the settings window.

### 🐛 Bug Fixes

- Fixed the Pomodoro plugin's "Start" and "Restart" buttons in the model window doing nothing, so the timer can now be started and reset without opening Settings.
- Fixed unreadable characters in model-window plugin panels: labels containing non-Latin text no longer show a slash followed by a row of empty boxes.

## 2.3.1 - 2026-10-13

### 🐛 Bug Fixes

- Fixed "Hide on mouse hover" only working in some cases while "Force mouse movement" was on.
- Fixed the settings window being unexpectedly hidden on macOS after turning off the Dock icon.
- Fixed the model window not being able to move above the menu bar on macOS.

### 🎨 Interface

- Renamed "Show Dock icon" to "Always show Dock icon".
- Improved the descriptions on the About page.

## 2.3.0 - 2026-10-12

### ✨ Features

- Model folders containing multiple levels of nested directories can now be imported.
- The Windows installer now asks which language to use.
- Added a "Model behavior → Allow overlapping motions" setting.
- Added a "Model window → Window behavior → Window mode" setting on Windows.
- Added a "Model window → Window appearance → Window background color" setting on Windows.
- Window mode and window background color can be used together, so OBS can capture the model window properly on Windows.

### 🐛 Bug Fixes

- Fixed the cat window missing from OBS's default window list on macOS.
- Fixed model windows losing their aspect ratio in some cases.
- Fixed "The selected model could not be activated" appearing when switching to some models.

### 🌍 Localization

Added French.

## 2.2.0 - 2026-10-07

### ✨ Features

- Added a "Model window → Window behavior → Hide when idle" setting.
- Added a "Model window → Window behavior → Idle hide delay" setting.
- Added a "Model window → Window behavior → Hold a modifier key to interact" setting.
- Added an "Input & interaction → Mouse → Force mouse movement" setting.
- Added a "Model behavior → Turn off an expression when triggered again" setting.
- Added renaming support to "Shortcuts → Model behavior shortcuts".

### 🐛 Bug Fixes

- Fixed noticeable soft fringes around model texture edges.
- Fixed misplaced key images in some converted Bongo-Cat-Mver models.

### 🎨 Interface

- Renamed "App & system → Startup & desktop → Open at login" to "Run at startup".

## 2.1.1 - 2026-10-03

### 🐛 Bug Fixes

- Fixed the app crashing on macOS when opening System Settings from the permission prompt.

## 2.1.0 - 2026-10-03

### ✨ Features

- Added a "Show every held key" setting under Input & interaction → Keyboard.
- Improved the macOS Input Monitoring permission flow with a guided panel that walks you through the authorization.

### 🐛 Bug Fixes

- Fixed some BongoCat Mver models failing to import with an "invalid model package" message.
- Fixed BongoCat freezing and quitting on Windows after clicking "Report an issue".
- Fixed the menu bar icon missing on macOS after updating to a new version and restarting.
- Fixed Xbox-mode controllers going unresponsive on Windows once another window took focus.
- Fixed flickering on Windows when a model uses a gamepad mode.
- Fixed dropdown menus in Settings being too narrow to show their options.

### 🌍 Localization

Added Korean.

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
