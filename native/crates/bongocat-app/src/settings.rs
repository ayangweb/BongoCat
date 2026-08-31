use crate::{Application, ApplicationError};
use bongocat_model::{
    ModelCatalogEntry, ModelDiagnostic, ModelImportProgress, ModelImportStage, ModelOrigin,
    ModelStoreDiagnostic,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_platform::{
    StartupItemEnvironment, StartupItemError, StartupItemState, StartupItemUnsupportedReason,
    set_startup_item_enabled, startup_item_state,
};
use bongocat_runtime::RuntimeState;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_ui::SettingsStartupItemError;
use bongocat_ui::{
    RuntimeHealth, SettingsClient, SettingsCommand, SettingsError, SettingsErrorCode,
    SettingsModelAvailability, SettingsModelCatalog, SettingsModelCatalogError,
    SettingsModelDiagnostic, SettingsModelEntry, SettingsModelImportProgress,
    SettingsModelImportStage, SettingsModelKey, SettingsModelOrigin, SettingsServiceEndpoint,
    SettingsSnapshot, SettingsStartupItemState, SettingsStartupItemStatus,
    SettingsStartupItemUnsupportedReason,
};
use std::{fmt, sync::Arc, thread};

const SETTINGS_COMMAND_CAPACITY: usize = 16;

pub struct ApplicationSettingsService {
    client: SettingsClient,
    worker: Option<thread::JoinHandle<()>>,
}

impl ApplicationSettingsService {
    pub fn start(application: Application) -> Result<Self, SettingsServiceJoinError> {
        Self::start_with_startup_item(application, Arc::new(SystemStartupItem))
    }

    fn start_with_startup_item(
        application: Application,
        startup_item: Arc<dyn StartupItemCapability>,
    ) -> Result<Self, SettingsServiceJoinError> {
        let (client, endpoint) = SettingsClient::bounded(SETTINGS_COMMAND_CAPACITY);
        let worker = thread::Builder::new()
            .name("bongocat-settings-service".to_owned())
            .spawn(move || run_service(application, endpoint, startup_item))
            .map_err(SettingsServiceJoinError::Spawn)?;
        Ok(Self {
            client,
            worker: Some(worker),
        })
    }

    pub fn client(&self) -> SettingsClient {
        self.client.clone()
    }

    pub fn join(mut self) -> Result<(), SettingsServiceJoinError> {
        self.worker
            .take()
            .expect("settings service worker is present")
            .join()
            .map_err(|_| SettingsServiceJoinError::Panicked)
    }
}

trait StartupItemCapability: Send + Sync + 'static {
    fn state(&self) -> SettingsStartupItemStatus;

    fn set_enabled(&self, enabled: bool) -> Result<SettingsStartupItemState, SettingsError>;
}

struct SystemStartupItem;

impl StartupItemCapability for SystemStartupItem {
    fn state(&self) -> SettingsStartupItemStatus {
        system_startup_item_state()
    }

    fn set_enabled(&self, enabled: bool) -> Result<SettingsStartupItemState, SettingsError> {
        system_set_startup_item_enabled(enabled)
    }
}

impl Drop for ApplicationSettingsService {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = self.client.shutdown_blocking();
            let _ = worker.join();
        }
    }
}

#[derive(Debug)]
pub enum SettingsServiceJoinError {
    Spawn(std::io::Error),
    Panicked,
}

impl fmt::Display for SettingsServiceJoinError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "failed to start settings service: {error}"),
            Self::Panicked => formatter.write_str("settings service panicked"),
        }
    }
}

impl std::error::Error for SettingsServiceJoinError {}

