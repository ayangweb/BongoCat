# Changelog

## Unreleased

- This release is a complete Rust rewrite of the desktop app. It replaces the old WebView version and should run more smoothly with fewer resources.

### ⚠️ Upgrade Notice

- This version uses a new settings and model-data format. Earlier settings, shortcuts, selected models, and installed models are not imported; reconfigure BongoCat and import your models again. The model window is visible on each launch, and hiding it lasts only for the current session.

### ✨ Features

- You can ignore mouse, keyboard, or gamepad input separately, set global shortcuts for them, and adjust gamepad dead zones.
- Model import checks the folder, shows progress, lets you cancel, and creates a cover from the model itself. If the cover cannot be created, the import is rolled back.
- You can rename built-in and imported models and give them new covers. BongoCatMver folders can also be converted during import, with selectable input modes shown on each card.
- Models can play a random motion or expression on a schedule. Motion audio starts off and can be turned on without a restart.
- The model window supports right-drag resizing, 25–400% scale, 1–100% opacity, and smoother placement when moving between displays.
- Automatic update checks can run every 1–8,760 hours. The update window shows download, verification, installation, retry, and restart status.
- Logs are readable dated files with adjustable levels, retention, and automatic cleanup.
- Settings are organized by task, and the light or dark theme follows supported system windows, menus, and file pickers.
- The tray and model-window context menus now share one menu set with grouped model-window controls, including always-on-top and hide-on-mouse-hover checks; the visibility preference uses the same “Hide model window” label and is unchecked by default, so source, version, restart, and duplicate size/opacity rows are no longer cluttering the menus.
- Window and model-behavior shortcuts have separate switches. Model shortcuts are opt-in, and holding a shortcut triggers its action only once.
- Closing Settings now destroys its window; reopening it in the same app process restores the last top-level sidebar page. Update windows keep their existing close-and-destroy behavior.
- The update window is now only as tall as the step it is showing, so “Checking…” and “You are up to date” are no longer a mostly-empty dialog. A release with a changelog gets the room it needs, up to the size the window already used, and scrolls past that. It opens and changes height at the size its content actually needs, instead of briefly appearing at its tallest and shrinking down, and asking it to check shows “Checking…” with its progress bar right away instead of the result of the previous check. It no longer has a resize handle — its height is its content's height.
- The changelog in the update window now renders tables and task lists, and follows the light or dark theme. Images and raw HTML in a changelog are shown as written and never downloaded, so a release note can no longer make the app fetch anything or embed remote content.

### 🐛 User-visible Fixes

- Held keys and buttons are now cleared after device changes, lock, sleep, and permission changes. Right Shift, Right Option, and Caps Lock are more reliable on macOS. The old Windows-only seconds-based timeout is now a configurable millisecond fallback.
- Keyboard artwork now resolves left/right Alt, Enter, keypad Enter, and converted-model key names consistently.
- Switching models or changing size and opacity no longer briefly blanks or makes the model window fully transparent.
- A triggered motion plays once and holds its final pose until another motion replaces it or you stop it.
- Corrupt settings automatically fall back to the newest valid backup, then to defaults if no valid backup exists.
- Invalid or incomplete v1 settings and model IDs are now rejected consistently instead of being partially accepted or silently ignored.

### 💻 Support Changes

- Supported platforms are Windows 10 1903+ (x64) and macOS 12+ (Intel/Apple silicon). Windows ARM runs the x64 build through emulation; x86, native ARM64, and Linux builds are not provided.
- Language choices are now System, Simplified Chinese, and English. Traditional Chinese, Portuguese, and Vietnamese are no longer available.
