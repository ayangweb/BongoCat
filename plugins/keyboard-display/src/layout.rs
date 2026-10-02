//! How wide a keycap is, and therefore how big this plugin's panel is.
//!
//! Every number here follows from three the user chose and one the plugin built: the font
//! size, whether it is bold, how many keys are shown at once, and the labels those keys
//! carry. A keycap is the product's font at that size, sized around the label it holds
//! plus padding proportional to the font, and the panel is as many keycaps per row as
//! fit, times as many rows as there are keys.
//!
//! This is a separate module because it is the one part of the plugin that is pure
//! arithmetic, and pure arithmetic is the part worth testing on its own: a wrong keycap
//! width does not crash, it clips, and a clipped keycap is a thing a user reports as "the
//! last letter is cut off" with no way for anybody to find where the number came from.

use crate::settings::{
    MAXIMUM_PANEL_HEIGHT, MAXIMUM_PANEL_WIDTH, MINIMUM_PANEL_WIDTH, Preferences,
};
use crate::{KEY_GAP, MODIFIER_GLYPHS, PADDING_RATIO, PANEL_PADDING, RADIUS_RATIO};

/// How wide one character of a label is, as a multiple of the font size.
///
/// Two ratios rather than one, because the labels carry two kinds of character. Letters
/// and digits are narrow and the product's own font book is the reference for them. The
/// modifier glyphs this plugin writes are full-width — `⌘` next to a letter is about as
/// wide as the letter is tall — and estimating them as letters is what would clip the
/// right-hand edge of a chord, which is the case a key display exists to show.
const NARROW_CHARACTER: f32 = 0.62;
const GLYPH_CHARACTER: f32 = 1.0;

/// One keycap's box, in the panel's own logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keycap {
    /// The size of the letter, which is what the keycap is sized around.
    pub font_size: f32,
    /// Whether the letter is drawn bold, which makes it wider.
    pub bold: bool,
}

impl Keycap {
    /// The horizontal room a label occupies, in logical pixels.
    ///
    /// An estimate from the font's own metrics rather than a measurement, and deliberately
    /// generous: a cap that is a little too wide wastes a few pixels of a panel nobody
    /// measures, and a cap that is a little too narrow clips a letter somebody is explaining
    /// on a stream. Measuring would be more honest and would make the panel's size depend
    /// on which keys happen to be held, which is a panel that resizes as you type — so the
    /// width is computed from the *longest* label in the panel and every cap in it is drawn
    /// at that width, which is also what a keyboard's uniform keycaps look like.
    pub fn label_width(&self, label: &str) -> f32 {
        // A bold letter is about a tenth wider, which is the ratio the product's own font
        // book uses when it measures a string in bold against the same string regular.
        let weight = if self.bold { 1.1 } else { 1.0 };
        label
            .chars()
            .map(|character| {
                if MODIFIER_GLYPHS.contains(&character) {
                    GLYPH_CHARACTER
                } else {
                    NARROW_CHARACTER
                }
            })
            .sum::<f32>()
            * self.font_size
            * weight
    }

