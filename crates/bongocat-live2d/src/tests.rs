//! The adapter's tests, split by the module they cover.
//!
//! The key-vocabulary tests live in one module because they share a fixture
//! idea rather than a subject: a key name only counts if a model the repository
//! ships can actually draw it, so they are all asked the same way.

use super::*;

use bongocat_live2d_render::{
    KeyImageInventory, key_name_candidates, load_background_asset, load_key_assets,
    resolve_key_overlays,
};
use bongocat_render::{BlendMode, KeySide};
use std::path::Path;

/// Every HID usage the key vocabulary has to cover, written out rather than
/// derived from the ranges this crate happens to use.
///
/// This is a **superset** of what either platform adapter can produce, and
/// deliberately so: the vocabulary is a contract with model authors, so a
/// key is named even when no keyboard can press it. Two usages here are
/// unreachable on both platforms today — `IntlHash` (`0x32`, macOS gives the
/// ISO `#` key the same keycode as ANSI `\`) and `F21` … `F24`
/// (`0x70..=0x73`, no Carbon keycode and no Windows scan code this adapter
/// maps). Everything else is reachable on at least one platform; each
/// adapter has its own exhaustive test pinning exactly which
/// (`bongocat-platform`'s `this_adapter_reports_exactly_the_keycodes_the_platform_defines`
/// and the Windows scan-code matrix).
///
/// The modifier block sits outside `0x04..=0x65` and outside `0x68..=0x73`,
/// and the globe key is not on the Keyboard/Keypad page at all, so no range
/// over that page can be made to include it. Both were dropped once by a
/// rewrite that only looked at the ranges the implementation used.
fn adapter_keyboard_usages() -> Vec<u16> {
    (0x04..=0x65)
        .chain([0x67])
        .chain(0x68..=0x73)
        .chain(0xe0..=0xe7)
        .chain([bongocat_render::GLOBE_KEY_USAGE])
        .collect()
}

mod automatic;
mod error;
mod model;
mod parameter;
mod vocabulary;
