use super::{
    SettingsBuildEnvironment, SettingsBuildInfo, SettingsError, SettingsErrorCode,
    SettingsLanguage, SettingsModelOrigin, ShortcutCaptureTarget,
};

pub(super) fn model_availability_summary(
    language: SettingsLanguage,
    origin: SettingsModelOrigin,
    active: bool,
    texture_count: usize,
    expression_count: usize,
    motion_count: usize,
) -> String {
    let origin = bongocat_i18n::text(
        language.catalog_locale(),
        match origin {
            SettingsModelOrigin::Preset => "models.identity.source.preset",
            SettingsModelOrigin::Installed => "models.identity.source.installed",
        },
    );
    let key = if active {
        "models.summary.active"
    } else {
        "models.summary.inactive"
    };
    bongocat_i18n::format_text(
        language.catalog_locale(),
        key,
        &[
            ("origin", origin.to_owned()),
            ("texture_count", texture_count.to_string()),
            ("expression_count", expression_count.to_string()),
            ("motion_count", motion_count.to_string()),
            (
                "active",
                bongocat_i18n::text(language.catalog_locale(), "models.identity.status.active")
                    .to_owned(),
            ),
        ],
    )
}

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

pub(super) fn backup_candidates_checked(
    language: SettingsLanguage,
    checked_backups: u32,
) -> String {
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "diagnostics.configuration.backup_candidates",
        &[
            ("count", checked_backups.to_string()),
            (
                "plural_suffix",
                if !matches!(language, SettingsLanguage::ChineseSimplified) && checked_backups != 1
                {
                    "s".to_owned()
                } else {
                    String::new()
                },
            ),
        ],
    )
}

pub(super) fn recovered_backup_detail(
    language: SettingsLanguage,
    schema_version: u32,
    skipped_newer_backups: u32,
) -> String {
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "diagnostics.configuration.recovered_backup_detail",
        &[
            ("schema_version", schema_version.to_string()),
            ("count", skipped_newer_backups.to_string()),
            (
                "plural_suffix",
                if !matches!(language, SettingsLanguage::ChineseSimplified)
                    && skipped_newer_backups != 1
                {
                    "s".to_owned()
                } else {
                    String::new()
                },
            ),
        ],
    )
}

pub(super) fn shortcut_accessibility_label(
    language: SettingsLanguage,
    target: &ShortcutCaptureTarget,
) -> String {
    let name = shortcut_target_name(language, target);
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "shortcuts.capture.accessibility",
        &[("name", name)],
    )
}

pub(super) fn shortcut_target_name(
    language: SettingsLanguage,
    target: &ShortcutCaptureTarget,
) -> String {
    match target {
        ShortcutCaptureTarget::Command(command) => {
            let key = match command.as_str() {
                "toggle_overlay" => "shortcuts.command_names.toggle_overlay",
                "open_settings" => "shortcuts.command_names.open_settings",
                "toggle_mirror" => "shortcuts.command_names.toggle_mirror",
                "toggle_click_through" => "shortcuts.command_names.toggle_click_through",
                "toggle_always_on_top" => "shortcuts.command_names.toggle_always_on_top",
                _ => return command.clone(),
            };
            bongocat_i18n::text(language.catalog_locale(), key).to_owned()
        }
        ShortcutCaptureTarget::ModelBehavior {
            model_id,
            behavior_id,
        } => format!("{model_id} ({behavior_id})"),
    }
}

