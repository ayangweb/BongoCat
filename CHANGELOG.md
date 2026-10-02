# Changelog

## Unreleased

### ✨ Features

- Several keys held at the same time now each show their own key image, stacked with the most recently pressed key on top, so a fast chord is no longer indistinguishable from a single tap. Turn it on with "Show every held key" under Settings → Input & interaction → Keyboard; it applies to gamepad buttons as well as keyboard keys, and is off by default. The paw reaction is unchanged: a hand is still down while any key bound to it is held.
- On macOS, the Input Monitoring prompt now opens a guided panel: it takes you to the right System Settings page and shows how to drag BongoCat into the authorization list. The prompt clears BongoCat's Input Monitoring permission first, so the authorization always starts from scratch, and it no longer carries the paragraph that told you to remove and re-add the entry by hand.
- Settings now has a Plugins page, and a plugin is its own program: it installs as a separate download, runs as its own process, and keeps its logic, its state and its configuration to itself, so adding one adds nothing to the BongoCat installer and a plugin that stops responding costs only its own card. Each plugin is a card in a grid laid out like the model library, with its own icon, name, author and version, and one click to install, update, remove, or show its panel. Plugins are not part of the BongoCat installer; they are fetched when you install them.
- A plugin adds a panel to the model window — a focus timer, a key display, a typing sound — and you press its buttons there, without opening another window. Its settings are drawn from the settings the plugin itself declares and carry the plugin's own labels in your language, opened from its own card; pressing Settings on a plugin that is switched off turns it on so that you can configure it. A plugin can also put a control on its own card, so the thing you reach for most is where you set it up — the Pomodoro's says Start, Pause or Resume depending on what its round is doing. A plugin that shows something on the model window is offered a Display position with nine places to choose from, and a place another plugin is already using is not offered at all, so two panels never land on the same corner; a plugin that shows nothing is offered no position. A panel redraws the moment you press something or change a setting, and a line a plugin asks to show above the model appears there and takes itself down.
- Three plugins ship with it. **Pomodoro** is a focus timer: you choose how long a round lasts, how long the short and long breaks are, what follows a round, and whether the next round starts itself; when a round ends the cat reacts and says so. **Key Display** shows the keys you press as keycaps in the order you pressed them — a combination is one cap, and a shortcut gets a line of its own so it does not read as the end of a word; a burst of typing is one line, half a second of quiet starts the next, and two seconds after the last key the panel takes itself down. You can show the mouse buttons too, and choose how many keys are shown and how large and bold they are; its panel is pinned to the top left of the model window, so it is always in the same place and that corner is kept free for other panels. **Typing Sound** gives the cat a voice while you type: each key plays one of your model's own motions, or a sound file of your own that you choose with your computer's own file picker; you also choose the volume, the shortest gap between two sounds, whether a held key counts once or many times, and whether the sound plays on press or on release.

### 🐛 Bug Fixes

- Some BongoCat Mver models no longer fail to import with an "invalid model package" message. A key table the model author annotated with `//` or `/* */` comments is read the way the original app reads it, and a motion that names an audio file the model never shipped no longer refuses the whole mode — the model plays without audio that was never there to begin with. When an import does still fail, the log now names which failure it was instead of a single catch-all code.
- On Windows, BongoCat no longer freezes and quits when you click a link that opens an external website, such as "Report an issue" under Settings → About. The window also keeps painting while the browser starts.
- On macOS, the menu bar icon no longer disappears after updating to a new version.
- On Windows, an Xbox-mode controller no longer stops responding while another window has focus. BongoCat was reading controllers through a Windows gaming input interface that Windows only delivers to the app that currently owns the foreground window, so gamepad input only arrived while a BongoCat window was focused. Controllers Windows reads as raw HID devices, such as DS4 and Switch modes, were not affected.
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
