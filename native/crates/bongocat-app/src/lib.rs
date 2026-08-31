#![forbid(unsafe_code)]

use bongocat_audio::{MotionAudioService, MotionAudioShutdownError};
use bongocat_config::{
    BuildEnvironment, ConfigError, ConfigRecovery, ConfigRevision, ConfigStore,
    InterruptedConfigRecovery, NativeConfig, PlatformStorageError, SelectedModelOrigin,
    StorageLayout, platform_layout,
};
use bongocat_model::{
    CommittedModel, InstalledModel, ModelCatalogEntry, ModelError, ModelId, ModelImportProgress,
    ModelOrigin, ModelPackageLimits, ModelStore, ModelStoreError, PresetModelCatalog,
};
use bongocat_render::{ModelCommitToken, RenderConsumer};
use bongocat_runtime::{
    CursorProducer, ExpressionId, ExpressionIdError, HandSide, InputBindings, InputProducer,
    MotionId, MotionIdError, MotionPriority, PhysicalKey, RuntimeClient, RuntimeCommand,
    RuntimeCommandFailure, RuntimeOwner, RuntimeSnapshot, SendError, ShutdownError,
};
use std::{collections::BTreeMap, fmt, path::Path, sync::Arc, time::Duration};

mod settings;
pub use settings::{ApplicationSettingsService, SettingsServiceJoinError};

const COMMAND_CAPACITY: usize = 64;
const AUDIO_COMMAND_CAPACITY: usize = 16;
const RUNTIME_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(bongocat_build_environment = "development")]
pub const BUILD_ENVIRONMENT: BuildEnvironment = BuildEnvironment::Development;

#[cfg(bongocat_build_environment = "production")]
pub const BUILD_ENVIRONMENT: BuildEnvironment = BuildEnvironment::Production;

#[derive(Debug)]
pub enum ApplicationError {
    PlatformStorage(PlatformStorageError),
    Config(ConfigError),
    Model(ModelError),
    ModelStore(ModelStoreError),
    MotionId(MotionIdError),
    ExpressionId(ExpressionIdError),
    PresetModelDeletion(ModelId),
    SelectedModelDeletion(ModelId),
    RuntimeCommand(SendError),
    RuntimeCommandFailed(RuntimeCommandFailure),
    RuntimeDidNotPublish,
    RuntimeDidNotPrepareModel,
    RenderConsumerUnavailable,
    Shutdown(ShutdownError),
    MotionAudioShutdown(MotionAudioShutdownError),
    ConfigRollback(ConfigError),
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformStorage(error) => write!(formatter, "storage setup failed: {error}"),
            Self::Config(error) => write!(formatter, "configuration failed: {error}"),
            Self::Model(error) => write!(formatter, "model preparation failed: {error}"),
            Self::ModelStore(error) => write!(formatter, "model store failed: {error}"),
            Self::MotionId(error) => write!(formatter, "motion id failed: {error}"),
            Self::ExpressionId(error) => write!(formatter, "expression id failed: {error}"),
            Self::PresetModelDeletion(id) => {
                write!(formatter, "preset model cannot be deleted: {}", id.as_str())
            }
            Self::SelectedModelDeletion(id) => {
                write!(
                    formatter,
                    "selected model cannot be deleted: {}",
                    id.as_str()
                )
            }
            Self::RuntimeCommand(error) => write!(formatter, "runtime command failed: {error}"),
            Self::RuntimeCommandFailed(failure) => write!(
                formatter,
                "runtime command {} failed: {:?}",
                failure.sequence, failure.code
            ),
            Self::RuntimeDidNotPublish => {
                formatter.write_str("runtime did not publish the requested revision")
            }
            Self::RuntimeDidNotPrepareModel => {
                formatter.write_str("runtime did not prepare the requested render model")
            }
            Self::RenderConsumerUnavailable => {
                formatter.write_str("application render consumer is unavailable")
            }
            Self::Shutdown(error) => write!(formatter, "shutdown failed: {error}"),
            Self::MotionAudioShutdown(error) => {
                write!(formatter, "motion audio shutdown failed: {error}")
            }
            Self::ConfigRollback(error) => {
                write!(formatter, "model selection config rollback failed: {error}")
            }
        }
    }
}

