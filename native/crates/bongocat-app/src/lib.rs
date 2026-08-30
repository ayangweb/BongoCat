#![forbid(unsafe_code)]

use bongocat_config::{
    BuildEnvironment, ConfigError, ConfigRevision, ConfigStore, NativeConfig, PlatformStorageError,
    StorageLayout, platform_layout,
};
use bongocat_model::{
    InstalledModel, ModelCatalogEntry, ModelError, ModelId, ModelPackageLimits, ModelStore,
    ModelStoreError,
};
use bongocat_runtime::{
    InputProducer, RuntimeClient, RuntimeCommand, RuntimeOwner, RuntimeSnapshot, SendError,
    ShutdownError,
};
use std::{fmt, path::Path, sync::Arc, time::Duration};

const COMMAND_CAPACITY: usize = 64;
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
    ActiveModelDeletion(ModelId),
    RuntimeCommand(SendError),
    RuntimeDidNotPublish,
    Shutdown(ShutdownError),
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformStorage(error) => write!(formatter, "storage setup failed: {error}"),
            Self::Config(error) => write!(formatter, "configuration failed: {error}"),
            Self::Model(error) => write!(formatter, "model preparation failed: {error}"),
            Self::ModelStore(error) => write!(formatter, "model store failed: {error}"),
            Self::ActiveModelDeletion(id) => {
                write!(formatter, "active model cannot be deleted: {}", id.as_str())
            }
            Self::RuntimeCommand(error) => write!(formatter, "runtime command failed: {error}"),
            Self::RuntimeDidNotPublish => {
                formatter.write_str("runtime did not publish the requested revision")
            }
            Self::Shutdown(error) => write!(formatter, "shutdown failed: {error}"),
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
}

impl Application {
    pub fn start() -> Result<Self, ApplicationError> {
        Self::start_with_layout(platform_layout(BUILD_ENVIRONMENT)?)
    }

    fn start_with_layout(layout: StorageLayout) -> Result<Self, ApplicationError> {
        let model_store = ModelStore::new(
            &layout.models,
            layout.locks.join("models.writer.lock"),
            ModelPackageLimits::default(),
        )?;
        let config_store = ConfigStore::new(layout)?;
        let (config, config_revision) = config_store.load_or_default()?;
        let runtime = RuntimeOwner::start(config.overlay.visible, COMMAND_CAPACITY);
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
        })
    }

    pub fn runtime_client(&self) -> RuntimeClient {
        self.runtime.client()
    }

    pub fn input_producer(&self) -> InputProducer {
        self.runtime.input_producer()
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

    pub fn model_catalog(&self) -> Result<Vec<ModelCatalogEntry>, ApplicationError> {
        self.model_store
            .list()
            .map_err(ApplicationError::ModelStore)
    }

    pub fn activate_installed_model(
        &mut self,
        id: impl Into<String>,
    ) -> Result<RuntimeSnapshot, ApplicationError> {
        let id = ModelId::parse(id)?;
        let prepared = self.model_store.load(&id)?;
        let client = self.runtime.client();
        let sequence = client
            .send(RuntimeCommand::ActivateModel(Arc::new(prepared)))
            .map_err(ApplicationError::RuntimeCommand)?;
        client
            .wait_for_command(sequence, RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)
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
        self.runtime
            .shutdown(RUNTIME_TIMEOUT)
            .map_err(ApplicationError::Shutdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_runtime::RuntimeState;
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

        let persisted = std::fs::read_to_string(config_path).expect("persisted config");
        assert!(persisted.contains("\"visible\": false"));
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
}
