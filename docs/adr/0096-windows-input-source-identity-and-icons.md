# ADR-0096: Follow the Windows input method by re-reading the session profile

Status: accepted; implemented
Date: 2026-10-21

## Context

ADR-0094 had the Windows adapter sample only the foreground thread's HKL. Measured
on Windows 11 26200 with 微软拼音, 微软五笔 and 微信输入法 installed, that model does
not describe what the user sees:

- Every installed Text Services input method reports `hklSubstitute` 0 from
  `ITfInputProcessorProfileMgr::EnumProfiles`, and activating one leaves the
  thread's HKL at the language layout. A window typing with 微软拼音 reports
  `0x08040804`, the `中文(简体) - 美式键盘` layout. The indicator therefore named a
  keyboard layout the user had not selected.
- `ImmGetDescriptionW` returns nothing for these input methods, so the IMM
  fallback could not correct it either.
- The adapter never extracted an icon, so every source rendered as a badge.

A first revision of this ADR worked around that by naming the language's single
installed input method when the language had exactly one, and the language
otherwise. That is a guess, and users hit it: installing a second input method, or
switching between two, left the indicator describing a state that no longer held,
and only a restart corrected it.

The cause of "only a restart corrects it" is the key fact this ADR records. Text
Services freezes a process's profile state when the process creates its first
profile manager:

- A manager created at process start answers `GetActiveProfile` with the input
  method that was running then, for the whole life of the object.
- While any manager is alive, even a newly created one answers with that frozen
  value.
- Releasing the last manager and creating a new one answers with the session's
  current input method, immediately.
- A freshly started process does the same, which is why restarting appeared to fix
  the indicator.

Measured with the session switched from 微信输入法 to 微软拼音 in between reads:

| state | answer |
| --- | --- |
| no manager held, before the switch | 微信输入法 |
| no manager held, after the switch | 微软拼音 |
| manager held across the switch | 微信输入法 (frozen) |
| new manager created while one is held | 微信输入法 (frozen) |
| after dropping the held manager | 微软拼音 |

Creating and dropping a manager costs about 5 µs, so reading the session on every
snapshot is cheaper than the 200 ms tick it serves.

The same probes established what else is and is not available: the conversion mode
of the focused thread's input method, read through its IME window; and the full
registration of every profile — localized description, icon file, icon index —
from the registry that mirrors the Text Services catalogue. Several documented
cross-process paths remain closed and are not used: `GetThreadLangBarItemMgr`
returns `E_FAIL`, `GetThreadMarshalInterface` returns an object supporting none of
its advertised interfaces, `GetInputProcessorProfiles(thread)` and the `ACTIVE`
flag are process-scoped, and the session's choice leaves no trace in the registry.

