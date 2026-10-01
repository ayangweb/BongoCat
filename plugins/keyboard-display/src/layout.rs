//! How wide a keycap is, and therefore how big this plugin's panel is.
//!
//! Every number here follows from two the user chose: the font size and whether it is
//! bold. A keycap is the product's font at that size plus padding proportional to it, and
//! the panel is as many keycaps per row as fit, times as many rows as there are keys.
//!
//! This is a separate module because it is the one part of the plugin that is pure
//! arithmetic, and pure arithmetic is the part worth testing on its own: a wrong keycap
//! width does not crash, it clips, and a clipped keycap is a thing a user reports as "the
//! last letter is cut off" with no way for anybody to find where the number came from.

use crate::settings::{MAXIMUM_PANEL_HEIGHT, MINIMUM_PANEL_WIDTH, Preferences};
use crate::{KEY_GAP, PADDING_RATIO, PANEL_PADDING, RADIUS_RATIO};

/// One keycap's box, in the panel's own logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keycap {
    /// The size of the letter, which is what the keycap is sized around.
    pub font_size: f32,
    /// Whether the letter is drawn bold, which makes it wider.
    pub bold: bool,
}

impl Keycap {
    /// The widest one label on this cap.
    ///
    /// The product's own bound, taken as given rather than measured: the cap reserves room
    /// for the longest label a key can carry — the arrow names and the modifier names the
    /// protocol spells — so a cap that is comfortable for `A` does not clip `PageDown`.
    /// Measuring would be more honest and would make the panel's size depend on which keys
    /// happen to be held, which is a panel that resizes as you type.
    pub const WIDEST_LABEL: &'static str = "ScrollLock";

    /// The horizontal room a label occupies, in logical pixels.
    ///
    /// An estimate from the font's own metrics rather than a measurement, and deliberately
    /// generous: a cap that is a little too wide wastes a few pixels of a panel nobody
    /// measures, and a cap that is a little too narrow clips a letter somebody is explaining
    /// on a stream.
    fn label_width(&self) -> f32 {
        // A bold letter is about a tenth wider, which is the ratio the product's own font
        // book uses when it measures a string in bold against the same string regular.
        let weight = if self.bold { 1.1 } else { 1.0 };
        Self::WIDEST_LABEL.chars().count() as f32 * self.font_size * 0.62 * weight
    }

    /// This cap's width, including its own padding.
    pub fn width(&self) -> f32 {
        self.label_width() + self.font_size * PADDING_RATIO * 2.0
    }

    /// This cap's height, including its own padding.
    ///
    /// Taller than the line height rather than equal to it: a letter needs room above and
    /// below its cap height, and a keycap whose letter touches its own edge is a keycap
    /// that looks like a mistake.
    pub fn height(&self) -> f32 {
        self.font_size * 1.4 + self.font_size * PADDING_RATIO * 2.0
    }

    /// This cap's padding, in logical pixels.
    pub fn padding(&self) -> f32 {
        self.font_size * PADDING_RATIO
    }

    /// This cap's corner radius, in logical pixels.
    pub fn radius(&self) -> f32 {
        self.font_size * RADIUS_RATIO
    }
}

/// The panel a given set of keys needs, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelBox {
    pub width: u32,
    pub height: u32,
    /// How many keycaps fit on one row at this width.
    pub columns: usize,
}