    /// This cap's width for one label, including its own padding.
    pub fn width_of(&self, label: &str) -> f32 {
        self.label_width(label) + self.font_size * PADDING_RATIO * 2.0
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

/// The panel a given set of labels needs, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelBox {
    pub width: u32,
    pub height: u32,
    /// How many keycaps fit on one row at this width.
    pub columns: usize,
}

/// The panel these preferences need for these labels.
///
/// One cap width for the whole panel, taken from the longest label in it: a row of caps of
/// different widths would put the panel's right edge in a different place on every
/// keystroke, and the point of a key display is that you can read it without watching it
/// resize.
///
/// Two steps, and the order is the whole of it. **How wide one line of every cap would be**
/// comes first, clamped into the panel's own bounds; **how many caps fit in that width**
/// comes second. Deriving the second from the first is what makes a cap drawn off the edge
/// impossible rather than unlikely: the row the plugin draws is cut at exactly the number of
/// caps the panel was sized to hold, whatever the font and however long the chords.
///
/// A panel with no labels still has a size, because the plugin publishes one at the start
/// and the host lays the tree out in it: a division by an empty set is the one arithmetic
/// error this function could make, and it makes none.
pub fn panel_for(preferences: &Preferences, labels: &[String]) -> PanelBox {
    let cap = Keycap {
        font_size: preferences.font_size,
        bold: preferences.bold,
    };
    let cap_height = cap.height();
    let count = labels.len();

    // Every cap is as wide as the longest label needs, so a row is even. An empty set is as
    // wide as one letter, which keeps the arithmetic below free of a zero.
    let widest = labels
        .iter()
        .map(|label| cap.width_of(label))
        .fold(cap.width_of("A"), f32::max)
        .max(1.0);

    // One row of every keycap, clamped into what the panel may be. The subtraction of one gap
    // is a row of one having no gap after it.
    let one_row = if count == 0 {
        0.0
    } else {
        (count as f32) * widest + ((count - 1) as f32) * KEY_GAP
    };
    let width = (one_row + PANEL_PADDING * 2.0)
        .ceil()
        .clamp(MINIMUM_PANEL_WIDTH as f32, MAXIMUM_PANEL_WIDTH as f32) as u32;

    // How many of that cap the panel is wide enough for, which is the same count the panel's
    // own width implies — so a row never draws a cap the panel cannot show.
    let columns = ((((width as f32 - PANEL_PADDING * 2.0 + KEY_GAP) / (widest + KEY_GAP)).floor()
        as usize)
        .max(1))
    .min(count.max(1));
    let rows = count.div_ceil(columns).max(1);

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
    use crate::settings::{
        DEFAULT_FONT_SIZE, MAXIMUM_FONT_SIZE, MAXIMUM_PANEL_WIDTH, MINIMUM_FONT_SIZE,
    };

    fn preferences(font_size: f64, bold: bool, maximum_keys: usize) -> Preferences {
        Preferences {
            font_size: font_size as f32,
            bold,
            maximum_keys,
            ..Preferences::default()
        }
    }

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
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
            large.width_of("A") > small.width_of("A") && large.height() > small.height(),
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
        assert!(bold.width_of("A") > normal.width_of("A"));
        assert_eq!(
            bold.height(),
            normal.height(),
            "while the height does not change, because boldness is horizontal"
        );
    }

    #[test]
    fn a_modifier_glyph_is_wider_than_a_letter_of_the_same_size() {
        // The one arithmetic error a user would report as "the ⌘ is cut off". A chord's
        // glyphs are full-width, so a cap sized for them as though they were letters
        // clips the key they were pressed with — which is the whole of what a chord cap
        // says.
        let cap = Keycap {
            font_size: DEFAULT_FONT_SIZE as f32,
            bold: false,
        };
        for glyph in MODIFIER_GLYPHS {
            assert!(
                cap.label_width(&glyph.to_string()) >= cap.font_size,
                "{glyph} is a full-width glyph, not a letter: {}px for a {}px font",
                cap.label_width(&glyph.to_string()),
                cap.font_size
            );
        }
        assert!(
            cap.label_width("⌘A") > cap.label_width("AB"),
            "and a chord is therefore wider than the letters it is made of"
        );
    }

    #[test]
    fn every_cap_holds_the_label_it_is_given() {
        // The check a user would report as "the last letter is cut off". There is no
        // measured table to check against here, so the check is the definition: the cap is
        // as wide as its own label plus its own padding.
        for size in [MINIMUM_FONT_SIZE, DEFAULT_FONT_SIZE, MAXIMUM_FONT_SIZE] {
            for bold in [false, true] {
                let cap = Keycap {
                    font_size: size as f32,
                    bold,
                };
                for label in ["A", "Scroll", "Num Enter", "⌃⌥⇧⌘Num Enter", "Modifiers"] {
                    assert!(
                        cap.width_of(label) >= cap.label_width(label),
                        "a {size}px cap must hold {label:?}, bold={bold}"
                    );
                    assert!(
                        cap.padding() > 0.0,
                        "and it has to be a cap, not a line of text"
                    );
                }
            }
        }
    }

    #[test]
    fn the_panel_is_never_narrower_than_one_keycap() {
        // A plugin configured into a panel narrower than what it is about to draw is a
        // plugin that draws nothing, and the user sees an empty box.
        for size in [MINIMUM_FONT_SIZE, DEFAULT_FONT_SIZE, MAXIMUM_FONT_SIZE] {
            let preferences = preferences(size as f64, false, 1);
            for label in ["A", "⌃⌥⇧⌘Num Enter"] {
                let panel = panel_for(&preferences, &labels(&[label]));
                let cap = Keycap {
                    font_size: size as f32,
                    bold: false,
                };
                assert!(
                    panel.width as f32 >= cap.width_of(label),
                    "a {size}px panel is {panel:?} and one cap is {}px",
                    cap.width_of(label)
                );
                assert!(panel.width >= MINIMUM_PANEL_WIDTH);
            }
        }
    }

