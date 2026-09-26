//! The catalogue of codes, and what each one means.
//!
//! A code is the stable part of a line. The severity and the message are looked
//! up from it rather than written at the call site, so the same event always reads
//! the same way and a reader can search a log for a code and get every line that
//! matters. The catalogue round-trips: a code that cannot be parsed back out of
//! its own rendering is a code that will not survive being logged.

use super::*;

/// Closed application event catalog. Code and message are fixed project-owned
/// values; only the small, allow-listed context after the message varies.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ApplicationLogCode {
    Started,
    PreviousRunUnclean,
    ShutdownStarted,
    ShutdownCompleted,
    ShutdownFailed,
    Panicked,
    LoggingSettingsChanged,
    RuntimeUnavailable,
    DiagnosticsExportFailed,
    ModelSelectionFallback,
    StartupFailed,
    StateRecovered,
    StatePersistFailed,
    ServiceDegraded,
    ServiceRecovered,
    ServiceFailed,
    ModelPrepareStarted,
    ModelActivationFailed,
    ModelOperationCompleted,
    ModelOperationFailed,
    WindowVisibilityChanged,
    WindowStatePersisted,
    InputStatusChanged,
    InputPermissionUnavailable,
    UpdateUnavailable,
    UpdateCheckCompleted,
    UpdateCheckFailed,
    UpdatePhaseChanged,
    UpdateInstallStarted,
    UpdateInstallCompleted,
    UpdateInstallFailed,
    SettingsCommandFailed,
    UiTransportFailed,
    FilesystemOperationFailed,
    NetworkOperationFailed,
    ParsingFailed,
}

