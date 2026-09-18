//! Application appearance for the native surfaces the platform layer owns.
//!
//! The product lets the user pick Light, Dark or System, but the surfaces that have to
//! follow that choice are not the ones the product paints: the window frame, the alert a
//! permission request raises, the tray and context menus and the open/save panels all
//! belong to the operating system. This module is the single place that knows what each
//! platform can and cannot restyle, so the UI layer does not have to.
//!
//! What each platform supports (verified 2026-09-18, see ADR-0048):
//!
//! | surface | macOS | Windows |
//! | --- | --- | --- |
//! | window frame | application theme | application theme (DWM attribute) |
//! | alerts | application theme | system theme |
//! | tray and context menus | application theme | system theme |
//! | open/save panels | application theme | system theme |
//!
//! macOS reaches every row through one process-wide `NSApplication.appearance`: the
//! appearance is inherited by each window and, through the window, by the alerts, menus
//! and panels it presents. The application must set it at startup through
//! [`apply_process_theme`] before creating the overlay; applying it from a settings window
//! alone would make the overlay's first context menu depend on that window having existed.
//! Windows has a documented API for the frame only (`DWMWA_USE_IMMERSIVE_DARK_MODE`); the
//! other rows follow the *system* theme, which [`init_native_theme`] opts the process into.
//!
//! Linux is outside the first release (ADR-0006). Its compositor draws the frame and the
//! GTK portal supplies the panels, so there is nothing here for the product to set and
//! every entry point is a documented no-op.

use raw_window_handle::HasWindowHandle;
use std::fmt;

/// The appearance the product asks its native surfaces to use.
///
/// This is a *resolved* choice: the caller decides what `System` means and never passes
/// it down, so nothing below has to re-derive it and two windows cannot disagree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppTheme {
    Light,
    Dark,
}

impl AppTheme {
    pub const fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }
}

/// The appearance the operating system is currently using for applications.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemAppearance {
    Light,
    Dark,
}

/// A failure to restyle a native surface.
///
/// None of these are fatal: a surface that could not be themed keeps the system
/// appearance, which is the documented fallback for every platform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeThemeError {
    /// The call has to run on the platform's UI thread and did not.
    WrongThread,
    /// The window did not expose a handle the platform can use.
    WindowHandleUnavailable,
    /// The window's handle belongs to a platform this call does not implement.
    UnsupportedWindowHandle,
    /// The platform call itself failed, or the API is absent on this system.
    NativeCallFailed,
}

impl NativeThemeError {
    pub const ALL: [Self; 4] = [
        Self::WrongThread,
        Self::WindowHandleUnavailable,
        Self::UnsupportedWindowHandle,
        Self::NativeCallFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WrongThread => "native_theme_wrong_thread",
            Self::WindowHandleUnavailable => "native_theme_window_handle_unavailable",
            Self::UnsupportedWindowHandle => "native_theme_unsupported_window_handle",
            Self::NativeCallFailed => "native_theme_native_call_failed",
        }
    }
}

impl fmt::Display for NativeThemeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for NativeThemeError {}

/// Applies the product's appearance choice to the process-wide native surfaces.
///
/// `theme` is the resolved choice; `None` means the product follows the operating
/// system, and each platform then restores its own system-driven appearance. This is
/// the startup entry point: it must run before the first product window exists, so a
/// context menu from the overlay does not depend on the settings window having been
/// created first.
///
/// On macOS this sets `NSApplication.appearance`. Windows has no process-wide application
/// theme for the surfaces in scope, so its implementation is a documented no-op; the
/// system-owned surfaces are prepared by [`init_native_theme`].
pub fn apply_process_theme(theme: Option<AppTheme>) -> Result<(), NativeThemeError> {
    platform::apply_process(theme)
}

/// Applies the product's appearance choice to the native surfaces of this process.
///
/// `theme` is the resolved choice; `None` means the product follows the operating
/// system, and each platform then restores its own system-driven appearance.
///
/// `window` identifies the window whose frame the platform can restyle. Only Windows
/// uses it: its application theme reaches the DWM frame and nothing else. macOS ignores
/// it because one process-wide appearance already covers every window and every panel a
/// window presents, and a per-window override would leave menus and alerts behind.
pub fn apply_theme(
    window: &impl HasWindowHandle,
    theme: Option<AppTheme>,
) -> Result<(), NativeThemeError> {
    platform::apply(window, theme)
}

