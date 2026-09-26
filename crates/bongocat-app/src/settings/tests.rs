//! Test fixtures shared by the settings service test modules.
//!
//! The tests live in this directory rather than beside the code they cover, so a
//! production module holds only production code. The capability doubles here are
//! the whole point: a test states what the file manager, the login item, the tray
//! icon or the diagnostics export did, without any of them running.

use super::capabilities::*;
use super::error_mapping::*;
use super::projection::*;
use super::snapshot::*;
use super::*;
use bongocat_config::{
    ConfigStore, OverlayWindowPlacement, StorageLayout, WINDOW_STATE_WRITER_LOCK_FILE_NAME,
    WindowStateStore,
};
use bongocat_input::{InputDiagnostics, InputEvent, InputTransportDiagnostics, MonotonicMillis};
use bongocat_runtime::{RuntimeOwner, RuntimeWorkDiagnostics};
use bongocat_ui_protocol::{
    DIAGNOSTICS_EXPORT_FORMAT_VERSION, SettingsModelImportRequest, SettingsModelOrigin,
    SettingsStartupItemError,
};
use std::{
    fs, io,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tempfile::tempdir;

mod lifecycle;
mod model_import;
mod models;
mod projection;
mod service;
mod shortcuts;
mod window_state;

struct TestStartupItem {
    status: Mutex<SettingsStartupItemStatus>,
    fail_updates: AtomicBool,
}

impl TestStartupItem {
    fn new(status: SettingsStartupItemStatus) -> Self {
        Self {
            status: Mutex::new(status),
            fail_updates: AtomicBool::new(false),
        }
    }

    fn replace(&self, status: SettingsStartupItemStatus) {
        *self
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = status;
    }
}

impl StartupItemCapability for TestStartupItem {
    fn state(&self) -> SettingsStartupItemStatus {
        *self
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set_enabled(&self, enabled: bool) -> Result<SettingsStartupItemState, SettingsError> {
        if self.fail_updates.load(Ordering::Acquire) {
            return Err(SettingsError::new(
                SettingsErrorCode::StartupItemUpdateFailed,
            ));
        }
        let state = if enabled {
            SettingsStartupItemState::Enabled
        } else {
            SettingsStartupItemState::Disabled
        };
        self.replace(SettingsStartupItemStatus::State(state));
        Ok(state)
    }
}

struct TestBackupLocation {
    invocations: AtomicUsize,
    fail: AtomicBool,
}

struct TestStatusIcon {
    visible: Mutex<bool>,
    updates: Mutex<Vec<bool>>,
    fail_updates: AtomicBool,
}

struct TestTaskbarIcon {
    visible: Mutex<bool>,
    updates: Mutex<Vec<bool>>,
    fail_updates: AtomicBool,
}

impl TestStatusIcon {
    fn new(visible: bool) -> Self {
        Self {
            visible: Mutex::new(visible),
            updates: Mutex::new(Vec::new()),
            fail_updates: AtomicBool::new(false),
        }
    }

    fn visible(&self) -> bool {
        *self
            .visible
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn updates(&self) -> Vec<bool> {
        self.updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl TestTaskbarIcon {
    fn new(visible: bool) -> Self {
        Self {
            visible: Mutex::new(visible),
            updates: Mutex::new(Vec::new()),
            fail_updates: AtomicBool::new(false),
        }
    }

    fn visible(&self) -> bool {
        *self
            .visible
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn updates(&self) -> Vec<bool> {
        self.updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl StatusIconCapability for TestStatusIcon {
    fn set_visible(&self, visible: bool) -> Result<(), SettingsError> {
        self.updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(visible);
        if self.fail_updates.load(Ordering::Acquire) {
            return Err(SettingsError::new(
                SettingsErrorCode::StatusIconUpdateFailed,
            ));
        }
        *self
            .visible
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = visible;
        Ok(())
    }
}

impl TaskbarIconCapability for TestTaskbarIcon {
    fn set_visible(&self, visible: bool) -> Result<(), SettingsError> {
        self.updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(visible);
        if self.fail_updates.load(Ordering::Acquire) {
            return Err(SettingsError::new(
                SettingsErrorCode::TaskbarIconUpdateFailed,
            ));
        }
        *self
            .visible
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = visible;
        Ok(())
    }
}

impl TestBackupLocation {
    fn new() -> Self {
        Self {
            invocations: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
        }
    }
}

impl BackupLocationCapability for TestBackupLocation {
    fn open(&self) -> Result<(), SettingsError> {
        self.invocations.fetch_add(1, Ordering::AcqRel);
        if self.fail.load(Ordering::Acquire) {
            Err(SettingsError::new(
                SettingsErrorCode::BackupLocationOpenFailed,
            ))
        } else {
            Ok(())
        }
    }
}

struct TestModelLocation {
    opened: Mutex<Vec<PathBuf>>,
    fail: AtomicBool,
}

impl TestModelLocation {
    fn new() -> Self {
        Self {
            opened: Mutex::new(Vec::new()),
            fail: AtomicBool::new(false),
        }
    }

    fn opened(&self) -> Vec<PathBuf> {
        self.opened
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl ModelLocationCapability for TestModelLocation {
    fn open(&self, path: &Path) -> Result<(), SettingsError> {
        self.opened
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(path.to_owned());
        if self.fail.load(Ordering::Acquire) {
            Err(SettingsError::new(
                SettingsErrorCode::ModelLocationOpenFailed,
            ))
        } else {
            Ok(())
        }
    }
}

struct TestLogLocation {
    invocations: AtomicUsize,
    fail: AtomicBool,
}

impl TestLogLocation {
    fn new() -> Self {
        Self {
            invocations: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
        }
    }
}

impl LogLocationCapability for TestLogLocation {
    fn open(&self) -> Result<(), SettingsError> {
        self.invocations.fetch_add(1, Ordering::AcqRel);
        if self.fail.load(Ordering::Acquire) {
            Err(SettingsError::new(SettingsErrorCode::LogLocationOpenFailed))
        } else {
            Ok(())
        }
    }
}

struct TestDiagnosticsExport;

impl DiagnosticsExportCapability for TestDiagnosticsExport {
    fn export(
        &self,
        _snapshot: &SettingsSnapshot,
        _application_logs: ApplicationLogDiagnostics,
        _core_logs: Option<CoreLogDiagnostics>,
        _update: Option<UpdateDiagnostics>,
    ) -> Result<SettingsDiagnosticsExportStatus, SettingsError> {
        Ok(SettingsDiagnosticsExportStatus {
            format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
            bytes_written: 1,
            preview_bundle_format_version: 1,
            preview_bundle_bytes_written: 2,
            preview_bundle_entry_count: 3,
            preview_bundle_skipped_source_files: 0,
        })
    }
}

struct FailingDiagnosticsExport {
    calls: AtomicUsize,
}

impl DiagnosticsExportCapability for FailingDiagnosticsExport {
    fn export(
        &self,
        _snapshot: &SettingsSnapshot,
        _application_logs: ApplicationLogDiagnostics,
        _core_logs: Option<CoreLogDiagnostics>,
        _update: Option<UpdateDiagnostics>,
    ) -> Result<SettingsDiagnosticsExportStatus, SettingsError> {
        if self.calls.fetch_add(1, Ordering::AcqRel) == 0 {
            Ok(SettingsDiagnosticsExportStatus {
                format_version: DIAGNOSTICS_EXPORT_FORMAT_VERSION,
                bytes_written: 11,
                preview_bundle_format_version: 1,
                preview_bundle_bytes_written: 22,
                preview_bundle_entry_count: 3,
                preview_bundle_skipped_source_files: 0,
            })
        } else {
            Err(SettingsError::new(
                SettingsErrorCode::DiagnosticsExportFailed,
            ))
        }
    }
}

fn model_fixture() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .join("shared/fixtures/model-fixtures/cases/非 ASCII 模型")
}

/// Seed the environment model store with a package stored under an exact
/// id. Imports always generate UUID ids, so a store entry whose id
/// collides with a preset id can only be produced through this direct
/// seeding; the merged catalog must still keep both identities.
fn seed_installed_model(models_root: &std::path::Path, id: &str) {
    let destination = models_root.join(id);
    copy_model_fixture_tree(&model_fixture(), &destination);
}

fn copy_model_fixture_tree(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).expect("seeded model directory");
    for entry in std::fs::read_dir(source).expect("fixture entries") {
        let entry = entry.expect("fixture entry");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("fixture file type").is_dir() {
            copy_model_fixture_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("seeded package file");
        }
    }
}

fn shortcut_fixture() -> SettingsShortcuts {
    shortcut_fixture_with(
        "toggle_overlay",
        "Control+Alt+B",
        "motion:TapBody:0",
        "Control+Alt+M",
    )
}

fn shortcut_fixture_with(
    command: &str,
    command_shortcut: &str,
    behavior_id: &str,
    behavior_shortcut: &str,
) -> SettingsShortcuts {
    SettingsShortcuts {
        commands: vec![SettingsShortcutBinding {
            command: command.to_owned(),
            shortcut: command_shortcut.to_owned(),
        }],
        model_behaviors: vec![SettingsModelBehaviorBinding {
            model: SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::BuiltIn,
            },
            behavior_id: behavior_id.to_owned(),
            shortcut: behavior_shortcut.to_owned(),
        }],
    }
}
