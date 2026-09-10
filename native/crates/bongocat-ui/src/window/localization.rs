use super::{
    SettingsBuildEnvironment, SettingsBuildInfo, SettingsError, SettingsErrorCode,
    SettingsInputDiagnostics, SettingsLanguage, SettingsModelOrigin, ShortcutCaptureTarget,
};
use crate::SettingsDiagnosticsExportStatus;

#[derive(Clone, Copy)]
pub(super) enum UiText {
    Settings,
    General,
    GeneralDescription,
    Models,
    ModelsDescription,
    About,
    AboutDescription,
    AboutBongoCat,
    ProductInformation,
    ProductInformationDescription,
    ApplicationLicense,
    ApplicationLicenseDescription,
    ThirdPartyLicenses,
    ThirdPartyLicensesDescription,
    CubismAttribution,
    CubismAttributionDescription,
    Privacy,
    PrivacyDescription,
    ModelCatalog,
    InstalledModels,
    InstalledModelsDescription,
    ModelId,
    Preset,
    Installed,
    Active,
    Unavailable,
    Activate,
    Cancel,
    Confirm,
    Delete,
    ConfirmDeletion,
    PackageLayoutInvalid,
    PackageSafetyLimitsExceeded,
    ModelDefinitionUnsupported,
    TextureInvalid,
    ModelFilesUnavailable,
    ModelResourceInvalid,
    NoFolderSelected,
    FolderSelected,
    ChoosingFolder,
    SelectionCancelledPreviousRetained,
    SelectionCancelled,
    FolderPickerRequiresUiThread,
    SelectedFolderUnavailable,
    FolderPickerUnavailable,
    CancellingImport,
    StartingImport,
    Preparing,
    Copying,
    Validating,
    Committing,
    ImportComplete,
    ImportCancelled,
    LoadingModels,
    ModelCatalogUnavailable,
    NoModelsAvailable,
    ActivatingModel,
    DeletingModel,
    RefreshingModels,
    CatalogUnavailable,
    Choosing,
    ChooseFolder,
    Import,
    AvailableModels,
    ModelBehaviors,
    NoModelBehaviors,
    Motion,
    Expression,
    Preview,
    PreviewingBehavior,
    Diagnostics,
    DiagnosticsDescription,
    RuntimeAndInput,
    RuntimeDiagnostics,
    RuntimeDiagnosticsDescription,
    LoadingDiagnostics,
    InputReliabilityCounters,
    RuntimeRenderer,
    BuildInformation,
    Version,
    Development,
    Production,
    DiagnosticsExport,
    NoReportExported,
    Export,
    Shortcuts,
    ShortcutsDescription,
    WindowShortcuts,
    ModelShortcuts,
    RestoreDefaults,
    ClearAll,
    PressCommandShortcut,
    PressBehaviorShortcut,
    PressRecordShortcut,
    ClickRecordShortcut,
    PressKey,
    Capture,
    Clear,
    NotSet,
    InputService,
    InputMonitoring,
    Configuration,
    Backups,
    CurrentState,
    InputProcessing,
    SequenceRecovery,
    Transport,
    RestoreDefaultConfiguration,
    RestoreDefaultConfigurationDescription,
    OpenConfigurationBackupsFolder,
    OpenConfigurationBackupsFolderDescription,
    ExportDiagnostics,
    ExportDiagnosticsDescription,
    RestoreDefaultShortcuts,
    RestoreDefaultShortcutsDescription,
    ClearAllShortcuts,
    ClearAllShortcutsDescription,
    WaitingForKeyCombination,
    NotStarted,
    Running,
    PermissionRequired,
    Granted,
    Unsupported,
    BackendUnavailable,
    StartupFailed,
    GpuPreparationFailed,
    ModelLoadFailed,
    ModelEvaluationFailed,
    MotionLoadFailed,
    ExpressionLoadFailed,
    PlatformUnsupported,
    RuntimeTransportClosed,
    OverlaySettingsInvalid,
    MaximumFpsInvalid,
    ReleaseFallbackTimeoutInvalid,
    NoRendererError,
    NoCommandFailures,
    RuntimeShutdownFailures,
    ConfigurationUnavailable,
    DefaultsRestored,
    RestartToContinue,
    RecoveredFromBackup,
    LoadedNormally,
    NoRecovery,
    Appearance,
    Overlay,
    Theme,
    System,
    Light,
    Dark,
    ThemeDescription,
    Language,
    LanguageDescription,
    RuntimeStatus,
    RuntimeStatusDescription,
    ShowDesktopCat,
    ShowDesktopCatDescription,
    AlwaysOnTop,
    AlwaysOnTopDescription,
    ClickThroughOverlay,
    ClickThroughOverlayDescription,
    KeepInsideWorkArea,
    KeepInsideWorkAreaDescription,
    MotionAudio,
    MotionAudioDescription,
    OverlayScale,
    OverlayScaleDescription,
    OverlayOpacity,
    OverlayOpacityDescription,
    MaximumFps,
    MaximumFpsDescription,
    ReleaseFallbackTimeout,
    ReleaseFallbackTimeoutDescription,
    ModelInteraction,
    BehaviorShortcuts,
    BehaviorShortcutsDescription,
    MirrorModel,
    MirrorModelDescription,
    MirrorPointerTracking,
    MirrorPointerTrackingDescription,
    IgnorePointerInput,
    IgnorePointerInputDescription,
    Input,
    GamepadStickDeadZone,
    GamepadStickDeadZoneDescription,
    GamepadTriggerDeadZone,
    GamepadTriggerDeadZoneDescription,
    Application,
    ShowStatusIcon,
    ShowStatusIconDescription,
    #[cfg(target_os = "windows")]
    ShowTaskbarIcon,
    #[cfg(target_os = "windows")]
    ShowTaskbarIconDescription,
    CheckForUpdatesAutomatically,
    CheckForUpdatesAutomaticallyDescription,
    OpenAtLogin,
    DecreaseOverlayScale,
    IncreaseOverlayScale,
    DecreaseOverlayOpacity,
    IncreaseOverlayOpacity,
    DecreaseMaximumFps,
    IncreaseMaximumFps,
    DecreaseReleaseFallbackTimeout,
    IncreaseReleaseFallbackTimeout,
    CheckingLoginStartup,
    LoginStartupStatusUnavailable,
    LoginStartupDisabled,
    LoginStartupEnabled,
    LoginStartupStale,
    LoginStartupRequiresApproval,
    LoginStartupNotFound,
    LoginStartupUnsupportedPlatform,
    LoginStartupUnsupportedOperatingSystem,
    LoginStartupUnsupportedBuild,
    Refreshing,
    Saving,
    Connecting,
    Starting,
    Ready,
    Degraded,
    Stopped,
    Refresh,
    Quit,
}

