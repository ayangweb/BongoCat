use super::{
    BehaviorKind, BehaviorOrdinal, SettingsBuildEnvironment, SettingsBuildInfo, SettingsError,
    SettingsErrorCode, SettingsLanguage, SettingsModelOrigin,
};

pub(super) fn build_info_detail(
    language: SettingsLanguage,
    build_info: &SettingsBuildInfo,
) -> String {
    let environment = match build_info.environment {
        SettingsBuildEnvironment::Development => "diagnostics.build.environment.development",
        SettingsBuildEnvironment::Production => "diagnostics.build.environment.production",
    };
    format!(
        "{} {} · {}",
        bongocat_i18n::text(language.catalog_locale(), "diagnostics.build.version"),
        build_info.product_version,
        bongocat_i18n::text(language.catalog_locale(), environment)
    )
}

pub(super) fn shortcut_command_name(language: SettingsLanguage, command: &str) -> String {
    let key = match command {
        "toggle_overlay" => "shortcuts.command_names.toggle_overlay",
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
    // The key is spelled out in full rather than assembled from a suffix. The source scan in
    // `bongocat-i18n` only sees literals, so a suffix would leave all of these keys unchecked.
    let key = match error.code() {
        SettingsErrorCode::ServiceUnavailable => "errors.settings.service_unavailable",
        SettingsErrorCode::SnapshotOutdated => "errors.settings.snapshot_outdated",
        SettingsErrorCode::RuntimeUnavailable => "errors.settings.runtime_unavailable",
        SettingsErrorCode::InvalidShortcutBindings => "errors.settings.invalid_shortcut_bindings",
        SettingsErrorCode::ConfigPersistFailed => "errors.settings.config_persist_failed",
        SettingsErrorCode::ConfigPermissionDenied => "errors.settings.config_permission_denied",
        SettingsErrorCode::ConfigStorageFull => "errors.settings.config_storage_full",
        SettingsErrorCode::ConfigTargetOccupied => "errors.settings.config_target_occupied",
        SettingsErrorCode::BackupLocationOpenFailed => {
            "errors.settings.backup_location_open_failed"
        }
        SettingsErrorCode::ModelUnavailable => "errors.settings.model_unavailable",
        SettingsErrorCode::ModelSwitchFailed => "errors.settings.model_switch_failed",
        SettingsErrorCode::ModelTitleInvalid => "errors.settings.model_title_invalid",
        SettingsErrorCode::PresetModelMetadataImmutable => {
            "errors.settings.preset_model_metadata_immutable"
        }
        SettingsErrorCode::ModelCoverInvalid => "errors.settings.model_cover_invalid",
        SettingsErrorCode::ModelCoverUpdateFailed => "errors.settings.model_cover_update_failed",
        SettingsErrorCode::ModelSourcePickerUnavailable => {
            "errors.settings.model_source_picker_unavailable"
        }
        SettingsErrorCode::ModelLocationOpenFailed => "errors.settings.model_location_open_failed",
        SettingsErrorCode::InvalidModelId => "errors.settings.invalid_model_id",
        SettingsErrorCode::ModelAlreadyInstalled => "errors.settings.model_already_installed",
        SettingsErrorCode::ModelImportInvalidPackage => {
            "errors.settings.model_import_invalid_package"
        }
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
        SettingsErrorCode::ModelStoreBusy => "errors.settings.model_store_busy",
        SettingsErrorCode::ModelImportFailed => "errors.settings.model_import_failed",
        SettingsErrorCode::PresetModelCannotBeDeleted => {
            "errors.settings.preset_model_cannot_be_deleted"
        }
        SettingsErrorCode::ModelNotInstalled => "errors.settings.model_not_installed",
        SettingsErrorCode::ModelDeleteFailed => "errors.settings.model_delete_failed",
        SettingsErrorCode::DiagnosticsExportFailed => "errors.settings.diagnostics_export_failed",
        SettingsErrorCode::StartupItemUpdateFailed => "errors.settings.startup_item_update_failed",
        SettingsErrorCode::StatusIconUpdateFailed => "errors.settings.status_icon_update_failed",
        SettingsErrorCode::TaskbarIconUpdateFailed => "errors.settings.taskbar_icon_update_failed",
        SettingsErrorCode::WindowHideFailed => "errors.settings.window_hide_failed",
        SettingsErrorCode::StatePersistFailed => "errors.settings.state_persist_failed",
        SettingsErrorCode::ShutdownFailed => "errors.settings.shutdown_failed",
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
            SettingsModelOrigin::Preset => "models.identity.source.preset",
            SettingsModelOrigin::Installed => "models.identity.source.installed",
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

#[cfg(test)]
pub(super) fn model_import_progress(
    language: SettingsLanguage,
    stage: &str,
    files_copied: u64,
    bytes_copied: u64,
) -> String {
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "models.import.progress.detail",
        &[
            ("stage", stage.to_owned()),
            ("files_copied", files_copied.to_string()),
            ("bytes_copied", bytes_copied.to_string()),
        ],
    )
}

pub(super) fn runtime_status(language: SettingsLanguage, health: &str, revision: u64) -> String {
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "settings.runtime.status_detail",
        &[
            ("health", health.to_owned()),
            ("revision", revision.to_string()),
        ],
    )
}
