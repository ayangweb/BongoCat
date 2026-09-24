use super::{
    BehaviorKind, BehaviorOrdinal, SettingsBuildEnvironment, SettingsBuildInfo, SettingsError,
    SettingsErrorCode, SettingsLanguage, SettingsModelOrigin,
};

pub(super) fn build_info_detail(
    language: SettingsLanguage,
    build_info: &SettingsBuildInfo,
) -> String {
    let environment = match build_info.environment {
        SettingsBuildEnvironment::Development => {
            "about.product_information.version.environment.development"
        }
        SettingsBuildEnvironment::Production => {
            "about.product_information.version.environment.release"
        }
    };
    format!(
        "{} {} · {}",
        bongocat_i18n::text(
            language.catalog_locale(),
            "about.product_information.version.label",
        ),
        build_info.product_version,
        bongocat_i18n::text(language.catalog_locale(), environment)
    )
}

pub(super) fn shortcut_command_name(language: SettingsLanguage, command: &str) -> String {
    let key = match command {
        "toggle_overlay" => "shortcuts.command_names.toggle_model_window",
        "open_settings" => "shortcuts.command_names.open_settings",
        "toggle_mirror" => "shortcuts.command_names.toggle_mirror",
        "toggle_click_through" => "shortcuts.command_names.toggle_click_through",
        "toggle_always_on_top" => "shortcuts.command_names.toggle_always_on_top",
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
        SettingsErrorCode::StartupItemUpdateFailed => "errors.settings.login_startup_update_failed",
        SettingsErrorCode::StatusIconUpdateFailed => "errors.settings.system_icon_update_failed",
        SettingsErrorCode::TaskbarIconUpdateFailed => "errors.settings.taskbar_icon_update_failed",
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

pub(super) fn model_invalid_summary(
    language: SettingsLanguage,
    origin: SettingsModelOrigin,
    diagnostic: &str,
) -> String {
    let origin = bongocat_i18n::text(
        language.catalog_locale(),
        match origin {
            SettingsModelOrigin::Preset => "models.identity.source.built_in",
            SettingsModelOrigin::Installed => "models.identity.source.imported",
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
