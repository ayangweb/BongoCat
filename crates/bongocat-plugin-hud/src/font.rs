//! The text face a plugin's panel is drawn with.
//!
//! The host does not bundle a font. A panel is a small label, a count or a
//! timer, and the platform's own face is what the user sees everywhere else in
//! the system, so loading it keeps the binary free of a font license obligation
//! while matching the surrounding text. A platform that has none of the expected
//! faces reports that once, and the caller drops the panel rather than drawing
//! labels in an invented fallback face.
//!
//! A plugin's label is arbitrary UTF-8 — the shipped timer writes both English
//! and Chinese — so a face that only covers Latin would draw every non-Latin
//! character as a placeholder box. The candidates are therefore ordered by
//! coverage first and plain looks second, and a character the chosen face still
//! lacks is skipped instead of being drawn as tofu. See [`crate::HudFont`].

use crate::{HudFont, HudRendererError};

/// The font files each platform ships, in preference order.
///
/// A face that covers the CJK ranges a localized label needs comes before the
/// plainer Latin-only faces, because the width and the look of a missing glyph
/// are both worse than a slightly heavier face.
#[cfg(target_os = "macos")]
const PLATFORM_FONT_CANDIDATES: &[(&str, u32)] = &[
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
    ("/System/Library/Fonts/Supplemental/Arial.ttf", 0),
    ("/System/Library/Fonts/Helvetica.ttc", 0),
    ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", 0),
    ("/System/Library/Fonts/SFNSMono.ttf", 0),
];

#[cfg(target_os = "windows")]
const PLATFORM_FONT_CANDIDATES: &[(&str, u32)] = &[
    (r"C:\Windows\Fonts\msyh.ttc", 0),
    (r"C:\Windows\Fonts\segoeui.ttf", 0),
    (r"C:\Windows\Fonts\arial.ttf", 0),
    (r"C:\Windows\Fonts\tahoma.ttf", 0),
    (r"C:\Windows\Fonts\malgun.ttf", 0),
];

/// Load the platform's text face.
///
/// The first candidate that reads and parses wins, so a trimmed-down install
/// that removed the preferred face still gets a usable one.
pub fn load_platform_font() -> Result<HudFont, HudRendererError> {
    let mut last_error = None;
    for (path, collection_index) in PLATFORM_FONT_CANDIDATES {
        match std::fs::read(path) {
            Ok(bytes) => match HudFont::from_bytes(&bytes, *collection_index) {
                Ok(font) => return Ok(font),
                Err(error) => last_error = Some(error),
            },
            Err(error) => {
                last_error = Some(HudRendererError::new(format!(
                    "cannot read platform font {path}: {error}"
                )));
            }
        }
    }
    Err(last_error
        .unwrap_or_else(|| HudRendererError::new("the platform ships no font the panel can use")))
}
