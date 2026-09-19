# Changelog

[English](CHANGELOG.md) | [简体中文](CHANGELOG.zh-CN.md)

BongoCat 2.0.0 is the first release recorded in this changelog.

## 2.0.0 - 2026-09-16

### ⚠️ Upgrade Notice

- This release uses entirely new configuration and model data. Settings, shortcuts, model selections, and installed-model lists from previous versions are not imported automatically. Reconfigure the app and import your models again after upgrading.

### ✨ Features

- Added a persistent model library for importing model folders or ZIP archives, with validation, progress, cancellation, and rejection of unsafe archives.
- BongoCatMver models can be imported directly. Each input mode the source carries (standard, keyboard, or gamepad) becomes a separate model named after the source and its mode, so the converter tool is no longer needed.
- The model library shows each model as a card with its own cover image and name. From a card you can switch models, open the model's folder, rename it, replace its cover image, or delete it, and the last active model is restored on startup. If a model cannot be activated, the currently working model remains available. Names and covers of the built-in models are part of the app and cannot be changed; import a copy to customise them.
- A model's motions and expressions are listed on the Shortcuts page, where each one can be assigned its own global shortcut.
- A model's motions and expressions are given their own global shortcuts automatically, the way the previous version assigned them: loading a model fills in every entry with `Ctrl`/`Cmd` plus `Shift`/`Alt` and a digit or letter, and each entry can still be changed on its own. A binding you change is never overwritten, so re-activating a model or restoring defaults only fills in what you have not recorded. The model behaviour switch on the General page starts off, so the assigned shortcuts do not take effect globally until you turn it on — the previous version bound them unconditionally, so a fresh install already carried global shortcuts nobody asked for. Turning the switch back on restores the recorded shortcuts without recording them again. Clearing all shortcuts removes the window shortcuts for good, but a model's defaults are filled in again the next time that model is activated — use the model behaviour switch to keep them from taking effect.
- Added a Diagnostics page for viewing runtime, rendering, and input status, exporting anonymous diagnostic reports, opening the backup folder, and restoring default settings when needed.
- Improved the update experience with a dedicated update window, download progress, signature verification, Markdown release notes, installation and restart states, and retry support. If an update fails, the current version remains installed.
- Added startup permission guidance when needed. On macOS, you can go directly to Input Monitoring settings; on Windows, BongoCat explains how administrator permissions affect global input. The model still starts when input permission is unavailable.
- Models can draw F1 to F24 with their own `F1.png` to `F24.png` images. A function key whose dedicated image is missing falls back to the model's shared `Fn.png`, so a model that ships only `Fn.png` keeps the previous behavior.
- The macOS Fn key — the globe key at the bottom-left — is now a key of its own, named `Globe`, and no longer shares a name with the shared function-key image. `Fn` keeps its existing meaning: the one image a model may ship for the whole F1 to F24 row, which is why `Fn.png` is neither renamed nor moved. A model that wants to draw the globe key ships `Globe.png`; the pre-rename `Function.png` is still recognised. Windows handles this key in the keyboard firmware and never reports it, so it does nothing there.
- The light and dark appearance choice now covers the native surfaces the operating system draws. The settings window's title bar follows the theme on both platforms; on macOS the app's alerts — including the startup permission prompt — its menus, the tray menu, and the open/save panels follow it too, and the model window's context menu is themed from the first launch. On Windows the window frame follows the theme, while its alerts, menus, and file dialogs keep following the system theme, which those surfaces cannot opt out of.

### ⚡️ Performance

- Improved model display to reduce unnecessary CPU/GPU usage and provide smoother scaling and continuous rendering.
- Added a configurable frame-rate limit from 15 to 240 FPS. Hidden models refresh less often to reduce background resource usage.

### 🐛 Bug Fixes

