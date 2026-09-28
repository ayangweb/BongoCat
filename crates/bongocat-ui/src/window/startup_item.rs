//! The login item, and the platforms that cannot have one.
//!
//! A platform without a login item, a build that must not install one, and a
//! user who turned it off are three states rather than one boolean, and the
//! control each needs is different. The presentation is computed here so the
//! page and the window agree on which of the three it is showing.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartupItemAction {
    None,
    Retry,
    SetEnabled(bool),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StartupItemPresentation {
    /// Row copy for the states the switch position cannot explain on its own: a
    /// read that is still in flight, a login item that needs repair, or a
    /// platform that cannot offer login startup at all.
    ///
    /// `Disabled` and `Enabled` carry none: the switch already shows that state,
    /// so a second line repeating it would only add height to the common case.
    pub(crate) description: Option<&'static str>,
    pub(crate) enabled: bool,
    pub(crate) action: StartupItemAction,
    /// Whether the whole row is greyed out and unavailable.
    ///
    /// This is a fact about the build, not about the current moment. Whether the
    /// control can act *right now* is a separate question answered by `action`:
    /// keeping the two apart lets a released build keep the row normally available
    /// while the snapshot is still loading.
    pub(crate) disabled: bool,
}

pub(crate) fn startup_item_presentation(
    status: Option<SettingsStartupItemStatus>,
    blocked: bool,
    language: SettingsLanguage,
) -> StartupItemPresentation {
    let mut presentation = match status {
        None => StartupItemPresentation {
            description: Some(bongocat_i18n::text(
                language.catalog_locale(),
                "settings.app_system.startup.checking",
            )),
            enabled: false,
            action: StartupItemAction::None,
            disabled: false,
        },
        Some(SettingsStartupItemStatus::ReadError(_)) => StartupItemPresentation {
            description: Some(bongocat_i18n::text(
                language.catalog_locale(),
                "settings.app_system.startup.unavailable",
            )),
            enabled: false,
            action: StartupItemAction::Retry,
            disabled: false,
        },
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled)) => {
            StartupItemPresentation {
                description: None,
                enabled: false,
                action: StartupItemAction::SetEnabled(true),
                disabled: false,
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled)) => {
            StartupItemPresentation {
                description: None,
                enabled: true,
                action: StartupItemAction::SetEnabled(false),
                disabled: false,
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Stale)) => {
            StartupItemPresentation {
                description: Some(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.app_system.startup.stale",
                )),
                enabled: false,
                action: StartupItemAction::SetEnabled(true),
                disabled: false,
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::RequiresApproval)) => {
            StartupItemPresentation {
                description: Some(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.app_system.startup.requires_approval",
                )),
                enabled: true,
                action: StartupItemAction::SetEnabled(false),
                disabled: false,
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::NotFound)) => {
            StartupItemPresentation {
                description: Some(bongocat_i18n::text(
                    language.catalog_locale(),
                    "settings.app_system.startup.not_found",
                )),
                enabled: false,
                action: StartupItemAction::SetEnabled(true),
                disabled: false,
            }
        }
        Some(SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(reason))) => {
            let (description, disabled) = match reason {
                SettingsStartupItemUnsupportedReason::Platform => (
                    Some(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.app_system.startup.unsupported_platform",
                    )),
                    false,
                ),
                SettingsStartupItemUnsupportedReason::OperatingSystem => (
                    Some(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.app_system.startup.unsupported_os",
                    )),
                    false,
                ),
                SettingsStartupItemUnsupportedReason::BuildEnvironment => (
                    Some(bongocat_i18n::text(
                        language.catalog_locale(),
                        "settings.app_system.startup.unsupported_build",
                    )),
                    true,
                ),
            };
            StartupItemPresentation {
                description,
                enabled: false,
                action: StartupItemAction::None,
                disabled,
            }
        }
    };
    if blocked {
        presentation.action = StartupItemAction::None;
    }
    presentation
}
