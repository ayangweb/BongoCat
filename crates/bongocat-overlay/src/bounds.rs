//! Where a window may sit, and what happens when it moves.
//!
//! A persisted box is honoured only if it is still on a screen the user has and
//! still the size the model wants; otherwise the overlay would come back
//! off-screen or at the wrong aspect after a monitor is unplugged. Clamping moves
//! a window without resizing it, because resizing to fit would change the aspect
//! the dimensions module chose.

use super::*;

/// Recompute only the height when a model switch changes the canvas.
///
/// The live width is deliberately the scale source: it is the width the user
/// currently sees, so it already includes the configured scale and any
/// right-button drag (and it can be a persisted/manual geometry). The shared
/// cover rounding keeps the model fully inside the native window and makes a
/// switch back to a model reproduce its startup dimensions.
pub(crate) fn model_switch_window_bounds(
    current: OverlayWindowBounds,
    canvas: CanvasInfo,
) -> OverlayWindowBounds {
    OverlayWindowBounds {
        height: model_window_height_for_width(canvas, current.width),
        ..current
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlaySessionOptions {
    pub click_through: bool,
    /// The physical modifier key whose hold gives the pointer back to the user.
    ///
    /// Read every frame against the runtime's pressed modifiers, never cached:
    /// the whole point is that the hold ends the moment the key comes up, and a
    /// cached answer would leave the overlay interactive after the user let go.
    /// `None` means no key does this.
    pub hold_modifier_to_interact: Option<ModifierKey>,
    pub always_on_top: bool,
    pub scale_percent: u16,
    pub opacity_percent: u8,
    /// Corner radius of the overlay window box as a percentage of its width and
    /// height. `0` keeps square corners; `50` clips the window content to the
    /// full inscribed ellipse. Larger values are rejected by configuration and
    /// clamped by the renderer, matching the legacy `border-radius` ceiling.
    pub corner_radius_percent: u8,
    /// Hide the overlay content while the pointer rests on the overlay window,
    /// mirroring the legacy `window.hideOnHover` switch. Unlike the other
    /// presentation options this is applied inside the frame tick, because the
    /// window must hide and restore while it keeps running rather than being
    /// replaced on every hover.
    pub hide_on_pointer_hover: bool,
    /// How long the pointer must stay inside the overlay window before the
    /// hover hide starts, in milliseconds. `0` hides immediately. The current
    /// v1 configuration stores whole seconds; the millisecond value is derived
    /// once, when the runtime settings are applied to the session.
    pub hide_on_pointer_hover_delay_ms: u32,
    /// Hide the overlay content after no input arrives for a stretch of
    /// time. Like the hover hide it is applied inside the frame tick, because
    /// the window must fade out and back in while it keeps running.
    pub hide_on_idle: bool,
    /// How long no input may arrive before the idle hide starts, in
    /// milliseconds. `0` hides as soon as input stops. The current v1
    /// configuration stores whole seconds; the millisecond value is derived
    /// once, when the runtime settings are applied to the session, and the
    /// bound is the runtime's shared ceiling rather than a literal here.
    pub hide_on_idle_delay_ms: u32,
    /// Keep the overlay window fully on a display. The region is the union of
    /// the connected displays' frames rather than one display's work area, so a
    /// window may sit over a taskbar, Dock or menu bar. The correction itself is
    /// delayed; see the placement module.
    pub keep_inside_screen: bool,
    pub maximum_fps: u16,
    pub window_bounds: Option<OverlayWindowBounds>,
    /// Windows only: whether the model window owns a taskbar button.
    ///
    /// The model window is a caption-less `WS_POPUP`, so this is the only part
    /// of its frame the product exposes to the shell; the settings window keeps
    /// the taskbar button GPUI gives it whatever this says, because hiding that
    /// one also replaces its caption with a tool window's short caption.
    #[cfg(target_os = "windows")]
    pub taskbar_icon_visible: bool,
}

impl OverlaySessionOptions {
    pub const fn with_runtime_settings(self, settings: OverlaySettings) -> Self {
        Self {
            click_through: settings.click_through,
            hold_modifier_to_interact: settings.hold_modifier_to_interact,
            always_on_top: settings.always_on_top,
            scale_percent: settings.scale_percent,
            opacity_percent: settings.opacity_percent,
            corner_radius_percent: settings.corner_radius_percent,
            hide_on_pointer_hover: settings.hide_on_pointer_hover,
            hide_on_pointer_hover_delay_ms: hover_hide_delay_ms(
                settings.hide_on_pointer_hover_delay_seconds,
            ),
            hide_on_idle: settings.hide_on_idle,
            hide_on_idle_delay_ms: idle_hide_delay_ms(settings.hide_on_idle_delay_seconds),
            keep_inside_screen: settings.keep_inside_screen,
            maximum_fps: self.maximum_fps,
            window_bounds: self.window_bounds,
            // The taskbar button is a system preference rather than overlay
            // state, so it is not part of the runtime overlay settings. The
            // session keeps the applied value here and re-applies it to a
            // replacement window.
            #[cfg(target_os = "windows")]
            taskbar_icon_visible: self.taskbar_icon_visible,
        }
    }

    /// Whether the configured hold modifier is down, and the overlay should
    /// therefore behave as if pointer routing and the hover hide were off.
    ///
    /// Both platform sessions read this in the same place, so "holding the key
    /// gives the pointer back" is one rule rather than two copies of it. It is a
    /// question about the current frame, never a latch: letting go of the key
    /// restores the configured behaviour on the next frame.
    pub const fn hold_modifier_pressed(self, pressed_modifiers: PressedModifiers) -> bool {
        match self.hold_modifier_to_interact {
            Some(modifier) => pressed_modifiers.holds(modifier),
            None => false,
        }
    }
}

impl Default for OverlaySessionOptions {
    fn default() -> Self {
        Self {
            click_through: false,
            hold_modifier_to_interact: None,
            always_on_top: true,
            scale_percent: 100,
            opacity_percent: 100,
            corner_radius_percent: 0,
            hide_on_pointer_hover: false,
            hide_on_pointer_hover_delay_ms: 0,
            hide_on_idle: false,
            hide_on_idle_delay_ms: idle_hide_delay_ms(DEFAULT_HIDE_ON_IDLE_DELAY_SECONDS),
            keep_inside_screen: true,
            maximum_fps: 60,
            window_bounds: None,
            // The shipped v1 configuration leaves the model window's taskbar
            // button off by default, so the fallback options have to agree with
            // it rather than invent a second default.
            #[cfg(target_os = "windows")]
            taskbar_icon_visible: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayWindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl OverlayWindowBounds {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(crate) fn validate(self) -> Result<Self, OverlayError> {
        const MAX_COORDINATE: i32 = 1_000_000;
        const MIN_DIMENSION: u32 = 64;
        const MAX_DIMENSION: u32 = 16_384;
        if !(-MAX_COORDINATE..=MAX_COORDINATE).contains(&self.x)
            || !(-MAX_COORDINATE..=MAX_COORDINATE).contains(&self.y)
            || !(MIN_DIMENSION..=MAX_DIMENSION).contains(&self.width)
            || !(MIN_DIMENSION..=MAX_DIMENSION).contains(&self.height)
        {
            return Err(OverlayError::new("overlay window bounds are invalid"));
        }
        Ok(self)
    }

    pub(crate) fn rescale(self, previous_percent: u16, next_percent: u16) -> Self {
        let ratio = f64::from(next_percent) / f64::from(previous_percent);
        Self {
            width: cover_window_dimension(f64::from(self.width) * ratio),
            height: cover_window_dimension(f64::from(self.height) * ratio),
            ..self
        }
    }
}