impl ApplicationLogCode {
    pub const ALL: &'static [Self] = &[
        Self::Started,
        Self::PreviousRunUnclean,
        Self::ShutdownStarted,
        Self::ShutdownCompleted,
        Self::ShutdownFailed,
        Self::Panicked,
        Self::LoggingSettingsChanged,
        Self::RuntimeUnavailable,
        Self::DiagnosticsExportFailed,
        Self::ModelSelectionFallback,
        Self::StartupFailed,
        Self::StateRecovered,
        Self::StatePersistFailed,
        Self::ServiceDegraded,
        Self::ServiceRecovered,
        Self::ServiceFailed,
        Self::ModelPrepareStarted,
        Self::ModelActivationFailed,
        Self::ModelOperationCompleted,
        Self::ModelOperationFailed,
        Self::WindowVisibilityChanged,
        Self::WindowStatePersisted,
        Self::InputStatusChanged,
        Self::InputPermissionUnavailable,
        Self::UpdateUnavailable,
        Self::UpdateCheckCompleted,
        Self::UpdateCheckFailed,
        Self::UpdatePhaseChanged,
        Self::UpdateInstallStarted,
        Self::UpdateInstallCompleted,
        Self::UpdateInstallFailed,
        Self::SettingsCommandFailed,
        Self::UiTransportFailed,
        Self::FilesystemOperationFailed,
        Self::NetworkOperationFailed,
        Self::ParsingFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "application/started",
            Self::PreviousRunUnclean => "application/previous_run_unclean",
            Self::ShutdownStarted => "application/shutdown_started",
            Self::ShutdownCompleted => "application/shutdown_completed",
            Self::ShutdownFailed => "application/shutdown_failed",
            Self::Panicked => "application/panicked",
            Self::LoggingSettingsChanged => "logging/settings_changed",
            Self::RuntimeUnavailable => "settings/runtime_unavailable",
            Self::DiagnosticsExportFailed => "settings/diagnostics_export_failed",
            Self::ModelSelectionFallback => "model/selection_fallback",
            Self::StartupFailed => "application/startup_failed",
            Self::StateRecovered => "application/state_recovered",
            Self::StatePersistFailed => "application/state_persist_failed",
            Self::ServiceDegraded => "service/degraded",
            Self::ServiceRecovered => "service/recovered",
            Self::ServiceFailed => "service/failed",
            Self::ModelPrepareStarted => "model/prepare_started",
            Self::ModelActivationFailed => "model/activation_failed",
            Self::ModelOperationCompleted => "model/operation_completed",
            Self::ModelOperationFailed => "model/operation_failed",
            Self::WindowVisibilityChanged => "window/visibility_changed",
            Self::WindowStatePersisted => "window/state_persisted",
            Self::InputStatusChanged => "input/status_changed",
            Self::InputPermissionUnavailable => "input/permission_unavailable",
            Self::UpdateUnavailable => "update/unavailable",
            Self::UpdateCheckCompleted => "update/check_completed",
            Self::UpdateCheckFailed => "update/check_failed",
            Self::UpdatePhaseChanged => "update/phase_changed",
            Self::UpdateInstallStarted => "update/install_started",
            Self::UpdateInstallCompleted => "update/install_completed",
            Self::UpdateInstallFailed => "update/install_failed",
            Self::SettingsCommandFailed => "settings/command_failed",
            Self::UiTransportFailed => "ui/transport_failed",
            Self::FilesystemOperationFailed => "filesystem/operation_failed",
            Self::NetworkOperationFailed => "network/operation_failed",
            Self::ParsingFailed => "parser/failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|code| code.as_str() == value)
    }

    pub const fn level(self) -> LogLevel {
        match self {
            Self::Started
            | Self::ShutdownStarted
            | Self::ShutdownCompleted
            | Self::LoggingSettingsChanged
            | Self::StateRecovered
            | Self::ServiceRecovered
            | Self::ModelOperationCompleted
            | Self::WindowVisibilityChanged
            | Self::InputStatusChanged
            | Self::UpdateUnavailable
            | Self::UpdateCheckCompleted
            | Self::UpdatePhaseChanged
            | Self::UpdateInstallStarted
            | Self::UpdateInstallCompleted => LogLevel::Info,
            Self::PreviousRunUnclean
            | Self::ModelSelectionFallback
            | Self::StatePersistFailed
            | Self::ServiceDegraded
            | Self::InputPermissionUnavailable
            | Self::SettingsCommandFailed => LogLevel::Warn,
            Self::ShutdownFailed
            | Self::Panicked
            | Self::RuntimeUnavailable
            | Self::DiagnosticsExportFailed
            | Self::StartupFailed
            | Self::ServiceFailed
            | Self::ModelActivationFailed
            | Self::ModelOperationFailed
            | Self::UpdateCheckFailed
            | Self::UpdateInstallFailed
            | Self::UiTransportFailed
            | Self::FilesystemOperationFailed
            | Self::NetworkOperationFailed
            | Self::ParsingFailed => LogLevel::Error,
            Self::ModelPrepareStarted | Self::WindowStatePersisted => LogLevel::Debug,
        }
    }

    pub const fn component(self) -> ApplicationLogComponent {
        match self {
            Self::Started
            | Self::PreviousRunUnclean
            | Self::ShutdownStarted
            | Self::ShutdownCompleted
            | Self::ShutdownFailed
            | Self::Panicked
            | Self::StartupFailed
            | Self::StateRecovered
            | Self::StatePersistFailed => ApplicationLogComponent::Application,
            Self::LoggingSettingsChanged => ApplicationLogComponent::Logging,
            Self::RuntimeUnavailable
            | Self::DiagnosticsExportFailed
            | Self::SettingsCommandFailed => ApplicationLogComponent::Settings,
            Self::ModelSelectionFallback
            | Self::ModelPrepareStarted
            | Self::ModelActivationFailed
            | Self::ModelOperationCompleted
            | Self::ModelOperationFailed => ApplicationLogComponent::Model,
            Self::ServiceDegraded | Self::ServiceRecovered | Self::ServiceFailed => {
                ApplicationLogComponent::Service
            }
            Self::WindowVisibilityChanged | Self::WindowStatePersisted => {
                ApplicationLogComponent::Window
            }
            Self::InputStatusChanged | Self::InputPermissionUnavailable => {
                ApplicationLogComponent::Input
            }
            Self::UpdateUnavailable
            | Self::UpdateCheckCompleted
            | Self::UpdateCheckFailed
            | Self::UpdatePhaseChanged
            | Self::UpdateInstallStarted
            | Self::UpdateInstallCompleted
            | Self::UpdateInstallFailed => ApplicationLogComponent::Update,
            Self::UiTransportFailed => ApplicationLogComponent::Ui,
            Self::FilesystemOperationFailed => ApplicationLogComponent::Filesystem,
            Self::NetworkOperationFailed => ApplicationLogComponent::Network,
            Self::ParsingFailed => ApplicationLogComponent::Parser,
        }
    }

    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Started => "Application started",
            Self::PreviousRunUnclean => "The previous run ended unexpectedly",
            Self::ShutdownStarted => "Application shutdown started",
            Self::ShutdownCompleted => "Application shutdown completed",
            Self::ShutdownFailed => "Application shutdown failed",
            Self::Panicked => "Application panicked",
            Self::LoggingSettingsChanged => "Logging settings changed",
            Self::RuntimeUnavailable => "Runtime service is unavailable",
            Self::DiagnosticsExportFailed => "Diagnostics export failed",
            Self::ModelSelectionFallback => {
                "The selected model was unavailable; restored the standard preset"
            }
            Self::StartupFailed => "Application startup failed",
            Self::StateRecovered => "Application state was recovered",
            Self::StatePersistFailed => "Application state could not be persisted",
            Self::ServiceDegraded => "A service is degraded",
            Self::ServiceRecovered => "A service recovered",
            Self::ServiceFailed => "A service failed",
            Self::ModelPrepareStarted => "Model preparation started",
            Self::ModelActivationFailed => "Model activation failed",
            Self::ModelOperationCompleted => "Model operation completed",
            Self::ModelOperationFailed => "Model operation failed",
            Self::WindowVisibilityChanged => "Model window visibility changed",
            Self::WindowStatePersisted => "Window state was persisted",
            Self::InputStatusChanged => "Input service status changed",
            Self::InputPermissionUnavailable => "Input monitoring permission is unavailable",
            Self::UpdateUnavailable => "Updates are unavailable",
            Self::UpdateCheckCompleted => "Update check completed",
            Self::UpdateCheckFailed => "Update check failed",
            Self::UpdatePhaseChanged => "Update phase changed",
            Self::UpdateInstallStarted => "Update installation started",
            Self::UpdateInstallCompleted => "Update installation completed",
            Self::UpdateInstallFailed => "Update installation failed",
            Self::SettingsCommandFailed => "Settings command failed",
            Self::UiTransportFailed => "Settings service transport failed",
            Self::FilesystemOperationFailed => "Filesystem operation failed",
            Self::NetworkOperationFailed => "Network operation failed",
            Self::ParsingFailed => "Data parsing failed",
        }
    }
}
