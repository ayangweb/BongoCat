use super::{
    SettingsBuildEnvironment, SettingsBuildInfo, SettingsError, SettingsErrorCode,
    SettingsInputDiagnostics, SettingsLanguage, SettingsModelOrigin, ShortcutCaptureTarget,
};
use crate::SettingsDiagnosticsExportStatus;

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

pub(super) fn diagnostics_export_status(
    language: SettingsLanguage,
    status: Option<SettingsDiagnosticsExportStatus>,
) -> String {
    let Some(status) = status else {
        return bongocat_i18n::text(language.catalog_locale(), "diagnostics.export.none")
            .to_owned();
    };
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "diagnostics.export.summary",
        &[
            ("format_version", status.format_version.to_string()),
            ("bytes_written", status.bytes_written.to_string()),
            (
                "preview_bundle_format_version",
                status.preview_bundle_format_version.to_string(),
            ),
            (
                "preview_bundle_entry_count",
                status.preview_bundle_entry_count.to_string(),
            ),
            (
                "preview_bundle_bytes_written",
                status.preview_bundle_bytes_written.to_string(),
            ),
            (
                "skipped_source_files",
                status.preview_bundle_skipped_source_files.to_string(),
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

pub(super) fn input_service_attempts(language: SettingsLanguage, attempts: u64) -> String {
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "diagnostics.input.start_attempts",
        &[("attempts", attempts.to_string())],
    )
}

pub(super) fn runtime_command_failure(
    language: SettingsLanguage,
    error: &str,
    sequence: u64,
) -> String {
    bongocat_i18n::format_text(
        language.catalog_locale(),
        "diagnostics.runtime.command_failure",
        &[
            ("error", error.to_owned()),
            ("sequence", sequence.to_string()),
        ],
    )
}

pub(super) fn runtime_shutdown_failures(language: SettingsLanguage, count: u64) -> String {
    bongocat_i18n::count_text(
        language.catalog_locale(),
        "diagnostics.runtime.shutdown_failures",
        count,
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

pub(super) fn input_diagnostic_metrics(
    language: SettingsLanguage,
    diagnostics: SettingsInputDiagnostics,
) -> [(&'static str, u64); 26] {
    let labels = [
        "pressed_keys",
        "pressed_mouse_buttons",
        "pressed_gamepad_buttons",
        "connected_gamepads",
        "captured_presses",
        "captured_releases",
        "reconciled_releases",
        "fallback_releases",
        "reset_releases",
        "duplicate_presses",
        "unmatched_releases",
        "invalid_sources",
        "resets",
        "sequence_gaps",
        "missing_events",
        "duplicate_events",
        "out_of_order_events",
        "non_monotonic_timestamps",
        "gamepad_connections",
        "gamepad_disconnections",
        "stale_gamepad_events",
        "released_on_disconnect",
        "events_enqueued",
        "queue_overflows",
        "overflow_recoveries",
        "rejected_after_shutdown",
    ];
    let values = [
        diagnostics.pressed_key_count as u64,
        diagnostics.pressed_mouse_button_count as u64,
        diagnostics.pressed_gamepad_button_count as u64,
        diagnostics.connected_gamepad_count as u64,
        diagnostics.captured_down,
        diagnostics.captured_up,
        diagnostics.reconciled_release,
        diagnostics.fallback_release,
        diagnostics.released_by_reset,
        diagnostics.duplicate_down,
        diagnostics.unmatched_release,
        diagnostics.invalid_source,
        diagnostics.reset_count,
        diagnostics.sequence_gap_count,
        diagnostics.missing_sequence_count,
        diagnostics.duplicate_sequence_count,
        diagnostics.out_of_order_sequence_count,
        diagnostics.non_monotonic_time_count,
        diagnostics.gamepad_connections,
        diagnostics.gamepad_disconnections,
        diagnostics.stale_gamepad_events,
        diagnostics.released_by_disconnect,
        diagnostics.transport_enqueued,
        diagnostics.transport_queue_full,
        diagnostics.transport_recovered_after_overflow,
        diagnostics.transport_runtime_stopped,
    ];
    std::array::from_fn(|index| {
        (
            bongocat_i18n::text(
                language.catalog_locale(),
                &format!("diagnostics.input.metrics.{}", labels[index]),
            ),
            values[index],
        )
    })
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
        SettingsErrorCode::ModelBehaviorPreviewUnavailable => "model_behavior_preview_unavailable",
        SettingsErrorCode::ModelBehaviorPreviewFailed => "model_behavior_preview_failed",
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
        SettingsErrorCode::WindowUnavailable => "window_unavailable",
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
