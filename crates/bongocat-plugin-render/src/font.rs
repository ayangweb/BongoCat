//! The one piece of text rendering the product did not have.
//!
//! Everything BongoCat draws is a texture the model window samples, and a plugin
//! panel is the same: a tree of nodes becomes an RGBA raster here, and the
//! overlay uploads it and draws one quad. Text therefore has to be rasterized on
//! the CPU, and this is where that happens.
//!
//! # Why the font is a system font
//!
//! There is no bundled typeface. Two reasons, and the second is the one that
//! matters:
//!
//! * A font file is a redistribution decision. Shipping one means choosing a
//!   license, shipping the license and the attribution, and keeping them in step
//!   across every artifact the packaging pipeline produces — for a decorative
//!   label at 13 logical pixels.
//! * A system font is what the rest of the operating system already uses for the
//!   same kind of text, so a panel reads as part of the system it sits in rather
//!   than as a product with a type choice.
//!
//! The cost is that the font depends on the machine. It is handled by a fixed,
//! per-platform candidate list and by one rule: a panel whose text cannot be
//! drawn is a panel with no text, not a failed load. A missing font degrades the
//! display; it never stops the plugin from running.
//!
//! # Why glyphs are cached by content
//!
//! Rasterizing a glyph is a curve walk and a rasterization, and a panel redraws
//! whenever a bound value changes — once a second for a countdown. Caching by
//! `(text, size, weight)` means the steady state of a running timer is a map
//! lookup per string, and the first draw of a given string is the only one that
//! costs. The cache is bounded, because a counter that shows a growing number
//! would otherwise accumulate one entry per value.

use crate::error::{PluginRenderError, PluginRenderErrorCode};
use ab_glyph::{Font, FontVec, GlyphId, PxScale, ScaleFont};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The fonts this build will try, in order.
///
/// A list rather than a lookup so the choice is reviewable: adding a font to
/// this list is a decision, and the order decides which face wins when several
/// are installed. The first entry is the one that will normally be used.
#[cfg(target_os = "macos")]
pub const FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/Geneva.ttf",
    "/System/Library/Fonts/Supplemental/Verdana.ttf",
    "/System/Library/Fonts/Supplemental/Tahoma.ttf",
];

/// The fonts this build will try, in order.
#[cfg(target_os = "windows")]
pub const FONT_CANDIDATES: &[&str] = &[
    r"C:\Windows\Fonts\segoeui.ttf",
    r"C:\Windows\Fonts\segoeuib.ttf",
    r"C:\Windows\Fonts\arial.ttf",
    r"C:\Windows\Fonts\verdana.ttf",
    r"C:\Windows\Fonts\tahoma.ttf",
];

/// The weight a face is loaded at.
///
/// Two faces rather than a variable font: a variable axis would let a panel ask
/// for any weight, and the two a panel can actually use are the two the system
/// provides as separate files. A weight that is not regular or bold is drawn at
/// the nearest one, so the worst case is a slightly wrong weight rather than a
/// missing one.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum FontWeight {
    #[default]
    Regular,
    Bold,
}

impl FontWeight {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Bold => "bold",
        }
    }
}

/// A loaded face, shared by every panel on the window.
#[derive(Clone)]
pub struct FontBook {
    faces: Arc<HashMap<FontWeight, Arc<FontVec>>>,
    /// Where the faces came from, for a log line. Not a user-facing message: a
    /// developer reproducing a rendering difference needs it, a user does not.
    loaded_from: Arc<Vec<(FontWeight, PathBuf)>>,
}

impl std::fmt::Debug for FontBook {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FontBook")
            .field("loaded_from", &self.loaded_from)
            .finish()
    }
}

impl FontBook {
    /// Load the first readable candidate for each weight.
    ///
    /// A weight with no readable candidate is simply absent; [`Self::face`]
    /// reports that and callers fall back. That is the whole degradation story:
    /// a machine with no readable system font gets panels without text and no
    /// error anywhere.
    pub fn load_system() -> Self {
        Self::load_from(FONT_CANDIDATES, FONT_CANDIDATES)
    }

