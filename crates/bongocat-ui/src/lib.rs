#![forbid(unsafe_code)]

//! GPUI views and the stable typed protocol re-export used by existing view
//! modules. The protocol itself lives in `bongocat-ui-protocol`, which has no
//! GPUI or operating-system dependency.

use std::time::{Duration, Instant};

mod pop_confirm;
mod window;
pub use window::{SettingsView, SettingsWindowHandle, SettingsWindowSeed, open_settings_window};

// Keep the module path used by update-window render tests while the protocol
// implementation and its public types live in the dedicated crate.
mod update;
mod update_markdown;
mod update_window;
pub use update_window::{UpdateView, UpdateWindowHandle, open_update_window};

pub(crate) use bongocat_ui_protocol::*;

pub(crate) fn settings_language_display_name(
    language: SettingsLanguage,
    display_language: SettingsLanguage,
) -> &'static str {
    let locale = display_language.catalog_locale();
    match language {
        SettingsLanguage::System => {
            bongocat_i18n::text(locale, "settings.appearance.language.options.system")
        }
        SettingsLanguage::ChineseSimplified => bongocat_i18n::text(
            locale,
            "settings.appearance.language.options.chinese_simplified",
        ),
        SettingsLanguage::EnglishUnitedStates => bongocat_i18n::text(
            locale,
            "settings.appearance.language.options.english_united_states",
        ),
    }
}

pub(crate) fn settings_language_from_display_name(
    name: &str,
    display_language: SettingsLanguage,
) -> Option<SettingsLanguage> {
    SettingsLanguage::ALL
        .into_iter()
        .find(|language| settings_language_display_name(*language, display_language) == name)
}

pub(crate) const SETTINGS_PATCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// Coalesces rapid typed setting updates while retaining values that were not
/// acknowledged by the settings service. This is view-local draft state; the
/// typed command itself remains in `bongocat-ui-protocol`.
#[derive(Clone, Debug)]
pub(crate) struct SettingsPatchDebouncer<T> {
    last_sent_at: Option<Instant>,
    pending: Option<T>,
    debounce: Duration,
}

impl<T> Default for SettingsPatchDebouncer<T> {
    fn default() -> Self {
        Self {
            last_sent_at: None,
            pending: None,
            debounce: SETTINGS_PATCH_DEBOUNCE,
        }
    }
}

impl<T: Clone + PartialEq> SettingsPatchDebouncer<T> {
    pub(crate) fn observe(&mut self, value: T, now: Instant) -> Option<T> {
        self.pending = Some(value);
        if self
            .last_sent_at
            .is_none_or(|last| now.saturating_duration_since(last) >= self.debounce)
        {
            self.last_sent_at = Some(now);
            self.pending.clone()
        } else {
            None
        }
    }

