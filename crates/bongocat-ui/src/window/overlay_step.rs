//! Nudging a bounded overlay setting from its control.
//!
//! The bounds belong to the schema and the step to the control, so stepping
//! clamps rather than wrapping: a user holding a key should not find the overlay
//! jumped from smallest to largest.

use super::*;

#[cfg(test)]
pub(crate) fn stepped_overlay_scale(mut settings: SettingsOverlay, delta: i16) -> SettingsOverlay {
    let next = i32::from(settings.scale_percent) + i32::from(delta);
    settings.scale_percent = next.clamp(25, 400) as u16;
    settings
}

#[cfg(test)]
pub(crate) fn stepped_overlay_opacity(
    mut settings: SettingsOverlay,
    delta: i16,
) -> SettingsOverlay {
    let next = i16::from(settings.opacity_percent) + delta;
    settings.opacity_percent = next.clamp(1, 100) as u8;
    settings
}

/// Whether the hover hide delay applies to the model window right now.
///
/// The overlay only reads the delay while "hide on pointer hover" is on — both
/// platform backends arm the behaviour with
/// `options.hide_on_pointer_hover && input_running` — so the delay row and its two
/// steppers are inert the rest of the time. The recorded value is deliberately left
/// alone: turning the switch back on restores the delay the user chose instead of a
/// default, which is why nothing here resets it.
pub(crate) fn hover_hide_delay_applies(overlay: SettingsOverlay) -> bool {
    overlay.hide_on_pointer_hover
}
