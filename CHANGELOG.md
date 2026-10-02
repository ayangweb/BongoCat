# Changelog

## Unreleased

### ✨ Features

- On macOS, the Input Monitoring prompt now opens a guided panel: it takes you to the right System Settings page and shows how to drag BongoCat into the authorization list. The prompt clears BongoCat's Input Monitoring permission first, so the authorization always starts from scratch, and it no longer carries the paragraph that told you to remove and re-add the entry by hand.
- A plugin can now put a control on its own card in Settings → Plugins, in your own language, so the thing you use most is where you set it up instead of inside a small panel on the model window. The Pomodoro has one: it says Start, Pause or Resume depending on what its round is doing, and pressing it there runs the same round the panel shows. A plugin that wants no control on its card is unaffected.
- Settings has a Plugins page that lists every plugin available to this version, with one click to install, update, remove, or show its panel. Each plugin is its own card in a grid — laid out like the model library, two to a card row, with a small icon, its name, who made it and which version is here — and its own settings panel, drawn from the settings the plugin itself declares and opened from its card.
- A plugin is its own program. It runs as a separate process, keeps its logic, its state and its configuration to itself, and adds nothing to BongoCat's own size — a focus timer, a keyboard display and a typing sound are all separate programs you install one at a time.
- A plugin adds a panel to the model window — a focus timer, a clock, a counter — and its buttons are pressed on the model window itself, without opening another window.
- Plugins are downloaded when you install them, so they are not part of the BongoCat installer.
- A plugin is now its own program, installed from a download and started when you switch it on. A plugin that stops responding costs its own card and nothing else, and adding a plugin never changes the BongoCat installer.
- Every plugin has its own settings, opened from its own card and filled in with the plugin's own labels in your language — a plugin that is not running cannot be configured, and a plugin nobody is running changes nothing.
- The first plugin is a Pomodoro timer: pick how long a round lasts, what follows it, and whether the next round starts itself. When a round ends the cat reacts and says so.
- A second plugin, Key Display, shows the keys you are holding on the model window as keycaps, in the order you pressed them. It can show the mouse buttons too, and can get out of the way when you are not typing.
- A third plugin, Typing Sound, gives the cat a voice while you type: each key plays one of your model's own motions, and the model's own sound for it. You choose the motion, the shortest gap between two sounds, and whether a held key counts once or many times.

### 🐛 Bug Fixes

- Plugin panels are no longer upside down. A panel was drawn the right way up, but the model window reused the vertex shader that draws the model — which flips the vertical coordinate for the model's bottom-left uv origin — and a panel's pixels are uploaded top row first too, so the flip turned the whole panel over. Text and buttons were inverted on the Pomodoro and Key Display panels. A panel now shares the model background quad's vertex layout and is drawn the way it declared itself.
- On Windows, BongoCat no longer freezes and quits when you click a link that opens an external website, such as "Report an issue" under Settings → About. The window also keeps painting while the browser starts.
- A plugin that plays a sound of your own can now have you choose the file. Its settings show the file it will play, with a button that opens your computer's own file picker — offering the formats the plugin asked for — instead of asking you to know where you put it.
- A plugin that does not show anything on the model window is no longer offered a "Display position" menu with nine choices in it, where every choice did nothing. Choosing one recorded a preference that was then ignored.
- A plugin that stops working now says so. A plugin whose process ended was still reported as running until some other plugin happened to cause a refresh, so a crashed plugin's card could claim it was alive for the rest of the session with no panel and no reason; and a plugin that could not start at all — a quarantined file, a half-copied directory — showed a card with no reason plus one page-wide error that named neither plugin. Each card now carries its own reason.
- Plugin panels stop flickering. The layer channel answered "the plugin worker has not published since you last asked" with the same thing it answered for "there is nothing to draw", and the window asks sixty times a second while a panel is produced about ten times a second — so five frames out of six the model window was told it had no panels and drew none. A panel was visible for one frame in six.
- The plugin center lists its plugins again. A plugin's `plugin.json` is one file read by two sides — the plugin embeds it when it builds, and BongoCat reads it out of the archive to check it — and the copy table had been added only to the plugin's reader. BongoCat's own manifest reader refuses a field it does not know, so every plugin carrying that table was refused: the center showed "this plugin's content does not apply to this version of BongoCat" and an empty list, and the Refresh button would not act. A plugin is now read by the same parser on both sides, so a manifest that builds also installs.
- Plugins draw on the model window again. A plugin announced itself and then wrote everything else — its panel, its buttons, its answers — to a stream the host never reads, so a plugin that introduced itself went silent for the rest of its life. No plugin had ever appeared on the model window, and the two bugs behind it could not be seen from inside a plugin's own tests.
- A plugin's settings appear on its card again. The form was drawn from what the host was told, and the host was told a plugin had no settings at all, because the schema a plugin declared for itself never left its own process. Every settings form in the product was an empty panel.
- On macOS, the menu bar icon no longer disappears after updating to a new version.
- On Windows, an Xbox-mode controller no longer stops responding while another window has focus. BongoCat was reading controllers through a Windows gaming input interface that Windows only delivers to the app that currently owns the foreground window, so gamepad input only arrived while a BongoCat window was focused. Controllers Windows reads as raw HID devices, such as DS4 and Switch modes, were not affected.
- On Windows, analog sticks and triggers on controllers that Windows reads as raw HID devices no longer report wrong values: a stick at rest read as full deflection in one direction, half its travel was clamped there, and an analog trigger at rest sat exactly on the press threshold so it could flicker. This affected Switch-mode, DS4-mode and other non-Xbox controller modes.
- An installed plugin no longer loses its Settings button while it is switched off. The button is always there, and pressing it turns the plugin on so you can configure it — previously a plugin you had just installed showed only a delete button and a switch, with no way to reach its settings at all.
- The switch on a plugin's card is now labelled "Enabled". It used to say "Show on the model window", which described what the switch does rather than what it is.
- A plugin panel's buttons now respond to clicks on the Windows model window.
- The plugin catalog is no longer read with the download timeout, so an unreachable mirror no longer stalls the model window.
- A plugin you install now shows its panel straight away, instead of waiting for you to switch it on.
- A plugin installed from an archive built on another operating system now starts, instead of failing with no explanation.