Open-source implementations were inspected:
[ImTip](https://github.com/aardio/ImTip) (aardio) and
[InputTip](https://github.com/abgox/InputTip) (AutoHotkey) report the language and
the 中/英 mode — `InputTip/src/core/ime.ahk` maps the layout id to CN/EN/US/JP/KR
and reads the open status and conversion mode of its IME window — because they read
a retained profile manager and therefore cannot see a switch either;
[IME_Indicator](https://github.com/HaujetZhao/IME_Indicator) (Rust) reads the mode
with `ImmGetDefaultIMEWnd` plus `WM_IME_CONTROL`, bounded by `SendMessageTimeoutW`,
which this adapter reuses. No code is copied.

## Decision

The adapter holds no Text Services object across snapshots. Every snapshot creates
one `ITfInputProcessorProfileMgr`, asks for the active profile of the keyboard
category, and drops it before returning; that release is what lets the next
snapshot see a later switch.

The identity is then the profile the session reports, not an inference:

1. **An active input method** is identified by its `(clsid, guid, langid)`. Its
   localized name and icon come from its own registration (`HKLM`, then `HKCU`,
   `SOFTWARE\Microsoft\CTF\TIP\{clsid}\LanguageProfile\0x<langid>\{guid}`:
   `Display Description`/`Description`, `IconFile`/`IconIndex`).
2. **A plain keyboard layout** — no active input method, or one the profile does not
   cover — is named from the layout registry: `Layout Display Name` resolved through
   `SHLoadIndirectString`, then `Layout Text`, then the language's localized display
   name. An input method that registered a substitute layout handle can still name
   itself through `ImmGetDescriptionW` and draw itself from `ImmGetIMEFileNameW`.
   A layout has no icon of its own, so it reports the label the system draws in that
   place: the language's three-letter ISO 639-2 code, upper-cased
   (`LOCALE_SISO639LANGNAME2`, so `ENG`, `DEU`, `FRA`, `JPN`, `KOR`). The HUD draws
   that label instead of a badge of its own; the name-derived badge remains only
   for a source that offers neither an icon nor a label.
3. **A session input method whose language differs from the focused window's layout**
   is ignored: that window is on another language whatever the session activated.

The identity is the profile alone. The conversion mode inside an input method —
the 中/英 toggle — is deliberately not read and not reported in this release: the
indicator shows which input method is running, and a toggle inside one method is
not a change of source, so it neither changes the display nor counts as a switch
for the plugin's switch-only policy. Should that state be wanted later, it is
available from the focused thread's IME window
(`ImmGetDefaultIMEWnd` + `WM_IME_CONTROL`/`IMC_GETOPENSTATUS` then
`IMC_GETCONVERSIONMODE`, bounded by `SendMessageTimeoutW` with
`SMTO_ABORTIFHUNG`), which is the mechanism ADR-0094's research recorded.

Icons are extracted with `SHDefExtractIconW` at 32x32, decoded from the color
bitmap and the AND mask into straight RGBA8, and box-filtered into the fixed slot.
A plain keyboard layout gets no icon: every layout module would extract the same
generic keyboard artwork, which is the placeholder this indicator must not show, so
the system's own label is shown instead.

No hook, no injection, no thread impersonation, and no unbounded cross-process call
is added. The plugin reads no text, composition state, caret position or
application identity, and it never switches an input method.

## Consequences

- Switching input methods is reflected on the next snapshot, within the 200 ms host
  tick, with the correct localized name and the method's own icon. No restart is
  involved. ADR-0094 records why the tick is the trigger on Windows: no
  notification for an in-place input method switch reaches a background process.
  Switching the 中/英 mode inside one method does not change the display, because
  the display is the input method.
- The "exactly one installed input method" heuristic is gone, along with the
  catalogue enumeration and the installed-method list it needed: the session says
  which method is running, so nothing has to be counted or guessed.
- A snapshot costs a manager create/drop (≈5 µs), a few registry reads for the
  name, and no work at all for the icon after the first decode. Measured round
  trip for a tick: 1.9 ms average, 3.8 ms worst.
- The name and icon follow the *session*, which is what the taskbar shows and what
  a restarted process reads. A window pinned to another language is reported from
  its own layout instead.
- Nothing is invented: an input method is drawn with its registered icon, a layout
  with the system's language label, and only a source with neither falls back to a
  badge cut from its name.
- `windows` gains the `Win32_Graphics_Gdi`, `Win32_System_Com`,
  `Win32_System_Environment`, `Win32_UI_Shell` and `Win32_UI_TextServices`
  features; no new crate is added and no type leaves the adapter.
- The plugin protocol, stored configuration, icon size and macOS behavior are
  unchanged.

## Verification

- `cargo test --locked -p bongocat-input-source`: handle classification, mode
  markers, GUID and profile-key spelling, the identity of two input methods sharing
  one layout handle, mask decoding and icon fitting, plus the packaged-process
  protocol test. Clippy with `-D warnings`, `cargo fmt --check`, `cargo check
  --workspace` and the desktop smoke example all pass.
- On a logged-in Windows 11 desktop, with the session switched between 微信输入法
  and 微软拼音 by a separate process while the plugin kept running: the plugin
  published `微信输入法`, then `微软拼音`, then `微信输入法`, each with its own
  4096-byte icon, on the next tick, with no restart.
- A 60-tick soak across three switches: no errors, tick round trip 1.9 ms average
  and 3.8 ms worst, clean shutdown.
- A 中/英 toggle inside the running method left the published name unchanged, as
  intended for this release.
- Icons extracted for all eleven installed input methods (微软拼音, 微软五笔,
  微信输入法, 微软仓颉, 微软速成, 微软注音, 繁体中文大易, 繁体中文行列, the
  Japanese and Korean Microsoft IMEs) were decoded and compared visually with each
  method's own artwork.
- The layout branch is covered by unit tests: the US layout reports `ENG` with no
  icon, and the HUD draws that label; a missing or over-long label falls back to the
  name badge.
- Not verified: Windows 10, a UWP or elevated foreground window, a legacy IMM32
  input method (none installed), a window pinned to a language other than the
  session's, and a Chinese keyboard layout (only input methods are installed for
  Chinese here, so its label could not be compared with the system's).