impl std::error::Error for ApplicationError {}

impl From<PlatformStorageError> for ApplicationError {
    fn from(error: PlatformStorageError) -> Self {
        Self::PlatformStorage(error)
    }
}

impl From<ConfigError> for ApplicationError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<ModelError> for ApplicationError {
    fn from(error: ModelError) -> Self {
        Self::Model(error)
    }
}

impl From<MotionIdError> for ApplicationError {
    fn from(error: MotionIdError) -> Self {
        Self::MotionId(error)
    }
}

impl From<ExpressionIdError> for ApplicationError {
    fn from(error: ExpressionIdError) -> Self {
        Self::ExpressionId(error)
    }
}

impl From<ModelStoreError> for ApplicationError {
    fn from(error: ModelStoreError) -> Self {
        Self::ModelStore(error)
    }
}

pub struct Application {
    config_store: ConfigStore,
    config: NativeConfig,
    config_revision: ConfigRevision,
    config_recovery: Option<ConfigRecovery>,
    interrupted_config_recovery: Option<InterruptedConfigRecovery>,
    preset_models: PresetModelCatalog,
    model_store: ModelStore,
    active_model_origin: Option<ModelOrigin>,
    runtime: RuntimeOwner,
    motion_audio: Option<MotionAudioService>,
    render_consumer: Option<RenderConsumer>,
}

impl Application {
    pub fn start(preset_root: impl AsRef<Path>) -> Result<Self, ApplicationError> {
        Self::start_with_layout_internal(
            platform_layout(BUILD_ENVIRONMENT)?,
            preset_root.as_ref(),
            true,
        )
    }

    #[cfg(test)]
    fn start_with_layout(layout: StorageLayout) -> Result<Self, ApplicationError> {
        Self::start_with_layout_internal(layout, repository_preset_root().as_path(), false)
    }

