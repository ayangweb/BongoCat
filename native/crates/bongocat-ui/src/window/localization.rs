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
    bongocat_i18n::text(locale(language), key.key())
}

fn locale(language: SettingsLanguage) -> &'static str {
    match language {
        SettingsLanguage::ChineseSimplified => "zh-CN",
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => "en-US",
    }
}

impl UiText {
    const fn key(self) -> &'static str {
        match self {
            Self::Settings => "navigation.settings.title",
            Self::General => "navigation.general.title",
            Self::GeneralDescription => "navigation.general.description",
            Self::Models => "navigation.models.title",
            Self::ModelsDescription => "navigation.models.description",
            Self::About => "navigation.about.title",
            Self::AboutDescription => "navigation.about.description",
            Self::AboutBongoCat => "about.page_title",
            Self::ProductInformation => "about.product_information.title",
            Self::ProductInformationDescription => "about.product_information.description",
            Self::ApplicationLicense => "about.legal.application_license.title",
            Self::ApplicationLicenseDescription => "about.legal.application_license.description",
            Self::ThirdPartyLicenses => "about.legal.third_party_licenses.title",
            Self::ThirdPartyLicensesDescription => "about.legal.third_party_licenses.description",
            Self::CubismAttribution => "about.legal.cubism_attribution.title",
            Self::CubismAttributionDescription => "about.legal.cubism_attribution.description",
            Self::Privacy => "about.privacy.title",
            Self::PrivacyDescription => "about.privacy.description",
            Self::ModelCatalog => "models.catalog.title",
            Self::InstalledModels => "models.installed.title",
            Self::InstalledModelsDescription => "models.installed.description",
            Self::ModelId => "models.identity.id",
            Self::Preset => "models.identity.source.preset",
            Self::Installed => "models.identity.source.installed",
            Self::Active => "models.identity.status.active",
            Self::Unavailable => "models.identity.status.unavailable",
            Self::Activate => "models.actions.activate",
            Self::Cancel => "actions.cancel",
            Self::Confirm => "actions.confirm",
            Self::Delete => "models.actions.delete",
            Self::ConfirmDeletion => "models.actions.confirm_deletion",
            Self::PackageLayoutInvalid => "models.validation.package_layout_invalid",
            Self::PackageSafetyLimitsExceeded => "models.validation.package_safety_limits_exceeded",
            Self::ModelDefinitionUnsupported => "models.validation.model_definition_unsupported",
            Self::TextureInvalid => "models.validation.texture_invalid",
            Self::ModelFilesUnavailable => "models.validation.files_unavailable",
            Self::ModelResourceInvalid => "models.validation.resource_invalid",
            Self::NoFolderSelected => "models.import.folder.none_selected",
            Self::FolderSelected => "models.import.folder.selected",
            Self::ChoosingFolder => "models.import.folder.choosing",
            Self::SelectionCancelledPreviousRetained => {
                "models.import.folder.cancelled_previous_retained"
            }
            Self::SelectionCancelled => "models.import.folder.cancelled",
            Self::FolderPickerRequiresUiThread => "models.import.folder.picker_requires_ui_thread",
            Self::SelectedFolderUnavailable => "models.import.folder.selected_unavailable",
            Self::FolderPickerUnavailable => "models.import.folder.picker_unavailable",
            Self::CancellingImport => "models.import.progress.cancelling",
            Self::StartingImport => "models.import.progress.starting",
            Self::Preparing => "models.import.progress.preparing",
            Self::Copying => "models.import.progress.copying",
            Self::Validating => "models.import.progress.validating",
            Self::Committing => "models.import.progress.committing",
            Self::ImportComplete => "models.import.progress.complete",
            Self::ImportCancelled => "models.import.progress.cancelled",
            Self::LoadingModels => "models.catalog.loading",
            Self::ModelCatalogUnavailable => "models.catalog.unavailable",
            Self::NoModelsAvailable => "models.catalog.empty",
            Self::ActivatingModel => "models.catalog.activating",
            Self::DeletingModel => "models.catalog.deleting",
            Self::RefreshingModels => "models.catalog.refreshing",
            Self::CatalogUnavailable => "models.catalog.error",
            Self::Choosing => "models.import.folder.choosing_short",
            Self::ChooseFolder => "models.import.actions.choose_folder",
            Self::Import => "models.import.actions.import",
            Self::AvailableModels => "models.catalog.available",
            Self::ModelBehaviors => "models.behaviors.title",
            Self::NoModelBehaviors => "models.behaviors.empty",
            Self::Motion => "models.behaviors.motion",
            Self::Expression => "models.behaviors.expression",
            Self::Preview => "models.behaviors.preview",
            Self::PreviewingBehavior => "models.behaviors.previewing",
            Self::Diagnostics => "navigation.diagnostics.title",
            Self::DiagnosticsDescription => "navigation.diagnostics.description",
            Self::RuntimeAndInput => "diagnostics.runtime_and_input.title",
            Self::RuntimeDiagnostics => "diagnostics.runtime.title",
            Self::RuntimeDiagnosticsDescription => "diagnostics.runtime.description",
            Self::LoadingDiagnostics => "diagnostics.loading",
            Self::InputReliabilityCounters => "diagnostics.input.reliability_counters",
            Self::RuntimeRenderer => "diagnostics.renderer.title",
            Self::BuildInformation => "diagnostics.build.title",
            Self::Version => "diagnostics.build.version",
            Self::Development => "diagnostics.build.environment.development",
            Self::Production => "diagnostics.build.environment.production",
            Self::DiagnosticsExport => "diagnostics.export.title",
            Self::NoReportExported => "diagnostics.export.none",
            Self::Export => "diagnostics.export.action",
            Self::Shortcuts => "navigation.shortcuts.title",
            Self::ShortcutsDescription => "navigation.shortcuts.description",
            Self::WindowShortcuts => "shortcuts.scopes.window",
            Self::ModelShortcuts => "shortcuts.scopes.model",
            Self::RestoreDefaults => "shortcuts.actions.restore_defaults",
            Self::ClearAll => "shortcuts.actions.clear_all",
            Self::PressCommandShortcut => "shortcuts.capture.command_prompt",
            Self::PressBehaviorShortcut => "shortcuts.capture.behavior_prompt",
            Self::PressRecordShortcut => "shortcuts.capture.press_to_record",
            Self::ClickRecordShortcut => "shortcuts.capture.click_to_record",
            Self::PressKey => "shortcuts.capture.press_key",
            Self::Capture => "shortcuts.actions.capture",
            Self::Clear => "shortcuts.actions.clear",
            Self::NotSet => "shortcuts.state.not_set",
            Self::InputService => "diagnostics.input.service",
            Self::InputMonitoring => "diagnostics.input.monitoring",
            Self::Configuration => "diagnostics.configuration.title",
            Self::Backups => "diagnostics.configuration.backups",
            Self::CurrentState => "diagnostics.configuration.current_state",
            Self::InputProcessing => "diagnostics.input.processing",
            Self::SequenceRecovery => "diagnostics.input.sequence_recovery",
            Self::Transport => "diagnostics.input.transport",
            Self::RestoreDefaultConfiguration => "diagnostics.configuration.restore_defaults",
            Self::RestoreDefaultConfigurationDescription => {
                "diagnostics.configuration.restore_defaults_description"
            }
            Self::OpenConfigurationBackupsFolder => "diagnostics.configuration.open_backups_folder",
            Self::OpenConfigurationBackupsFolderDescription => {
                "diagnostics.configuration.open_backups_folder_description"
            }
            Self::ExportDiagnostics => "diagnostics.export.action",
            Self::ExportDiagnosticsDescription => "diagnostics.export.description",
            Self::RestoreDefaultShortcuts => "shortcuts.actions.restore_defaults",
            Self::RestoreDefaultShortcutsDescription => {
                "shortcuts.actions.restore_defaults_description"
            }
            Self::ClearAllShortcuts => "shortcuts.actions.clear_all",
            Self::ClearAllShortcutsDescription => "shortcuts.actions.clear_all_description",
            Self::WaitingForKeyCombination => "shortcuts.capture.waiting",
            Self::NotStarted => "status.not_started",
            Self::Running => "status.running",
            Self::PermissionRequired => "status.permission_required",
            Self::Granted => "status.granted",
            Self::Unsupported => "status.unsupported",
            Self::BackendUnavailable => "status.backend_unavailable",
            Self::StartupFailed => "errors.runtime.startup_failed",
            Self::GpuPreparationFailed => "errors.runtime.gpu_preparation_failed",
            Self::ModelLoadFailed => "errors.models.load_failed",
            Self::ModelEvaluationFailed => "errors.models.evaluation_failed",
            Self::MotionLoadFailed => "errors.models.motion_load_failed",
            Self::ExpressionLoadFailed => "errors.models.expression_load_failed",
            Self::PlatformUnsupported => "errors.runtime.platform_unsupported",
            Self::RuntimeTransportClosed => "errors.runtime.transport_closed",
            Self::OverlaySettingsInvalid => "errors.settings.overlay_invalid",
            Self::MaximumFpsInvalid => "errors.settings.maximum_fps_invalid",
            Self::ReleaseFallbackTimeoutInvalid => {
                "errors.settings.release_fallback_timeout_invalid"
            }
            Self::NoRendererError => "errors.runtime.no_renderer",
            Self::NoCommandFailures => "diagnostics.runtime.no_command_failures",
            Self::RuntimeShutdownFailures => "diagnostics.runtime.shutdown_failures",
            Self::ConfigurationUnavailable => "errors.settings.configuration_unavailable",
            Self::DefaultsRestored => "diagnostics.configuration.defaults_restored",
            Self::RestartToContinue => "diagnostics.configuration.restart_to_continue",
            Self::RecoveredFromBackup => "diagnostics.configuration.recovered_from_backup",
            Self::LoadedNormally => "diagnostics.configuration.loaded_normally",
            Self::NoRecovery => "diagnostics.configuration.no_recovery",
            Self::Appearance => "settings.appearance.title",
            Self::Overlay => "settings.overlay.title",
            Self::Theme => "settings.appearance.theme.label",
            Self::System => "settings.appearance.theme.options.system",
            Self::Light => "settings.appearance.theme.options.light",
            Self::Dark => "settings.appearance.theme.options.dark",
            Self::ThemeDescription => "settings.appearance.theme.description",
            Self::Language => "settings.appearance.language.label",
            Self::LanguageDescription => "settings.appearance.language.description",
            Self::RuntimeStatus => "settings.runtime.title",
            Self::RuntimeStatusDescription => "settings.runtime.description",
            Self::ShowDesktopCat => "settings.overlay.visibility.label",
            Self::ShowDesktopCatDescription => "settings.overlay.visibility.description",
            Self::AlwaysOnTop => "settings.overlay.always_on_top.label",
            Self::AlwaysOnTopDescription => "settings.overlay.always_on_top.description",
            Self::ClickThroughOverlay => "settings.overlay.click_through.label",
            Self::ClickThroughOverlayDescription => "settings.overlay.click_through.description",
            Self::KeepInsideWorkArea => "settings.overlay.keep_inside_work_area.label",
            Self::KeepInsideWorkAreaDescription => {
                "settings.overlay.keep_inside_work_area.description"
            }
            Self::MotionAudio => "settings.overlay.motion_audio.label",
            Self::MotionAudioDescription => "settings.overlay.motion_audio.description",
            Self::OverlayScale => "settings.overlay.scale.label",
            Self::OverlayScaleDescription => "settings.overlay.scale.description",
            Self::OverlayOpacity => "settings.overlay.opacity.label",
            Self::OverlayOpacityDescription => "settings.overlay.opacity.description",
            Self::MaximumFps => "settings.overlay.maximum_fps.label",
            Self::MaximumFpsDescription => "settings.overlay.maximum_fps.description",
            Self::ReleaseFallbackTimeout => "settings.overlay.release_fallback_timeout.label",
            Self::ReleaseFallbackTimeoutDescription => {
                "settings.overlay.release_fallback_timeout.description"
            }
            Self::ModelInteraction => "settings.model_interaction.title",
            Self::BehaviorShortcuts => "settings.model_interaction.behavior_shortcuts.label",
            Self::BehaviorShortcutsDescription => {
                "settings.model_interaction.behavior_shortcuts.description"
            }
            Self::MirrorModel => "settings.model_interaction.mirror_model.label",
            Self::MirrorModelDescription => "settings.model_interaction.mirror_model.description",
            Self::MirrorPointerTracking => {
                "settings.model_interaction.mirror_pointer_tracking.label"
            }
            Self::MirrorPointerTrackingDescription => {
                "settings.model_interaction.mirror_pointer_tracking.description"
            }
            Self::IgnorePointerInput => "settings.model_interaction.ignore_pointer_input.label",
            Self::IgnorePointerInputDescription => {
                "settings.model_interaction.ignore_pointer_input.description"
            }
            Self::Input => "settings.input.title",
            Self::GamepadStickDeadZone => "settings.input.gamepad_stick_dead_zone.label",
            Self::GamepadStickDeadZoneDescription => {
                "settings.input.gamepad_stick_dead_zone.description"
            }
            Self::GamepadTriggerDeadZone => "settings.input.gamepad_trigger_dead_zone.label",
            Self::GamepadTriggerDeadZoneDescription => {
                "settings.input.gamepad_trigger_dead_zone.description"
            }
            Self::Application => "settings.application.title",
            Self::ShowStatusIcon => "settings.application.status_icon.label",
            Self::ShowStatusIconDescription => "settings.application.status_icon.description",
            #[cfg(target_os = "windows")]
            Self::ShowTaskbarIcon => "settings.application.taskbar_icon.label",
            #[cfg(target_os = "windows")]
            Self::ShowTaskbarIconDescription => "settings.application.taskbar_icon.description",
            Self::CheckForUpdatesAutomatically => "settings.application.auto_update.label",
            Self::CheckForUpdatesAutomaticallyDescription => {
                "settings.application.auto_update.description"
            }
            Self::OpenAtLogin => "settings.application.open_at_login.label",
            Self::DecreaseOverlayScale => "shortcuts.actions.decrease_overlay_scale",
            Self::IncreaseOverlayScale => "shortcuts.actions.increase_overlay_scale",
            Self::DecreaseOverlayOpacity => "shortcuts.actions.decrease_overlay_opacity",
            Self::IncreaseOverlayOpacity => "shortcuts.actions.increase_overlay_opacity",
            Self::DecreaseMaximumFps => "shortcuts.actions.decrease_maximum_fps",
            Self::IncreaseMaximumFps => "shortcuts.actions.increase_maximum_fps",
            Self::DecreaseReleaseFallbackTimeout => {
                "shortcuts.actions.decrease_release_fallback_timeout"
            }
            Self::IncreaseReleaseFallbackTimeout => {
                "shortcuts.actions.increase_release_fallback_timeout"
            }
            Self::CheckingLoginStartup => "settings.application.startup.checking",
            Self::LoginStartupStatusUnavailable => "settings.application.startup.unavailable",
            Self::LoginStartupDisabled => "settings.application.startup.disabled",
            Self::LoginStartupEnabled => "settings.application.startup.enabled",
            Self::LoginStartupStale => "settings.application.startup.stale",
            Self::LoginStartupRequiresApproval => "settings.application.startup.requires_approval",
            Self::LoginStartupNotFound => "settings.application.startup.not_found",
            Self::LoginStartupUnsupportedPlatform => {
                "settings.application.startup.unsupported_platform"
            }
            Self::LoginStartupUnsupportedOperatingSystem => {
                "settings.application.startup.unsupported_os"
            }
            Self::LoginStartupUnsupportedBuild => "settings.application.startup.unsupported_build",
            Self::Refreshing => "status.refreshing",
            Self::Saving => "status.saving",
            Self::Connecting => "status.connecting",
            Self::Starting => "status.starting",
            Self::Ready => "status.ready",
            Self::Degraded => "status.degraded",
            Self::Stopped => "status.stopped",
            Self::Refresh => "actions.refresh",
            Self::Quit => "actions.quit",
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
    let key = if active {
        "models.summary.active"
    } else {
        "models.summary.inactive"
    };
    bongocat_i18n::format_text(
        locale(language),
        key,
        &[
            ("origin", origin.to_owned()),
            ("texture_count", texture_count.to_string()),
            ("expression_count", expression_count.to_string()),
            ("motion_count", motion_count.to_string()),
            ("active", text(language, UiText::Active).to_owned()),
        ],
    )
}

pub(super) fn diagnostics_export_status(
    language: SettingsLanguage,
    status: Option<SettingsDiagnosticsExportStatus>,
) -> String {
    let Some(status) = status else {
        return text(language, UiText::NoReportExported).to_owned();
    };
    bongocat_i18n::format_text(
        locale(language),
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
    bongocat_i18n::format_text(
        locale(language),
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
        locale(language),
        "diagnostics.runtime.command_failure",
        &[
            ("error", error.to_owned()),
            ("sequence", sequence.to_string()),
        ],
    )
}

pub(super) fn runtime_shutdown_failures(language: SettingsLanguage, count: u64) -> String {
    bongocat_i18n::count_text(
        locale(language),
        UiText::RuntimeShutdownFailures.key(),
        count,
    )
}

pub(super) fn backup_candidates_checked(
    language: SettingsLanguage,
    checked_backups: u32,
) -> String {
    bongocat_i18n::format_text(
        locale(language),
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
        locale(language),
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
        locale(language),
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
            bongocat_i18n::text(locale(language), key).to_owned()
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
                locale(language),
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
    bongocat_i18n::text(locale(language), &key)
}

pub(super) fn shortcut_conflict_message(language: SettingsLanguage, shortcut: &str) -> String {
    bongocat_i18n::format_text(
        locale(language),
        "shortcuts.errors.conflict",
        &[("shortcut", shortcut.to_owned())],
    )
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
    bongocat_i18n::format_text(
        locale(language),
        "models.invalid_summary",
        &[
            ("origin", origin.to_owned()),
            ("diagnostic", diagnostic.to_owned()),
        ],
    )
}

pub(super) fn model_delete_confirmation(language: SettingsLanguage, status: &str) -> String {
    bongocat_i18n::format_text(
        locale(language),
        "models.delete_confirmation",
        &[
            ("status", status.to_owned()),
            (
                "confirm_deletion",
                text(language, UiText::ConfirmDeletion).to_owned(),
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
        locale(language),
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
        locale(language),
        "settings.runtime.status_detail",
        &[
            ("health", health.to_owned()),
            ("revision", revision.to_string()),
        ],
    )
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