#[derive(Clone, Copy)]
pub(super) struct AboutSection {
    pub(super) title: UiText,
    pub(super) description: UiText,
}

pub(super) const ABOUT_SECTIONS: [AboutSection; 4] = [
    AboutSection {
        title: UiText::ApplicationLicense,
        description: UiText::ApplicationLicenseDescription,
    },
    AboutSection {
        title: UiText::ThirdPartyLicenses,
        description: UiText::ThirdPartyLicensesDescription,
    },
    AboutSection {
        title: UiText::CubismAttribution,
        description: UiText::CubismAttributionDescription,
    },
    AboutSection {
        title: UiText::Privacy,
        description: UiText::PrivacyDescription,
    },
];

pub(super) fn text(language: SettingsLanguage, key: UiText) -> &'static str {
    let locale = match language {
        SettingsLanguage::ChineseSimplified => "zh-CN",
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => "en-US",
    };
    bongocat_i18n::text(locale, key.key())
}

impl UiText {
    const fn key(self) -> &'static str {
        match self {
            Self::Settings => "ui.settings",
            Self::General => "ui.general",
            Self::GeneralDescription => "ui.general_description",
            Self::Models => "ui.models",
            Self::ModelsDescription => "ui.models_description",
            Self::About => "ui.about",
            Self::AboutDescription => "ui.about_description",
            Self::AboutBongoCat => "ui.about_bongo_cat",
            Self::ProductInformation => "ui.product_information",
            Self::ProductInformationDescription => "ui.product_information_description",
            Self::ApplicationLicense => "ui.application_license",
            Self::ApplicationLicenseDescription => "ui.application_license_description",
            Self::ThirdPartyLicenses => "ui.third_party_licenses",
            Self::ThirdPartyLicensesDescription => "ui.third_party_licenses_description",
            Self::CubismAttribution => "ui.cubism_attribution",
            Self::CubismAttributionDescription => "ui.cubism_attribution_description",
            Self::Privacy => "ui.privacy",
            Self::PrivacyDescription => "ui.privacy_description",
            Self::ModelCatalog => "ui.model_catalog",
            Self::InstalledModels => "ui.installed_models",
            Self::InstalledModelsDescription => "ui.installed_models_description",
            Self::ModelId => "ui.model_id",
            Self::Preset => "ui.preset",
            Self::Installed => "ui.installed",
            Self::Active => "ui.active",
            Self::Unavailable => "ui.unavailable",
            Self::Activate => "ui.activate",
            Self::Cancel => "ui.cancel",
            Self::Confirm => "ui.confirm",
            Self::Delete => "ui.delete",
            Self::ConfirmDeletion => "ui.confirm_deletion",
            Self::PackageLayoutInvalid => "ui.package_layout_invalid",
            Self::PackageSafetyLimitsExceeded => "ui.package_safety_limits_exceeded",
            Self::ModelDefinitionUnsupported => "ui.model_definition_unsupported",
            Self::TextureInvalid => "ui.texture_invalid",
            Self::ModelFilesUnavailable => "ui.model_files_unavailable",
            Self::ModelResourceInvalid => "ui.model_resource_invalid",
            Self::NoFolderSelected => "ui.no_folder_selected",
            Self::FolderSelected => "ui.folder_selected",
            Self::ChoosingFolder => "ui.choosing_folder",
            Self::SelectionCancelledPreviousRetained => "ui.selection_cancelled_previous_retained",
            Self::SelectionCancelled => "ui.selection_cancelled",
            Self::FolderPickerRequiresUiThread => "ui.folder_picker_requires_ui_thread",
            Self::SelectedFolderUnavailable => "ui.selected_folder_unavailable",
            Self::FolderPickerUnavailable => "ui.folder_picker_unavailable",
            Self::CancellingImport => "ui.cancelling_import",
            Self::StartingImport => "ui.starting_import",
            Self::Preparing => "ui.preparing",
            Self::Copying => "ui.copying",
            Self::Validating => "ui.validating",
            Self::Committing => "ui.committing",
            Self::ImportComplete => "ui.import_complete",
            Self::ImportCancelled => "ui.import_cancelled",
            Self::LoadingModels => "ui.loading_models",
            Self::ModelCatalogUnavailable => "ui.model_catalog_unavailable",
            Self::NoModelsAvailable => "ui.no_models_available",
            Self::ActivatingModel => "ui.activating_model",
            Self::DeletingModel => "ui.deleting_model",
            Self::RefreshingModels => "ui.refreshing_models",
            Self::CatalogUnavailable => "ui.catalog_unavailable",
            Self::Choosing => "ui.choosing",
            Self::ChooseFolder => "ui.choose_folder",
            Self::Import => "ui.import",
            Self::AvailableModels => "ui.available_models",
            Self::ModelBehaviors => "ui.model_behaviors",
            Self::NoModelBehaviors => "ui.no_model_behaviors",
            Self::Motion => "ui.motion",
            Self::Expression => "ui.expression",
            Self::Preview => "ui.preview",
            Self::PreviewingBehavior => "ui.previewing_behavior",
            Self::Diagnostics => "ui.diagnostics",
            Self::DiagnosticsDescription => "ui.diagnostics_description",
            Self::RuntimeAndInput => "ui.runtime_and_input",
            Self::RuntimeDiagnostics => "ui.runtime_diagnostics",
            Self::RuntimeDiagnosticsDescription => "ui.runtime_diagnostics_description",
            Self::LoadingDiagnostics => "ui.loading_diagnostics",
            Self::InputReliabilityCounters => "ui.input_reliability_counters",
            Self::RuntimeRenderer => "ui.runtime_renderer",
            Self::BuildInformation => "ui.build_information",
            Self::Version => "ui.version",
            Self::Development => "ui.development",
            Self::Production => "ui.production",
            Self::DiagnosticsExport => "ui.diagnostics_export",
            Self::NoReportExported => "ui.no_report_exported",
            Self::Export => "ui.export",
            Self::Shortcuts => "ui.shortcuts",
            Self::ShortcutsDescription => "ui.shortcuts_description",
            Self::WindowShortcuts => "ui.window_shortcuts",
            Self::ModelShortcuts => "ui.model_shortcuts",
            Self::RestoreDefaults => "ui.restore_defaults",
            Self::ClearAll => "ui.clear_all",
            Self::PressCommandShortcut => "ui.press_command_shortcut",
            Self::PressBehaviorShortcut => "ui.press_behavior_shortcut",
            Self::PressRecordShortcut => "ui.press_record_shortcut",
            Self::ClickRecordShortcut => "ui.click_record_shortcut",
            Self::PressKey => "ui.press_key",
            Self::Capture => "ui.capture",
            Self::Clear => "ui.clear",
            Self::NotSet => "ui.not_set",
            Self::InputService => "ui.input_service",
            Self::InputMonitoring => "ui.input_monitoring",
            Self::Configuration => "ui.configuration",
            Self::Backups => "ui.backups",
            Self::CurrentState => "ui.current_state",
            Self::InputProcessing => "ui.input_processing",
            Self::SequenceRecovery => "ui.sequence_recovery",
            Self::Transport => "ui.transport",
            Self::RestoreDefaultConfiguration => "ui.restore_default_configuration",
            Self::RestoreDefaultConfigurationDescription => {
                "ui.restore_default_configuration_description"
            }
            Self::OpenConfigurationBackupsFolder => "ui.open_configuration_backups_folder",
            Self::OpenConfigurationBackupsFolderDescription => {
                "ui.open_configuration_backups_folder_description"
            }
            Self::ExportDiagnostics => "ui.export_diagnostics",
            Self::ExportDiagnosticsDescription => "ui.export_diagnostics_description",
            Self::RestoreDefaultShortcuts => "ui.restore_default_shortcuts",
            Self::RestoreDefaultShortcutsDescription => "ui.restore_default_shortcuts_description",
            Self::ClearAllShortcuts => "ui.clear_all_shortcuts",
            Self::ClearAllShortcutsDescription => "ui.clear_all_shortcuts_description",
            Self::WaitingForKeyCombination => "ui.waiting_for_key_combination",
            Self::NotStarted => "ui.not_started",
            Self::Running => "ui.running",
            Self::PermissionRequired => "ui.permission_required",
            Self::Granted => "ui.granted",
            Self::Unsupported => "ui.unsupported",
            Self::BackendUnavailable => "ui.backend_unavailable",
            Self::StartupFailed => "ui.startup_failed",
            Self::GpuPreparationFailed => "ui.gpu_preparation_failed",
            Self::ModelLoadFailed => "ui.model_load_failed",
            Self::ModelEvaluationFailed => "ui.model_evaluation_failed",
            Self::MotionLoadFailed => "ui.motion_load_failed",
            Self::ExpressionLoadFailed => "ui.expression_load_failed",
            Self::PlatformUnsupported => "ui.platform_unsupported",
            Self::RuntimeTransportClosed => "ui.runtime_transport_closed",
            Self::OverlaySettingsInvalid => "ui.overlay_settings_invalid",
            Self::MaximumFpsInvalid => "ui.maximum_fps_invalid",
            Self::ReleaseFallbackTimeoutInvalid => "ui.release_fallback_timeout_invalid",
            Self::NoRendererError => "ui.no_renderer_error",
            Self::NoCommandFailures => "ui.no_command_failures",
            Self::RuntimeShutdownFailures => "ui.runtime_shutdown_failures",
            Self::ConfigurationUnavailable => "ui.configuration_unavailable",
            Self::DefaultsRestored => "ui.defaults_restored",
            Self::RestartToContinue => "ui.restart_to_continue",
            Self::RecoveredFromBackup => "ui.recovered_from_backup",
            Self::LoadedNormally => "ui.loaded_normally",
            Self::NoRecovery => "ui.no_recovery",
            Self::Appearance => "ui.appearance",
            Self::Overlay => "ui.overlay",
            Self::Theme => "ui.theme",
            Self::System => "ui.system",
            Self::Light => "ui.light",
            Self::Dark => "ui.dark",
            Self::ThemeDescription => "ui.theme_description",
            Self::Language => "ui.language",
            Self::LanguageDescription => "ui.language_description",
            Self::RuntimeStatus => "ui.runtime_status",
            Self::RuntimeStatusDescription => "ui.runtime_status_description",
            Self::ShowDesktopCat => "ui.show_desktop_cat",
            Self::ShowDesktopCatDescription => "ui.show_desktop_cat_description",
            Self::AlwaysOnTop => "ui.always_on_top",
            Self::AlwaysOnTopDescription => "ui.always_on_top_description",
            Self::ClickThroughOverlay => "ui.click_through_overlay",
            Self::ClickThroughOverlayDescription => "ui.click_through_overlay_description",
            Self::KeepInsideWorkArea => "ui.keep_inside_work_area",
            Self::KeepInsideWorkAreaDescription => "ui.keep_inside_work_area_description",
            Self::MotionAudio => "ui.motion_audio",
            Self::MotionAudioDescription => "ui.motion_audio_description",
            Self::OverlayScale => "ui.overlay_scale",
            Self::OverlayScaleDescription => "ui.overlay_scale_description",
            Self::OverlayOpacity => "ui.overlay_opacity",
            Self::OverlayOpacityDescription => "ui.overlay_opacity_description",
            Self::MaximumFps => "ui.maximum_fps",
            Self::MaximumFpsDescription => "ui.maximum_fps_description",
            Self::ReleaseFallbackTimeout => "ui.release_fallback_timeout",
            Self::ReleaseFallbackTimeoutDescription => "ui.release_fallback_timeout_description",
            Self::ModelInteraction => "ui.model_interaction",
            Self::BehaviorShortcuts => "ui.behavior_shortcuts",
            Self::BehaviorShortcutsDescription => "ui.behavior_shortcuts_description",
            Self::MirrorModel => "ui.mirror_model",
            Self::MirrorModelDescription => "ui.mirror_model_description",
            Self::MirrorPointerTracking => "ui.mirror_pointer_tracking",
            Self::MirrorPointerTrackingDescription => "ui.mirror_pointer_tracking_description",
            Self::IgnorePointerInput => "ui.ignore_pointer_input",
            Self::IgnorePointerInputDescription => "ui.ignore_pointer_input_description",
            Self::Input => "ui.input",
            Self::GamepadStickDeadZone => "ui.gamepad_stick_dead_zone",
            Self::GamepadStickDeadZoneDescription => "ui.gamepad_stick_dead_zone_description",
            Self::GamepadTriggerDeadZone => "ui.gamepad_trigger_dead_zone",
            Self::GamepadTriggerDeadZoneDescription => "ui.gamepad_trigger_dead_zone_description",
            Self::Application => "ui.application",
            Self::ShowStatusIcon => "ui.show_status_icon",
            Self::ShowStatusIconDescription => "ui.show_status_icon_description",
            #[cfg(target_os = "windows")]
            Self::ShowTaskbarIcon => "ui.show_taskbar_icon",
            #[cfg(target_os = "windows")]
            Self::ShowTaskbarIconDescription => "ui.show_taskbar_icon_description",
            Self::CheckForUpdatesAutomatically => "ui.check_for_updates_automatically",
            Self::CheckForUpdatesAutomaticallyDescription => {
                "ui.check_for_updates_automatically_description"
            }
            Self::OpenAtLogin => "ui.open_at_login",
            Self::DecreaseOverlayScale => "ui.decrease_overlay_scale",
            Self::IncreaseOverlayScale => "ui.increase_overlay_scale",
            Self::DecreaseOverlayOpacity => "ui.decrease_overlay_opacity",
            Self::IncreaseOverlayOpacity => "ui.increase_overlay_opacity",
            Self::DecreaseMaximumFps => "ui.decrease_maximum_fps",
            Self::IncreaseMaximumFps => "ui.increase_maximum_fps",
            Self::DecreaseReleaseFallbackTimeout => "ui.decrease_release_fallback_timeout",
            Self::IncreaseReleaseFallbackTimeout => "ui.increase_release_fallback_timeout",
            Self::CheckingLoginStartup => "ui.checking_login_startup",
            Self::LoginStartupStatusUnavailable => "ui.login_startup_status_unavailable",
            Self::LoginStartupDisabled => "ui.login_startup_disabled",
            Self::LoginStartupEnabled => "ui.login_startup_enabled",
            Self::LoginStartupStale => "ui.login_startup_stale",
            Self::LoginStartupRequiresApproval => "ui.login_startup_requires_approval",
            Self::LoginStartupNotFound => "ui.login_startup_not_found",
            Self::LoginStartupUnsupportedPlatform => "ui.login_startup_unsupported_platform",
            Self::LoginStartupUnsupportedOperatingSystem => {
                "ui.login_startup_unsupported_operating_system"
            }
            Self::LoginStartupUnsupportedBuild => "ui.login_startup_unsupported_build",
            Self::Refreshing => "ui.refreshing",
            Self::Saving => "ui.saving",
            Self::Connecting => "ui.connecting",
            Self::Starting => "ui.starting",
            Self::Ready => "ui.ready",
            Self::Degraded => "ui.degraded",
            Self::Stopped => "ui.stopped",
            Self::Refresh => "ui.refresh",
            Self::Quit => "ui.quit",
        }
    }
}
pub(super) fn model_availability_summary(
    language: SettingsLanguage,
    origin: SettingsModelOrigin,
    active: bool,
    texture_count: usize,
    expression_count: usize,
    motion_count: usize,
) -> String {
    let origin = text(
        language,
        match origin {
            SettingsModelOrigin::Preset => UiText::Preset,
            SettingsModelOrigin::Installed => UiText::Installed,
        },
    );
    let active = active.then(|| text(language, UiText::Active));
    match language {
        SettingsLanguage::ChineseSimplified => format!(
            "{origin}{} · {texture_count} 个纹理 · {expression_count} 个表情 · {motion_count} 个动作",
            active.map_or(String::new(), |active| format!(" · {active}"))
        ),
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => format!(
            "{origin}{} · {texture_count} textures · {expression_count} expressions · {motion_count} motions",
            active.map_or(String::new(), |active| format!(" · {active}"))
        ),
    }
}