    fn start_with_layout_internal(
        layout: StorageLayout,
        preset_root: &Path,
        enable_rendering: bool,
    ) -> Result<Self, ApplicationError> {
        let preset_models = PresetModelCatalog::open(preset_root, ModelPackageLimits::default())?;
        let model_store = ModelStore::new(
            &layout.models,
            layout.locks.join("models.writer.lock"),
            ModelPackageLimits::default(),
        )?;
        let config_store = ConfigStore::new(layout)?;
        let loaded_config = config_store.load_or_default()?;
        let config = loaded_config.config;
        let config_revision = loaded_config.revision;
        let config_recovery = loaded_config.recovery;
        let interrupted_config_recovery = loaded_config.interrupted_recovery;
        let (motion_audio, motion_audio_client) =
            match MotionAudioService::start(AUDIO_COMMAND_CAPACITY) {
                Ok(service) => {
                    let client = service.client();
                    (Some(service), client)
                }
                Err(_) => (None, bongocat_audio::MotionAudioClient::unavailable()),
            };
        let (runtime, render_consumer) = if enable_rendering {
            let (runtime, consumer) = RuntimeOwner::start_with_rendering_and_audio(
                config.overlay.visible,
                config.model.play_motion_audio,
                COMMAND_CAPACITY,
                motion_audio_client,
            );
            (runtime, Some(consumer))
        } else {
            (
                RuntimeOwner::start_with_audio(
                    config.overlay.visible,
                    config.model.play_motion_audio,
                    COMMAND_CAPACITY,
                    motion_audio_client,
                ),
                None,
            )
        };
        runtime
            .client()
            .wait_for_revision(1, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        let active_model_origin = config
            .model
            .selected_model_origin
            .map(model_origin_from_config);
        Ok(Self {
            config_store,
            config,
            config_revision,
            config_recovery,
            interrupted_config_recovery,
            preset_models,
            model_store,
            active_model_origin,
            runtime,
            motion_audio,
            render_consumer,
        })
    }

    pub fn runtime_client(&self) -> RuntimeClient {
        self.runtime.client()
    }

    pub fn input_producer(&self) -> InputProducer {
        self.runtime.input_producer()
    }

    pub fn cursor_producer(&self) -> CursorProducer {
        self.runtime.cursor_producer()
    }

    pub fn take_render_consumer(&mut self) -> Result<RenderConsumer, ApplicationError> {
        self.render_consumer
            .take()
            .ok_or(ApplicationError::RenderConsumerUnavailable)
    }

    pub fn config(&self) -> &NativeConfig {
        &self.config
    }

    pub const fn config_recovery(&self) -> Option<ConfigRecovery> {
        self.config_recovery
    }

    pub const fn interrupted_config_recovery(&self) -> Option<InterruptedConfigRecovery> {
        self.interrupted_config_recovery
    }

    pub fn set_overlay_visible(
        &mut self,
        visible: bool,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.overlay.visible = visible;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.config_revision)?;

        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::SetOverlayVisible(visible))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        self.config = next_config;
        self.config_revision = next_revision;
        Ok(snapshot)
    }

    pub fn set_motion_audio_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let mut next_config = self.config.clone();
        next_config.model.play_motion_audio = enabled;
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.config_revision)?;

        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::SetMotionAudioEnabled(enabled))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        self.config = next_config;
        self.config_revision = next_revision;
        Ok(snapshot)
    }

    pub fn model_catalog(&self) -> Result<Vec<ModelCatalogEntry>, ApplicationError> {
        let mut entries = self.preset_models.list()?;
        entries.extend(self.model_store.list()?);
        entries.sort_by(|left, right| {
            left.id().as_str().cmp(right.id().as_str()).then_with(|| {
                model_origin_order(left.origin()).cmp(&model_origin_order(right.origin()))
            })
        });
        Ok(entries)
    }

    pub const fn active_model_origin(&self) -> Option<ModelOrigin> {
        self.active_model_origin
    }

    pub fn start_motion(
        &self,
        group: impl Into<String>,
        index: usize,
        priority: MotionPriority,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        self.wait_for_model_command(RuntimeCommand::StartMotion {
            motion: MotionId::new(group, index)?,
            priority,
        })
    }

    pub fn stop_motion(
        &self,
        group: impl Into<String>,
        index: usize,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        self.wait_for_model_command(RuntimeCommand::StopMotion(MotionId::new(group, index)?))
    }

    pub fn set_expression(
        &self,
        name: impl Into<String>,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        self.wait_for_model_command(RuntimeCommand::SetExpression(ExpressionId::new(name)?))
    }

    pub fn prepare_model(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
    ) -> Result<ModelCommitToken, ApplicationError> {
        if self.render_consumer.is_none() {
            return Err(ApplicationError::RenderConsumerUnavailable);
        }
        let id = ModelId::parse(id)?;
        let committed = self.load_model(origin, &id)?;
        let input_bindings = input_bindings_for_model(origin, id.as_str());
        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::ActivateModelWithBindings {
                model: Arc::new(committed),
                input_bindings: Arc::new(input_bindings),
            })
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_model_preparation(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPrepareModel)?;
        if let Some(failure) = snapshot
            .last_command_failure
            .filter(|failure| failure.sequence == sequence)
        {
            return Err(ApplicationError::RuntimeCommandFailed(failure));
        }
        let token = snapshot
            .pending_model
            .filter(|pending| pending.token.command_sequence == sequence)
            .map(|pending| pending.token)
            .ok_or(ApplicationError::RuntimeDidNotPrepareModel)?;
        self.active_model_origin = Some(origin);
        Ok(token)
    }

    pub fn select_model(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let id = ModelId::parse(id)?;
        let committed = self.load_model(origin, &id)?;
        let mut next_config = self.config.clone();
        next_config.model.selected_model_id = Some(id.as_str().to_owned());
        next_config.model.selected_model_origin = Some(config_origin_from_model(origin));
        let next_revision = self
            .config_store
            .commit_if_revision(&next_config, self.config_revision)?;

        let result = self.wait_for_model_command(RuntimeCommand::ActivateModelWithBindings {
            model: Arc::new(committed),
            input_bindings: Arc::new(input_bindings_for_model(origin, id.as_str())),
        });
        match result {
            Ok(snapshot) => {
                self.config = next_config;
                self.config_revision = next_revision;
                self.active_model_origin = Some(origin);
                Ok(snapshot)
            }
            Err(error) => {
                self.config_revision = self
                    .config_store
                    .commit_if_revision(&self.config, next_revision)
                    .map_err(ApplicationError::ConfigRollback)?;
                Err(error)
            }
        }
    }

    fn load_model(
        &self,
        origin: ModelOrigin,
        id: &ModelId,
    ) -> Result<CommittedModel, ApplicationError> {
        match origin {
            ModelOrigin::Preset => self.preset_models.load(id).map_err(ApplicationError::Model),
            ModelOrigin::Installed => self
                .model_store
                .load(id)
                .map(CommittedModel::from)
                .map_err(ApplicationError::ModelStore),
        }
    }

    fn wait_for_model_command(
        &self,
        command: RuntimeCommand,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let client = self.runtime.client();
        let sequence = client
            .send(command)
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        if let Some(failure) = snapshot
            .last_command_failure
            .filter(|failure| failure.sequence == sequence)
        {
            return Err(ApplicationError::RuntimeCommandFailed(failure));
        }
        Ok(snapshot)
    }

    pub fn delete_model(
        &mut self,
        origin: ModelOrigin,
        id: impl Into<String>,
    ) -> Result<(), ApplicationError> {
        let id = ModelId::parse(id)?;
        if origin == ModelOrigin::Preset {
            return Err(ApplicationError::PresetModelDeletion(id));
        }
        let active_installed = self.active_model_origin == Some(ModelOrigin::Installed)
            && self
                .runtime
                .client()
                .snapshot()
                .active_model
                .as_ref()
                .is_some_and(|active| active.id == id);
        let configured_installed = self.config.model.selected_model_origin
            == Some(SelectedModelOrigin::Installed)
            && self.config.model.selected_model_id.as_deref() == Some(id.as_str());
        if active_installed || configured_installed {
            return Err(ApplicationError::SelectedModelDeletion(id));
        }
        self.model_store
            .delete(&id)
            .map_err(ApplicationError::ModelStore)
    }

    pub fn import_model(
        &mut self,
        id: impl Into<String>,
        source_root: impl AsRef<Path>,
    ) -> Result<InstalledModel, ApplicationError> {
        self.import_model_with_observer(id, source_root, |_| {}, || false)
    }

    pub fn import_model_with_observer<Observe, IsCancelled>(
        &mut self,
        id: impl Into<String>,
        source_root: impl AsRef<Path>,
        observe: Observe,
        is_cancelled: IsCancelled,
    ) -> Result<InstalledModel, ApplicationError>
    where
        Observe: FnMut(ModelImportProgress),
        IsCancelled: FnMut() -> bool,
    {
        let id = ModelId::parse(id)?;
        self.model_store
            .import_with_observer(id, source_root, observe, is_cancelled)
            .map_err(ApplicationError::ModelStore)
    }

    pub fn shutdown(self) -> Result<RuntimeSnapshot, ApplicationError> {
        let runtime_result = self.runtime.shutdown(RUNTIME_TIMEOUT);
        let audio_result = self
            .motion_audio
            .map(|service| service.shutdown(RUNTIME_TIMEOUT))
            .transpose();
        let stopped = runtime_result.map_err(ApplicationError::Shutdown)?;
        audio_result.map_err(ApplicationError::MotionAudioShutdown)?;
        Ok(stopped)
    }
}

