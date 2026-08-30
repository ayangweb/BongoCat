#![forbid(unsafe_code)]

use bongocat_config::{
    BuildEnvironment, ConfigError, ConfigRevision, ConfigStore, NativeConfig, PlatformStorageError,
    StorageLayout, platform_layout,
};
use bongocat_runtime::{
    RuntimeClient, RuntimeCommand, RuntimeOwner, RuntimeSnapshot, SendError, ShutdownError,
};
use std::{fmt, time::Duration};

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
    RuntimeCommand(SendError),
    RuntimeDidNotPublish,
    Shutdown(ShutdownError),
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformStorage(error) => write!(formatter, "storage setup failed: {error}"),
            Self::Config(error) => write!(formatter, "configuration failed: {error}"),
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

pub struct Application {
    config_store: ConfigStore,
    config: NativeConfig,
    config_revision: ConfigRevision,
    runtime: RuntimeOwner,
}

impl Application {
    pub fn start() -> Result<Self, ApplicationError> {
        Self::start_with_layout(platform_layout(BUILD_ENVIRONMENT)?)
    }

    fn start_with_layout(layout: StorageLayout) -> Result<Self, ApplicationError> {
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
            runtime,
        })
    }

    pub fn runtime_client(&self) -> RuntimeClient {
        self.runtime.client()
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
        let current_runtime_revision = client.snapshot().revision;
        client
            .send(RuntimeCommand::SetOverlayVisible(visible))
            .map_err(ApplicationError::RuntimeCommand)?;
        let snapshot = client
            .wait_for_revision(current_runtime_revision.saturating_add(1), RUNTIME_TIMEOUT)
            .ok_or(ApplicationError::RuntimeDidNotPublish)?;
        self.config = next_config;
        self.config_revision = next_revision;
        Ok(snapshot)
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

        let development_app =
            Application::start_with_layout(development).expect("development application");
        let production_app =
            Application::start_with_layout(production).expect("production application");

        assert!(development_root.join("config.json").is_file());
        assert!(production_root.join("config.json").is_file());
        assert_ne!(development_root, production_root);
        development_app.shutdown().expect("development shutdown");
        production_app.shutdown().expect("production shutdown");
    }
}
