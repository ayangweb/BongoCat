//! Startup permission check and user guidance.
//!
//! The product needs one platform capability before global input works: the macOS Input Monitoring
//! grant, or an elevated Windows token. The check reads the platform state on every start and
//! stores nothing, so a user who dismissed the prompt is asked again while the capability is still
//! missing (ADR-0032).
//!
//! The check never runs on the main thread: the product starts its windows first and the caller
//! spawns a dedicated worker for this function, so a pending prompt cannot delay any product
//! window. Prompt copy is built here from the translation catalog and handed to the platform
//! adapter, which owns the native dialog; the adapter never reads configuration or product copy.

use bongocat_config::Language;

/// Translation keys for the prompt this platform shows. The keys are resolved at compile time, so
/// exactly one platform prompt is built.
mod keys {
    /// Shared dismiss button. Both prompts offer the same choice, so one key names it instead of
    /// repeating the copy per platform: the user keeps using the product and sets the missing
    /// capability up later.
    pub(super) const SECONDARY: &str = "startup_permission.later";

    #[cfg(target_os = "macos")]
    pub(super) const TITLE: &str = "startup_permission.input_monitoring.title";
    #[cfg(target_os = "macos")]
    pub(super) const DESCRIPTION: &str = "startup_permission.input_monitoring.description";
    #[cfg(target_os = "macos")]
    pub(super) const PRIMARY: &str = "startup_permission.input_monitoring.open_settings";

    #[cfg(target_os = "windows")]
    pub(super) const TITLE: &str = "startup_permission.administrator.title";
    #[cfg(target_os = "windows")]
    pub(super) const DESCRIPTION: &str = "startup_permission.administrator.description";
    #[cfg(target_os = "windows")]
    pub(super) const PRIMARY: &str = "startup_permission.administrator.open_program_folder";
}

/// Runs the startup permission check for this platform.
///
/// `language` must be the resolved product language: the caller resolves it once and hands it to
/// the dedicated startup-permission worker, so the prompt does not depend on any settings window.
/// Nothing is returned to the caller as state: the outcome only describes what this start did, and
/// the next start re-reads the platform.
pub fn ensure_startup_permission(language: Language) -> bongocat_platform::StartupPermissionStatus {
    let locale = bongocat_i18n::locale_code(language.code());
    let text = |key| bongocat_i18n::text(locale, key).to_owned();
    bongocat_platform::check_startup_permission(&bongocat_platform::StartupPermissionPrompt {
        title: text(keys::TITLE),
        description: text(keys::DESCRIPTION),
        primary: text(keys::PRIMARY),
        secondary: text(keys::SECONDARY),
    })
}

#[cfg(test)]
mod tests {
    use super::keys;

    #[test]
    fn prompt_keys_are_translated_for_every_supported_language() {
        for locale in ["en-US", "zh-CN"] {
            for key in [
                keys::TITLE,
                keys::DESCRIPTION,
                keys::PRIMARY,
                keys::SECONDARY,
            ] {
                let value = bongocat_i18n::text(locale, key);
                assert!(!value.is_empty(), "missing {key} for {locale}");
                assert_ne!(value, key, "untranslated {key} for {locale}");
            }
        }
    }

    #[test]
    fn prompt_labels_stay_distinguishable_in_every_language() {
        // The macOS mapping from an `rfd` result back to the user's choice compares the returned
        // label with the primary label, so the two labels must never be interchangeable.
        for code in ["zh-CN", "en-US"] {
            assert_ne!(
                bongocat_i18n::text(code, keys::PRIMARY),
                bongocat_i18n::text(code, keys::SECONDARY),
                "{code}"
            );
        }
    }
}
