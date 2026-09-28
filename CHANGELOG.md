# Changelog

## Unreleased

### ⚠️ Upgrade Notice

- BongoCat 2.0.0 is a complete Rust rewrite of the desktop app, replacing the WebView version.
- Settings and model data use a new format. Earlier settings, shortcuts, selected models and installed models are not imported; reconfigure BongoCat and import your models again.
- The model window is shown on every launch, and hiding it lasts only for the current session.

### ✨ Features

- Ignore mouse, keyboard or gamepad input separately, set a global shortcut for each, and adjust gamepad dead zones.
- Switch models when a gamepad connects or disconnects. It is off by default; each direction starts from the last model you used and can be pinned to a specific model, and the connected list offers gamepad models only.
- Model import checks the folder, shows progress and can be cancelled, and creates a cover from the model itself. An import is rolled back if the cover cannot be created.
- Rename built-in and imported models and replace their covers. BongoCatMver folders can be converted on import, with an input mode selected for each.
- Play random motions or expressions on a schedule, choosing expressions only, motions only, or both. Motion audio can be turned on without a restart.
- Bring each model back to the expression you last used on it. Turn on "Remember the last expression of each model" in Model behavior, and the expression is remembered for that model alone and restored on the next launch or switch back.
- Flip mouse tracking horizontally, and flip it vertically as well to correct a model that looks up when the cursor moves down.
- Resize the model window by right-dragging, with 25–400% scale, 1–100% opacity, and smoother placement across displays.
- Schedule automatic update checks every 1–8,760 hours. The update window shows download, verification, installation, retry and restart status.
- Logs are dated files with an adjustable level and retention, and old logs are cleaned up automatically.
- Show or hide the Dock icon under "App & system" on macOS. It is off by default and does not affect the menu bar icon, the model window, or how the app is quit.
- Window shortcuts and model-behaviour shortcuts have separate switches. Model shortcuts are off by default, and holding a shortcut triggers its action only once.
- Closing Settings destroys its window; reopening it in the same process restores the last sidebar page.
- The update window is only as tall as the step it shows, and the changelog it renders supports tables and task lists and follows the theme. Images and raw HTML in a changelog are shown as written and never downloaded.

### 🐛 Bug Fixes

- Held keys and buttons are cleared after device, lock, sleep and permission changes, and Right Shift and Right Option are more reliable on macOS. A held key is now released only by a real release event or one of those changes, never by a timer; the old Windows-only timeout is gone.
- Caps Lock triggers briefly again on macOS instead of staying held; the trigger releases itself after 100 ms.
- Pressing a gamepad button now shows that button's key image and moves the matching paw. The bundled gamepad model's key images are renamed to the product's own button names (`LeftShoulder`/`RightShoulder`, `LeftTrigger`/`RightTrigger`, `LeftStick`/`RightStick`, `DpadUp`…`DpadDown`), and an older gamepad model is renamed on import. `Select`, `Start` and the two stick clicks still show nothing, because the bundled model ships no artwork for them.
- Keyboard artwork resolves left and right Alt, Enter, keypad Enter, and converted-model key names.
- Switching models or changing size and opacity no longer briefly blanks the model window or makes it fully transparent.
- A triggered motion plays once and holds its final pose until another motion replaces it or you stop it.
- Corrupt settings fall back to the newest valid backup, then to the default settings.
- Invalid or incomplete v1 settings and model IDs are rejected consistently instead of being partially accepted or silently ignored.
- Fixed a background CPU core on macOS. Gamepad support was polling from startup, whether or not a controller was connected; idle use drops from about a full core to a few percent.
- The model window waits for the graphics card to finish each frame instead of polling it, and the settings and update windows only re-read what they display when it changes.
- The Settings window keeps its full title bar on Windows in every state, and "Show taskbar icon" now controls the model window's taskbar button, which is off by default.
- The Settings and update windows show the BongoCat icon in their title bars and taskbar buttons on Windows.

### 🗑️ Removals

- The "Key release timeout" setting and the `input.keyboard.release_fallback_timeout_ms` key it wrote are removed. The Keyboard group holds only "Ignore keyboard input", and a configuration file that still carries the old key is rejected as an unknown field.
- Traditional Chinese, Portuguese and Vietnamese are no longer available. The language choices are now System, Simplified Chinese and English.

### 🎨 Interface

- Settings are organised by task, and the light or dark theme follows supported system windows, menus and file pickers.
- The tray and model-window context menus share one menu, grouped by model-window action, with always-on-top and hide-on-hover checks. Visibility uses the same "Hide model window" switch, off by default.
- The three flip settings are worded as flips and name the axis they flip: flip model horizontally, flip mouse tracking horizontally, and flip mouse tracking vertically.

### 💻 Support Changes

- Supported platforms are Windows 10 1903+ (x64) and macOS 12+ (Intel/Apple silicon). Windows ARM runs the x64 build through emulation; x86, native ARM64 and Linux builds are not provided.
