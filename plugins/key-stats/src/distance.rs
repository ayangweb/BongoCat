//! Putting a physical size on a number the host measured in fractions of a window.
//!
//! The host's `MouseMove` carries a distance in the model window's own normalized units,
//! which is the right number to count: it is the same whatever the window's size or the
//! display's scale, so a counter in one model and a counter in another agree. It is also
//! not something a person can read, so this plugin has to turn it into one — and every
//! conversion needs an assumption about the physical world, so the assumptions are here,
//! named, and tested.
//!
//! **Screen widths are exact.** One unit is one window width by definition, and a user who
//! has never heard of centimetres can still read "4.2 screen widths" as a lot of mouse.
//!
//! **Centimetres are an assumption.** A window's width in physical units is its diagonal
//! divided by the aspect ratio's Pythagoras, and neither the diagonal nor the ratio is
//! something this plugin can know. So it assumes a 24-inch 16:9 screen and says so, in the
//! setting's own description, where a user who cares will read it.

use crate::copy;

/// How wide one window is on the assumed screen, in inches.
///
/// `diagonal / hypot(1, aspect)` — the Pythagoras of a 16:9 rectangle's diagonal and its
/// two sides. Worked out once, here, so the number the panel shows and the number the
/// tests check are the same number.
pub fn assumed_width_inches() -> f64 {
    copy::ASSUMED_DIAGONAL_INCHES / (1.0 + copy::ASSUMED_ASPECT.powi(2)).sqrt()
}

/// A distance in the host's units, as a distance a person can read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Units {
    /// Screen widths. Exact, because the unit *is* the window's width.
    ScreenWidths,
    /// Centimetres. An assumption, stated in the setting that chooses it.
    Centimetres,
}

impl Units {
    /// This unit, from the string the settings form stores.
    ///
    /// A value this build does not recognise is screen widths, which is the exact one: a
    /// hand-edited file asking for centimetres in a build without them should show a
    /// number that is right rather than a number that is in the wrong unit.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "cm" => Self::Centimetres,
            _ => Self::ScreenWidths,
        }
    }

    /// The string the settings form stores for this unit.
    pub const fn as_setting(self) -> &'static str {
        match self {
            Self::ScreenWidths => "screen_widths",
            Self::Centimetres => "cm",
        }
    }

    /// This distance, in this unit, as a number with at most one decimal place.
    ///
    /// One decimal because a distance is a rough thing: "41 cm" is a fact about a mouse and
    /// "41.37 cm" is a number with two more digits than the assumption behind it deserves.
    /// The rounding is what makes the number *change* at a readable rate, which is the
    /// other half of why it is bounded.
    pub fn show(self, host_units: f32) -> String {
        let units = f64::from(host_units);
        let value = match self {
            Self::ScreenWidths => units,
            Self::Centimetres => units * assumed_width_inches() * 2.54,
        };
        // Not finite becomes zero: a tally that read `NaN cm` because a mouse reported a
        // non-finite coordinate would be a panel full of nonsense, and zero is at least the
        // direction of the answer.
        let value = if value.is_finite() { value } else { 0.0 };
        let rounded = (value * 10.0).round() / 10.0;
        if rounded >= 100.0 {
            format!("{rounded:.0}")
        } else {
            format!("{rounded:.1}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_width_is_exactly_one_screen_width() {
        assert_eq!(Units::ScreenWidths.show(1.0), "1.0");
        assert_eq!(Units::ScreenWidths.show(0.0), "0.0");
        assert_eq!(
            Units::ScreenWidths.show(4.25),
            "4.3",
            "and rounds to a tenth"
        );
    }

    #[test]
    fn a_centimetre_answer_says_what_assumption_it_made() {
        // A 24-inch *diagonal* 16:9 screen is 24 inches across the corner and about 11.8
        // inches along the side, so one window width is about 30 cm and not the 61 a reader
        // might assume from "24 inches". If this number ever moves, it moves because the
        // assumption moved — and the assumption is written in the setting's own
        // description, where a user who cares will read it.
        let width_cm = assumed_width_inches() * 2.54;
        assert!(
            (29.0..=31.0).contains(&width_cm),
            "one window width is {width_cm:.1} cm on the assumed screen"
        );
        assert_eq!(Units::Centimetres.show(1.0), format!("{width_cm:.1}"));
    }

    #[test]
    fn the_two_units_agree_about_magnitude() {
        // Whatever the assumption, four screen widths must not read as four centimetres.
        let widths: f64 = Units::ScreenWidths.show(4.0).parse().expect("a number");
        let centimetres: f64 = Units::Centimetres.show(4.0).parse().expect("a number");
        assert!(centimetres > widths * 10.0, "{centimetres} vs {widths}");
    }

    #[test]
    fn a_distance_beyond_a_thousand_centimetres_is_not_given_a_decimal_place() {
        // A digit after the point on a number in the hundreds is noise, and it moves on
        // every mouse movement, which is the difference between a number and a flicker.
        // Three window widths is about 90 cm, so this is the pair of readings either side
        // of a hundred: one with a decimal place and one without.
        assert_eq!(
            Units::Centimetres.show(3.0),
            "89.7",
            "under a hundred keeps it"
        );
        let shown = Units::Centimetres.show(4.0);
        assert!(!shown.contains('.'), "and over a hundred drops it: {shown}");
    }

    #[test]
    fn a_distance_that_is_not_a_number_reads_as_none_rather_than_as_nonsense() {
        // A mouse that reports a non-finite coordinate is a fact about the platform, and
        // the panel showing `NaN cm` would be a panel showing a bug rather than a tally.
        for broken in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(Units::ScreenWidths.show(broken), "0.0");
            assert_eq!(Units::Centimetres.show(broken), "0.0");
        }
    }

    #[test]
    fn a_unit_this_build_does_not_know_reads_as_the_exact_one() {
        assert_eq!(Units::from_setting("cm"), Units::Centimetres);
        assert_eq!(Units::from_setting("screen_widths"), Units::ScreenWidths);
        assert_eq!(
            Units::from_setting("furlongs"),
            Units::ScreenWidths,
            "so a hand-edited file shows a right number in the wrong unit rather than a wrong \\
             number in the unit it asked for"
        );
        for unit in [Units::ScreenWidths, Units::Centimetres] {
            assert_eq!(Units::from_setting(unit.as_setting()), unit);
        }
    }
}