    pub(crate) fn mark_sent(&mut self, value: &T) {
        if self.pending.as_ref() == Some(value) {
            self.pending = None;
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn pending_value(&self) -> Option<&T> {
        self.pending.as_ref()
    }

    pub(crate) fn discard_pending(&mut self) {
        self.pending = None;
    }

    pub(crate) fn ready(&self, now: Instant) -> Option<T> {
        self.pending.as_ref().and_then(|pending| {
            self.last_sent_at
                .is_none_or(|last| now.saturating_duration_since(last) >= self.debounce)
                .then(|| pending.clone())
        })
    }

    pub(crate) fn flush(&mut self, now: Instant) -> Option<T> {
        if self.pending.is_some() {
            self.last_sent_at = Some(now);
        }
        self.pending.clone()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal settings snapshot for GPUI view tests. Keeping this helper in
    /// the view crate avoids making protocol tests depend on GPUI fixtures.
    pub(crate) fn snapshot(
        revision: u64,
        overlay_visible: bool,
        motion_audio_enabled: bool,
    ) -> SettingsSnapshot {
        SettingsSnapshot {
            revision,
            config_revision: Some(revision),
            build_info: SettingsBuildInfo {
                product_version: env!("CARGO_PKG_VERSION").to_owned(),
                environment: SettingsBuildEnvironment::Development,
            },
            runtime_health: RuntimeHealth::Ready,
            runtime_diagnostics: SettingsRuntimeDiagnostics::default(),
            appearance_theme: SettingsTheme::System,
            language: SettingsLanguage::System,
            resolved_language: SettingsLanguage::EnglishUnitedStates,
            status_icon_visible: true,
            taskbar_icon_visible: true,
            check_for_updates_automatically: true,
            overlay_visible,
            overlay: SettingsOverlay::default(),
            motion_audio_enabled,
            command_shortcuts_enabled: true,
            behavior_shortcuts_enabled: true,
            maximum_fps: 60,
            release_fallback_timeout_ms: 500,
            model_settings: SettingsModelSettings::default(),
            gamepad_axis_settings: SettingsGamepadAxisSettings::default(),
            logging: SettingsLogging::default(),
            shortcuts: SettingsShortcuts::default(),
            startup_item: SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled),
            diagnostics_export: None,
            input_diagnostics: SettingsInputDiagnostics::default(),
            active_model: Some(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            }),
            model_catalog: SettingsModelCatalog::default(),
        }
    }

    #[test]
    fn language_preferences_have_stable_codes_and_localized_names() {
        let expected = [
            ("system", "System"),
            ("zh-CN", "简体中文"),
            ("en-US", "English"),
        ];
        for (language, (code, display_name)) in SettingsLanguage::ALL.into_iter().zip(expected) {
            assert_eq!(language.code(), code);
            assert_eq!(
                settings_language_display_name(language, SettingsLanguage::EnglishUnitedStates),
                display_name
            );
            assert_eq!(
                settings_language_from_display_name(
                    display_name,
                    SettingsLanguage::EnglishUnitedStates
                ),
                Some(language)
            );
        }
        assert_eq!(
            settings_language_display_name(
                SettingsLanguage::System,
                SettingsLanguage::ChineseSimplified,
            ),
            "跟随系统"
        );
        assert_eq!(
            settings_language_from_display_name("Deutsch", SettingsLanguage::EnglishUnitedStates,),
            None
        );
    }

    #[test]
    fn settings_patch_debouncer_coalesces_and_confirms_latest_value() {
        let origin = Instant::now();
        let mut debouncer = SettingsPatchDebouncer::default();

        assert_eq!(debouncer.observe(10_u16, origin), Some(10));
        debouncer.mark_sent(&10);
        assert_eq!(
            debouncer.observe(20, origin + Duration::from_millis(50)),
            None
        );
        assert_eq!(
            debouncer.observe(30, origin + Duration::from_millis(100)),
            None
        );
        assert_eq!(
            debouncer.observe(30, origin + Duration::from_millis(150)),
            Some(30)
        );
        debouncer.mark_sent(&30);
        assert_eq!(debouncer.flush(origin + Duration::from_millis(200)), None);
    }

    #[test]
    fn settings_patch_debouncer_retains_unconfirmed_value_for_retry_and_flush() {
        let origin = Instant::now();
        let mut debouncer = SettingsPatchDebouncer::default();

        assert_eq!(debouncer.observe("first", origin), Some("first"));
        assert_eq!(
            debouncer.observe("latest", origin + Duration::from_millis(25)),
            None
        );
        assert_eq!(
            debouncer.flush(origin + Duration::from_millis(30)),
            Some("latest")
        );
        assert_eq!(
            debouncer.flush(origin + Duration::from_millis(31)),
            Some("latest")
        );
        debouncer.mark_sent(&"latest");
        assert_eq!(debouncer.flush(origin + Duration::from_millis(32)), None);
    }

    #[test]
    fn settings_patch_debouncer_waits_for_the_stable_window_before_retry() {
        let origin = Instant::now();
        let mut debouncer = SettingsPatchDebouncer::default();
        assert_eq!(debouncer.observe(10_u16, origin), Some(10));
        assert_eq!(
            debouncer.observe(20, origin + Duration::from_millis(50)),
            None
        );
        assert_eq!(debouncer.ready(origin + Duration::from_millis(149)), None);
        assert_eq!(
            debouncer.ready(origin + Duration::from_millis(150)),
            Some(20)
        );
        assert!(debouncer.is_pending());
    }

    #[test]
    fn one_logging_debouncer_keeps_the_complete_policy_across_both_fields() {
        let origin = Instant::now();
        let mut debouncer = SettingsPatchDebouncer::default();
        let level_only = SettingsLogging {
            level: SettingsLogLevel::Debug,
            retention_days: 7,
        };
        let complete = SettingsLogging {
            level: SettingsLogLevel::Trace,
            retention_days: 30,
        };

        assert_eq!(debouncer.observe(level_only, origin), Some(level_only));
        assert_eq!(
            debouncer.observe(complete, origin + Duration::from_millis(25)),
            None
        );
        assert_eq!(debouncer.pending_value(), Some(&complete));
        assert_eq!(
            debouncer.flush(origin + Duration::from_millis(30)),
            Some(complete)
        );

        debouncer.mark_sent(&level_only);
        assert_eq!(
            debouncer.pending_value(),
            Some(&complete),
            "acknowledging the first policy must retain the newer complete policy"
        );
        debouncer.mark_sent(&complete);
        assert_eq!(debouncer.pending_value(), None);
    }
}