    /// Load from explicit candidate lists, one per weight.
    pub fn load_from(regular: &[&str], bold: &[&str]) -> Self {
        let mut faces = HashMap::new();
        let mut loaded_from = Vec::new();
        for (weight, candidates) in [(FontWeight::Regular, regular), (FontWeight::Bold, bold)] {
            for candidate in candidates {
                let Ok(bytes) = std::fs::read(candidate) else {
                    continue;
                };
                let Ok(face) = FontVec::try_from_vec(bytes) else {
                    // A file that is not a font this parser understands is a
                    // candidate that does not apply, not a failure: the list has
                    // more than one entry precisely so a surprising installation
                    // does not end the search.
                    continue;
                };
                loaded_from.push((weight, PathBuf::from(candidate)));
                faces.insert(weight, Arc::new(face));
                break;
            }
        }
        Self {
            faces: Arc::new(faces),
            loaded_from: Arc::new(loaded_from),
        }
    }

    /// The face for a weight, preferring the exact one and falling back to the
    /// other.
    ///
    /// A panel asking for bold on a machine with no bold face gets regular text
    /// rather than no text.
    pub fn face(&self, weight: FontWeight) -> Option<Arc<FontVec>> {
        self.faces
            .get(&weight)
            .or_else(|| self.faces.get(&FontWeight::Regular))
            .or_else(|| self.faces.get(&FontWeight::Bold))
            .cloned()
    }

    /// Whether any face loaded at all.
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// The candidate list this build would try, for a message that explains a
    /// machine with no usable font.
    pub fn candidates() -> &'static [&'static str] {
        FONT_CANDIDATES
    }

    /// Where each face was loaded from, for a log line.
    pub fn loaded_from(&self) -> &[(FontWeight, PathBuf)] {
        &self.loaded_from
    }
}

/// One glyph in a measured run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedGlyph {
    pub id: GlyphId,
    /// Pen position at this glyph, in logical pixels from the run's origin.
    pub x: f32,
}

/// A measured line of text: its glyphs, its width, and where it sits.
///
/// A measurement is in **logical** pixels and is deliberately independent of the
/// canvas it will be drawn on. That is what makes the cache worth having: the
/// same `25:00` measured once is reused at every display scale, and only the
/// outline rasterization at draw time differs. It is also what keeps a layout
/// pass free of the display's scale, so a panel is laid out the same on a Retina
/// laptop and on an external monitor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLine {
    pub glyphs: Vec<PositionedGlyph>,
    /// Advance width in logical pixels.
    pub width: f32,
    /// Distance from the baseline to the top of the font's tallest glyph.
    pub ascent: f32,
    /// Distance from the baseline down to the bottom of the font's lowest glyph.
    ///
    /// A positive distance, not a signed one. The font's own metric is negative
    /// because it points down the page, but every consumer here wants "how far
    /// below the baseline does this run reach", and a negative answer turns a
    /// height into a subtraction that can come out negative.
    pub descent: f32,
    /// The logical size the run was measured at.
    pub size: f32,
    pub weight: FontWeight,
}

impl TextLine {
    /// The width of the run, in logical pixels.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// The height of the run from the top of the font's tallest glyph to the
    /// bottom of the lowest, in logical pixels.
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

/// What one run of text needs before it can be measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Cap height in pixels.
    pub size: f32,
    pub weight: FontWeight,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 14.0,
            weight: FontWeight::Regular,
        }
    }
}

/// Measures a run of text and caches the result.
///
/// The cache is keyed by the string, the size and the weight, and holds a bounded
/// number of entries. The bound is the reason a counter that shows a growing
/// number does not grow this forever: past the bound the least recently measured
/// entry is dropped, which costs a re-measure and nothing else.
#[derive(Debug)]
pub struct TextMeasurer {
    book: FontBook,
    cache: HashMap<TextKey, TextLine>,
    order: Vec<TextKey>,
}

/// A cache key.
///
/// The size is stored as its bit pattern rather than as an `f32`, because the key
/// has to be hashable and `f32` is not — and bit equality is stricter than value
/// equality here in a way that costs nothing: two sizes that differ only in their
/// lowest mantissa bit are two different renderings.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TextKey {
    text: String,
    size_bits: u32,
    weight: FontWeight,
}

impl TextKey {
    fn new(text: &str, style: TextStyle) -> Self {
        Self {
            text: text.to_string(),
            size_bits: style.size.to_bits(),
            weight: style.weight,
        }
    }
}