pub(super) fn diagnostics_export_status(
    language: SettingsLanguage,
    status: Option<SettingsDiagnosticsExportStatus>,
) -> String {
    match (language, status) {
        (SettingsLanguage::ChineseSimplified, Some(status)) => format!(
            "报告 v{}：{} 字节 · 预览包 v{}：{} 个条目，{} 字节 · 跳过 {} 个来源日志",
            status.format_version,
            status.bytes_written,
            status.preview_bundle_format_version,
            status.preview_bundle_entry_count,
            status.preview_bundle_bytes_written,
            status.preview_bundle_skipped_source_files,
        ),
        (SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates, Some(status)) => {
            format!(
                "Report v{}: {} bytes · Preview bundle v{}: {} entries, {} bytes · Skipped {} source logs",
                status.format_version,
                status.bytes_written,
                status.preview_bundle_format_version,
                status.preview_bundle_entry_count,
                status.preview_bundle_bytes_written,
                status.preview_bundle_skipped_source_files,
            )
        }
        (_, None) => text(language, UiText::NoReportExported).to_owned(),
    }
}

pub(super) fn build_info_detail(
    language: SettingsLanguage,
    build_info: &SettingsBuildInfo,
) -> String {
    let environment = match build_info.environment {
        SettingsBuildEnvironment::Development => UiText::Development,
        SettingsBuildEnvironment::Production => UiText::Production,
    };
    format!(
        "{} {} · {}",
        text(language, UiText::Version),
        build_info.product_version,
        text(language, environment)
    )
}