/// The panel these preferences need for `keys` keycaps.
///
/// Bounded at both ends and the bounds are the interesting part. The width is at least one
/// keycap plus the panel's padding, so a configuration can never produce a panel too narrow
/// to hold what it is about to draw. The height is capped because the model window has a
/// height: a panel taller than the window is a panel whose top row is off the screen, and
/// the honest answer for "sixteen keys at forty pixels each" is fewer rows rather than a
/// panel nobody can see all of.
pub fn panel_for(preferences: &Preferences, keys: usize) -> PanelBox {
    let cap = Keycap {
        font_size: preferences.font_size,
        bold: preferences.bold,
    };
    let cap_width = cap.width();
    let cap_height = cap.height();

    // One column and one row first, so the arithmetic below never divides by zero for a
    // plugin that has been told to show no keys at all.
    let usable = cap_width.max(1.0);
    let columns = (((usable + PANEL_PADDING * 2.0) / (usable + KEY_GAP)).floor() as usize).max(1);
    let rows = keys.div_ceil(columns).max(1);

    let width =
        ((columns as f32) * cap_width + ((columns - 1) as f32) * KEY_GAP + PANEL_PADDING * 2.0)
            .ceil()
            .max(MINIMUM_PANEL_WIDTH as f32) as u32;
    let height = ((rows as f32) * cap_height + ((rows - 1) as f32) * KEY_GAP + PANEL_PADDING * 2.0)
        .ceil()
        .min(MAXIMUM_PANEL_HEIGHT as f32) as u32;
    PanelBox {
        width,
        height,
        columns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{DEFAULT_FONT_SIZE, MAXIMUM_FONT_SIZE, MINIMUM_FONT_SIZE};

    fn preferences(font_size: f64, bold: bool, maximum_keys: usize) -> Preferences {
        Preferences {
            font_size: font_size as f32,
            bold,
            maximum_keys,
            ..Preferences::default()
        }
    }

    #[test]
    fn a_keycap_grows_with_the_font_the_user_asked_for() {
        let small = Keycap {
            font_size: MINIMUM_FONT_SIZE as f32,
            bold: false,
        };
        let large = Keycap {
            font_size: MAXIMUM_FONT_SIZE as f32,
            bold: false,
        };
        assert!(
            large.width() > small.width() && large.height() > small.height(),
            "because a bigger letter has to fit inside a bigger cap, and a cap that did not \
             grow would clip it"
        );
    }

    #[test]
    fn a_bold_keycap_is_wider_than_a_normal_one_of_the_same_size() {
        // The reason the weight is a setting at all rather than a detail: a bold letter is
        // a wider letter, and a cap sized for the normal one clips it.
        let size = DEFAULT_FONT_SIZE as f32;
        let normal = Keycap {
            font_size: size,
            bold: false,
        };
        let bold = Keycap {
            font_size: size,
            bold: true,
        };
        assert!(bold.width() > normal.width());
        assert_eq!(
            bold.height(),
            normal.height(),
            "while the height does not change, because boldness is horizontal"
        );
    }

    #[test]
    fn every_key_the_cap_is_sized_for_fits_inside_it() {
        // The one arithmetic error a user would report as "the last letter is cut off". The
        // cap reserves room for the longest label the protocol can produce, so a check
        // against that label is a check against every label.
        for size in [MINIMUM_FONT_SIZE, DEFAULT_FONT_SIZE, MAXIMUM_FONT_SIZE] {
            for bold in [false, true] {
                let cap = Keycap {
                    font_size: size as f32,
                    bold,
                };
                assert!(
                    cap.width() >= cap.label_width(),
                    "a {size}px cap must hold the widest label the protocol names, bold={bold}"
                );
                assert!(
                    cap.padding() > 0.0,
                    "and it has to be a cap, not a line of text"
                );
            }
        }
    }

    #[test]
    fn the_panel_is_never_narrower_than_one_keycap() {
        // A plugin configured into a panel narrower than what it is about to draw is a
        // plugin that draws nothing, and the user sees an empty box.
        for size in [MINIMUM_FONT_SIZE, DEFAULT_FONT_SIZE, MAXIMUM_FONT_SIZE] {
            let preferences = preferences(size as f64, false, 1);
            let panel = panel_for(&preferences, 1);
            let cap = Keycap {
                font_size: size as f32,
                bold: false,
            };
            assert!(
                panel.width as f32 >= cap.width(),
                "a {size}px panel is {panel:?} and one cap is {}px",
                cap.width()
            );
            assert!(panel.width >= MINIMUM_PANEL_WIDTH);
        }
    }

    #[test]
    fn the_panel_never_exceeds_the_model_windows_height() {
        // Sixteen keys at the largest font is more rows than a model window has room for,
        // and the honest answer is a bounded panel rather than one whose first row is off
        // the top of the screen.
        let preferences = preferences(MAXIMUM_FONT_SIZE as f64, true, 16);
        let panel = panel_for(&preferences, 16);
        assert!(
            panel.height <= MAXIMUM_PANEL_HEIGHT,
            "because a panel taller than the window cannot be read: {panel:?}"
        );
    }

    #[test]
    fn more_keys_never_make_the_panel_narrower() {
        // A panel that shrinks as you press more keys is a panel that moves under the
        // person's hands, which is worse than one that is a little too big.
        let preferences = preferences(DEFAULT_FONT_SIZE as f64, false, 16);
        let mut previous = panel_for(&preferences, 0);
        for keys in 1..=16 {
            let panel = panel_for(&preferences, keys);
            assert!(
                panel.width >= previous.width,
                "going from {} to {keys} keys shrank the panel from {previous:?} to {panel:?}",
                keys - 1
            );
            previous = panel;
        }
    }

    #[test]
    fn a_panel_with_no_keys_is_still_a_panel() {
        // The idle line is drawn in the same panel as the keycaps, so the panel has to exist
        // before any key is held. This is the case that would divide by zero.
        let preferences = preferences(DEFAULT_FONT_SIZE as f64, false, 8);
        let panel = panel_for(&preferences, 0);
        assert!(
            panel.columns >= 1,
            "and it has a column to draw the idle line in"
        );
        assert!(panel.width >= MINIMUM_PANEL_WIDTH);
        assert!(panel.height > 0);
    }

    #[test]
    fn every_row_holds_at_least_one_key() {
        let preferences = preferences(MAXIMUM_FONT_SIZE as f64, true, 16);
        let panel = panel_for(&preferences, 16);
        let cap = Keycap {
            font_size: MAXIMUM_FONT_SIZE as f32,
            bold: true,
        };
        let fits = ((panel.width as f32 - PANEL_PADDING * 2.0 + KEY_GAP) / (cap.width() + KEY_GAP))
            .floor() as usize;
        assert_eq!(
            fits,
            panel.columns,
            "because a column count the panel cannot actually hold is a row of keycaps drawn \
             off the edge: {panel:?} with a {}px cap",
            cap.width()
        );
    }
}