/// Prepares the native surfaces that can only follow the *system* theme.
///
/// Windows is the only platform that needs this. Alerts, menus and the file dialog are
/// drawn by ComCtl32 and the shell, which render dark only when the process has asked
/// for it before any window exists. That request is a process-wide, undocumented switch,
/// so it is made once, from `main`, and never undone.
///
/// Idempotent: repeated calls repeat nothing.
pub fn init_native_theme() -> Result<(), NativeThemeError> {
    platform::init()
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{AppTheme, NativeThemeError, SystemAppearance};
    use objc2::{MainThreadMarker, rc::Retained};
    use objc2_app_kit::{NSAppearance, NSApplication};
    use objc2_foundation::NSString;
    use raw_window_handle::HasWindowHandle;

    /// The appearance names AppKit is asked for, written out rather than read from the
    /// `NSAppearanceName*` statics: those are `extern` statics, so touching them needs an
    /// `unsafe` block, while `appearanceNamed:` and `-isEqualToString:` both compare by
    /// string content. The values are the documented contents of those constants.
    const AQUA: &str = "NSAppearanceNameAqua";
    const DARK_NAMES: [&str; 3] = [
        "NSAppearanceNameDarkAqua",
        "NSAppearanceNameVibrantDark",
        "NSAppearanceNameAccessibilityHighContrastDarkAqua",
    ];

    /// The system appearance is queried only while the product follows it, and every
    /// other surface in this module is reached through `NSApplication.appearance`, so
    /// the query lives here next to the override it depends on.
    pub fn system_appearance() -> SystemAppearance {
        // `window_content_top_inset` sets the precedent for a query that cannot run off
        // the main thread: report the neutral value instead of panicking. Light is that
        // value, and it is also what AppKit reports when no dark appearance is named.
        let Some(marker) = MainThreadMarker::new() else {
            return SystemAppearance::Light;
        };
        let name = NSApplication::sharedApplication(marker)
            .effectiveAppearance()
            .name();
        if DARK_NAMES
            .iter()
            .any(|dark| name.isEqualToString(&NSString::from_str(dark)))
        {
            SystemAppearance::Dark
        } else {
            SystemAppearance::Light
        }
    }

    fn named(name: &str) -> Result<Retained<NSAppearance>, NativeThemeError> {
        let name = NSString::from_str(name);
        // The named appearances are as old as the appearance API itself (10.14), so a
        // missing name means the system is older than the product supports, not a race.
        NSAppearance::appearanceNamed(&name).ok_or(NativeThemeError::NativeCallFailed)
    }

    pub(super) fn apply_process(theme: Option<AppTheme>) -> Result<(), NativeThemeError> {
        let marker = MainThreadMarker::new().ok_or(NativeThemeError::WrongThread)?;
        let appearance = match theme {
            None => None,
            Some(AppTheme::Light) => Some(named(AQUA)?),
            Some(AppTheme::Dark) => Some(named(DARK_NAMES[0])?),
        };
        NSApplication::sharedApplication(marker).setAppearance(appearance.as_deref());
        Ok(())
    }

    pub(super) fn apply(
        _window: &impl HasWindowHandle,
        theme: Option<AppTheme>,
    ) -> Result<(), NativeThemeError> {
        // The window is deliberately unused: `NSApplication.appearance` is inherited by
        // every window, and by the alerts, menus and panels those windows present, which
        // is exactly the set the product wants themed. Setting it per window instead
        // would leave the menu and panel rows of the table above on the system theme.
        apply_process(theme)
    }

    pub(super) fn init() -> Result<(), NativeThemeError> {
        // macOS applies the application theme to the system surfaces as soon as the
        // appearance above is set; there is no second switch to prepare.
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{AppTheme, NativeThemeError};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{ffi::c_void, mem::size_of, ptr::from_ref, sync::OnceLock};
    use windows::{
        Win32::{
            Foundation::HWND,
            Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute},
            System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        },
        core::{BOOL, PCSTR, w},
    };

    /// `uxtheme.dll` exports `SetPreferredAppMode` by ordinal only; Microsoft never
    /// published a header or a name for it. See ADR-0048 for why the product accepts
    /// that dependency and what it is allowed to affect.
    const SET_PREFERRED_APP_MODE_ORDINAL: usize = 135;

    /// `PreferredAppMode::AllowDark`: render the ComCtl32 and shell surfaces dark when
    /// the system is dark, and leave them light when it is not. `ForceDark` is
    /// deliberately not used — the product's contract for these surfaces is "follow the
    /// system", not "follow the application theme".
    const PREFERRED_APP_MODE_ALLOW_DARK: i32 = 1;

    /// The address of `SetPreferredAppMode`, resolved once. The module is never freed:
    /// the theme engine has to stay loaded for the lifetime of the process.
    static SET_PREFERRED_APP_MODE: OnceLock<Option<unsafe extern "system" fn(i32) -> i32>> =
        OnceLock::new();

    pub(super) fn apply_process(_theme: Option<AppTheme>) -> Result<(), NativeThemeError> {
        // Windows does not expose a supported process-wide application theme for these
        // surfaces. ComCtl32 and the shell follow the system after `init()` opts into
        // their system dark-mode policy.
        Ok(())
    }

    pub(super) fn init() -> Result<(), NativeThemeError> {
        let procedure = *SET_PREFERRED_APP_MODE.get_or_init(resolve_set_preferred_app_mode);
        let Some(procedure) = procedure else {
            // Ordinal 135 is absent on the builds that predate the switch. The surfaces
            // it governs then keep the system appearance, which is the documented
            // fallback; refusing to start over it would be worse.
            return Err(NativeThemeError::NativeCallFailed);
        };
        // SAFETY: `procedure` was resolved from `uxtheme.dll` at ordinal 135 and this is
        // the call the API expects: it reads one enum-sized argument and returns the
        // previous mode, which the product does not use.
        unsafe { procedure(PREFERRED_APP_MODE_ALLOW_DARK) };
        Ok(())
    }

    fn resolve_set_preferred_app_mode() -> Option<unsafe extern "system" fn(i32) -> i32> {
        // SAFETY: `uxtheme.dll` is resolved through the system search path, so no
        // attacker-controlled directory can substitute it, and the module is intentionally
        // leaked — see `SET_PREFERRED_APP_MODE`. `GetProcAddress` returns either a valid
        // address or null, and the null case is handled by the caller.
        unsafe {
            let module =
                LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
            // `MAKEINTRESOURCEA(135)`: an ordinal is passed as an integer cast to a
            // pointer, which is what the API expects when the name is not a string.
            let name = PCSTR(SET_PREFERRED_APP_MODE_ORDINAL as *const u8);
            GetProcAddress(module, name).map(|procedure| {
                // SAFETY: the caller verified this ordinal is `SetPreferredAppMode`,
                // whose signature is `PreferredAppMode(PreferredAppMode)`: one
                // 32-bit enum in, one 32-bit enum out.
                std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(i32) -> i32,
                >(procedure)
            })
        }
    }

    pub(super) fn apply(
        window: &impl HasWindowHandle,
        theme: Option<AppTheme>,
    ) -> Result<(), NativeThemeError> {
        let Some(theme) = theme else {
            // The product follows the system: Windows draws the frame from the system
            // preference on its own, so there is nothing to override and no attribute to
            // clear. gpui already applied that value when it created the window.
            return Ok(());
        };
        let handle = window
            .window_handle()
            .map_err(|_| NativeThemeError::WindowHandleUnavailable)?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err(NativeThemeError::UnsupportedWindowHandle);
        };
        let dark: BOOL = theme.is_dark().into();
        // SAFETY: the HWND belongs to a window this process created, and `window` keeps
        // it alive for the duration of the call. DWM reads one `BOOL` from the pointer
        // and the length passed matches it.
        unsafe {
            DwmSetWindowAttribute(
                HWND(handle.hwnd.get() as *mut c_void),
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                from_ref(&dark).cast::<c_void>(),
                size_of::<BOOL>() as u32,
            )
        }
        .map_err(|_| NativeThemeError::NativeCallFailed)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use super::{AppTheme, NativeThemeError};
    use raw_window_handle::HasWindowHandle;

    pub(super) fn apply_process(_theme: Option<AppTheme>) -> Result<(), NativeThemeError> {
        Ok(())
    }

    pub(super) fn apply(
        _window: &impl HasWindowHandle,
        _theme: Option<AppTheme>,
    ) -> Result<(), NativeThemeError> {
        // The compositor owns the frame and the GTK portal owns the panels, so there is
        // no application-level switch to set. See the module docs.
        Ok(())
    }

    pub(super) fn init() -> Result<(), NativeThemeError> {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub use platform::system_appearance;

#[cfg(test)]
mod tests {
    use super::{AppTheme, NativeThemeError};

    #[test]
    fn error_codes_are_stable_and_unique() {
        let mut codes = NativeThemeError::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.iter().all(|code| code.starts_with("native_theme_")));
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), NativeThemeError::ALL.len());
        assert_eq!(
            NativeThemeError::WrongThread.to_string(),
            "native_theme_wrong_thread"
        );
    }

    #[test]
    fn only_dark_reports_itself_as_dark() {
        assert!(AppTheme::Dark.is_dark());
        assert!(!AppTheme::Light.is_dark());
    }
}
