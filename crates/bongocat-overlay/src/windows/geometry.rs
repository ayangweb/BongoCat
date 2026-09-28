//! Where the overlay sits on the desktop.
//!
//! The overlay follows the cursor on first run and keeps a saved box
//! afterwards, so both positions have to survive a monitor being unplugged: a
//! box that no longer lands on a screen is corrected to the nearest one rather
//! than restored off the edge of the desktop. All sizing is logical, converted
//! here against the window's own DPI, so a drag means the same number of points
//! on every display scaling.

use super::*;

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

pub(crate) fn logical_to_physical(logical: u32, dpi: u32) -> WindowsResult<u32> {
    let physical = (u64::from(logical) * u64::from(dpi) + 48) / 96;
    if physical == 0 || physical > i32::MAX as u64 {
        return Err(invariant_error("overlay dimension exceeds Win32 limits"));
    }
    Ok(physical as u32)
}

/// The physical window size that `100%` maps to for one model canvas.
///
/// The drag state machine works in physical pixels because that is the unit
/// `SetWindowPos` takes, while the `100%` size is defined in logical pixels by
/// the DPI-independent overlay contract.
pub(crate) fn resize_base_for_dpi(
    base_width: u32,
    base_height: u32,
    dpi: u32,
) -> Option<ResizeBase> {
    let width = logical_to_physical(base_width, dpi).ok()?;
    let height = logical_to_physical(base_height, dpi).ok()?;
    ResizeBase::new(f64::from(width), f64::from(height))
}