fn run_service(
    mut application: Application,
    endpoint: SettingsServiceEndpoint,
    startup_item: Arc<dyn StartupItemCapability>,
) {
    let mut clock = SettingsSnapshotClock::new(application.runtime_client().snapshot().revision);
    loop {
        let Ok(command) = endpoint.recv_blocking() else {
            let _ = application.shutdown();
            break;
        };
        match command {
            SettingsCommand::ReadSnapshot { reply } => {
                let _ = reply.respond(Ok(snapshot(
                    &application,
                    &mut clock,
                    false,
                    startup_item.state(),
                )));
            }
            SettingsCommand::SetOverlayVisible { visible, reply } => {
                let result = application
                    .set_overlay_visible(visible)
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()))
                    .map_err(map_application_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::SetMotionAudioEnabled { enabled, reply } => {
                let result = application
                    .set_motion_audio_enabled(enabled)
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()))
                    .map_err(map_application_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::SetStartupItemEnabled { enabled, reply } => {
                let result = startup_item.set_enabled(enabled).map(|state| {
                    snapshot(
                        &application,
                        &mut clock,
                        false,
                        SettingsStartupItemStatus::State(state),
                    )
                });
                let _ = reply.respond(result);
            }
            SettingsCommand::SelectModel { model, reply } => {
                let result = application
                    .select_model(model_origin(model.origin), model.id)
                    .map(|_| snapshot(&application, &mut clock, false, startup_item.state()))
                    .map_err(map_application_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::ImportModel {
                request,
                operation,
                reply,
            } => {
                let progress = operation.clone();
                let cancellation = operation.clone();
                let result = application
                    .import_model_with_observer(
                        request.id,
                        request.source_root,
                        move |update| {
                            let _ = progress.report_progress(settings_import_progress(update));
                        },
                        move || cancellation.is_cancelled(),
                    )
                    .map(|_| snapshot(&application, &mut clock, true, startup_item.state()))
                    .map_err(map_model_import_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::DeleteModel { model, reply } => {
                let result = application
                    .delete_model(model_origin(model.origin), model.id)
                    .map(|_| snapshot(&application, &mut clock, true, startup_item.state()))
                    .map_err(map_model_delete_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::Shutdown { reply } => {
                let before_shutdown =
                    snapshot(&application, &mut clock, false, startup_item.state());
                let result = application.shutdown().map(|stopped| {
                    clock.observe_runtime(stopped.revision);
                    SettingsSnapshot {
                        revision: clock.revision,
                        runtime_health: RuntimeHealth::Stopped,
                        ..before_shutdown
                    }
                });
                let _ = reply.respond(
                    result.map_err(|_| SettingsError::new(SettingsErrorCode::ShutdownFailed)),
                );
                break;
            }
        }
    }
}

const fn settings_import_progress(progress: ModelImportProgress) -> SettingsModelImportProgress {
    SettingsModelImportProgress {
        stage: match progress.stage {
            ModelImportStage::Preparing => SettingsModelImportStage::Preparing,
            ModelImportStage::Copying => SettingsModelImportStage::Copying,
            ModelImportStage::Validating => SettingsModelImportStage::Validating,
            ModelImportStage::Committing => SettingsModelImportStage::Committing,
        },
        files_copied: progress.files_copied,
        bytes_copied: progress.bytes_copied,
    }
}

struct SettingsSnapshotClock {
    revision: u64,
    observed_runtime_revision: u64,
    observed_startup_item: Option<SettingsStartupItemStatus>,
}

impl SettingsSnapshotClock {
    const fn new(runtime_revision: u64) -> Self {
        Self {
            revision: runtime_revision,
            observed_runtime_revision: runtime_revision,
            observed_startup_item: None,
        }
    }

    fn observe_runtime(&mut self, runtime_revision: u64) {
        if runtime_revision != self.observed_runtime_revision {
            self.revision = self.revision.saturating_add(1);
            self.observed_runtime_revision = runtime_revision;
        }
    }

    fn mark_catalog_changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    fn observe_startup_item(&mut self, status: SettingsStartupItemStatus) {
        match self.observed_startup_item.replace(status) {
            Some(previous) if previous != status => {
                self.revision = self.revision.saturating_add(1);
            }
            Some(_) | None => {}
        }
    }
}

fn snapshot(
    application: &Application,
    clock: &mut SettingsSnapshotClock,
    catalog_changed: bool,
    startup_item: SettingsStartupItemStatus,
) -> SettingsSnapshot {
    let runtime = application.runtime_client().snapshot();
    clock.observe_runtime(runtime.revision);
    clock.observe_startup_item(startup_item);
    if catalog_changed {
        clock.mark_catalog_changed();
    }
    SettingsSnapshot {
        revision: clock.revision,
        runtime_health: match runtime.state {
            RuntimeState::Starting => RuntimeHealth::Starting,
            RuntimeState::Ready => RuntimeHealth::Ready,
            RuntimeState::Degraded | RuntimeState::Stopping => RuntimeHealth::Degraded,
            RuntimeState::Stopped => RuntimeHealth::Stopped,
        },
        overlay_visible: runtime.overlay_visible,
        motion_audio_enabled: runtime.motion_audio_enabled,
        startup_item,
        active_model: runtime
            .active_model
            .and_then(|model| {
                application
                    .active_model_origin()
                    .map(|origin| SettingsModelKey {
                        id: model.id.as_str().to_owned(),
                        origin: settings_model_origin(origin),
                    })
            })
            .or_else(|| configured_model_key(application)),
        model_catalog: settings_model_catalog(application),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn system_startup_item_state() -> SettingsStartupItemStatus {
    startup_item_state(startup_item_environment())
        .map(settings_startup_item_state)
        .map(SettingsStartupItemStatus::State)
        .unwrap_or_else(|error| {
            SettingsStartupItemStatus::ReadError(settings_startup_item_error(error))
        })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const fn system_startup_item_state() -> SettingsStartupItemStatus {
    SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
        SettingsStartupItemUnsupportedReason::Platform,
    ))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn system_set_startup_item_enabled(
    enabled: bool,
) -> Result<SettingsStartupItemState, SettingsError> {
    set_startup_item_enabled(startup_item_environment(), enabled)
        .map(settings_startup_item_state)
        .map_err(|_| SettingsError::new(SettingsErrorCode::StartupItemUpdateFailed))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_set_startup_item_enabled(
    _enabled: bool,
) -> Result<SettingsStartupItemState, SettingsError> {
    Err(SettingsError::new(
        SettingsErrorCode::StartupItemUpdateFailed,
    ))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
const fn startup_item_environment() -> StartupItemEnvironment {
    match crate::BUILD_ENVIRONMENT {
        bongocat_config::BuildEnvironment::Development => StartupItemEnvironment::Development,
        bongocat_config::BuildEnvironment::Production => StartupItemEnvironment::Production,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
const fn settings_startup_item_state(state: StartupItemState) -> SettingsStartupItemState {
    match state {
        StartupItemState::Unsupported(reason) => {
            SettingsStartupItemState::Unsupported(match reason {
                StartupItemUnsupportedReason::Platform => {
                    SettingsStartupItemUnsupportedReason::Platform
                }
                StartupItemUnsupportedReason::OperatingSystem => {
                    SettingsStartupItemUnsupportedReason::OperatingSystem
                }
                StartupItemUnsupportedReason::BuildEnvironment => {
                    SettingsStartupItemUnsupportedReason::BuildEnvironment
                }
            })
        }
        StartupItemState::Disabled => SettingsStartupItemState::Disabled,
        StartupItemState::Enabled => SettingsStartupItemState::Enabled,
        StartupItemState::Stale => SettingsStartupItemState::Stale,
        StartupItemState::RequiresApproval => SettingsStartupItemState::RequiresApproval,
        StartupItemState::NotFound => SettingsStartupItemState::NotFound,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
const fn settings_startup_item_error(error: StartupItemError) -> SettingsStartupItemError {
    match error {
        StartupItemError::CurrentExecutableUnavailable => {
            SettingsStartupItemError::CurrentExecutableUnavailable
        }
        StartupItemError::InvalidExecutablePath => SettingsStartupItemError::InvalidExecutablePath,
        StartupItemError::BackendUnavailable => SettingsStartupItemError::BackendUnavailable,
        StartupItemError::StateReadFailed => SettingsStartupItemError::StateReadFailed,
        StartupItemError::EnableFailed => SettingsStartupItemError::EnableFailed,
        StartupItemError::DisableFailed => SettingsStartupItemError::DisableFailed,
    }
}

fn settings_model_catalog(application: &Application) -> SettingsModelCatalog {
    match application.model_catalog() {
        Ok(entries) => SettingsModelCatalog {
            entries: entries.into_iter().map(settings_model_entry).collect(),
            error: None,
        },
        Err(_) => SettingsModelCatalog {
            entries: Vec::new(),
            error: Some(SettingsModelCatalogError::Unavailable),
        },
    }
}

fn configured_model_key(application: &Application) -> Option<SettingsModelKey> {
    let id = application.config().model.selected_model_id.clone()?;
    let origin = application.config().model.selected_model_origin?;
    Some(SettingsModelKey {
        id,
        origin: match origin {
            bongocat_config::SelectedModelOrigin::Preset => SettingsModelOrigin::Preset,
            bongocat_config::SelectedModelOrigin::Installed => SettingsModelOrigin::Installed,
        },
    })
}

const fn settings_model_origin(origin: ModelOrigin) -> SettingsModelOrigin {
    match origin {
        ModelOrigin::Preset => SettingsModelOrigin::Preset,
        ModelOrigin::Installed => SettingsModelOrigin::Installed,
    }
}

const fn model_origin(origin: SettingsModelOrigin) -> ModelOrigin {
    match origin {
        SettingsModelOrigin::Preset => ModelOrigin::Preset,
        SettingsModelOrigin::Installed => ModelOrigin::Installed,
    }
}

fn settings_model_entry(entry: ModelCatalogEntry) -> SettingsModelEntry {
    let id = entry.id().as_str().to_owned();
    let origin = match entry.origin() {
        ModelOrigin::Preset => SettingsModelOrigin::Preset,
        ModelOrigin::Installed => SettingsModelOrigin::Installed,
    };
    let availability = match entry {
        ModelCatalogEntry::Ready { snapshot, .. } => SettingsModelAvailability::Ready {
            texture_count: snapshot.texture_count,
            expression_count: snapshot.expression_count,
            motion_count: snapshot.motion_count,
        },
        ModelCatalogEntry::Invalid { code, .. } => SettingsModelAvailability::Invalid {
            diagnostic: settings_model_diagnostic(code),
        },
    };
    SettingsModelEntry {
        id,
        origin,
        availability,
    }
}

const fn settings_model_diagnostic(diagnostic: ModelDiagnostic) -> SettingsModelDiagnostic {
    match diagnostic {
        ModelDiagnostic::InvalidModelId => SettingsModelDiagnostic::InvalidModelId,
        ModelDiagnostic::ModelEntryAmbiguous => SettingsModelDiagnostic::ModelEntryAmbiguous,
        ModelDiagnostic::ModelEntryMissing => SettingsModelDiagnostic::ModelEntryMissing,
        ModelDiagnostic::ModelFileCountExceeded => SettingsModelDiagnostic::ModelFileCountExceeded,
        ModelDiagnostic::ModelFileTooLarge => SettingsModelDiagnostic::ModelFileTooLarge,
        ModelDiagnostic::ModelIoError => SettingsModelDiagnostic::ModelIoError,
        ModelDiagnostic::ModelJsonInvalid => SettingsModelDiagnostic::ModelJsonInvalid,
        ModelDiagnostic::ModelJsonTooLarge => SettingsModelDiagnostic::ModelJsonTooLarge,
        ModelDiagnostic::ModelMocMissing => SettingsModelDiagnostic::ModelMocMissing,
        ModelDiagnostic::ModelPackageDepthExceeded => {
            SettingsModelDiagnostic::ModelPackageDepthExceeded
        }
        ModelDiagnostic::ModelPackageSizeExceeded => {
            SettingsModelDiagnostic::ModelPackageSizeExceeded
        }
        ModelDiagnostic::ModelReferenceEscapesRoot => {
            SettingsModelDiagnostic::ModelReferenceEscapesRoot
        }
        ModelDiagnostic::ModelReferenceInvalid => SettingsModelDiagnostic::ModelReferenceInvalid,
        ModelDiagnostic::ModelReferenceSymlinkEscape => {
            SettingsModelDiagnostic::ModelReferenceSymlinkEscape
        }
        ModelDiagnostic::ModelResourceInvalid => SettingsModelDiagnostic::ModelResourceInvalid,
        ModelDiagnostic::ModelResourceMissing => SettingsModelDiagnostic::ModelResourceMissing,
        ModelDiagnostic::ModelResourceNotFile => SettingsModelDiagnostic::ModelResourceNotFile,
        ModelDiagnostic::ModelSymlinkDirectoryUnsupported => {
            SettingsModelDiagnostic::ModelSymlinkDirectoryUnsupported
        }
        ModelDiagnostic::ModelTextureDimensionExceeded => {
            SettingsModelDiagnostic::ModelTextureDimensionExceeded
        }
        ModelDiagnostic::ModelTextureInvalidPng => SettingsModelDiagnostic::ModelTextureInvalidPng,
        ModelDiagnostic::ModelTextureMissing => SettingsModelDiagnostic::ModelTextureMissing,
        ModelDiagnostic::ModelUnsupportedVersion => {
            SettingsModelDiagnostic::ModelUnsupportedVersion
        }
    }
}

fn map_application_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::PlatformStorage(_)
        | ApplicationError::Config(_)
        | ApplicationError::ConfigRollback(_) => SettingsErrorCode::ConfigPersistFailed,
        ApplicationError::Model(_) | ApplicationError::ModelStore(_) => {
            SettingsErrorCode::ModelUnavailable
        }
        ApplicationError::Shutdown(_) | ApplicationError::MotionAudioShutdown(_) => {
            SettingsErrorCode::ShutdownFailed
        }
        ApplicationError::RuntimeCommand(_)
        | ApplicationError::RuntimeCommandFailed(_)
        | ApplicationError::RuntimeDidNotPublish
        | ApplicationError::RuntimeDidNotPrepareModel => SettingsErrorCode::ModelSwitchFailed,
        _ => SettingsErrorCode::RuntimeUnavailable,
    };
    SettingsError::new(code)
}

fn map_model_import_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Model(error) => {
            if error.code == ModelDiagnostic::InvalidModelId {
                SettingsErrorCode::InvalidModelId
            } else {
                SettingsErrorCode::ModelImportInvalidPackage
            }
        }
        ApplicationError::ModelStore(error) => map_model_store_import_diagnostic(error.code),
        error => return map_application_error(error),
    };
    SettingsError::new(code)
}

const fn map_model_store_import_diagnostic(diagnostic: ModelStoreDiagnostic) -> SettingsErrorCode {
    match diagnostic {
        ModelStoreDiagnostic::AlreadyExists => SettingsErrorCode::ModelAlreadyInstalled,
        ModelStoreDiagnostic::Cancelled => SettingsErrorCode::ModelImportCancelled,
        ModelStoreDiagnostic::InvalidPackage => SettingsErrorCode::ModelImportInvalidPackage,
        ModelStoreDiagnostic::SourceContainsStore => SettingsErrorCode::ModelImportSourceInvalid,
        ModelStoreDiagnostic::SourceChanged => SettingsErrorCode::ModelImportSourceChanged,
        ModelStoreDiagnostic::SourceSymlinkUnsupported
        | ModelStoreDiagnostic::SourceEntryUnsupported => {
            SettingsErrorCode::ModelImportSourceUnsupported
        }
        ModelStoreDiagnostic::StoreBusy => SettingsErrorCode::ModelStoreBusy,
        ModelStoreDiagnostic::IoError
        | ModelStoreDiagnostic::NotFound
        | ModelStoreDiagnostic::StoreEntryUnsupported => SettingsErrorCode::ModelImportFailed,
    }
}

fn map_model_delete_error(error: ApplicationError) -> SettingsError {
    let code = match error {
        ApplicationError::Model(error) if error.code == ModelDiagnostic::InvalidModelId => {
            SettingsErrorCode::InvalidModelId
        }
        ApplicationError::PresetModelDeletion(_) => SettingsErrorCode::PresetModelCannotBeDeleted,
        ApplicationError::SelectedModelDeletion(_) => {
            SettingsErrorCode::SelectedModelCannotBeDeleted
        }
        ApplicationError::ModelStore(error) => map_model_store_delete_diagnostic(error.code),
        error => return map_application_error(error),
    };
    SettingsError::new(code)
}

const fn map_model_store_delete_diagnostic(diagnostic: ModelStoreDiagnostic) -> SettingsErrorCode {
    match diagnostic {
        ModelStoreDiagnostic::NotFound => SettingsErrorCode::ModelNotInstalled,
        ModelStoreDiagnostic::StoreBusy => SettingsErrorCode::ModelStoreBusy,
        ModelStoreDiagnostic::AlreadyExists
        | ModelStoreDiagnostic::Cancelled
        | ModelStoreDiagnostic::InvalidPackage
        | ModelStoreDiagnostic::IoError
        | ModelStoreDiagnostic::SourceContainsStore
        | ModelStoreDiagnostic::SourceChanged
        | ModelStoreDiagnostic::SourceSymlinkUnsupported
        | ModelStoreDiagnostic::SourceEntryUnsupported
        | ModelStoreDiagnostic::StoreEntryUnsupported => SettingsErrorCode::ModelDeleteFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_config::StorageLayout;
    use bongocat_ui::{SettingsModelImportRequest, SettingsStartupItemError};
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use tempfile::tempdir;

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

    fn model_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .join("shared/fixtures/model-fixtures/cases/非 ASCII 模型")
    }

    #[test]
    fn service_orders_updates_persists_them_and_stops_runtime() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        assert_eq!(initial.model_catalog.entries.len(), 3);
        assert!(initial.model_catalog.error.is_none());
        assert!(initial.model_catalog.entries.iter().all(|entry| {
            entry.origin == SettingsModelOrigin::Preset
                && matches!(entry.availability, SettingsModelAvailability::Ready { .. })
        }));
        let selected = client
            .select_model_blocking(SettingsModelKey {
                id: "keyboard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            })
            .expect("select preset model");
        assert_eq!(
            selected.active_model,
            Some(SettingsModelKey {
                id: "keyboard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            })
        );
        let hidden = client
            .set_overlay_visible_blocking(false)
            .expect("hide overlay");
        let muted = client
            .set_motion_audio_enabled_blocking(false)
            .expect("disable motion audio");
        assert!(hidden.revision > initial.revision);
        assert!(muted.revision > hidden.revision);
        assert!(!muted.overlay_visible);
        assert!(!muted.motion_audio_enabled);

        let persisted = std::fs::read_to_string(config_path).expect("persisted config");
        assert!(persisted.contains("\"visible\": false"));
        assert!(persisted.contains("\"play_motion_audio\": false"));
        assert!(persisted.contains("\"selected_model_id\": \"keyboard\""));
        assert!(persisted.contains("\"selected_model_origin\": \"preset\""));

        let stopped = client.shutdown_blocking().expect("service shutdown");
        assert_eq!(stopped.runtime_health, RuntimeHealth::Stopped);
        service.join().expect("service join");
    }

    #[test]
    fn service_observes_and_updates_startup_item_without_touching_config() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        let startup_item = Arc::new(TestStartupItem::new(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Disabled,
        )));
        let service =
            ApplicationSettingsService::start_with_startup_item(application, startup_item.clone())
                .expect("service start");
        let client = service.client();

        let initial = client.read_snapshot_blocking().expect("initial snapshot");
        let initial_config = std::fs::read(&config_path).expect("initial config");
        assert_eq!(
            initial.startup_item,
            SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled)
        );

        startup_item.replace(SettingsStartupItemStatus::ReadError(
            SettingsStartupItemError::StateReadFailed,
        ));
        let read_failed = client
            .read_snapshot_blocking()
            .expect("read failure remains a snapshot");
        assert!(read_failed.revision > initial.revision);
        assert_eq!(
            read_failed.startup_item,
            SettingsStartupItemStatus::ReadError(SettingsStartupItemError::StateReadFailed)
        );

        startup_item.replace(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Stale,
        ));
        let externally_changed = client.read_snapshot_blocking().expect("external change");
        assert!(externally_changed.revision > read_failed.revision);
        assert_eq!(
            externally_changed.startup_item,
            SettingsStartupItemStatus::State(SettingsStartupItemState::Stale)
        );

        let enabled = client
            .set_startup_item_enabled_blocking(true)
            .expect("enable startup item");
        assert!(enabled.revision > externally_changed.revision);
        assert_eq!(
            enabled.startup_item,
            SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled)
        );
        assert_eq!(
            std::fs::read(&config_path).expect("unchanged config"),
            initial_config
        );

        startup_item.fail_updates.store(true, Ordering::Release);
        let failed = client
            .set_startup_item_enabled_blocking(false)
            .expect_err("failed startup update");
        assert_eq!(failed.code(), SettingsErrorCode::StartupItemUpdateFailed);
        let unchanged = client.read_snapshot_blocking().expect("unchanged state");
        assert_eq!(unchanged.revision, enabled.revision);
        assert_eq!(unchanged.startup_item, enabled.startup_item);
        assert_eq!(
            std::fs::read(&config_path).expect("config after failure"),
            initial_config
        );

        let stopped = client.shutdown_blocking().expect("service shutdown");
        assert_eq!(stopped.startup_item, unchanged.startup_item);
        service.join().expect("service join");
    }

    #[test]
    fn client_reports_closed_service_without_exposing_application_errors() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
        assert_eq!(
            client
                .read_snapshot_blocking()
                .expect_err("closed service")
                .code(),
            SettingsErrorCode::ServiceUnavailable
        );
    }

    #[test]
    fn service_imports_a_model_without_selecting_it_and_refreshes_the_catalog() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");

        let imported = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "custom-model".to_owned(),
                source_root: model_fixture(),
            })
            .expect("import model");
        assert!(imported.revision > initial.revision);
        assert_eq!(
            imported.active_model, None,
            "import must not implicitly activate the model"
        );
        assert!(imported.model_catalog.entries.iter().any(|entry| {
            entry.id == "custom-model"
                && entry.origin == SettingsModelOrigin::Installed
                && matches!(entry.availability, SettingsModelAvailability::Ready { .. })
        }));
        assert!(models_root.join("custom-model/猫.model3.json").is_file());

        let duplicate = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "custom-model".to_owned(),
                source_root: model_fixture(),
            })
            .expect_err("duplicate import");
        assert_eq!(duplicate.code(), SettingsErrorCode::ModelAlreadyInstalled);
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged.revision, imported.revision);
        assert_eq!(unchanged.model_catalog, imported.model_catalog);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_observes_import_cancellation_without_committing_or_revising_catalog() {
        let source = tempdir().expect("model source");
        std::fs::write(source.path().join("model.moc3"), b"moc").expect("moc");
        std::fs::write(
            source.path().join("cat.model3.json"),
            r#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        )
        .expect("model3");
        std::fs::File::create(source.path().join("payload.bin"))
            .and_then(|file| file.set_len(16 * 1024 * 1024))
            .expect("large payload");

        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        let initial = client.read_snapshot_blocking().expect("initial snapshot");

        let operation = client
            .start_model_import_blocking(SettingsModelImportRequest {
                id: "cancelled-model".to_owned(),
                source_root: source.path().to_owned(),
            })
            .expect("start import");
        let operation_id = operation.operation_id();
        assert!(operation.cancel());
        let final_result = operation.final_result_blocking();
        assert_eq!(final_result.operation_id, operation_id);
        assert_eq!(
            final_result.result.expect_err("cancelled import").code(),
            SettingsErrorCode::ModelImportCancelled
        );
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged.revision, initial.revision);
        assert!(!models_root.join("cancelled-model").exists());

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn every_model_store_import_diagnostic_has_a_stable_ui_code() {
        let cases = [
            (
                ModelStoreDiagnostic::AlreadyExists,
                SettingsErrorCode::ModelAlreadyInstalled,
            ),
            (
                ModelStoreDiagnostic::Cancelled,
                SettingsErrorCode::ModelImportCancelled,
            ),
            (
                ModelStoreDiagnostic::InvalidPackage,
                SettingsErrorCode::ModelImportInvalidPackage,
            ),
            (
                ModelStoreDiagnostic::SourceContainsStore,
                SettingsErrorCode::ModelImportSourceInvalid,
            ),
            (
                ModelStoreDiagnostic::SourceChanged,
                SettingsErrorCode::ModelImportSourceChanged,
            ),
            (
                ModelStoreDiagnostic::SourceSymlinkUnsupported,
                SettingsErrorCode::ModelImportSourceUnsupported,
            ),
            (
                ModelStoreDiagnostic::SourceEntryUnsupported,
                SettingsErrorCode::ModelImportSourceUnsupported,
            ),
            (
                ModelStoreDiagnostic::StoreBusy,
                SettingsErrorCode::ModelStoreBusy,
            ),
            (
                ModelStoreDiagnostic::IoError,
                SettingsErrorCode::ModelImportFailed,
            ),
            (
                ModelStoreDiagnostic::NotFound,
                SettingsErrorCode::ModelImportFailed,
            ),
            (
                ModelStoreDiagnostic::StoreEntryUnsupported,
                SettingsErrorCode::ModelImportFailed,
            ),
        ];

        for (diagnostic, expected) in cases {
            assert_eq!(map_model_store_import_diagnostic(diagnostic), expected);
        }
    }

    #[test]
    fn every_model_store_delete_diagnostic_has_a_stable_ui_code() {
        let cases = [
            (
                ModelStoreDiagnostic::NotFound,
                SettingsErrorCode::ModelNotInstalled,
            ),
            (
                ModelStoreDiagnostic::StoreBusy,
                SettingsErrorCode::ModelStoreBusy,
            ),
            (
                ModelStoreDiagnostic::AlreadyExists,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::Cancelled,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::InvalidPackage,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::IoError,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::SourceContainsStore,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::SourceChanged,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::SourceSymlinkUnsupported,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::SourceEntryUnsupported,
                SettingsErrorCode::ModelDeleteFailed,
            ),
            (
                ModelStoreDiagnostic::StoreEntryUnsupported,
                SettingsErrorCode::ModelDeleteFailed,
            ),
        ];

        for (diagnostic, expected) in cases {
            assert_eq!(map_model_store_delete_diagnostic(diagnostic), expected);
        }
    }

    #[test]
    fn service_deletes_only_unselected_installed_source_identity() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let imported = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "standard".to_owned(),
                source_root: model_fixture(),
            })
            .expect("import installed duplicate");
        let selected = client
            .select_model_blocking(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            })
            .expect("select preset duplicate");
        let deleted = client
            .delete_model_blocking(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::Installed,
            })
            .expect("delete installed duplicate");
        assert!(selected.revision > imported.revision);
        assert!(deleted.revision > selected.revision);
        assert_eq!(
            deleted.active_model,
            Some(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            })
        );
        assert!(!deleted.model_catalog.entries.iter().any(|entry| {
            entry.id == "standard" && entry.origin == SettingsModelOrigin::Installed
        }));
        assert!(deleted.model_catalog.entries.iter().any(|entry| {
            entry.id == "standard" && entry.origin == SettingsModelOrigin::Preset
        }));

        let preset_error = client
            .delete_model_blocking(SettingsModelKey {
                id: "standard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            })
            .expect_err("preset deletion");
        assert_eq!(
            preset_error.code(),
            SettingsErrorCode::PresetModelCannotBeDeleted
        );
        let missing_error = client
            .delete_model_blocking(SettingsModelKey {
                id: "missing".to_owned(),
                origin: SettingsModelOrigin::Installed,
            })
            .expect_err("missing installed model");
        assert_eq!(missing_error.code(), SettingsErrorCode::ModelNotInstalled);

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_rejects_deleting_the_selected_installed_model() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();
        client
            .import_model_blocking(SettingsModelImportRequest {
                id: "selected".to_owned(),
                source_root: model_fixture(),
            })
            .expect("import model");
        let selected = client
            .select_model_blocking(SettingsModelKey {
                id: "selected".to_owned(),
                origin: SettingsModelOrigin::Installed,
            })
            .expect("select installed model");

        let error = client
            .delete_model_blocking(SettingsModelKey {
                id: "selected".to_owned(),
                origin: SettingsModelOrigin::Installed,
            })
            .expect_err("selected deletion");
        assert_eq!(
            error.code(),
            SettingsErrorCode::SelectedModelCannotBeDeleted
        );
        let unchanged = client.read_snapshot_blocking().expect("unchanged snapshot");
        assert_eq!(unchanged.revision, selected.revision);
        assert!(unchanged.model_catalog.entries.iter().any(|entry| {
            entry.id == "selected" && entry.origin == SettingsModelOrigin::Installed
        }));

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn service_maps_invalid_model_inputs_to_stable_errors() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        let client = service.client();

        let invalid_id = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "../escape".to_owned(),
                source_root: model_fixture(),
            })
            .expect_err("invalid model id");
        assert_eq!(invalid_id.code(), SettingsErrorCode::InvalidModelId);

        let invalid_delete_id = client
            .delete_model_blocking(SettingsModelKey {
                id: "../escape".to_owned(),
                origin: SettingsModelOrigin::Installed,
            })
            .expect_err("invalid delete model id");
        assert_eq!(invalid_delete_id.code(), SettingsErrorCode::InvalidModelId);

        let invalid_package_source = tempdir().expect("invalid package");
        std::fs::write(
            invalid_package_source.path().join("not-a-model.txt"),
            b"invalid",
        )
        .expect("invalid model marker");
        let invalid_package = client
            .import_model_blocking(SettingsModelImportRequest {
                id: "invalid-package".to_owned(),
                source_root: invalid_package_source.path().to_owned(),
            })
            .expect_err("invalid package");
        assert_eq!(
            invalid_package.code(),
            SettingsErrorCode::ModelImportInvalidPackage
        );
        assert!(
            !invalid_package
                .to_string()
                .contains(&invalid_package_source.path().display().to_string())
        );

        client.shutdown_blocking().expect("service shutdown");
        service.join().expect("service join");
    }

    #[test]
    fn dropping_the_service_performs_a_fallback_shutdown_and_join() {
        let base = tempdir().expect("temporary storage");
        let layout = StorageLayout::under(base.path(), crate::BUILD_ENVIRONMENT);
        let application = Application::start_with_layout(layout).expect("application start");
        let service = ApplicationSettingsService::start(application).expect("service start");
        drop(service);
    }
}