pub(super) fn settings_error(language: SettingsLanguage, error: SettingsError) -> &'static str {
    let suffix = match error.code() {
        SettingsErrorCode::ServiceUnavailable => "service_unavailable",
        SettingsErrorCode::SnapshotOutdated => "snapshot_outdated",
        SettingsErrorCode::RuntimeUnavailable => "runtime_unavailable",
        SettingsErrorCode::InvalidMaximumFps => "invalid_maximum_fps",
        SettingsErrorCode::InvalidReleaseFallbackTimeout => "invalid_release_fallback_timeout",
        SettingsErrorCode::InvalidGamepadAxisSettings => "invalid_gamepad_axis_settings",
        SettingsErrorCode::InvalidShortcutBindings => "invalid_shortcut_bindings",
        SettingsErrorCode::ConfigPersistFailed => "config_persist_failed",
        SettingsErrorCode::ConfigPermissionDenied => "config_permission_denied",
        SettingsErrorCode::ConfigStorageFull => "config_storage_full",
        SettingsErrorCode::ConfigTargetOccupied => "config_target_occupied",
        SettingsErrorCode::BackupLocationOpenFailed => "backup_location_open_failed",
        SettingsErrorCode::ConfigurationRecoveryRequired => "configuration_recovery_required",
        SettingsErrorCode::ConfigurationRecoveryFailed => "configuration_recovery_failed",
        SettingsErrorCode::ModelUnavailable => "model_unavailable",
        SettingsErrorCode::ModelSwitchFailed => "model_switch_failed",
        SettingsErrorCode::ModelTitleInvalid => "model_title_invalid",
        SettingsErrorCode::PresetModelMetadataImmutable => "preset_model_metadata_immutable",
        SettingsErrorCode::ModelCoverInvalid => "model_cover_invalid",
        SettingsErrorCode::ModelCoverUpdateFailed => "model_cover_update_failed",
        SettingsErrorCode::ModelSourcePickerUnavailable => "model_source_picker_unavailable",
        SettingsErrorCode::ModelLocationOpenFailed => "model_location_open_failed",
        SettingsErrorCode::InvalidModelId => "invalid_model_id",
        SettingsErrorCode::ModelAlreadyInstalled => "model_already_installed",
        SettingsErrorCode::ModelImportInvalidPackage => "model_import_invalid_package",
        SettingsErrorCode::ModelImportSourceInvalid => "model_import_source_invalid",
        SettingsErrorCode::ModelImportSourceChanged => "model_import_source_changed",
        SettingsErrorCode::ModelImportSourceUnsupported => "model_import_source_unsupported",
        SettingsErrorCode::ModelImportCancelled => "model_import_cancelled",
        SettingsErrorCode::ModelStoreBusy => "model_store_busy",
        SettingsErrorCode::ModelImportFailed => "model_import_failed",
        SettingsErrorCode::PresetModelCannotBeDeleted => "preset_model_cannot_be_deleted",
        SettingsErrorCode::SelectedModelCannotBeDeleted => "selected_model_cannot_be_deleted",
        SettingsErrorCode::ModelNotInstalled => "model_not_installed",
        SettingsErrorCode::ModelDeleteFailed => "model_delete_failed",
        SettingsErrorCode::DiagnosticsExportFailed => "diagnostics_export_failed",
        SettingsErrorCode::StartupItemUpdateFailed => "startup_item_update_failed",
        SettingsErrorCode::StatusIconUpdateFailed => "status_icon_update_failed",
        SettingsErrorCode::TaskbarIconUpdateFailed => "taskbar_icon_update_failed",
        SettingsErrorCode::WindowHideFailed => "window_hide_failed",
        SettingsErrorCode::StatePersistFailed => "state_persist_failed",
        SettingsErrorCode::ShutdownFailed => "shutdown_failed",
    };
    let key = format!("errors.settings.{suffix}");
    // The catalog is compile-time embedded; the returned string is leaked and
    // cached by the i18n facade just like every other UI message.
    bongocat_i18n::text(language.catalog_locale(), &key)
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

pub(super) fn model_delete_confirmation(language: SettingsLanguage, status: &str) -> String {
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "models.delete_confirmation",
        &[
            ("status", status.to_owned()),
            (
                "confirm_deletion",
                bongocat_i18n::text(language.catalog_locale(), "models.actions.confirm_deletion")
                    .to_owned(),
            ),
        ],
    )
}

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
