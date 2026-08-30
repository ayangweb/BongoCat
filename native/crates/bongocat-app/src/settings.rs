use crate::{Application, ApplicationError};
use bongocat_model::{ModelCatalogEntry, ModelDiagnostic, ModelOrigin, ModelStoreDiagnostic};
use bongocat_runtime::RuntimeState;
use bongocat_ui::{
    RuntimeHealth, SettingsClient, SettingsCommand, SettingsError, SettingsErrorCode,
    SettingsModelAvailability, SettingsModelCatalog, SettingsModelCatalogError,
    SettingsModelDiagnostic, SettingsModelEntry, SettingsModelKey, SettingsModelOrigin,
    SettingsServiceEndpoint, SettingsSnapshot,
};
use std::{fmt, thread};

const SETTINGS_COMMAND_CAPACITY: usize = 16;

pub struct ApplicationSettingsService {
    client: SettingsClient,
    worker: Option<thread::JoinHandle<()>>,
}

impl ApplicationSettingsService {
    pub fn start(application: Application) -> Result<Self, SettingsServiceJoinError> {
        let (client, endpoint) = SettingsClient::bounded(SETTINGS_COMMAND_CAPACITY);
        let worker = thread::Builder::new()
            .name("bongocat-settings-service".to_owned())
            .spawn(move || run_service(application, endpoint))
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

fn run_service(mut application: Application, endpoint: SettingsServiceEndpoint) {
    let mut clock = SettingsSnapshotClock::new(application.runtime_client().snapshot().revision);
    loop {
        let Ok(command) = endpoint.recv_blocking() else {
            let _ = application.shutdown();
            break;
        };
        match command {
            SettingsCommand::ReadSnapshot { reply } => {
                let _ = reply.respond(Ok(snapshot(&application, &mut clock, false)));
            }
            SettingsCommand::SetOverlayVisible { visible, reply } => {
                let result = application
                    .set_overlay_visible(visible)
                    .map(|_| snapshot(&application, &mut clock, false))
                    .map_err(map_application_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::SetMotionAudioEnabled { enabled, reply } => {
                let result = application
                    .set_motion_audio_enabled(enabled)
                    .map(|_| snapshot(&application, &mut clock, false))
                    .map_err(map_application_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::SelectModel { model, reply } => {
                let result = application
                    .select_model(model_origin(model.origin), model.id)
                    .map(|_| snapshot(&application, &mut clock, false))
                    .map_err(map_application_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::ImportModel { request, reply } => {
                let result = application
                    .import_model(request.id, request.source_root)
                    .map(|_| snapshot(&application, &mut clock, true))
                    .map_err(map_model_import_error);
                let _ = reply.respond(result);
            }
            SettingsCommand::Shutdown { reply } => {
                let before_shutdown = snapshot(&application, &mut clock, false);
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

struct SettingsSnapshotClock {
    revision: u64,
    observed_runtime_revision: u64,
}

impl SettingsSnapshotClock {
    const fn new(runtime_revision: u64) -> Self {
        Self {
            revision: runtime_revision,
            observed_runtime_revision: runtime_revision,
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
}

fn snapshot(
    application: &Application,
    clock: &mut SettingsSnapshotClock,
    catalog_changed: bool,
) -> SettingsSnapshot {
    let runtime = application.runtime_client().snapshot();
    clock.observe_runtime(runtime.revision);
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

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_config::StorageLayout;
    use bongocat_ui::SettingsModelImportRequest;
    use tempfile::tempdir;

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
    fn every_model_store_import_diagnostic_has_a_stable_ui_code() {
        let cases = [
            (
                ModelStoreDiagnostic::AlreadyExists,
                SettingsErrorCode::ModelAlreadyInstalled,
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
    fn service_maps_invalid_import_inputs_to_stable_errors() {
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
