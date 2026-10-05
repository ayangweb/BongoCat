//! Startup permission check and user guidance.
//!
//! The product needs one platform capability before global input works: the macOS Input Monitoring
//! grant, an elevated Windows token, or Linux evdev device access. The check reads the platform
//! state on every start and stores nothing, so a user who dismissed an available prompt is asked
//! again while the capability is still missing (ADR-0032).
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

    #[cfg(target_os = "linux")]
    pub(super) const TITLE: &str = "startup_permission.input_monitoring.title";
    #[cfg(target_os = "linux")]
    pub(super) const DESCRIPTION: &str = "startup_permission.input_monitoring.description";
    #[cfg(target_os = "linux")]
    pub(super) const PRIMARY: &str = "startup_permission.input_monitoring.open_settings";
}

/// Runs the startup permission check for this platform.
///
/// `language` must be the resolved product language: the caller resolves it once and hands it to
/// the dedicated startup-permission worker, so the prompt does not depend on any settings window.
/// Nothing is returned to the caller as state: the outcome only describes what this start did, and
/// the next start re-reads the platform.
pub fn ensure_startup_permission(language: Language) -> bongocat_platform::StartupPermissionStatus {
    let prompt = localized_prompt(language);
    bongocat_platform::check_startup_permission(&prompt)
}

fn localized_prompt(language: Language) -> bongocat_platform::StartupPermissionPrompt {
    let locale = bongocat_i18n::locale_code(language.code());
    let text = |key| bongocat_i18n::text(locale, key).to_owned();
    bongocat_platform::StartupPermissionPrompt {
        title: text(keys::TITLE),
        description: text(keys::DESCRIPTION),
        primary: text(keys::PRIMARY),
        secondary: text(keys::SECONDARY),
        // The macOS guided flow is a Swift panel with its own catalog, so the adapter needs the
        // product language to make the panel and this prompt agree (ADR-0078). It is the same
        // resolved locale the copy above came from.
        locale: locale.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::keys;
    use bongocat_config::Language;

    #[test]
    fn prompt_keys_are_translated_for_every_supported_language() {
        // `Language::ALL` rather than a list of codes: a language the product
        // can be switched to and a catalog that has to carry the prompt are the
        // same question, and answering it from the enum means adding a language
        // cannot leave this prompt untranslated.
        for language in Language::ALL {
            let locale = bongocat_i18n::locale_code(language.code());
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
        for language in Language::ALL {
            let code = language.code();
            let locale = bongocat_i18n::locale_code(code);
            assert_ne!(
                bongocat_i18n::text(locale, keys::PRIMARY),
                bongocat_i18n::text(locale, keys::SECONDARY),
                "{code}"
            );
        }
    }

    /// The macOS description is one purpose sentence, not a how-to.
    ///
    /// It used to end with a second paragraph telling the user to remove BongoCat from the Input
    /// Monitoring list and add it again. The guided flow clears the grant through `tccutil` before
    /// the panel opens, so that entry is gone by the time the user looks at the list, and the
    /// paragraph only described work the product already did. A blank line is how a second
    /// paragraph would come back, which is what this pins. The Windows prompt keeps its own setup
    /// instructions — nothing else on that platform explains the compatibility flag — so the rule
    /// is macOS-only.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_macos_prompt_description_is_a_single_paragraph() {
        for language in Language::ALL {
            let locale = bongocat_i18n::locale_code(language.code());
            let description = bongocat_i18n::text(locale, keys::DESCRIPTION);
            assert!(
                !description.contains("\n\n"),
                "{}: the macOS prompt description grew a second paragraph again: {description}",
                language.code()
            );
        }
    }
}