impl TextMeasurer {
    /// How many distinct strings are held at once.
    ///
    /// A panel has a handful of strings; thirty is several panels' worth and
    /// keeps the map small enough that the lookup is faster than a re-measure on
    /// any real machine.
    pub const CAPACITY: usize = 30;

    pub fn new(book: FontBook) -> Self {
        Self {
            book,
            cache: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// A measurer with no face loaded.
    ///
    /// Every measurement then returns `None` and every panel draws without text,
    /// which is the whole degradation path on a machine with no readable system
    /// font. It exists so a caller can construct one without touching the
    /// filesystem.
    pub fn empty() -> Self {
        Self::new(FontBook::load_from(&[], &[]))
    }

    pub fn book(&self) -> &FontBook {
        &self.book
    }

    /// Measure a run of text, reusing a previous measurement when it applies.
    ///
    /// Returns `None` when no face is available, which is the signal to draw the
    /// node without text rather than to fail the panel.
    pub fn measure(&mut self, text: &str, style: TextStyle) -> Option<&TextLine> {
        let key = TextKey::new(text, style);
        if !self.cache.contains_key(&key) {
            if self.order.len() >= Self::CAPACITY
                && let Some(evicted) = self.order.first().cloned()
            {
                // The oldest entry goes, and its order marker with it, so the
                // order list stays exactly as long as the cache.
                self.order.remove(0);
                self.cache.remove(&evicted);
            }
            // An empty run measures to nothing, and a missing face measures to
            // nothing as well: both come back as an empty line rather than as
            // `None`, so a node's height is the same whether or not the machine
            // can draw the text. Only a *missing* face returns `None`, and that
            // case is handled by the cache entry below never being created.
            let line = if text.is_empty() {
                TextLine {
                    size: style.size,
                    weight: style.weight,
                    ..TextLine::default()
                }
            } else {
                let face = self.book.face(style.weight)?;
                measure_with(&face, text, style)
            };
            self.cache.insert(key.clone(), line);
            self.order.push(key.clone());
        }
        self.cache.get(&key)
    }
}

fn measure_with(face: &FontVec, text: &str, style: TextStyle) -> TextLine {
    // Measured at a fixed reference size and scaled afterwards, rather than at the
    // requested size, so one measurement per face serves every size. Advance
    // widths and vertical metrics are both linear in the pixel scale, so scaling
    // them is exact; the only thing that is not is hinting, which this renderer
    // does not use.
    const REFERENCE_SIZE: f32 = 100.0;
    let scaled = face.as_scaled(PxScale::from(REFERENCE_SIZE));
    let mut line = TextLine {
        size: style.size,
        weight: style.weight,
        ..TextLine::default()
    };
    let mut pen = 0.0_f32;
    for character in text.chars() {
        let id = scaled.glyph_id(character);
        line.glyphs.push(PositionedGlyph { id, x: pen });
        pen += scaled.h_advance(id);
    }
    let factor = style.size / REFERENCE_SIZE;
    line.width = pen * factor;
    // The vertical metrics come from the *scaled* font, not from
    // `ascent_unscaled`: the unscaled pair is in the face's own design units —
    // 1854 for a face with 2048 units per em — so multiplying it by a pixel
    // size produces a number three orders of magnitude too large. `ScaledFont`
    // has already divided by the em, so its values are in pixels.
    line.ascent = scaled.ascent() * factor;
    line.descent = -scaled.descent() * factor;
    line
}

/// A loaded image a scene named, decoded once.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Straight RGBA8, top row first.
    pub pixels: Vec<u8>,
}

impl DecodedImage {
    /// Read a PNG from disk.
    pub fn read_png(path: &Path) -> Result<Self, PluginRenderError> {
        let bytes = std::fs::read(path).map_err(|error| {
            PluginRenderError::new(PluginRenderErrorCode::ImageUnreadable, error)
        })?;
        Self::decode_png(&bytes).map_err(|error| {
            PluginRenderError::new(
                PluginRenderErrorCode::ImageUnreadable,
                error
                    .detail
                    .unwrap_or_else(|| "not a readable PNG".to_string()),
            )
        })
    }