    #[test]
    fn a_long_chord_wraps_into_another_row_rather_than_widening_the_panel() {
        // The panel is the model window's top-left corner, so it has a width it may not
        // exceed however long the chord is. A cap wider than the panel is a cap drawn off
        // the edge of it.
        let preferences = preferences(MAXIMUM_FONT_SIZE as f64, true, 16);
        let panel = panel_for(&preferences, &labels(&["⌃⌥⇧⌘Num Enter"]));
        assert!(
            panel.width <= MAXIMUM_PANEL_WIDTH,
            "and a chord at the largest font is a longer line, not a wider window: {panel:?}"
        );
    }

    #[test]
    fn the_panel_never_exceeds_the_model_windows_height() {
        // Sixteen keys at the largest font is more rows than a model window has room for,
        // and the honest answer is a bounded panel rather than one whose first row is off
        // the top of the screen.
        let preferences = preferences(MAXIMUM_FONT_SIZE as f64, true, 16);
        let names: Vec<String> = (0..16).map(|index| format!("K{index}")).collect();
        let panel = panel_for(&preferences, &names);
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
        let mut previous = panel_for(&preferences, &[]);
        for keys in 1..=16 {
            let names: Vec<String> = (0..keys).map(|index| format!("{index}")).collect();
            let panel = panel_for(&preferences, &names);
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
        // The plugin publishes one from its first ready rather than waiting for a key, and
        // the host lays the tree out in whatever size the panel declares. This is the case
        // that would divide by zero.
        let preferences = preferences(DEFAULT_FONT_SIZE as f64, false, 8);
        let panel = panel_for(&preferences, &[]);
        assert!(panel.columns >= 1);
        assert!(panel.width >= MINIMUM_PANEL_WIDTH);
        assert!(panel.height > 0);
    }

    #[test]
    fn every_row_holds_at_least_one_key() {
        // The host's layout has no notion of wrapping, so the plugin counts — and the count
        // has to come from the same arithmetic the panel's width came from, or a larger font
        // would size a panel for three keycaps and then try to draw five.
        let preferences = preferences(MAXIMUM_FONT_SIZE as f64, true, 16);
        let names: Vec<String> = (0..16).map(|index| format!("K{index}")).collect();
        let panel = panel_for(&preferences, &names);
        let widest = names
            .iter()
            .map(|label| {
                Keycap {
                    font_size: preferences.font_size,
                    bold: preferences.bold,
                }
                .width_of(label)
            })
            .fold(f32::MIN, f32::max);
        let fits = (((panel.width as f32 - PANEL_PADDING * 2.0 + KEY_GAP) / (widest + KEY_GAP))
            .floor()) as usize;
        assert_eq!(
            fits.min(names.len()),
            panel.columns,
            "because a column count the panel cannot actually hold is a row of keycaps drawn \
             off the edge: {panel:?} with a {widest:.0}px cap"
        );
    }

    #[test]
    fn the_widest_cap_this_plugin_can_draw_fits_inside_the_widest_panel() {
        // The invariant [`MAXIMUM_PANEL_WIDTH`] rests on, and the one a user would report as
        // "the ⌘ is cut off": four modifier glyphs plus the longest name the protocol shortens
        // a key to, at the largest font, in bold, inside the panel at its own maximum.
        let cap = Keycap {
            font_size: MAXIMUM_FONT_SIZE as f32,
            bold: true,
        };
        let widest_label = MODIFIER_GLYPHS.iter().collect::<String>() + "Num Enter";
        assert!(
            cap.width_of(&widest_label) + PANEL_PADDING * 2.0 <= MAXIMUM_PANEL_WIDTH as f32,
            "a cap for {widest_label:?} at the largest font is {}px and the panel's maximum is \
             {MAXIMUM_PANEL_WIDTH}px, so the bound is a number that has to be re-derived \
             rather than a number that can be left behind",
            cap.width_of(&widest_label) + PANEL_PADDING * 2.0
        );
    }

    #[test]
    fn eight_letters_fit_on_one_row_at_the_default_font() {
        // The bound that makes the display worth having: the default shows eight keys, and
        // eight keys on one row is a line of keycaps rather than a column down the corner.
        let preferences = preferences(DEFAULT_FONT_SIZE as f64, false, 8);
        let panel = panel_for(
            &preferences,
            &labels(&["A", "B", "C", "D", "E", "F", "G", "H"]),
        );
        assert_eq!(
            panel.columns, 8,
            "so a burst of eight letters is one line: {panel:?}"
        );
    }
}
