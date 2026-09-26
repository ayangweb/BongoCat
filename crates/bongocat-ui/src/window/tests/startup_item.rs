//! The login-item row in every state the build can offer it in.

use super::*;

#[test]
fn startup_item_presentations_cover_every_platform_state_and_retry() {
    let cases = [
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled),
            false,
            StartupItemAction::SetEnabled(true),
        ),
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled),
            true,
            StartupItemAction::SetEnabled(false),
        ),
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::Stale),
            false,
            StartupItemAction::SetEnabled(true),
        ),
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::RequiresApproval),
            true,
            StartupItemAction::SetEnabled(false),
        ),
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::NotFound),
            false,
            StartupItemAction::SetEnabled(true),
        ),
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
                SettingsStartupItemUnsupportedReason::BuildEnvironment,
            )),
            false,
            StartupItemAction::None,
        ),
        (
            SettingsStartupItemStatus::ReadError(crate::SettingsStartupItemError::StateReadFailed),
            false,
            StartupItemAction::Retry,
        ),
    ];

    for (status, enabled, action) in cases {
        let presentation =
            startup_item_presentation(Some(status), false, SettingsLanguage::EnglishUnitedStates);
        assert_eq!(presentation.enabled, enabled);
        assert_eq!(presentation.action, action);
        // The switch position answers the two steady states, so they are the only
        // ones without row copy; every state that needs explaining still has it.
        let steady_state = matches!(
            status,
            SettingsStartupItemStatus::State(
                SettingsStartupItemState::Disabled | SettingsStartupItemState::Enabled
            )
        );
        assert_eq!(
            presentation.description.is_some(),
            !steady_state,
            "{status:?} must carry row copy exactly when the switch cannot explain itself"
        );
        let build_unavailable = matches!(
            status,
            SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
                SettingsStartupItemUnsupportedReason::BuildEnvironment
            ))
        );
        assert_eq!(
            presentation.disabled, build_unavailable,
            "{status:?} must grey the whole row exactly when the build cannot offer login startup"
        );
        assert_eq!(
            startup_item_presentation(Some(status), true, SettingsLanguage::EnglishUnitedStates,)
                .action,
            StartupItemAction::None
        );
    }
    assert_eq!(
        startup_item_presentation(None, false, SettingsLanguage::EnglishUnitedStates).action,
        StartupItemAction::None
    );
    assert_eq!(
        startup_item_presentation(
            Some(SettingsStartupItemStatus::State(
                SettingsStartupItemState::Enabled
            )),
            false,
            SettingsLanguage::ChineseSimplified,
        )
        .description,
        None
    );
    // A state the switch cannot explain still carries the localized copy, so the
    // steady states are the only rows that lost their second line.
    assert_eq!(
        startup_item_presentation(
            Some(SettingsStartupItemStatus::State(
                SettingsStartupItemState::Stale
            )),
            false,
            SettingsLanguage::ChineseSimplified,
        )
        .description,
        Some("应用位置已变化；重新开关一次即可修复")
    );
}

/// The whole row is greyed out exactly where the build cannot offer login startup.
///
/// Transient states do not disable it: `action` already reports whether the
/// control can act right now. A released build therefore keeps the row normally
/// available while the snapshot is still loading.
#[test]
fn the_startup_row_is_disabled_exactly_where_the_build_cannot_offer_it() {
    let statuses = [
        None,
        Some(SettingsStartupItemStatus::ReadError(
            crate::SettingsStartupItemError::StateReadFailed,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Disabled,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Enabled,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Stale,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::RequiresApproval,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::NotFound,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Unsupported(
                SettingsStartupItemUnsupportedReason::BuildEnvironment,
            ),
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Unsupported(SettingsStartupItemUnsupportedReason::Platform),
        )),
    ];

    for status in statuses {
        for blocked in [false, true] {
            let presentation =
                startup_item_presentation(status, blocked, SettingsLanguage::EnglishUnitedStates);
            let build_cannot_offer_it = matches!(
                status,
                Some(SettingsStartupItemStatus::State(
                    SettingsStartupItemState::Unsupported(
                        SettingsStartupItemUnsupportedReason::BuildEnvironment
                    )
                ))
            );
            assert_eq!(
                presentation.disabled, build_cannot_offer_it,
                "{status:?} blocked={blocked} disabled the row for the wrong reason"
            );
        }
    }
}

/// A development build's row is greyed out and explains why in the visible copy.
///
/// The copy comes from one catalog entry, so the reason shown next to a disabled
/// row cannot drift from the unavailable state.
#[test]
fn a_development_build_disables_the_startup_row_with_visible_copy() {
    let status = SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
        SettingsStartupItemUnsupportedReason::BuildEnvironment,
    ));

    let presentation =
        startup_item_presentation(Some(status), false, SettingsLanguage::ChineseSimplified);
    assert!(presentation.disabled);
    assert_eq!(presentation.description, Some("开发版本不支持登录时启动"));
    assert_eq!(presentation.action, StartupItemAction::None);
}

/// Every state a released build can produce keeps the row operable.
///
/// The presentation knows nothing about the build environment, so this is the
/// released direction expressed where it can be checked without a released
/// build: no actionable state is greyed out.
#[test]
fn the_startup_row_stays_operable_in_every_actionable_state() {
    for (status, expected) in [
        (SettingsStartupItemState::Disabled, true),
        (SettingsStartupItemState::Enabled, false),
        (SettingsStartupItemState::RequiresApproval, false),
    ] {
        let presentation = startup_item_presentation(
            Some(SettingsStartupItemStatus::State(status)),
            false,
            SettingsLanguage::EnglishUnitedStates,
        );
        assert!(!presentation.disabled);
        assert_eq!(presentation.action, StartupItemAction::SetEnabled(expected));
    }
}