fn model_origin_order(origin: ModelOrigin) -> u8 {
    match origin {
        ModelOrigin::Preset => 0,
        ModelOrigin::Installed => 1,
    }
}

const fn config_origin_from_model(origin: ModelOrigin) -> SelectedModelOrigin {
    match origin {
        ModelOrigin::Preset => SelectedModelOrigin::Preset,
        ModelOrigin::Installed => SelectedModelOrigin::Installed,
    }
}

const fn model_origin_from_config(origin: SelectedModelOrigin) -> ModelOrigin {
    match origin {
        SelectedModelOrigin::Preset => ModelOrigin::Preset,
        SelectedModelOrigin::Installed => ModelOrigin::Installed,
    }
}

#[cfg(test)]
fn repository_preset_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
        .join("native/resources/models")
}

fn input_bindings_for_model(origin: ModelOrigin, model_id: &str) -> InputBindings {
    if origin == ModelOrigin::Installed {
        return InputBindings::default();
    }
    const RIGHT_ARROW: PhysicalKey = PhysicalKey::from_hid_usage(0x4f);
    let mut key_hands = BTreeMap::new();
    if matches!(model_id, "standard" | "keyboard") {
        for usage in 0x04..=0x27 {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Left);
        }
        for usage in [
            0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x35, 0x38, 0x39, 0x4c, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4,
            0xe5, 0xe6, 0xe7,
        ] {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Left);
        }
    } else {
        key_hands.insert(PhysicalKey::KEY_A, HandSide::Left);
    }
    if matches!(model_id, "keyboard" | "gamepad") {
        for usage in RIGHT_ARROW.hid_usage()..=0x52 {
            key_hands.insert(PhysicalKey::from_hid_usage(usage), HandSide::Right);
        }
    }
    InputBindings::new(key_hands)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use bongocat_render::{ModelCommitErrorCode, ModelCommitFeedback, ModelCommitOutcome};
    use bongocat_runtime::RuntimeState;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use std::time::Instant;
    use tempfile::tempdir;

    fn repository_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_owned()
    }

    #[test]
    fn build_environment_is_compiled_into_the_application() {
        assert!(matches!(
            BUILD_ENVIRONMENT,
            BuildEnvironment::Development | BuildEnvironment::Production
        ));
    }

    #[test]
    fn application_loads_config_updates_runtime_and_stops() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let mut application = Application::start_with_layout(layout).expect("start application");
        assert!(application.config().overlay.visible);

        let snapshot = application
            .set_overlay_visible(false)
            .expect("update overlay visibility");
        assert!(!snapshot.overlay_visible);
        assert!(!application.config().overlay.visible);

        let audio_snapshot = application
            .set_motion_audio_enabled(false)
            .expect("disable motion audio");
        assert!(!audio_snapshot.motion_audio_enabled);
        assert!(!application.config().model.play_motion_audio);

        let persisted = std::fs::read_to_string(config_path).expect("persisted config");
        assert!(persisted.contains("\"visible\": false"));
        assert!(persisted.contains("\"play_motion_audio\": false"));
        let stopped = application.shutdown().expect("clean shutdown");
        assert_eq!(stopped.state, RuntimeState::Stopped);
    }

    #[test]
    fn application_starts_from_validated_config_backup_after_corruption() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut config = store.load_or_default().expect("default config").config;
        config.overlay.visible = false;
        store.commit(&config).expect("hidden config commit");
        config.overlay.visible = true;
        store.commit(&config).expect("visible config commit");
        std::fs::write(&layout.config, b"corrupt-current").expect("corrupt current config");

        let application = Application::start_with_layout(layout.clone()).expect("recover startup");
        assert!(!application.config().overlay.visible);
        assert!(!application.runtime_client().snapshot().overlay_visible);
        let recovery = application.config_recovery().expect("recovery diagnostic");
        assert_eq!(
            recovery.source_schema_version(),
            bongocat_config::SCHEMA_VERSION
        );
        assert_eq!(recovery.skipped_newer_backups(), 0);
        assert!(
            std::fs::read_dir(&layout.backups)
                .expect("backup directory")
                .any(|entry| {
                    entry
                        .expect("backup entry")
                        .file_name()
                        .to_string_lossy()
                        .starts_with("config-corrupt-")
                })
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn application_starts_from_interrupted_config_without_exposing_storage_details() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let store = ConfigStore::new(layout.clone()).expect("config store");
        let mut current = store.load_or_default().expect("default config").config;
        let interrupted_bytes = std::fs::read(&layout.config).expect("visible config bytes");
        current.overlay.visible = false;
        store.commit(&current).expect("hidden current config");
        std::fs::write(layout.config.with_extension("json.tmp"), interrupted_bytes)
            .expect("interrupted config temp");

        let application = Application::start_with_layout(layout).expect("recover startup");
        assert!(!application.config().overlay.visible);
        assert_eq!(
            application.interrupted_config_recovery(),
            Some(InterruptedConfigRecovery::ArchivedStaleTemp)
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn development_and_production_applications_never_share_roots() {
        let base = tempdir().expect("temp directory");
        let development = StorageLayout::under(base.path(), BuildEnvironment::Development);
        let production = StorageLayout::under(base.path(), BuildEnvironment::Production);
        let development_root = development.root.clone();
        let production_root = production.root.clone();

        let mut development_app =
            Application::start_with_layout(development).expect("development application");
        let mut production_app =
            Application::start_with_layout(production).expect("production application");

        assert!(development_root.join("config.json").is_file());
        assert!(production_root.join("config.json").is_file());
        assert_ne!(development_root, production_root);

        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        development_app
            .import_model("same-id", &source)
            .expect("development import");
        production_app
            .import_model("same-id", source)
            .expect("production import");
        assert_eq!(
            installed_catalog_ids(&development_app),
            vec!["same-id".to_owned()]
        );
        assert_eq!(
            installed_catalog_ids(&production_app),
            vec!["same-id".to_owned()]
        );
        assert!(development_root.join("models/same-id").is_dir());
        assert!(production_root.join("models/same-id").is_dir());
        development_app.shutdown().expect("development shutdown");
        production_app.shutdown().expect("production shutdown");
    }

    #[test]
    fn failed_installed_model_preparation_preserves_the_active_model() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let mut application = Application::start_with_layout(layout).expect("start application");
        let fixtures = repository_root().join("shared/fixtures/model-fixtures/cases");

        application
            .import_model("active", fixtures.join("非 ASCII 模型"))
            .expect("import active model");
        application
            .import_model("broken", fixtures.join("非 ASCII 模型"))
            .expect("import model to corrupt");

        let active = application
            .select_model(ModelOrigin::Installed, "active")
            .expect("activate valid model");
        let active_revision = active.revision;
        assert_eq!(
            active
                .active_model
                .as_ref()
                .expect("active model")
                .id
                .as_str(),
            "active"
        );

        std::fs::remove_file(models_root.join("broken/模型 数据.moc3"))
            .expect("corrupt installed model");

        let error = application
            .select_model(ModelOrigin::Installed, "broken")
            .expect_err("invalid model must be rejected");
        assert!(matches!(error, ApplicationError::ModelStore(_)));
        let preserved = application.runtime_client().snapshot();
        assert_eq!(preserved.revision, active_revision);
        assert_eq!(preserved.active_model, active.active_model);
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn application_imports_into_its_environment_model_store() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let models_root = layout.models.clone();
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");

        let imported = application
            .import_model("unicode", source)
            .expect("import model");
        assert_eq!(
            imported.root(),
            models_root
                .canonicalize()
                .expect("canonical models root")
                .join("unicode")
        );
        assert!(imported.root().join("猫.model3.json").is_file());

        let catalog = application.model_catalog().expect("model catalog");
        assert!(catalog.iter().any(|entry| {
            entry.origin() == bongocat_model::ModelOrigin::Installed
                && entry.id().as_str() == "unicode"
        }));

        application
            .delete_model(ModelOrigin::Installed, "unicode")
            .expect("delete model");
        assert!(installed_catalog_ids(&application).is_empty());
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn merged_model_catalog_retains_source_identity_for_duplicate_ids() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        application
            .import_model("standard", source)
            .expect("install duplicate id");

        let catalog = application.model_catalog().expect("merged catalog");
        let duplicate = catalog
            .iter()
            .filter(|entry| entry.id().as_str() == "standard")
            .map(ModelCatalogEntry::origin)
            .collect::<Vec<_>>();
        assert_eq!(
            duplicate,
            [
                bongocat_model::ModelOrigin::Preset,
                bongocat_model::ModelOrigin::Installed,
            ]
        );
        assert!(catalog.windows(2).all(|entries| {
            let left = (
                entries[0].id().as_str(),
                model_origin_order(entries[0].origin()),
            );
            let right = (
                entries[1].id().as_str(),
                model_origin_order(entries[1].origin()),
            );
            left <= right
        }));
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn installed_duplicate_selection_persists_its_origin_across_restart() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        application
            .import_model("standard", source)
            .expect("install duplicate id");
        let selected = application
            .select_model(ModelOrigin::Installed, "standard")
            .expect("select installed duplicate");
        assert_eq!(
            selected
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );
        assert_eq!(
            application.active_model_origin(),
            Some(ModelOrigin::Installed)
        );
        assert_eq!(
            application.config().model.selected_model_origin,
            Some(SelectedModelOrigin::Installed)
        );
        application.shutdown().expect("clean shutdown");

        let mut restarted = Application::start_with_layout(layout).expect("restart application");
        assert_eq!(
            restarted.config().model.selected_model_id.as_deref(),
            Some("standard")
        );
        assert_eq!(
            restarted.config().model.selected_model_origin,
            Some(SelectedModelOrigin::Installed)
        );
        restarted
            .select_model(ModelOrigin::Installed, "standard")
            .expect("reload installed duplicate");
        assert_eq!(
            restarted.active_model_origin(),
            Some(ModelOrigin::Installed)
        );
        restarted.shutdown().expect("clean restart shutdown");
    }

    #[test]
    fn selected_installed_model_must_be_replaced_before_deletion() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        application
            .import_model("active", source)
            .expect("import model");
        application
            .select_model(ModelOrigin::Installed, "active")
            .expect("activate model");

        let error = application
            .delete_model(ModelOrigin::Installed, "active")
            .expect_err("selected model deletion must fail");
        assert!(matches!(error, ApplicationError::SelectedModelDeletion(_)));
        assert_eq!(
            installed_catalog_ids(&application),
            vec!["active".to_owned()]
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn installed_duplicate_can_be_deleted_while_same_id_preset_is_selected() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        application
            .import_model("standard", source)
            .expect("import duplicate");
        application
            .select_model(ModelOrigin::Preset, "standard")
            .expect("select preset");

        application
            .delete_model(ModelOrigin::Installed, "standard")
            .expect("delete installed duplicate");
        assert!(installed_catalog_ids(&application).is_empty());

        let preset_error = application
            .delete_model(ModelOrigin::Preset, "standard")
            .expect_err("preset deletion must fail");
        assert!(matches!(
            preset_error,
            ApplicationError::PresetModelDeletion(_)
        ));
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn configured_installed_model_cannot_be_deleted_before_restart_activation() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        let mut application =
            Application::start_with_layout(layout.clone()).expect("start application");
        application
            .import_model("selected", source)
            .expect("import model");
        application
            .select_model(ModelOrigin::Installed, "selected")
            .expect("select installed model");
        application.shutdown().expect("clean shutdown");

        let mut restarted = Application::start_with_layout(layout).expect("restart application");
        assert!(restarted.runtime_client().snapshot().active_model.is_none());
        let error = restarted
            .delete_model(ModelOrigin::Installed, "selected")
            .expect_err("configured model deletion must fail");
        assert!(matches!(error, ApplicationError::SelectedModelDeletion(_)));
        assert_eq!(
            installed_catalog_ids(&restarted),
            vec!["selected".to_owned()]
        );
        restarted.shutdown().expect("clean restart shutdown");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn rejected_gpu_model_switch_restores_the_previous_config_selection() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let config_path = layout.config.clone();
        let mut application = Application::start_with_layout_internal(
            layout,
            repository_preset_root().as_path(),
            true,
        )
        .expect("start rendering application");
        let initial_token = application
            .prepare_model(ModelOrigin::Preset, "standard")
            .expect("prepare initial model");
        let consumer = application
            .take_render_consumer()
            .expect("take render consumer");
        let initial_frame = wait_for_model_commit_frame(&consumer, initial_token);
        consumer
            .report_model_commit(ModelCommitFeedback {
                token: initial_frame.model_commit.expect("initial commit token"),
                outcome: ModelCommitOutcome::Prepared,
            })
            .expect("commit initial model");
        application
            .runtime_client()
            .wait_for_command(initial_token.command_sequence, RUNTIME_TIMEOUT)
            .expect("initial model activation");

        let switch = std::thread::spawn(move || {
            let rejected = matches!(
                application.select_model(ModelOrigin::Preset, "keyboard"),
                Err(ApplicationError::RuntimeCommandFailed(_))
            );
            (application, rejected)
        });
        let candidate = wait_for_any_model_commit_frame(&consumer);
        consumer
            .report_model_commit(ModelCommitFeedback {
                token: candidate.model_commit.expect("candidate commit token"),
                outcome: ModelCommitOutcome::Rejected(
                    ModelCommitErrorCode::ResourcePreparationFailed,
                ),
            })
            .expect("reject candidate model");
        let (application, rejected) = switch.join().expect("selection worker");
        assert!(rejected);
        assert_eq!(
            application
                .runtime_client()
                .snapshot()
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );
        assert_eq!(application.config().model.selected_model_id, None);
        assert_eq!(application.config().model.selected_model_origin, None);
        let persisted = std::fs::read_to_string(config_path).expect("restored config");
        assert!(persisted.contains("\"selected_model_id\": null"));
        assert!(persisted.contains("\"selected_model_origin\": null"));
        application.shutdown().expect("clean shutdown");
    }

    fn installed_catalog_ids(application: &Application) -> Vec<String> {
        application
            .model_catalog()
            .expect("model catalog")
            .into_iter()
            .filter(|entry| entry.origin() == bongocat_model::ModelOrigin::Installed)
            .map(|entry| entry.id().as_str().to_owned())
            .collect()
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn wait_for_model_commit_frame(
        consumer: &RenderConsumer,
        token: ModelCommitToken,
    ) -> bongocat_render::RenderFrame {
        let deadline = Instant::now() + RUNTIME_TIMEOUT;
        loop {
            if let Some(frame) = consumer.take_latest()
                && frame.model_commit == Some(token)
            {
                return frame;
            }
            assert!(Instant::now() < deadline, "model frame timed out");
            std::thread::yield_now();
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn wait_for_any_model_commit_frame(consumer: &RenderConsumer) -> bongocat_render::RenderFrame {
        let deadline = Instant::now() + RUNTIME_TIMEOUT;
        loop {
            if let Some(frame) = consumer.take_latest()
                && frame.model_commit.is_some()
            {
                return frame;
            }
            assert!(Instant::now() < deadline, "model frame timed out");
            std::thread::yield_now();
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn application_owns_the_rendering_runtime_and_issues_one_consumer() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout_internal(
            layout,
            repository_preset_root().as_path(),
            true,
        )
        .expect("start rendering application");
        let token = application
            .prepare_model(ModelOrigin::Preset, "standard")
            .expect("prepare preset model");
        assert_eq!(token.model_generation, 0);
        assert!(
            application
                .runtime_client()
                .snapshot()
                .active_model
                .is_none()
        );

        let consumer = application
            .take_render_consumer()
            .expect("take render consumer");
        assert!(matches!(
            application.take_render_consumer(),
            Err(ApplicationError::RenderConsumerUnavailable)
        ));
        let deadline = Instant::now() + RUNTIME_TIMEOUT;
        let frame = loop {
            if let Some(frame) = consumer.take_latest() {
                break frame;
            }
            assert!(
                Instant::now() < deadline,
                "runtime did not publish a render frame"
            );
            std::thread::yield_now();
        };
        assert_eq!(frame.model_generation, 0);
        assert!(!frame.snapshot.drawables.is_empty());
        assert_eq!(frame.model_commit, Some(token));
        consumer
            .report_model_commit(bongocat_render::ModelCommitFeedback {
                token,
                outcome: bongocat_render::ModelCommitOutcome::Prepared,
            })
            .expect("report prepared GPU model");
        let activated = application
            .runtime_client()
            .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
            .expect("commit preset model");
        assert_eq!(
            activated
                .active_model
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("standard")
        );
        assert!(activated.pending_model.is_none());

        let stopped = application.shutdown().expect("clean shutdown");
        assert_eq!(stopped.state, RuntimeState::Stopped);
    }
}