### 🌍 Localization

Korean is now available in Settings → Appearance & language. Existing configurations are unaffected and keep the language they already had.

## 2.0.1 - 2026-09-29

### 🐛 Bug Fixes

- A plugin that plays a sound of your own can now have you choose the file. Its settings show the file it will play, with a button that opens your computer's own file picker — offering the formats the plugin asked for — instead of asking you to know where you put it.
- A plugin that does not show anything on the model window is no longer offered a "Display position" menu with nine choices in it, where every choice did nothing. Choosing one recorded a preference that was then ignored.
- A plugin that stops working now says so. A plugin whose process ended was still reported as running until some other plugin happened to cause a refresh, so a crashed plugin's card could claim it was alive for the rest of the session with no panel and no reason; and a plugin that could not start at all — a quarantined file, a half-copied directory — showed a card with no reason plus one page-wide error that named neither plugin. Each card now carries its own reason.
- Plugin panels stop flickering. The layer channel answered "the plugin worker has not published since you last asked" with the same thing it answered for "there is nothing to draw", and the window asks sixty times a second while a panel is produced about ten times a second — so five frames out of six the model window was told it had no panels and drew none. A panel was visible for one frame in six.
- The plugin center lists its plugins again. A plugin's `plugin.json` is one file read by two sides — the plugin embeds it when it builds, and BongoCat reads it out of the archive to check it — and the copy table had been added only to the plugin's reader. BongoCat's own manifest reader refuses a field it does not know, so every plugin carrying that table was refused: the center showed "this plugin's content does not apply to this version of BongoCat" and an empty list, and the Refresh button would not act. A plugin is now read by the same parser on both sides, so a manifest that builds also installs.
- Plugins draw on the model window again. A plugin announced itself and then wrote everything else — its panel, its buttons, its answers — to a stream the host never reads, so a plugin that introduced itself went silent for the rest of its life. No plugin had ever appeared on the model window, and the two bugs behind it could not be seen from inside a plugin's own tests.
- A plugin's settings appear on its card again. The form was drawn from what the host was told, and the host was told a plugin had no settings at all, because the schema a plugin declared for itself never left its own process. Every settings form in the product was an empty panel.

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

- A plugin that plays a sound of your own can now have you choose the file. Its settings show the file it will play, with a button that opens your computer's own file picker — offering the formats the plugin asked for — instead of asking you to know where you put it.
- A plugin that does not show anything on the model window is no longer offered a "Display position" menu with nine choices in it, where every choice did nothing. Choosing one recorded a preference that was then ignored.
- A plugin that stops working now says so. A plugin whose process ended was still reported as running until some other plugin happened to cause a refresh, so a crashed plugin's card could claim it was alive for the rest of the session with no panel and no reason; and a plugin that could not start at all — a quarantined file, a half-copied directory — showed a card with no reason plus one page-wide error that named neither plugin. Each card now carries its own reason.
- Plugin panels stop flickering. The layer channel answered "the plugin worker has not published since you last asked" with the same thing it answered for "there is nothing to draw", and the window asks sixty times a second while a panel is produced about ten times a second — so five frames out of six the model window was told it had no panels and drew none. A panel was visible for one frame in six.
- The plugin center lists its plugins again. A plugin's `plugin.json` is one file read by two sides — the plugin embeds it when it builds, and BongoCat reads it out of the archive to check it — and the copy table had been added only to the plugin's reader. BongoCat's own manifest reader refuses a field it does not know, so every plugin carrying that table was refused: the center showed "this plugin's content does not apply to this version of BongoCat" and an empty list, and the Refresh button would not act. A plugin is now read by the same parser on both sides, so a manifest that builds also installs.
- Plugins draw on the model window again. A plugin announced itself and then wrote everything else — its panel, its buttons, its answers — to a stream the host never reads, so a plugin that introduced itself went silent for the rest of its life. No plugin had ever appeared on the model window, and the two bugs behind it could not be seen from inside a plugin's own tests.
- A plugin's settings appear on its card again. The form was drawn from what the host was told, and the host was told a plugin had no settings at all, because the schema a plugin declared for itself never left its own process. Every settings form in the product was an empty panel.

- Held keys and gamepad buttons no longer stay stuck after device, lock, sleep and permission changes.
- Switching models or resizing the window no longer leaves the model window black, transparent or flickering.

### 🗑️ Removals

- The "Key release timeout" setting is removed.
- Traditional Chinese, Portuguese and Vietnamese are no longer available.

### 💻 Support Changes

- Supported platforms are Windows 10 1903+ (x64) and macOS 12+ (Intel/Apple Silicon).
- Windows ARM runs the x64 build through emulation.
- **Windows x86, Windows ARM64 and Linux builds are not provided.**
