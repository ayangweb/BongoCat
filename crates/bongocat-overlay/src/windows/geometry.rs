//! Where the overlay sits on the desktop.
//!
//! The overlay follows the cursor on first run and keeps a saved box
//! afterwards, so both positions have to survive a monitor being unplugged: a
//! box that no longer lands on a screen is corrected to the nearest one rather
//! than restored off the edge of the desktop. All sizing is logical, converted
//! here against the window's own DPI, so a drag means the same number of points
//! on every display scaling.

use super::*;
use crate::resize_drag::{MAXIMUM_RESIZE_DRAG_SCALE_PERCENT, MINIMUM_RESIZE_DRAG_SCALE_PERCENT};

pub(crate) fn current_cursor_position() -> POINT {
    let mut point = POINT { x: 80, y: 80 };
    // SAFETY: GetCursorPos writes only to the initialized stack value.
    let _ = unsafe { GetCursorPos(&mut point) };
    point
}

pub(crate) fn centered_position(cursor: POINT, width: u32, height: u32) -> (i32, i32) {
    // SAFETY: the monitor handle is used only for the immediate bounds query,
    // whose output points to initialized stack storage.
    unsafe {
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT::default(),
            rcWork: RECT::default(),
            dwFlags: 0,
        };
        if monitor.is_invalid() || !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return (80, 80);
        }
        // The full monitor rectangle, not `rcWork`: the overlay is allowed over
        // the taskbar, so only the display itself bounds it.
        let area = info.rcMonitor;
        (
            area.left + (area.right - area.left - width as i32) / 2,
            area.top + (area.bottom - area.top - height as i32) / 2,
        )
    }
}

/// Every display's full rectangle, including the strip the taskbar occupies.
///
/// A failed enumeration yields an empty list rather than a guess; the placement
/// constraint then leaves the window where it is.
pub(crate) fn screen_bounds_all() -> Vec<OverlayScreenBounds> {
    let mut screens = Vec::new();
    let data = LPARAM(std::ptr::from_mut(&mut screens) as isize);
    // SAFETY: the callback receives `data`, which borrows `screens` for the
    // duration of this synchronous enumeration, and the enumeration call ends
    // before that borrow does.
    let enumerated = unsafe { EnumDisplayMonitors(None, None, Some(collect_monitor), data) };
    if enumerated.as_bool() {
        screens
    } else {
        Vec::new()
    }
}

pub(crate) unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _device: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: EnumDisplayMonitors hands back the pointer `screen_bounds_all`
    // supplied, which stays valid for the whole enumeration and is only touched
    // from the calling thread.
    let screens = unsafe { &mut *(data.0 as *mut Vec<OverlayScreenBounds>) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT::default(),
        rcWork: RECT::default(),
        dwFlags: 0,
    };
    // SAFETY: `monitor` was supplied by the enumeration and `info` is
    // initialized stack storage with the size the call requires. A monitor that
    // cannot be queried is skipped instead of failing the whole enumeration.
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return true.into();
    }
    if let (Ok(width), Ok(height)) = (
        u32::try_from(info.rcMonitor.right - info.rcMonitor.left),
        u32::try_from(info.rcMonitor.bottom - info.rcMonitor.top),
    ) && width > 0
        && height > 0
    {
        screens.push(OverlayScreenBounds {
            x: info.rcMonitor.left,
            y: info.rcMonitor.top,
            width,
            height,
        });
    }
    true.into()
}

pub(crate) fn overlay_bounds_visible(bounds: OverlayWindowBounds) -> bool {
    let rect = RECT {
        left: bounds.x,
        top: bounds.y,
        right: bounds.x.saturating_add_unsigned(bounds.width),
        bottom: bounds.y.saturating_add_unsigned(bounds.height),
    };
    // SAFETY: MonitorFromRect only reads the initialized rectangle.
    !unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) }.is_invalid()
}

/// One client-size policy for every Windows sizing entry point. Keep the canvas
/// ratio unrounded; physical pixels are rounded only after choosing the width.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowSizing {
    canvas_width: f64,
    canvas_height: f64,
}

impl WindowSizing {
    pub(crate) fn new(canvas: CanvasInfo) -> Option<Self> {
        (canvas.width.is_finite()
            && canvas.width > 0.0
            && canvas.height.is_finite()
            && canvas.height > 0.0)
            .then_some(Self {
                canvas_width: f64::from(canvas.width),
                canvas_height: f64::from(canvas.height),
            })
    }

    pub(crate) fn resize_base(self, dpi: u32) -> Option<ResizeBase> {
        let width = f64::from(crate::DEFAULT_OVERLAY_WINDOW_WIDTH) * f64::from(dpi) / 96.0;
        ResizeBase::new(width, width * self.canvas_height / self.canvas_width)
    }

    pub(crate) fn dimensions_for_scale(self, dpi: u32, scale_percent: u16) -> (u32, u32) {
        let width = f64::from(crate::DEFAULT_OVERLAY_WINDOW_WIDTH)
            * f64::from(dpi)
            * f64::from(scale_percent)
            / 9600.0;
        self.dimensions_for_width(width)
    }

    pub(crate) fn scale_percent_for_width(self, dpi: u32, width: u32) -> Option<u16> {
        let base = self.resize_base(dpi)?;
        // Several low percentages can share the legal minimum size. Keep the
        // native gesture at the same 25% endpoint as the numeric control.
        Some(
            if width
                <= self
                    .dimensions_for_scale(dpi, MINIMUM_RESIZE_DRAG_SCALE_PERCENT)
                    .0
            {
                MINIMUM_RESIZE_DRAG_SCALE_PERCENT
            } else if width
                >= self
                    .dimensions_for_scale(dpi, MAXIMUM_RESIZE_DRAG_SCALE_PERCENT)
                    .0
            {
                MAXIMUM_RESIZE_DRAG_SCALE_PERCENT
            } else {
                base.scale_percent_for_width(width)
            },
        )
    }

    pub(crate) fn width_for_height(self, height: u32) -> f64 {
        f64::from(height) * self.canvas_width / self.canvas_height
    }

    pub(crate) fn dimensions_for_width(self, width: f64) -> (u32, u32) {
        let minimum = f64::from(crate::MIN_OVERLAY_WINDOW_DIMENSION);
        let maximum = f64::from(crate::MAX_OVERLAY_WINDOW_DIMENSION);
        let minimum_width = minimum
            .max(minimum * self.canvas_width / self.canvas_height)
            .ceil();
        let maximum_width = maximum
            .min(maximum * self.canvas_width / self.canvas_height)
            .floor();
        // Clamp the uniform size, not each axis independently. For canvases
        // whose extreme ratio cannot fit legal bounds, keep the existing limits.
        let width = if minimum_width <= maximum_width {
            width.clamp(minimum_width, maximum_width)
        } else {
            width
        };
        let width = crate::cover_window_dimension(width);
        let height = crate::cover_window_dimension(
            f64::from(width) * self.canvas_height / self.canvas_width,
        );
        (width, height)
    }

    pub(crate) fn normalize(self, bounds: OverlayWindowBounds) -> OverlayWindowBounds {
        let (width, height) = self.dimensions_for_width(f64::from(bounds.width));
        OverlayWindowBounds {
            width,
            height,
            ..bounds
        }
    }
}
