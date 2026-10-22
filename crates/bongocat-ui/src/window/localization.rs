use super::plugins_host::{PluginCardView, PluginFailure, PluginFailureReport};
use super::{
    BehaviorKind, BehaviorOrdinal, SettingsBuildInfo, SettingsError, SettingsErrorCode,
    SettingsLanguage, SettingsModelOrigin,
};
use bongocat_plugin_service::PluginCommandError;
use gpui_kit::SharedString;

/// The version row the About page shows.
///
/// The build environment is deliberately absent. Development and Production are
/// the same binary with different data roots, and which one is running is the
/// update channel's business rather than the reader's: a version a user is
/// looking at should name the version. The environment is still carried in the
/// snapshot and still travels in the copied software information report, where
/// a triager reads it as its own field instead of as a suffix.
pub(super) fn build_info_detail(
    language: SettingsLanguage,
    build_info: &SettingsBuildInfo,
) -> String {
    format!(
        "{} {}",
        bongocat_i18n::text(
            language.catalog_locale(),
            "about.product_information.version.label",
        ),
        build_info.product_version,
    )
}

pub(super) fn shortcut_command_name(language: SettingsLanguage, command: &str) -> String {
    let key = match command {
        "toggle_overlay" => "shortcuts.command_names.toggle_model_window",
        "open_settings" => "shortcuts.command_names.open_settings",
        "toggle_ignore_mouse_input" => "shortcuts.command_names.toggle_ignore_mouse_input",
        "toggle_ignore_keyboard_input" => "shortcuts.command_names.toggle_ignore_keyboard_input",
        "toggle_ignore_gamepad_input" => "shortcuts.command_names.toggle_ignore_gamepad_input",
        _ => return command.to_owned(),
    };
    bongocat_i18n::text(language.catalog_locale(), key).to_owned()
}

/// The label of one model behavior.
///
/// A behavior is named by its position, not by its resource identity. The
/// package declares it as `motion:CAT_motion:0` or
/// `expression:live2d_expression0.exp3.json`, which is an internal spelling and
/// reads as noise in a settings list; a user picking a chord for "the second
/// motion" needs a name they can count, not the motion group the clip happens
/// to live in.
///
/// `ordinal` is the behavior's flattened one-based position, so the numbering
/// stays continuous across groups and can never repeat inside one kind. Both
/// locale keys are spelled out rather than selected from a table: the source
/// scan in `bongocat-i18n` only sees literals.
pub(super) fn shortcut_behavior_name(
    language: SettingsLanguage,
    ordinal: BehaviorOrdinal,
) -> String {
    let number = ordinal.number.to_string();
    match ordinal.kind {
        BehaviorKind::Motion => bongocat_i18n::format_text(
            language.catalog_locale(),
            "shortcuts.behavior_names.motion",
            &[("index", number)],
        ),
        BehaviorKind::Expression => bongocat_i18n::format_text(
            language.catalog_locale(),
            "shortcuts.behavior_names.expression",
            &[("index", number)],
        ),
    }
}