pub(super) fn input_service_attempts(language: SettingsLanguage, attempts: u64) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => format!("启动尝试：{attempts}"),
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => {
            format!("Start attempts: {attempts}")
        }
    }
}

pub(super) fn runtime_command_failure(
    language: SettingsLanguage,
    error: &str,
    sequence: u64,
) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => format!("{error} · 命令 #{sequence}"),
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => {
            format!("{error} · command #{sequence}")
        }
    }
}

pub(super) fn runtime_shutdown_failures(language: SettingsLanguage, count: u64) -> String {
    let locale = match language {
        SettingsLanguage::ChineseSimplified => "zh-CN",
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => "en-US",
    };
    bongocat_i18n::count_text(locale, UiText::RuntimeShutdownFailures.key(), count)
}

pub(super) fn backup_candidates_checked(
    language: SettingsLanguage,
    checked_backups: u32,
) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => {
            format!("已检查 {checked_backups} 个备份候选")
        }
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => format!(
            "{} backup candidate{} checked",
            checked_backups,
            if checked_backups == 1 { "" } else { "s" }
        ),
    }
}

pub(super) fn recovered_backup_detail(
    language: SettingsLanguage,
    schema_version: u32,
    skipped_newer_backups: u32,
) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => {
            format!("Schema v{schema_version} · 已跳过 {skipped_newer_backups} 个较新的备份")
        }
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => format!(
            "Schema v{} · {} newer backup{} skipped",
            schema_version,
            skipped_newer_backups,
            if skipped_newer_backups == 1 { "" } else { "s" }
        ),
    }
}

