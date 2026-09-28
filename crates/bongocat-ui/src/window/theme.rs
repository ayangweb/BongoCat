//! Which theme the window runs in, and how it got there.
//!
//! Three things decide it: what the user picked, whether they pinned a native
//! title bar along with it, and what the system currently reports. They are kept
//! apart because they answer at different times — a system change is observed
//! continuously, while a pin only takes effect on the next open.

use super::*;

pub(crate) fn sync_system_component_theme(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
}

/// The component-library mode a preference pins, or `None` when it follows the system.
pub(crate) const fn pinned_theme_mode(theme: SettingsTheme) -> Option<ThemeMode> {
    match theme {
        SettingsTheme::System => None,
        SettingsTheme::Light => Some(ThemeMode::Light),
        SettingsTheme::Dark => Some(ThemeMode::Dark),
    }
}

/// The native appearance a preference pins, or `None` when it follows the system.
pub(crate) const fn pinned_native_theme(
    theme: SettingsTheme,
) -> Option<bongocat_platform::AppTheme> {
    match theme {
        SettingsTheme::System => None,
        SettingsTheme::Light => Some(bongocat_platform::AppTheme::Light),
        SettingsTheme::Dark => Some(bongocat_platform::AppTheme::Dark),
    }
}

/// Hands the preference to the platform layer, which owns every native surface that has
/// to follow it: the window frame, the alerts, the tray and context menus, and the
/// open/save panels.
///
/// A failure is not raised. The surfaces that can refuse are the ones that cannot follow
/// an application theme at all on that platform, and their documented fallback is the
/// system appearance — which is what refusing leaves them on. The product still applies
/// the choice to everything it paints itself, so the user's selection is never lost to a
/// cosmetic failure.
pub(crate) fn apply_native_theme(theme: SettingsTheme, window: &Window) {
    let _ = bongocat_platform::apply_theme(window, pinned_native_theme(theme));
}

/// The appearance the operating system is using, for the "follow the system" choice.
///
/// macOS asks the platform layer rather than gpui, for two separate reasons:
///
/// - `Window::appearance()` is a cached field, refreshed from a deferred
///   `appearance_changed` callback. On the frame where the application override is
///   cleared the cache still reports the value that was just cleared, and `SettingsView`
///   remembers that it already applied the preference, so it would never correct itself.
/// - `App::window_appearance()` is live — it and the platform query both read
///   `NSApplication.effectiveAppearance` — but its name mapping only recognises `Aqua`,
///   `DarkAqua`, `VibrantLight` and `VibrantDark`, and falls through to `Light` (printing
///   to stdout) for anything else. With "Increase contrast" on, AppKit reports
///   `AccessibilityHighContrastDarkAqua`, so gpui would call a dark system light and the
///   product would paint a light UI inside a dark one. The platform query knows that name.
///
/// `window` is only consulted off macOS, and only when there is one. A caller without a
/// window — the smoke assertions — falls back to the application appearance, which is the
/// same platform query.
pub(crate) fn system_appearance(window: Option<&Window>, cx: &App) -> WindowAppearance {
    #[cfg(target_os = "macos")]
    {
        // Neither argument is consulted: the whole point of this branch is to avoid the
        // value gpui holds, and the platform query needs no window.
        let _ = (window, cx);
        match bongocat_platform::system_appearance() {
            bongocat_platform::SystemAppearance::Light => WindowAppearance::Light,
            bongocat_platform::SystemAppearance::Dark => WindowAppearance::Dark,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        window.map_or_else(|| cx.window_appearance(), Window::appearance)
    }
}

/// The component-library mode a preference resolves to.
///
/// One definition for the whole crate. The render path, the optimistic path and the smoke
/// assertions all have to agree on what a preference means, and the way to make them agree
/// is to have only one of them compute it — the smoke exists to prove what the product
/// does, so it must not re-derive the answer with a second formula.
pub(crate) fn resolved_theme_mode(
    theme: SettingsTheme,
    window: Option<&Window>,
    cx: &App,
) -> ThemeMode {
    match pinned_theme_mode(theme) {
        Some(mode) => mode,
        None => component_theme_mode(theme, system_appearance(window, cx)),
    }
}

/// Applies the preference to the component colours before the configuration roundtrip,
/// so the user sees the switch on the frame they clicked rather than one snapshot later.
///
/// `System` resolves against the live system appearance, which on macOS is only the
/// truth once the application override a pinned Light/Dark installed has been dropped —
/// so the override is dropped first, mirroring the ordering of `apply_component_theme`
/// (native first, then resolve). Windows has no process override to drop; its frame is
/// corrected by the roundtrip's `apply_component_theme` right after this.
pub(crate) fn apply_optimistic_component_theme(theme: SettingsTheme, cx: &mut App) {
    if theme == SettingsTheme::System {
        let _ = bongocat_platform::apply_process_theme(None);
    }
    let mode = resolved_theme_mode(theme, None, cx);
    if cx.theme().mode != mode {
        Theme::change(mode, None, cx);
    }
}

/// Applies a preference to both halves of the appearance: the native surfaces the
/// platform draws, and the component colours the product draws.
///
/// The order is not interchangeable. On macOS the native call installs the very override
/// that the component mode is derived from when the preference is `System`, so asking
/// for the mode first would resolve against the previous override.
pub(crate) fn apply_component_theme(theme: SettingsTheme, window: &mut Window, cx: &mut App) {
    apply_native_theme(theme, window);
    let mode = resolved_theme_mode(theme, Some(window), cx);
    if cx.theme().mode != mode {
        Theme::change(mode, Some(window), cx);
    }
}

pub(crate) fn component_theme_mode(
    theme: SettingsTheme,
    system_appearance: WindowAppearance,
) -> ThemeMode {
    match theme {
        SettingsTheme::System => system_appearance.into(),
        SettingsTheme::Light => ThemeMode::Light,
        SettingsTheme::Dark => ThemeMode::Dark,
    }
}

pub(crate) const fn theme_index(theme: SettingsTheme) -> usize {
    match theme {
        SettingsTheme::System => 0,
        SettingsTheme::Light => 1,
        SettingsTheme::Dark => 2,
    }
}

pub(crate) fn theme_options(language: SettingsLanguage) -> [&'static str; 3] {
    [
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.appearance.theme.options.system",
        ),
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.appearance.theme.options.light",
        ),
        bongocat_i18n::text(
            language.catalog_locale(),
            "settings.appearance.theme.options.dark",
        ),
    ]
}

pub(crate) fn theme_display_name(theme: SettingsTheme, language: SettingsLanguage) -> &'static str {
    theme_options(language)[theme_index(theme)]
}

pub(crate) fn theme_from_display_name(
    name: &str,
    language: SettingsLanguage,
) -> Option<SettingsTheme> {
    theme_options(language)
        .into_iter()
        .position(|option| option == name)
        .and_then(theme_from_index)
}

pub(crate) const fn theme_from_index(index: usize) -> Option<SettingsTheme> {
    match index {
        0 => Some(SettingsTheme::System),
        1 => Some(SettingsTheme::Light),
        2 => Some(SettingsTheme::Dark),
        _ => None,
    }
}