pub(super) fn settings_error(language: SettingsLanguage, error: SettingsError) -> &'static str {
    // The enum names are stable protocol identifiers; these presentation paths deliberately use
    // the terms shown to the user. The key is spelled out in full rather than assembled from a suffix.
    // The source scan in
    // `bongocat-i18n` only sees literals, so a suffix would leave all of these keys unchecked.
    let key = match error.code() {
        SettingsErrorCode::ServiceUnavailable => "errors.settings.service_unavailable",
        SettingsErrorCode::SnapshotOutdated => "errors.settings.snapshot_outdated",
        SettingsErrorCode::RuntimeUnavailable => "errors.settings.setting_not_applied",
        SettingsErrorCode::InvalidShortcutBindings => "errors.settings.invalid_shortcut_bindings",
        SettingsErrorCode::ConfigPersistFailed => "errors.settings.setting_save_failed",
        SettingsErrorCode::ConfigPermissionDenied => "errors.settings.config_permission_denied",
        SettingsErrorCode::ConfigStorageFull => "errors.settings.config_storage_full",
        SettingsErrorCode::ConfigTargetOccupied => "errors.settings.config_target_occupied",
        SettingsErrorCode::BackupLocationOpenFailed => {
            "errors.settings.configuration_backup_open_failed"
        }
        SettingsErrorCode::ModelUnavailable => "errors.settings.model_unavailable",
        SettingsErrorCode::ModelSwitchFailed => "errors.settings.model_activation_failed",
        SettingsErrorCode::ModelBehaviorPreviewUnavailable => {
            "errors.settings.model_behavior_unavailable"
        }
        SettingsErrorCode::ModelBehaviorPreviewFailed => {
            "errors.settings.model_behavior_play_failed"
        }
        SettingsErrorCode::ModelTitleInvalid => "errors.settings.model_name_invalid",
        SettingsErrorCode::ModelCoverInvalid => "errors.settings.model_cover_invalid",
        SettingsErrorCode::ModelCoverUpdateFailed => "errors.settings.model_cover_update_failed",
        SettingsErrorCode::ModelSourcePickerUnavailable => {
            "errors.settings.model_file_picker_unavailable"
        }
        SettingsErrorCode::ModelLocationOpenFailed => "errors.settings.model_folder_open_failed",
        SettingsErrorCode::InvalidModelId => "errors.settings.model_id_invalid",
        SettingsErrorCode::ModelAlreadyInstalled => "errors.settings.model_already_installed",
        SettingsErrorCode::ModelImportInvalidPackage => {
            "errors.settings.model_import_invalid_package"
        }
        SettingsErrorCode::ModelImportDropInvalid => "errors.settings.model_import_drop_invalid",
        SettingsErrorCode::ModelImportSourceInvalid => {
            "errors.settings.model_import_source_invalid"
        }
        SettingsErrorCode::ModelImportSourceChanged => {
            "errors.settings.model_import_source_changed"
        }
        SettingsErrorCode::ModelImportSourceUnsupported => {
            "errors.settings.model_import_source_unsupported"
        }
        SettingsErrorCode::ModelImportCancelled => "errors.settings.model_import_cancelled",
        SettingsErrorCode::ModelStoreBusy => "errors.settings.model_operations_busy",
        SettingsErrorCode::ModelImportFailed => "errors.settings.model_import_failed",
        SettingsErrorCode::PresetModelCannotBeDeleted => {
            "errors.settings.built_in_model_cannot_be_deleted"
        }
        SettingsErrorCode::ModelNotFound => "errors.settings.model_not_found",
        SettingsErrorCode::ModelDeleteFailed => "errors.settings.model_delete_failed",
        SettingsErrorCode::DiagnosticsExportFailed => "errors.settings.diagnostics_export_failed",
        SettingsErrorCode::SoftwareInfoCopyFailed => "errors.settings.software_info_copy_failed",
        SettingsErrorCode::ExternalLinkOpenFailed => "errors.settings.external_link_open_failed",
        SettingsErrorCode::LogLocationOpenFailed => "errors.settings.log_location_open_failed",
        SettingsErrorCode::StartupItemUpdateFailed => "errors.settings.login_startup_update_failed",
        SettingsErrorCode::StatusIconUpdateFailed => "errors.settings.system_icon_update_failed",
        SettingsErrorCode::TaskbarIconUpdateFailed => "errors.settings.taskbar_icon_update_failed",
        SettingsErrorCode::DockIconUpdateFailed => "errors.settings.dock_icon_update_failed",
        SettingsErrorCode::WindowHideFailed => "errors.settings.window_hide_failed",
        SettingsErrorCode::WindowStatePersistFailed => "errors.settings.window_layout_save_failed",
        SettingsErrorCode::ShutdownFailed => "errors.settings.application_shutdown_failed",
    };
    // The catalog is compile-time embedded; the returned string is leaked and
    // cached by the i18n facade just like every other UI message.
    bongocat_i18n::text(language.catalog_locale(), key)
}

pub(super) fn shortcut_conflict_message(language: SettingsLanguage, shortcut: &str) -> String {
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "shortcuts.errors.conflict",
        &[("shortcut", shortcut.to_owned())],
    )
}

