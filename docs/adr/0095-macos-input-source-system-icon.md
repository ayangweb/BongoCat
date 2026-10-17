# ADR-0095: Use the system's input source icon on macOS

Status: accepted; implemented
Date: 2026-10-17

## Context

ADR-0094 had the macOS monitor read only `kTISPropertyIconImageURL`. That is
enough for an input method that ships a file, but it is not where the system
gets the icon it draws itself. On current releases every `com.apple.keylayout.*`
source has no icon URL at all — `com.apple.keylayout.ABC` reports `NULL` — while
the input menu and the switcher draw a rounded dark tile with a white `A`.
Because the plugin then fell back to a two-character badge, the model window
showed `AB` beside `ABC` instead of the system's own artwork.

The artwork is produced by `KLInputSourceIconManager` in the private
`KeyboardLayouts.framework`. Its `imageForTISInputSource:` takes a TIS input
source and returns the same `NSImage` (44x32 for a label icon, rendered from the
source's `TISPropertyInputSourceIconLabels` primary label) that System Settings
and the input menu use. The framework is loaded in-process; no system process is
queried, no accessibility or input-monitoring permission is involved, and no
text, composition state or application identity is read.

## Decision

The macOS adapter resolves an icon in this order:

1. `-[KLInputSourceIconManager imageForTISInputSource:]` from the system
   framework, so the plugin shows exactly what the system shows.
2. `kTISPropertyIconImageURL`, decoded with the existing AppKit/CoreGraphics
   path, for an input method whose system icon manager returns nothing.
3. The localized-name badge in the plugin, unchanged.

The private framework is a platform adapter concern only and stays inside
`bongocat-platform/src/input_source/macos.rs`; no private type or handle leaves
the adapter. The framework is loaded with `dlopen(RTLD_LOCAL)` and deliberately
never unloaded, because its class and image cache outlive the call and a
`dlclose` could invalidate artwork still in use. A missing framework, class,
manager, or image is not an error: the next fallback runs and the plugin
remains usable.

The `__TISInputSource *` parameter is declared locally with its runtime
encoding, so objc2's checked message send verifies the selector and its ABI
instead of bypassing verification. The returned autoreleased image is retained
before the pool drains.

Icons are fitted into the fixed 32x32 RGBA slot with their aspect ratio kept
and centered. System label artwork is wider than it is tall, and stretching it
to the slot would be the second way to display something the system does not.

## Consequences

- Keyboard layouts now show the system's own tile, including after switching
  between input methods and on Retina displays, because the image is drawn into
  the same fixed slot as before.
- The private dependency is limited to one adapter function and one fallback
  order; a future macOS release that renames the class degrades to the documented
  URL and then to the badge, rather than failing the plugin.
- The plugin protocol, stored configuration and icon size are unchanged.
- Windows behavior is unchanged.

## Verification

- `cargo test --locked -p bongocat-platform --lib` covers row normalization and
  the aspect-ratio fit; a manual smoke on a logged-in macOS desktop compares the
  model-window icon with the system input menu for ABC and a third-party input
  method.
