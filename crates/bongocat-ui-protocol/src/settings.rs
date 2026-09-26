//! The settings contract: what the window may ask for, and what it reads back.
//!
//! This module is the whole vocabulary both sides share. The types are split by
//! the question they answer — where the window is, what the runtime is doing,
//! what one read of the configuration looks like, what a model import is doing,
//! why anything was refused — and the two ends of the bounded channel live in
//! `client` and `endpoint`.

mod build;
mod client;
mod command;
mod endpoint;
mod error;
mod input_diagnostics;
mod logging;
mod model_catalog;
mod model_diagnostic;
mod model_import;
mod runtime;
mod snapshot;
mod startup;
#[cfg(test)]
mod tests;
mod window;

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use async_channel::{Receiver, Sender};

// The public surface, one module at a time. Nothing here is crate-private — this
// is the contract both sides speak — so the re-exports are the whole of it and
// no glob is needed to carry a second, narrower copy.
pub use build::{DIAGNOSTICS_EXPORT_FORMAT_VERSION, SettingsBuildEnvironment, SettingsBuildInfo};
pub use client::SettingsClient;
pub use command::{SettingsApplicationShortcut, SettingsCommand, SettingsReply};
pub use endpoint::{SettingsServiceClosed, SettingsServiceEndpoint};
pub use error::{SettingsError, SettingsErrorCode};
pub use input_diagnostics::{
    SettingsDiagnosticsExportStatus, SettingsInputDiagnostics, SettingsInputMonitoringPermission,
    SettingsInputServiceStatus,
};
pub use logging::{SettingsLanguage, SettingsLogLevel, SettingsLogging};
pub use model_catalog::{
    SettingsModelAvailability, SettingsModelBehavior, SettingsModelCatalog, SettingsModelEntry,
    SettingsModelKey, SettingsModelOrigin,
};
pub use model_diagnostic::{SettingsModelCatalogError, SettingsModelDiagnostic};
pub use model_import::{
    SettingsModelImportControl, SettingsModelImportFinalResult, SettingsModelImportMonitor,
    SettingsModelImportOperation, SettingsModelImportProgress, SettingsModelImportRequest,
    SettingsModelImportStage, SettingsModelMode, SettingsModelSourceContent, SettingsMverMode,
    SettingsOperationId, model_source_display_name,
};
pub use runtime::{
    RuntimeHealth, SettingsRuntimeCommandFailure, SettingsRuntimeCommandTransportDiagnostics,
    SettingsRuntimeDiagnostics, SettingsRuntimeErrorCode,
};
pub use snapshot::{
    AutomaticUpdateSettings, SettingsGamepadAutoSwitch, SettingsGamepadAxisSettings,
    SettingsModelBehaviorBinding, SettingsModelSettings, SettingsOverlay, SettingsRandomBehavior,
    SettingsShortcutBinding, SettingsShortcuts, SettingsSnapshot, SettingsTheme,
};
pub use startup::{
    SettingsStartupItemError, SettingsStartupItemState, SettingsStartupItemStatus,
    SettingsStartupItemUnsupportedReason,
};
pub use window::{
    MAX_SETTINGS_WINDOW_COORDINATE, MAX_SETTINGS_WINDOW_DIMENSION, MIN_SETTINGS_WINDOW_HEIGHT,
    MIN_SETTINGS_WINDOW_WIDTH, SettingsWindowPlacement, SettingsWindowState,
};
