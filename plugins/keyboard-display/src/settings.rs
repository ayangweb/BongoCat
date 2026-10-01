//! What the user configured, and the settings that change it.
//!
//! Split from `main` because the settings and the drawing are two different kinds of
//! code: one is a declaration the settings window renders, and the other is a panel this
//! plugin rebuilds whenever a key changes. They agree because both read the same
//! [`Preferences`], and a setting that did not reach the drawing would be a control in the
//! settings window that does nothing.
//!
//! Every number here is this plugin's. The panel is this plugin's, the keycap is this
//! plugin's, and the font size is a thing the user sets — so nothing in this file is
//! something the host would have to know what a keycap is in order to decide.

use bongocat_plugin_sdk::prelude::*;

use crate::copy;

/// The keys shown when the user has not chosen.
///
/// Eight, because that is two tidy rows on a panel of this size and because a display that
/// has to scroll to show what your hands are doing is a display you cannot read while
/// typing.
pub const DEFAULT_MAXIMUM_KEYS: i64 = 8;

/// The most keys this plugin will show.
///
/// A bound rather than a preference: a hundred keycaps is a hundred labels, the panel
/// would be taller than the model window, and the host would be laying out a scene with a
/// thousand nodes at a hundred and twenty times a second. Sixteen is past what a person's
/// hands cover and short of what a window cannot show.
pub const MAXIMUM_KEYS: i64 = 16;

/// The size a keycap's letter is drawn at when the user has not chosen.
///
/// The product's own body size, which is what the rest of the model window's text uses — so
/// a key display at its default matches the window it sits in rather than carrying a font of
/// its own.
pub const DEFAULT_FONT_SIZE: i64 = 13;

/// The smallest a keycap's letter may be drawn.
///
/// Past this a keycap cannot hold two characters legibly at the product's own raster
/// scale, so a smaller setting would produce keycaps with letters cut off rather than
/// smaller keycaps.
pub const MINIMUM_FONT_SIZE: i64 = 9;

/// The largest a keycap's letter may be drawn.
///
/// Bounded rather than for taste: this plugin grows its panel with the font, and a keycap
/// wider than the model window is a panel the model window cannot show.
pub const MAXIMUM_FONT_SIZE: i64 = 32;

/// The smallest width this plugin's panel may take.
///
/// A floor rather than a fixed width, because the panel's width follows the font the user
/// chose — and a plugin that could be configured into a panel narrower than one keycap
/// would be a plugin that draws nothing at all.
pub const MINIMUM_PANEL_WIDTH: u32 = 180;

/// The tallest this plugin's panel may take.
///
/// The same reason as the width: the panel grows with the font, and the model window has a
/// height.
pub const MAXIMUM_PANEL_HEIGHT: u32 = 460;

/// The value the font-weight setting stores for a normal keycap.
///
/// A stable key, never localized, because it is what the plugin reads out of its own file
/// and a plugin that reworded it would read a document an older version wrote as "not bold".
/// The label the user sees is [`copy::regular`], which is a different thing entirely.
pub const REGULAR: &str = "regular";

/// The value the font-weight setting stores for a bold keycap.
pub const BOLD: &str = "bold";

/// What the user configured.
///
/// Read through the SDK's typed accessors, so a field nobody has touched reads as its own
/// default and there is no `unwrap_or` written twice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preferences {
    /// How many keycaps are shown at once.
    pub maximum_keys: usize,
    /// The size a keycap's letter is drawn at.
    pub font_size: f32,
    /// Whether the letter is drawn bold.
    pub bold: bool,
    /// Whether the mouse buttons are shown alongside the keys.
    pub include_mouse: bool,
    /// Whether the panel takes itself down when nothing is held.
    pub hide_when_idle: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            maximum_keys: DEFAULT_MAXIMUM_KEYS as usize,
            font_size: DEFAULT_FONT_SIZE as f32,
            bold: false,
            include_mouse: true,
            hide_when_idle: false,
        }
    }
}

impl Preferences {
    /// The user's settings, as this build understands them.
    ///
    /// Clamped as well as declared, for the reason the pomodoro gives: the host fits every
    /// document to the schema before it arrives, so this clamp is unreachable through the
    /// product — and it is here so that a `Values` built any other way still cannot produce
    /// a zero-sized keycap, which is a panel that draws nothing.
    pub fn read(values: &Values) -> Self {
        Self {
            maximum_keys: values.integer("maximum_keys").clamp(1, MAXIMUM_KEYS) as usize,
            // Read as a decimal rather than an integer because a font size is a number of
            // pixels and half a pixel is a real thing to ask for. A setting field here
            // rather than a whole number would be this plugin's own inconsistency: the
            // user sets 13 and gets 13, not 13.5 because the field rounded it.
            font_size: values
                .decimal("font_size")
                .clamp(MINIMUM_FONT_SIZE as f64, MAXIMUM_FONT_SIZE as f64)
                as f32,
            // Anything that is not the bold key is a normal one, which is the same reading
            // the pomodoro gives a choice value from a newer build: the value is not this
            // build's to interpret, and the ordinary answer is the one it was most likely
            // written as.
            bold: values.text("font_weight") == BOLD,
            include_mouse: values.flag("include_mouse"),
            hide_when_idle: values.flag("hide_when_idle"),
        }
    }
}

/// The settings this plugin declares, which *are* the settings panel.
///
/// Five rows, and the order is the order a person would set this plugin up in: how many
/// keys, then how big, then how heavy, then what else to show, then whether to be on screen
/// at all when there is nothing to say.
pub fn declared_settings() -> Settings {
    Settings::new()
        .with(
            Integer::ranged(
                "maximum_keys",
                copy::maximum_keys_label(),
                DEFAULT_MAXIMUM_KEYS,
                1,
                MAXIMUM_KEYS,
            )
            .described(copy::maximum_keys_help())
            .into(),
        )
        .with(
            Decimal::ranged(
                "font_size",
                copy::font_size_label(),
                DEFAULT_FONT_SIZE as f64,
                MINIMUM_FONT_SIZE as f64,
                MAXIMUM_FONT_SIZE as f64,
            )
            .stepping(1.0)
            .with_unit("px")
            .described(copy::font_size_help())
            .into(),
        )
        .with(
            Choice::new(
                "font_weight",
                copy::font_weight_label(),
                vec![
                    Option_::new(REGULAR, copy::regular()),
                    Option_::new(BOLD, copy::bold()),
                ],
            )
            .described(copy::font_weight_help())
            .into(),
        )
        .with(
            Toggle::new("include_mouse", copy::mouse_label())
                .described(copy::mouse_help())
                .into(),
        )
        .with(
            Toggle::new("hide_when_idle", copy::hide_when_idle_label())
                .described(copy::hide_when_idle_help())
                .into(),
        )
}