pub(super) fn shortcut_accessibility_label(
    language: SettingsLanguage,
    target: &ShortcutCaptureTarget,
) -> String {
    let name = shortcut_target_name(language, target);
    match language {
        SettingsLanguage::ChineseSimplified => format!("为{name}录入快捷键"),
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => {
            format!("Capture shortcut for {name}")
        }
    }
}

pub(super) fn shortcut_target_name(
    language: SettingsLanguage,
    target: &ShortcutCaptureTarget,
) -> String {
    match target {
        ShortcutCaptureTarget::Command(command) => {
            let values = match command.as_str() {
                "toggle_overlay" => ["Show or hide model window", "显示或隐藏模型窗口"],
                "open_settings" => ["Show or hide settings window", "显示或隐藏设置窗口"],
                "toggle_mirror" => ["Toggle model mirror", "切换模型镜像"],
                "toggle_click_through" => ["Toggle click-through", "切换鼠标穿透"],
                "toggle_always_on_top" => ["Toggle always on top", "切换始终置顶"],
                _ => return command.clone(),
            };
            values[match language {
                SettingsLanguage::ChineseSimplified => 1,
                SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => 0,
            }]
            .to_owned()
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
    let labels = match language {
        SettingsLanguage::ChineseSimplified => [
            "按下的按键",
            "按下的鼠标按键",
            "按下的手柄按键",
            "已连接的手柄",
            "捕获的按下事件",
            "捕获的释放事件",
            "校正释放",
            "兜底释放",
            "重置释放",
            "重复按下",
            "未匹配的释放",
            "无效来源",
            "重置次数",
            "序列缺口",
            "缺失事件",
            "重复事件",
            "乱序事件",
            "非单调时间戳",
            "手柄连接",
            "手柄断开",
            "过期手柄事件",
            "断开时释放",
            "入队事件",
            "队列溢出",
            "溢出恢复",
            "关闭后拒绝",
        ],
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => [
            "Pressed keys",
            "Pressed mouse buttons",
            "Pressed gamepad buttons",
            "Connected gamepads",
            "Captured presses",
            "Captured releases",
            "Reconciled releases",
            "Fallback releases",
            "Released by reset",
            "Duplicate presses",
            "Unmatched releases",
            "Invalid sources",
            "Resets",
            "Sequence gaps",
            "Missing events",
            "Duplicate events",
            "Out-of-order events",
            "Non-monotonic timestamps",
            "Gamepad connections",
            "Gamepad disconnections",
            "Stale gamepad events",
            "Released on disconnect",
            "Events enqueued",
            "Queue overflows",
            "Overflow recoveries",
            "Rejected after shutdown",
        ],
    };
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
    std::array::from_fn(|index| (labels[index], values[index]))
}

pub(super) fn settings_error(language: SettingsLanguage, error: SettingsError) -> &'static str {
    let values = match error.code() {
        SettingsErrorCode::ServiceUnavailable => {
            ["settings service is unavailable", "设置服务不可用"]
        }
        SettingsErrorCode::SnapshotOutdated => [
            "settings changed in the background; review the latest values and retry",
            "设置已在后台更改；请检查最新值后重试",
        ],
        SettingsErrorCode::RuntimeUnavailable => {
            ["runtime did not apply the setting", "运行时未应用此设置"]
        }
        SettingsErrorCode::InvalidMaximumFps => [
            "maximum FPS must be between 15 and 240",
            "最大帧率必须在 15 到 240 之间",
        ],
        SettingsErrorCode::InvalidReleaseFallbackTimeout => [
            "key release fallback timeout must be between 0 and 60000 milliseconds",
            "按键释放兜底超时必须在 0 到 60000 毫秒之间",
        ],
        SettingsErrorCode::InvalidGamepadAxisSettings => [
            "gamepad dead-zone settings are out of range",
            "手柄死区设置超出范围",
        ],
        SettingsErrorCode::InvalidShortcutBindings => [
            "shortcut bindings are invalid or conflict",
            "快捷键绑定无效或存在冲突",
        ],
        SettingsErrorCode::ConfigPersistFailed => ["setting could not be saved", "无法保存设置"],
        SettingsErrorCode::ConfigPermissionDenied => [
            "configuration storage is not writable; check permissions and retry",
            "配置存储不可写；请检查权限后重试",
        ],
        SettingsErrorCode::ConfigStorageFull => [
            "configuration storage is full; free space and retry",
            "配置存储空间已满；请释放空间后重试",
        ],
        SettingsErrorCode::ConfigTargetOccupied => [
            "configuration storage is blocked; remove the blocking item and retry",
            "配置存储位置被占用；请移除占用项后重试",
        ],
        SettingsErrorCode::BackupLocationOpenFailed => [
            "configuration backup folder could not be opened",
            "无法打开配置备份文件夹",
        ],
        SettingsErrorCode::ConfigurationRecoveryRequired => [
            "configuration must be recovered before this action",
            "执行此操作前必须恢复配置",
        ],
        SettingsErrorCode::ConfigurationRecoveryFailed => [
            "default configuration could not be restored",
            "无法恢复默认配置",
        ],
        SettingsErrorCode::ModelUnavailable => ["selected model is unavailable", "所选模型不可用"],
        SettingsErrorCode::ModelSwitchFailed => {
            ["selected model could not be activated", "无法启用所选模型"]
        }
        SettingsErrorCode::ModelBehaviorPreviewUnavailable => [
            "the selected behavior is not available for the active model",
            "所选行为不适用于当前模型",
        ],
        SettingsErrorCode::ModelBehaviorPreviewFailed => [
            "the selected behavior could not be previewed",
            "无法预览所选行为",
        ],
        SettingsErrorCode::InvalidModelId => ["model id is invalid", "模型 ID 无效"],
        SettingsErrorCode::ModelAlreadyInstalled => {
            ["model id is already installed", "该模型 ID 已安装"]
        }
        SettingsErrorCode::ModelImportInvalidPackage => ["model package is invalid", "模型包无效"],
        SettingsErrorCode::ModelImportSourceInvalid => {
            ["model source cannot be imported", "无法导入模型来源"]
        }
        SettingsErrorCode::ModelImportSourceChanged => [
            "model source changed during import",
            "模型来源在导入期间发生变化",
        ],
        SettingsErrorCode::ModelImportSourceUnsupported => [
            "model source contains an unsupported entry",
            "模型来源包含不支持的项目",
        ],
        SettingsErrorCode::ModelImportCancelled => ["model import was cancelled", "模型导入已取消"],
        SettingsErrorCode::ModelStoreBusy => ["model storage is busy", "模型存储正忙"],
        SettingsErrorCode::ModelImportFailed => ["model could not be imported", "无法导入模型"],
        SettingsErrorCode::PresetModelCannotBeDeleted => {
            ["preset model cannot be deleted", "无法删除预置模型"]
        }
        SettingsErrorCode::SelectedModelCannotBeDeleted => [
            "selected model must be replaced before deletion",
            "删除当前模型前必须先切换到其他模型",
        ],
        SettingsErrorCode::ModelNotInstalled => {
            ["installed model was not found", "找不到已安装模型"]
        }
        SettingsErrorCode::ModelDeleteFailed => {
            ["installed model could not be deleted", "无法删除已安装模型"]
        }
        SettingsErrorCode::DiagnosticsExportFailed => {
            ["diagnostics could not be exported", "无法导出诊断信息"]
        }
        SettingsErrorCode::StartupItemUpdateFailed => [
            "startup setting could not be updated",
            "无法更新登录启动设置",
        ],
        SettingsErrorCode::StatusIconUpdateFailed => [
            "status icon visibility could not be updated",
            "无法更新状态图标可见性",
        ],
        SettingsErrorCode::TaskbarIconUpdateFailed => [
            "taskbar icon visibility could not be updated",
            "无法更新任务栏图标可见性",
        ],
        SettingsErrorCode::WindowUnavailable => {
            ["settings window could not be hidden", "无法隐藏设置窗口"]
        }
        SettingsErrorCode::StatePersistFailed => {
            ["window layout could not be saved", "无法保存窗口布局"]
        }
        SettingsErrorCode::ShutdownFailed => {
            ["application shutdown did not complete", "应用未能完成关闭"]
        }
    };
    values[match language {
        SettingsLanguage::ChineseSimplified => 1,
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => 0,
    }]
}

pub(super) fn shortcut_conflict_message(language: SettingsLanguage, shortcut: &str) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => format!("{shortcut} 已被其他快捷键占用"),
        SettingsLanguage::EnglishUnitedStates | SettingsLanguage::System => {
            format!("{shortcut} is already assigned to another shortcut")
        }
    }
}