- Improved keyboard and mouse state recovery, with automatic resets after lock screen, sleep, device changes, or permission changes to reduce stuck keys and buttons.
- Right Shift, Right Alt (Option), and CapsLock now respond immediately and reliably on macOS. The listening event tap now sits at the head of the HID layer like rdev does: on macOS 26 the session-level tap never receives Right Shift release events, while the HID-level tap receives complete press/release pairs for every modifier. Modifier press/release direction is decoded from per-key device flag transitions instead of ambiguous family flags, CapsLock's latching flag is handled by edge alternation, and the reconciliation pass consults the family key state that right-side keycodes fail to report.
- Settings are now backed up automatically and can recover from corruption, helping prevent lost configuration after an unexpected exit.
- The left and right Alt keys are now drawn as the two separate keys they are. Pressing right Alt used to show the left Alt image and a model's own right-Alt artwork (`AltGr.png`) was never used. Models that still name the two keys `Alt` and `AltGr` are converted to the new names automatically on import, and already installed models keep drawing the right artwork.
- The main Enter key and the keypad Enter key are now named and drawn correctly. The main key's artwork is looked up as `Enter` (the old `Return` name is converted automatically on import, and already installed models keep drawing their image), and the keypad Enter key falls back to the main Enter image when a model does not ship a dedicated `KpEnter.png`; pressing the keypad Enter used to show nothing at all.
- The numeric keypad is now drawn instead of silently showing nothing. Every keypad key that duplicates a main keyboard key — `Num 1` to `Num 9`, `Num 0`, keypad Enter, and keypad `/` — falls back to that key's artwork, so a model that only draws the main keyboard reacts to the keypad too. The five keypad keys with no main keyboard counterpart (`Num Lock`, `*`, `-`, `+`, `.`) still draw nothing, because no model has ever shipped artwork for them. This also makes the keypad Enter fallback described above reachable for the first time.
- Every key of the standard keyboard now has a name, including the ones no model has ever drawn: the punctuation keys (`.`, `,`, `-`, `=`, `[`, `]`, `\`, `;`, `'`), PrintScreen, Scroll Lock, Pause, the navigation cluster (Insert, Home, Page Up, Delete, End, Page Down), the ISO extra key, Apps, and keypad `=`. A model that ships an image for any of them now draws it with no further change. The Delete key's image has shipped with the built-in models all along but could never be drawn, and now works.
- A key now only reacts when the active model actually has an image for it. Pressing a key the model does not draw — for example `.` on the built-in models, which ship no `Dot.png` — used to make the paw press down with no key appearing. Such keys now do nothing at all: no paw movement and no key layer, so the paw never reacts for something you cannot see. Models that do ship the image keep working unchanged, including the keypad keys that fall back to the main keyboard artwork, the function keys that fall back to `Fn.png`, and the built-in `Delete` key.
- Models now look the same on Windows and macOS. On Windows the model window was drawn with its midtones too dark, because its rendering path never converted the final colors back for display. Pure black and white were unaffected, and semi-transparent edges were also weighted wrongly.
- BongoCatMver sources can now convert the function keys F13 to F24, each to its own image. A converted backslash key also installs `BackSlash.png` — the image the app actually looks up; the converter used to write `Backslash.png`, which no key could reach, so pressing that key did nothing at all.
- The Apps key — the Menu key next to right Ctrl — now works on both platforms, so a model that ships `Apps.png` draws it. It had a name and a binding from the start, but neither platform reported the key: Windows Raw Input had no scan code for it and macOS had no keycode for `kVK_ContextualMenu`, so the image could never be reached. Apple keyboards have no such key, but a third-party one sends the macOS keycode.
- A model's motion shortcut now plays its motion once instead of looping forever. The built-in motions declare themselves as looping, so a triggered action kept playing until the app was restarted and the model never returned to its idle pose. Pressing and holding a shortcut also no longer triggers the action again on every key repeat, and pressing it again after the action has finished plays it again.

### 🎨 UI and Experience

- Redesigned settings navigation and status feedback, with more complete loading, empty, error, and retry states.
- Model scaling now supports 25%–400%, and opacity supports 1%–100%.
- The model window corner radius can be set from 0% to 50% of its width and height. 0% keeps square corners and 50% clips the window to a full ellipse.
- Added a "Hide on pointer hover" setting that fades the model window out and lets pointer input pass through while the pointer rests on it, restoring it when the pointer leaves. The hover delay is configurable from 0 to 60 seconds, where 0 hides immediately.
- The model window stays fully on screen while being allowed over the taskbar, Dock and menu bar. A window dragged off the screen returns after you stop dragging instead of springing back the moment you let go, so moving it between screens is not interrupted.
- Added dead-zone settings for gamepad sticks and triggers.
- Model management reports its errors consistently: a folder or cover dialog that cannot open, a failed import, a model folder that cannot be read, and a rename or cover change that fails all appear as the same kind of notification instead of inline text.

### 💻 Platform Changes

- Supported platforms are Windows 10 1903+ (x64), macOS 12+ (Intel), and macOS 12+ (Apple silicon).
- Windows ARM devices run the x64 build through system emulation. Windows x86 and native Windows ARM64 packages are no longer provided.
- Linux is not supported in this initial release.
- “Launch at login” on macOS requires macOS 13 or later.

### 🗑️ Removed or No Longer Supported

- Settings, shortcuts, model configuration, and installed models from previous versions are no longer imported automatically.
- Models can no longer be imported by dragging them into the window. Use the folder or ZIP import options in the model library instead.
- The per-model expression button is gone. A model's expressions and motions are listed on the Shortcuts page, next to the shortcut they are bound to, so the same list is not shown in two places.
- Removed Traditional Chinese, Portuguese, and Vietnamese language options. Available choices are System, Simplified Chinese, and English.
- The Windows key auto-release delay is now configured in milliseconds as a release fallback timeout instead of seconds.
- Removed the toolbar at the bottom of the settings window. Its refresh and quit buttons are gone: the window keeps showing the current settings on its own, and BongoCat can still be quit from the system menu.
