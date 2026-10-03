//! The overlay window's own placement and shape.
//!
//! The bounds are bounded here rather than at the point of use, because this is
//! the only place a value read out of a document is a number the product will
//! later multiply into pixels. A corner radius over 50 clips the window content
//! to less than the full ellipse, which is a shape rather than a rounding.

use bongocat_input::ModifierKey;

use super::*;

/// Overlay window configuration. Every field is a property of the single
/// product overlay window; the settings window and every other product window
/// keep their own platform chrome and are not affected.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OverlayConfig {
    pub click_through: bool,
    /// The physical modifier key whose hold hands the pointer back to the user
    /// for as long as it is down.
    ///
    /// Click-through and hover hide both exist to keep the overlay out of the
    /// way, and together they leave no way to move it: the pointer reaches
    /// through the window, and stopping on it hides the very thing the user is
    /// trying to grab. Holding this key suspends both — the window stops passing
    /// pointer events through and stops fading out — so the overlay can be
    /// dragged again without turning either setting off first. It suspends
    /// nothing else: the release ends it immediately, the settings themselves
    /// are untouched, and no pressed key is remembered across the hold.
    ///
    /// The value is the *physical* key, so the two sides of a modifier are two
    /// settings rather than one. A user who drags with the thumb on the right
    /// shift and never touches the left one would otherwise have to hold both.
    ///
    /// `None` is the shipped default and means no key does this.
    /// `#[serde(default)]` keeps a configuration written before the field
    /// existed on the "no modifier" side rather than failing the strict v1
    /// parse. A name outside the eight modifiers is refused by the same parse
    /// rather than resolved to a key the user did not choose.
    #[serde(default)]
    pub hold_modifier_to_interact: Option<ModifierKey>,
    pub always_on_top: bool,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 25, max = 400))
    )]
    pub scale_percent: u16,
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 1, max = 100))
    )]
    pub opacity_percent: u8,
    /// Maximum frame rate for the product overlay and its runtime scheduler.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 15, max = 240))
    )]
    pub maximum_fps: u16,
    /// Corner radius of the overlay window box, as a percentage of the window
    /// width and height. The value keeps the legacy `border-radius: N%`
    /// semantics: each corner arc is an ellipse with a horizontal semi-axis of
    /// `N%` of the window width and a vertical semi-axis of `N%` of the window
    /// height, so the same number rounds a wide window more horizontally than
    /// vertically. `0` leaves square corners. At `50` the four arcs meet and the
    /// window content is clipped to the full inscribed ellipse; the legacy
    /// implementation scaled every radius above that point back down to the same
    /// ellipse, so `50` is the effective upper bound of the legacy behavior.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0, max = 50))
    )]
    pub corner_radius_percent: u8,
    /// Hide the overlay while the pointer rests on it, keeping the model out of
    /// the way of whatever the pointer is reaching for underneath.
    ///
    /// This mirrors the legacy `window.hideOnHover` switch. The overlay stays a
    /// normal window and keeps presenting frames; only its rendered alpha drops
    /// to zero, and pointer events pass through until the pointer leaves the
    /// window box again.
    pub hide_on_pointer_hover: bool,
    /// How long the pointer must stay inside the overlay box before the hover
    /// hide starts, in whole seconds. `0` hides as soon as the pointer enters.
    ///
    /// The legacy input also took whole seconds with a lower bound of `0`, so
    /// this field keeps the unit the settings page edits and needs no
    /// conversion between the stored value and the visible one. The cap at `60`
    /// seconds is a first-version contract decision rather than a legacy
    /// ceiling (the legacy input had no upper bound): a hover delay longer than
    /// a minute is indistinguishable from leaving the feature off. The overlay
    /// frame loop still counts in milliseconds and converts once at its own
    /// boundary, because the hover state machine compares against the
    /// monotonic millisecond clock.
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0, max = 60))
    )]
    pub hide_on_pointer_hover_delay_seconds: u32,
    /// Hide the overlay content after the user stops touching the mouse,
    /// keyboard and gamepad for a while, so the cat is not in the way of a
    /// desktop being watched.
    ///
    /// This is the same temporary presentation state as
    /// `hide_on_pointer_hover`: the window and the frame loop keep running,
    /// only the rendered alpha drops to zero and pointer events pass through
    /// until the next input resets the idle timer. `#[serde(default)]` keeps
    /// a configuration written before the field existed on the "off" side
    /// rather than failing the strict v1 parse.
    #[serde(default)]
    pub hide_on_idle: bool,
    /// How long the input may stay untouched before the idle hide starts, in
    /// whole seconds. `0` hides as soon as no fresh input event arrives.
    ///
    /// The minutes-scale ceiling is deliberate: unlike the hover delay, a
    /// user legitimately idles for several minutes while watching their
    /// desktop, so the bound sits at `600` rather than the hover ceiling.
    /// The overlay compares against the millisecond clock, and the
    /// conversion lives in `bongocat_runtime::idle_hide_delay_ms`.
    #[serde(default = "default_hide_on_idle_delay_seconds")]
    #[cfg_attr(
        any(test, feature = "schema-generation"),
        schemars(range(min = 0, max = 600))
    )]
    pub hide_on_idle_delay_seconds: u32,
    /// Keep the overlay window fully on a display. The window stays on
    /// the union of the connected displays, so it may cover a taskbar, Dock or
    /// menu bar, and a window dragged off the desktop is moved back only after
    /// the drag has stopped. This replaces the earlier work-area constraint,
    /// which forbade the desktop chrome strip entirely.
    pub keep_inside_screen: bool,
}

/// Upper bound of the hover hide delay, in whole seconds.
///
/// See [`OverlayConfig::hide_on_pointer_hover_delay_seconds`] for why the legacy
/// implementation's unbounded second-valued input is capped here.
pub const MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS: u32 = 60;

/// Upper bound of the idle hide delay, in whole seconds.
///
/// Unlike the hover delay, an idle watch can legitimately last several
/// minutes, so the ceiling sits higher than
/// [`MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS`] while staying a documented
/// first-version bound rather than an unbounded input.
pub const MAXIMUM_HIDE_ON_IDLE_DELAY_SECONDS: u32 = 600;

/// Default idle hide delay, in whole seconds.
///
/// Ten seconds matches the first-version request: long enough that ordinary
/// reading a static desktop is not interrupted, short enough that the cat
/// leaves the screen quickly when nobody is using the machine.
pub const DEFAULT_HIDE_ON_IDLE_DELAY_SECONDS: u32 = 10;

fn default_hide_on_idle_delay_seconds() -> u32 {
    DEFAULT_HIDE_ON_IDLE_DELAY_SECONDS
}