pub(super) fn model_invalid_summary(
    language: SettingsLanguage,
    origin: SettingsModelOrigin,
    diagnostic: &str,
) -> String {
    let origin = text(
        language,
        match origin {
            SettingsModelOrigin::Preset => UiText::Preset,
            SettingsModelOrigin::Installed => UiText::Installed,
        },
    );
    format!("{origin} · {diagnostic}")
}

pub(super) fn model_delete_confirmation(language: SettingsLanguage, status: &str) -> String {
    format!("{status} · {}", text(language, UiText::ConfirmDeletion))
}

pub(super) fn model_import_progress(
    language: SettingsLanguage,
    stage: &str,
    files_copied: u64,
    bytes_copied: u64,
) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => {
            format!("{stage} · {files_copied} 个文件 · {bytes_copied} 字节")
        }
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => {
            format!("{stage} · {files_copied} files · {bytes_copied} bytes")
        }
    }
}

pub(super) fn runtime_status(language: SettingsLanguage, health: &str, revision: u64) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => {
            format!("运行状态：{health} - 修订 {revision}")
        }
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => {
            format!("Runtime {health} - revision {revision}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_display_languages_have_nonempty_shell_text() {
        let keys = [
            UiText::Settings,
            UiText::General,
            UiText::Models,
            UiText::ModelsDescription,
            UiText::About,
            UiText::AboutDescription,
            UiText::AboutBongoCat,
            UiText::ProductInformation,
            UiText::ProductInformationDescription,
            UiText::ApplicationLicense,
            UiText::ApplicationLicenseDescription,
            UiText::ThirdPartyLicenses,
            UiText::ThirdPartyLicensesDescription,
            UiText::CubismAttribution,
            UiText::CubismAttributionDescription,
            UiText::Privacy,
            UiText::PrivacyDescription,
            UiText::ModelCatalog,
            UiText::InstalledModels,
            UiText::InstalledModelsDescription,
            UiText::ModelId,
            UiText::Preset,
            UiText::Installed,
            UiText::Active,
            UiText::Unavailable,
            UiText::Activate,
            UiText::Cancel,
            UiText::Confirm,
            UiText::Delete,
            UiText::ConfirmDeletion,
            UiText::PackageLayoutInvalid,
            UiText::PackageSafetyLimitsExceeded,
            UiText::ModelDefinitionUnsupported,
            UiText::TextureInvalid,
            UiText::ModelFilesUnavailable,
            UiText::ModelResourceInvalid,
            UiText::NoFolderSelected,
            UiText::FolderSelected,
            UiText::ChoosingFolder,
            UiText::SelectionCancelledPreviousRetained,
            UiText::SelectionCancelled,
            UiText::FolderPickerRequiresUiThread,
            UiText::SelectedFolderUnavailable,
            UiText::FolderPickerUnavailable,
            UiText::CancellingImport,
            UiText::StartingImport,
            UiText::Preparing,
            UiText::Copying,
            UiText::Validating,
            UiText::Committing,
            UiText::ImportComplete,
            UiText::ImportCancelled,
            UiText::LoadingModels,
            UiText::ModelCatalogUnavailable,
            UiText::NoModelsAvailable,
            UiText::ActivatingModel,
            UiText::DeletingModel,
            UiText::RefreshingModels,
            UiText::CatalogUnavailable,
            UiText::Choosing,
            UiText::ChooseFolder,
            UiText::Import,
            UiText::AvailableModels,
            UiText::ModelBehaviors,
            UiText::NoModelBehaviors,
            UiText::Motion,
            UiText::Expression,
            UiText::Preview,
            UiText::PreviewingBehavior,
            UiText::Diagnostics,
            UiText::DiagnosticsDescription,
            UiText::RuntimeAndInput,
            UiText::RuntimeDiagnostics,
            UiText::RuntimeDiagnosticsDescription,
            UiText::LoadingDiagnostics,
            UiText::InputReliabilityCounters,
            UiText::RuntimeRenderer,
            UiText::BuildInformation,
            UiText::Version,
            UiText::Development,
            UiText::Production,
            UiText::DiagnosticsExport,
            UiText::NoReportExported,
            UiText::Export,
            UiText::Shortcuts,
            UiText::ShortcutsDescription,
            UiText::WindowShortcuts,
            UiText::ModelShortcuts,
            UiText::RestoreDefaults,
            UiText::ClearAll,
            UiText::PressCommandShortcut,
            UiText::PressBehaviorShortcut,
            UiText::PressRecordShortcut,
            UiText::ClickRecordShortcut,
            UiText::PressKey,
            UiText::Capture,
            UiText::Clear,
            UiText::NotSet,
            UiText::InputService,
            UiText::InputMonitoring,
            UiText::Configuration,
            UiText::Backups,
            UiText::CurrentState,
            UiText::InputProcessing,
            UiText::SequenceRecovery,
            UiText::Transport,
            UiText::RestoreDefaultConfiguration,
            UiText::RestoreDefaultConfigurationDescription,
            UiText::OpenConfigurationBackupsFolder,
            UiText::OpenConfigurationBackupsFolderDescription,
            UiText::ExportDiagnostics,
            UiText::ExportDiagnosticsDescription,
            UiText::RestoreDefaultShortcuts,
            UiText::RestoreDefaultShortcutsDescription,
            UiText::ClearAllShortcuts,
            UiText::ClearAllShortcutsDescription,
            UiText::WaitingForKeyCombination,
            UiText::NotStarted,
            UiText::Running,
            UiText::PermissionRequired,
            UiText::Granted,
            UiText::Unsupported,
            UiText::BackendUnavailable,
            UiText::StartupFailed,
            UiText::GpuPreparationFailed,
            UiText::ModelLoadFailed,
            UiText::ModelEvaluationFailed,
            UiText::MotionLoadFailed,
            UiText::ExpressionLoadFailed,
            UiText::PlatformUnsupported,
            UiText::RuntimeTransportClosed,
            UiText::OverlaySettingsInvalid,
            UiText::MaximumFpsInvalid,
            UiText::ReleaseFallbackTimeoutInvalid,
            UiText::NoRendererError,
            UiText::NoCommandFailures,
            UiText::ConfigurationUnavailable,
            UiText::DefaultsRestored,
            UiText::RestartToContinue,
            UiText::RecoveredFromBackup,
            UiText::LoadedNormally,
            UiText::NoRecovery,
            UiText::Language,
            UiText::RuntimeStatus,
            UiText::RuntimeStatusDescription,
            UiText::ShowDesktopCat,
            UiText::ShowDesktopCatDescription,
            UiText::AlwaysOnTop,
            UiText::AlwaysOnTopDescription,
            UiText::ClickThroughOverlay,
            UiText::ClickThroughOverlayDescription,
            UiText::KeepInsideWorkArea,
            UiText::KeepInsideWorkAreaDescription,
            UiText::MotionAudio,
            UiText::MotionAudioDescription,
            UiText::OverlayScale,
            UiText::OverlayScaleDescription,
            UiText::OverlayOpacity,
            UiText::OverlayOpacityDescription,
            UiText::MaximumFps,
            UiText::MaximumFpsDescription,
            UiText::ReleaseFallbackTimeout,
            UiText::ReleaseFallbackTimeoutDescription,
            UiText::ModelInteraction,
            UiText::BehaviorShortcuts,
            UiText::BehaviorShortcutsDescription,
            UiText::MirrorModel,
            UiText::MirrorModelDescription,
            UiText::MirrorPointerTracking,
            UiText::MirrorPointerTrackingDescription,
            UiText::IgnorePointerInput,
            UiText::IgnorePointerInputDescription,
            UiText::Input,
            UiText::GamepadStickDeadZone,
            UiText::GamepadStickDeadZoneDescription,
            UiText::GamepadTriggerDeadZone,
            UiText::GamepadTriggerDeadZoneDescription,
            UiText::Application,
            UiText::ShowStatusIcon,
            UiText::ShowStatusIconDescription,
            #[cfg(target_os = "windows")]
            UiText::ShowTaskbarIcon,
            #[cfg(target_os = "windows")]
            UiText::ShowTaskbarIconDescription,
            UiText::CheckForUpdatesAutomatically,
            UiText::CheckForUpdatesAutomaticallyDescription,
            UiText::OpenAtLogin,
            UiText::DecreaseOverlayScale,
            UiText::IncreaseOverlayScale,
            UiText::DecreaseOverlayOpacity,
            UiText::IncreaseOverlayOpacity,
            UiText::DecreaseMaximumFps,
            UiText::IncreaseMaximumFps,
            UiText::DecreaseReleaseFallbackTimeout,
            UiText::IncreaseReleaseFallbackTimeout,
            UiText::CheckingLoginStartup,
            UiText::LoginStartupStatusUnavailable,
            UiText::LoginStartupDisabled,
            UiText::LoginStartupEnabled,
            UiText::LoginStartupStale,
            UiText::LoginStartupRequiresApproval,
            UiText::LoginStartupNotFound,
            UiText::LoginStartupUnsupportedPlatform,
            UiText::LoginStartupUnsupportedOperatingSystem,
            UiText::LoginStartupUnsupportedBuild,
            UiText::Refresh,
            UiText::Quit,
        ];
        for language in [
            SettingsLanguage::EnglishUnitedStates,
            SettingsLanguage::ChineseSimplified,
        ] {
            assert!(keys.into_iter().all(|key| !text(language, key).is_empty()));
        }
        assert!(keys.into_iter().any(|key| {
            text(SettingsLanguage::ChineseSimplified, key)
                != text(SettingsLanguage::EnglishUnitedStates, key)
        }));
        assert!(keys.into_iter().all(|key| {
            text(SettingsLanguage::System, key) == text(SettingsLanguage::EnglishUnitedStates, key)
        }));
    }

    #[test]
    fn about_sections_cover_license_cubism_dependencies_and_privacy() {
        assert_eq!(ABOUT_SECTIONS.len(), 4);
        for language in [
            SettingsLanguage::EnglishUnitedStates,
            SettingsLanguage::ChineseSimplified,
        ] {
            for section in ABOUT_SECTIONS {
                assert!(!text(language, section.title).is_empty());
                assert!(!text(language, section.description).is_empty());
            }
        }
        let cubism = text(
            SettingsLanguage::EnglishUnitedStates,
            UiText::CubismAttributionDescription,
        );
        assert!(cubism.contains("5-r.5"));
        assert!(cubism.contains("06.00.0001"));
        assert!(cubism.contains("Copyright Live2D"));
        assert!(cubism.contains("subject to Live2D approval"));
        assert_ne!(
            text(
                SettingsLanguage::ChineseSimplified,
                UiText::PrivacyDescription,
            ),
            text(
                SettingsLanguage::EnglishUnitedStates,
                UiText::PrivacyDescription,
            )
        );
    }

    #[test]
    fn stable_settings_errors_have_complete_localized_messages() {
        for code in SettingsErrorCode::ALL {
            let error = SettingsError::new(code);
            assert_eq!(
                settings_error(SettingsLanguage::EnglishUnitedStates, error),
                error.to_string()
            );
            assert_ne!(
                settings_error(SettingsLanguage::ChineseSimplified, error),
                settings_error(SettingsLanguage::EnglishUnitedStates, error)
            );
            assert_eq!(
                settings_error(SettingsLanguage::System, error),
                settings_error(SettingsLanguage::EnglishUnitedStates, error)
            );
        }
    }
}
