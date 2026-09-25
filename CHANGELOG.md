# Changelog

## Unreleased

- This release is a complete Rust rewrite of the desktop app. It replaces the old WebView version and should run more smoothly with fewer resources.

### ✨ Features

- You can now ignore keyboard or gamepad input separately. New global shortcuts can toggle mouse, keyboard, and gamepad input, and none of them come with a default binding.
- You can choose how often BongoCat checks for updates, from every hour to once a year. Manual checks still work whenever you need them.
- Logs are now ordinary dated files you can open in any text editor. You can set the log level and how long to keep them, and old logs are cleaned up automatically.
- Motion audio starts off. Turn it on and the next action will play its sound—no restart needed.
- You can resize the model window with the right mouse button. A normal right-click still opens the menu.
- Model import now checks the source, shows progress, lets you cancel, and creates a cover from the model itself. Models that cannot be prepared are rejected.
- You can rename the built-in models and give them new covers without changing the app's own files.
- BongoCatMver models can be converted while you import them. Choose which input modes to convert, and each card remembers its mode.
- You can set the Model behavior page to play a random motion or expression every so often.
- Models can now use artwork for F13–F24, and the macOS globe key can be drawn separately.
- Light and dark mode now follow supported system windows, menus, and file pickers too.
- You can set dead zones for gamepad sticks and triggers.
- Shortcut groups now have their own switches. Model shortcuts stay off until you turn their group on.
- Settings are now organized by task, with model library and model behavior in their own pages.
- The model window is visible whenever the app starts. Hiding it only lasts for the current session.

### 🐛 Bug Fixes

- Input no longer gets stuck after locking, sleeping, connecting a device, or changing permissions. Right Shift, Right Alt, and CapsLock are also more reliable on macOS.
- Alt keys, Enter, keypad Enter, and the rest of the standard keyboard now use the right artwork.
- If settings are damaged, BongoCat restores the newest good backup, or starts with defaults when there isn't one.
- The BongoCatMver conversion window opens reliably instead of crashing.
- Reopening Settings brings you back to the same page and scroll position.
- A finished motion stays in its final pose until another motion replaces it or you stop it.

### ⚡️ Performance

- The frame-rate setting now actually reaches the number you choose.
- Settings stops rebuilding everything in the background, and a closed window stops refreshing to save CPU.

### ⚠️ Upgrade Notice

- This version uses a new settings and model data format. It won't bring over settings, shortcuts, selected models, or installed models from older versions, so you'll need to set things up and import your models again.

### 💻 Platform Changes

- Supported: Windows 10 1903+ (x64) and macOS 12+ (Intel/Apple silicon).
- Windows ARM runs the x64 build through emulation; x86 and native ARM64 packages aren't available.
- Linux isn't supported in this release.

### 🗑️ Removed or No Longer Supported

- The expression button on each model card is gone; motions and expressions now live together on Shortcuts.
- Traditional Chinese, Portuguese, and Vietnamese are no longer available. You can choose System, Simplified Chinese, or English.
- The Windows key release timeout is now in milliseconds instead of seconds.