    /// Decode a PNG from memory.
    ///
    /// The PNG signature is checked before anything else: a plugin's image is
    /// display artwork, so a file that is not a PNG is a wrong picture rather
    /// than a broken plugin, and refusing it early keeps the failure legible.
    pub fn decode_png(bytes: &[u8]) -> Result<Self, PluginRenderError> {
        const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        if !bytes.starts_with(&PNG_SIGNATURE) {
            return Err(PluginRenderError::new(
                PluginRenderErrorCode::ImageUnreadable,
                "not a PNG",
            ));
        }
        let image = image::load_from_memory(bytes)
            .map_err(|error| PluginRenderError::new(PluginRenderErrorCode::ImageUnreadable, error))?
            .to_rgba8();
        Ok(Self {
            width: image.width(),
            height: image.height(),
            pixels: image.into_raw(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system() -> FontBook {
        FontBook::load_system()
    }

    #[test]
    fn a_system_font_loads_on_this_machine() {
        // Not every machine has one of the candidates, and the product has to
        // work on one that does not. The assertion is that *this* machine, which
        // is a supported platform, does — so a candidate list that has stopped
        // matching reality is caught here rather than in a screenshot.
        let book = system();
        assert!(
            !book.is_empty(),
            "no candidate font loaded; tried {:?}",
            FontBook::candidates()
        );
        assert!(book.face(FontWeight::Regular).is_some());
    }

    #[test]
    fn a_missing_candidate_is_skipped_rather_than_failing() {
        let book = FontBook::load_from(&["/nonexistent/a.ttf"], &["/nonexistent/b.ttf"]);
        assert!(book.is_empty());
        assert!(book.face(FontWeight::Regular).is_none());
    }

    #[test]
    fn a_weight_with_no_face_falls_back_rather_than_disappearing() {
        let mut faces = HashMap::new();
        faces.insert(
            FontWeight::Regular,
            system().face(FontWeight::Regular).unwrap(),
        );
        let book = FontBook {
            faces: Arc::new(faces),
            loaded_from: Arc::new(Vec::new()),
        };
        assert!(book.face(FontWeight::Bold).is_some());
    }

    #[test]
    fn measuring_a_run_accumulates_advance_widths() {
        let mut measurer = TextMeasurer::new(system());
        let line = measurer.measure("Focus", TextStyle::default()).unwrap();
        assert!(line.width > 0.0, "a non-empty run has width");
        assert_eq!(line.glyphs.len(), "Focus".chars().count());
        assert!(line.ascent > 0.0);
    }

    #[test]
    fn an_empty_run_measures_to_nothing_and_still_says_what_size_it_was() {
        let mut measurer = TextMeasurer::new(system());
        let style = TextStyle {
            size: 22.0,
            weight: FontWeight::Regular,
        };
        let line = measurer.measure("", style).cloned().unwrap();
        assert_eq!(line.glyphs.len(), 0);
        assert_eq!(line.width(), 0.0);
        // The size is carried even with no glyphs, because a node that fell back
        // to the text's own size still needs it, and a zero here would make an
        // empty label collapse its row.
        assert_eq!(line.size, 22.0);
    }

    #[test]
    fn the_same_string_is_measured_once() {
        let mut measurer = TextMeasurer::new(system());
        let first = measurer.measure("25:00", TextStyle::default()).cloned();
        let second = measurer.measure("25:00", TextStyle::default()).cloned();
        assert_eq!(first, second);
        assert_eq!(measurer.order.len(), 1);
    }

    #[test]
    fn a_different_size_is_a_different_entry() {
        let mut measurer = TextMeasurer::new(system());
        measurer.measure("25:00", TextStyle::default());
        measurer.measure(
            "25:00",
            TextStyle {
                size: 24.0,
                weight: FontWeight::Regular,
            },
        );
        assert_eq!(measurer.order.len(), 2);
    }

    #[test]
    fn the_cache_is_bounded_so_a_changing_number_does_not_grow_it() {
        let mut measurer = TextMeasurer::new(system());
        for index in 0..(TextMeasurer::CAPACITY * 3) {
            measurer.measure(&index.to_string(), TextStyle::default());
        }
        assert_eq!(measurer.order.len(), TextMeasurer::CAPACITY);
        assert!(measurer.cache.len() <= TextMeasurer::CAPACITY);
    }

    #[test]
    fn a_non_png_is_refused_rather_than_decoded() {
        assert!(DecodedImage::decode_png(b"not a png at all").is_err());
    }
}