/// The one notification message for a failed plugin command.
///
/// The plugin is named by the copy its own manifest publishes, falling back to
/// its id only when no card is available. The operation is always named, so a
/// failure reads as "plugin update failed" rather than the unhelpful "load
/// failed" that could belong to any subsystem.
pub(super) fn plugin_failure_message(
    language: SettingsLanguage,
    report: &PluginFailureReport,
    cards: &[PluginCardView],
) -> String {
    let locale = language.catalog_locale();
    let plugin_name = |id: &str| {
        cards
            .iter()
            .find(|card| card.id == id)
            .map_or(id, |card| card.copy(locale).0)
            .to_owned()
    };
    let message = match &report.failure {
        PluginFailure::Refresh => {
            bongocat_i18n::text(locale, "plugins.discovery_failed").to_owned()
        }
        PluginFailure::Load { plugin, running } => bongocat_i18n::format_text(
            locale,
            if *running {
                "plugins.errors.runtime_failed"
            } else {
                "plugins.errors.load_failed"
            },
            &[("name", plugin_name(plugin))],
        ),
        PluginFailure::Reported {
            plugin, message, ..
        } => bongocat_i18n::format_text(
            locale,
            "plugins.errors.reported",
            &[("name", plugin_name(plugin)), ("reason", message.clone())],
        ),
        PluginFailure::Preview { plugin, .. } => bongocat_i18n::format_text(
            locale,
            "plugins.errors.preview_failed",
            &[("name", plugin_name(plugin))],
        ),
        PluginFailure::Install { plugin, update } => {
            let key = if *update {
                "plugins.errors.update_failed"
            } else {
                "plugins.errors.install_failed"
            };
            bongocat_i18n::format_text(locale, key, &[("name", plugin_name(plugin))])
        }
        PluginFailure::Enable { plugin, enabled } => {
            let key = if *enabled {
                "plugins.errors.enable_failed"
            } else {
                "plugins.errors.disable_failed"
            };
            bongocat_i18n::format_text(locale, key, &[("name", plugin_name(plugin))])
        }
        PluginFailure::Uninstall { plugin } => bongocat_i18n::format_text(
            locale,
            "plugins.errors.uninstall_failed",
            &[("name", plugin_name(plugin))],
        ),
        PluginFailure::Action { plugin, .. } => bongocat_i18n::format_text(
            locale,
            "plugins.errors.action_failed",
            &[("name", plugin_name(plugin))],
        ),
        PluginFailure::Configure { plugin, .. } => bongocat_i18n::format_text(
            locale,
            "plugins.errors.configure_failed",
            &[("name", plugin_name(plugin))],
        ),
        PluginFailure::UpdateAll { failed } => {
            let names = failed
                .iter()
                .map(|id| plugin_name(id))
                .collect::<Vec<_>>()
                .join(", ");
            bongocat_i18n::format_text(
                locale,
                "plugins.errors.update_all_failed",
                &[("names", names)],
            )
        }
    };
    match report.code {
        PluginCommandError::ServiceUnavailable => bongocat_i18n::format_text(
            locale,
            "plugins.errors.with_reason",
            &[
                ("message", message),
                (
                    "reason",
                    bongocat_i18n::text(locale, "plugins.errors.service_unavailable").to_owned(),
                ),
            ],
        ),
        PluginCommandError::NotFound => bongocat_i18n::format_text(
            locale,
            "plugins.errors.with_reason",
            &[
                ("message", message),
                (
                    "reason",
                    bongocat_i18n::text(locale, "plugins.errors.not_installed").to_owned(),
                ),
            ],
        ),
        PluginCommandError::NotRunning => bongocat_i18n::format_text(
            locale,
            "plugins.errors.with_reason",
            &[
                ("message", message),
                (
                    "reason",
                    bongocat_i18n::text(locale, "plugins.errors.not_running").to_owned(),
                ),
            ],
        ),
        PluginCommandError::Operation => message,
    }
}

/// The notification slot one failure belongs to.
pub(super) fn plugin_failure_notification_id(failure: &PluginFailure) -> SharedString {
    match failure {
        PluginFailure::Refresh => "plugin-refresh".into(),
        PluginFailure::Load { plugin, .. } => format!("plugin-load-{plugin}").into(),
        PluginFailure::Reported { plugin, id, .. } => {
            format!("plugin-reported-{plugin}-{id}").into()
        }
        PluginFailure::Preview { plugin, key } => format!("plugin-preview-{plugin}-{key}").into(),
        PluginFailure::Install { plugin, .. } => format!("plugin-install-{plugin}").into(),
        PluginFailure::Enable { plugin, enabled } => {
            format!("plugin-enable-{plugin}-{enabled}").into()
        }
        PluginFailure::Uninstall { plugin } => format!("plugin-uninstall-{plugin}").into(),
        PluginFailure::Action { plugin, action } => {
            format!("plugin-action-{plugin}-{action}").into()
        }
        PluginFailure::Configure { plugin, key } => {
            format!("plugin-configure-{plugin}-{key}").into()
        }
        PluginFailure::UpdateAll { .. } => "plugin-update-all".into(),
    }
}

pub(super) fn model_invalid_summary(
    language: SettingsLanguage,
    origin: SettingsModelOrigin,
    diagnostic: &str,
) -> String {
    let origin = bongocat_i18n::text(
        language.catalog_locale(),
        match origin {
            SettingsModelOrigin::BuiltIn => "models.identity.source.built_in",
            SettingsModelOrigin::Imported => "models.identity.source.imported",
        },
    );
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "models.invalid_summary",
        &[
            ("origin", origin.to_owned()),
            ("diagnostic", diagnostic.to_owned()),
        ],
    )
}
