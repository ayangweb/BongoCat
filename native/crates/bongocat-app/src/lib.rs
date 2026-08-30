#![forbid(unsafe_code)]

use bongocat_audio::{MotionAudioService, MotionAudioShutdownError};
use bongocat_config::{
    BuildEnvironment, ConfigError, ConfigRevision, ConfigStore, NativeConfig, PlatformStorageError,
    StorageLayout, platform_layout,
};
use bongocat_model::{
    CommittedModel, InstalledModel, ModelCatalogEntry, ModelError, ModelId, ModelPackageLimits,
    ModelStore, ModelStoreError, PresetModelCatalog,
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
    ActiveModelDeletion(ModelId),
    RuntimeCommand(SendError),
    RuntimeCommandFailed(RuntimeCommandFailure),
    RuntimeDidNotPublish,
    RuntimeDidNotPrepareModel,
    RenderConsumerUnavailable,
    Shutdown(ShutdownError),
    MotionAudioShutdown(MotionAudioShutdownError),
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
            Self::ActiveModelDeletion(id) => {
                write!(formatter, "active model cannot be deleted: {}", id.as_str())
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
    model_store: ModelStore,
    runtime: RuntimeOwner,
    motion_audio: Option<MotionAudioService>,
    render_consumer: Option<RenderConsumer>,
}

impl Application {
    pub fn start() -> Result<Self, ApplicationError> {
        Self::start_with_layout_internal(platform_layout(BUILD_ENVIRONMENT)?, true)
    }

    #[cfg(test)]
    fn start_with_layout(layout: StorageLayout) -> Result<Self, ApplicationError> {
        Self::start_with_layout_internal(layout, false)
    }

    fn start_with_layout_internal(
        layout: StorageLayout,
        enable_rendering: bool,
    ) -> Result<Self, ApplicationError> {
        let model_store = ModelStore::new(
            &layout.models,
            layout.locks.join("models.writer.lock"),
            ModelPackageLimits::default(),
        )?;
        let config_store = ConfigStore::new(layout)?;
        let (config, config_revision) = config_store.load_or_default()?;
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
        Ok(Self {
            config_store,
            config,
            config_revision,
            model_store,
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
        self.model_store
            .list()
            .map_err(ApplicationError::ModelStore)
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

    pub fn activate_installed_model(
        &mut self,
        id: impl Into<String>,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let id = ModelId::parse(id)?;
        self.activate_committed_model(CommittedModel::from(self.model_store.load(&id)?))
    }

    pub fn prepare_preset_model(
        &mut self,
        preset_root: impl AsRef<Path>,
        id: impl Into<String>,
    ) -> Result<ModelCommitToken, ApplicationError> {
        if self.render_consumer.is_none() {
            return Err(ApplicationError::RenderConsumerUnavailable);
        }
        let id = ModelId::parse(id)?;
        let catalog = PresetModelCatalog::open(preset_root, ModelPackageLimits::default())?;
        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::ActivateModelWithBindings {
                model: Arc::new(catalog.load(&id)?),
                input_bindings: Arc::new(preset_input_bindings(id.as_str())),
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
        snapshot
            .pending_model
            .filter(|pending| pending.token.command_sequence == sequence)
            .map(|pending| pending.token)
            .ok_or(ApplicationError::RuntimeDidNotPrepareModel)
    }

    fn activate_committed_model(
        &self,
        committed: CommittedModel,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        self.wait_for_model_command(RuntimeCommand::ActivateModel(Arc::new(committed)))
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

    pub fn delete_model(&mut self, id: impl Into<String>) -> Result<(), ApplicationError> {
        let id = ModelId::parse(id)?;
        if self
            .runtime
            .client()
            .snapshot()
            .active_model
            .as_ref()
            .is_some_and(|active| active.id == id)
        {
            return Err(ApplicationError::ActiveModelDeletion(id));
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
        let id = ModelId::parse(id)?;
        self.model_store
            .import(id, source_root)
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

fn preset_input_bindings(model_id: &str) -> InputBindings {
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
    use bongocat_runtime::RuntimeState;
    #[cfg(target_os = "macos")]
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
            development_app.model_catalog().expect("dev catalog").len(),
            1
        );
        assert_eq!(
            production_app.model_catalog().expect("prod catalog").len(),
            1
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
            .activate_installed_model("active")
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
            .activate_installed_model("broken")
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
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].id().as_str(), "unicode");

        application.delete_model("unicode").expect("delete model");
        assert!(
            application
                .model_catalog()
                .expect("catalog after deletion")
                .is_empty()
        );
        application.shutdown().expect("clean shutdown");
    }

    #[test]
    fn active_model_must_be_replaced_before_deletion() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout(layout).expect("start application");
        let source = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
        application
            .import_model("active", source)
            .expect("import model");
        application
            .activate_installed_model("active")
            .expect("activate model");

        let error = application
            .delete_model("active")
            .expect_err("active model deletion must fail");
        assert!(matches!(error, ApplicationError::ActiveModelDeletion(_)));
        assert_eq!(application.model_catalog().expect("catalog").len(), 1);
        application.shutdown().expect("clean shutdown");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn application_owns_the_rendering_runtime_and_issues_one_consumer() {
        let base = tempdir().expect("temp directory");
        let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
        let mut application = Application::start_with_layout_internal(layout, true)
            .expect("start rendering application");
        let token = application
            .prepare_preset_model(
                repository_root().join("native/resources/models"),
                "standard",
            )
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
