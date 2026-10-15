//! Process-backed plugin registry. A failed child only removes that entry.
use crate::loader::Process;
use bongocat_plugin_sdk::{Configuration, HostMessage, PluginMetadata, PluginOutput};
use std::collections::BTreeMap;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("plugin not found: {0}")]
    NotFound(String),
    #[error("plugin is already running: {0}")]
    AlreadyRunning(String),
    #[error("plugin is not running: {0}")]
    NotRunning(String),
    #[error("plugin process failed: {0}")]
    Process(#[from] std::io::Error),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginState {
    Discovered,
    Enabled,
    Disabled,
    Failed,
}
struct Entry {
    metadata: PluginMetadata,
    root: PathBuf,
    state: PluginState,
    process: Option<Process>,
    config: Configuration,
    last_output: PluginOutput,
    data_file: PathBuf,
    enabled: bool,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedSession {
    #[serde(default = "session_schema")]
    schema_version: u32,
    #[serde(default)]
    config: Configuration,
    #[serde(default)]
    state: Vec<u8>,
    #[serde(default)]
    enabled: bool,
}
fn session_schema() -> u32 {
    1
}
impl Default for SavedSession {
    fn default() -> Self {
        Self {
            schema_version: 1,
            config: Configuration::new(),
            state: Vec::new(),
            enabled: false,
        }
    }
}
pub struct PluginRegistry {
    entries: BTreeMap<String, Entry>,
    /// The catalog tag the host is showing, delivered to running plugins that
    /// declare [`bongocat_plugin_sdk::Capability::LocalizedViewsV1`].
    ///
    /// The registry owns it rather than the caller because a plugin started
    /// later — enable, upgrade, resume at startup — has to receive the tag the
    /// host was already showing, and a value that lived in the command layer
    /// would have to be replayed into every start path.
    locale: Option<String>,
}
impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            locale: None,
        }
    }
    pub fn register(
        &mut self,
        metadata: PluginMetadata,
        root: PathBuf,
    ) -> Result<(), RegistryError> {
        if let Some(entry) = self.entries.get_mut(&metadata.id) {
            if entry.root != root || entry.metadata != metadata {
                if entry.process.is_some() {
                    return Err(RegistryError::AlreadyRunning(metadata.id));
                }
                entry.metadata = metadata;
                entry.root = root;
            }
            return Ok(());
        }
        let data_file = root
            .ancestors()
            .nth(3)
            .ok_or_else(|| std::io::Error::other("invalid package root"))?
            .join(crate::DATA_DIRECTORY)
            .join(&metadata.id)
            .join("session.json");
        let saved = match std::fs::read(&data_file) {
            Ok(bytes) if bytes.len() <= bongocat_plugin_sdk::MAX_MESSAGE_BYTES => {
                let saved: SavedSession =
                    serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
                if saved.schema_version != 1 {
                    return Err(std::io::Error::other("unsupported plugin session schema").into());
                }
                if !(PluginOutput {
                    config: saved.config.clone(),
                    state: saved.state.clone(),
                    ..Default::default()
                })
                .validate()
                {
                    return Err(std::io::Error::other("invalid saved plugin data").into());
                }
                saved
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => SavedSession::default(),
            Err(error) => return Err(error.into()),
            _ => return Err(std::io::Error::other("saved plugin data exceeds limit").into()),
        };
        self.entries.insert(
            metadata.id.clone(),
            Entry {
                metadata,
                root,
                state: PluginState::Discovered,
                process: None,
                config: saved.config.clone(),
                last_output: PluginOutput {
                    config: saved.config,
                    state: saved.state,
                    ..Default::default()
                },
                data_file,
                enabled: saved.enabled,
            },
        );
        Ok(())
    }
    pub fn list(&self) -> Vec<(PluginMetadata, PluginState)> {
        self.entries
            .values()
            .map(|e| (e.metadata.clone(), e.state))
            .collect()
    }
    /// Whether a plugin currently has a running process.
    pub fn is_running(&self, id: &str) -> bool {
        self.entries
            .get(id)
            .is_some_and(|entry| entry.process.is_some())
    }
    /// Drop a plugin from the registry entirely, stopping its process first.
    ///
    /// Used by uninstall, where the package the entry pointed at no longer
    /// exists: keeping the entry would leave a card for a plugin that is gone.
    pub fn forget(&mut self, id: &str) {
        if let Some(mut entry) = self.entries.remove(id)
            && let Some(mut process) = entry.process.take()
        {
            let _ = process.stop();
        }
    }
    /// Drop every registered plugin whose id is not in `present`.
    ///
    /// Discovery runs this after a scan so a package removed behind the product's
    /// back does not keep a stale entry, while a plugin that is still on disk
    /// keeps its state and configuration across rescans.
    pub fn forget_missing(&mut self, present: &[String]) {
        let missing = self
            .entries
            .keys()
            .filter(|id| !present.iter().any(|kept| kept == *id))
            .cloned()
            .collect::<Vec<_>>();
        for id in missing {
            self.forget(&id);
        }
    }
    pub fn enable(&mut self, id: &str) -> Result<PluginOutput, RegistryError> {
        let locale = self.locale.clone();
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.into()))?;
        if entry.process.is_some() {
            return Ok(entry.last_output.clone());
        }
        let mut process = Process::start(&entry.root, &entry.metadata)?;
        let output = process.exchange(HostMessage::Start {
            config: entry.config.clone(),
            state: entry.last_output.state.clone(),
        })?;
        entry.config = output.config.clone();
        entry.last_output = output.clone();
        entry.process = Some(process);
        entry.state = PluginState::Enabled;
        entry.enabled = true;
        // `Start` is answered before the locale is delivered, so a plugin that
        // cannot localize still publishes its fallback copy for the first frame
        // rather than failing the start. The reply replaces that frame.
        deliver_locale(entry, locale.as_deref());
        persist(entry)?;
        Ok(entry.last_output.clone())
    }
    pub fn disable(&mut self, id: &str) -> Result<(), RegistryError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.into()))?;
        entry.enabled = false;
        if let Some(mut process) = entry.process.take() {
            let output = process.stop()?;
            entry.config = output.config.clone();
            entry.last_output = output;
            persist(entry)?;
        }
        entry.state = PluginState::Disabled;
        persist(entry)?;
        Ok(())
    }
    pub fn configure(
        &mut self,
        id: &str,
        config: Configuration,
    ) -> Result<PluginOutput, RegistryError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.into()))?;
        let Some(process) = entry.process.as_mut() else {
            return Ok(entry.last_output.clone());
        };
        let output = process.exchange(HostMessage::Configure { config })?;
        entry.config = output.config.clone();
        entry.last_output = output.clone();
        persist(entry)?;
        Ok(output)
    }
    pub fn action(&mut self, id: &str, action: &str) -> Result<PluginOutput, RegistryError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.into()))?;
        let process = entry
            .process
            .as_mut()
            .ok_or_else(|| RegistryError::NotRunning(id.into()))?;
        let output = process.exchange(HostMessage::Action { id: action.into() })?;
        entry.last_output = output.clone();
        Ok(output)
    }
    pub fn tick(&mut self, elapsed_ms: u64) -> Vec<(String, PluginOutput)> {
        let mut outputs = Vec::new();
        for (id, entry) in &mut self.entries {
            let Some(process) = entry.process.as_mut() else {
                continue;
            };
            match process.exchange(HostMessage::Tick { elapsed_ms }) {
                Ok(output) => {
                    entry.last_output = output.clone();
                    outputs.push((id.clone(), output));
                }
                Err(_) => {
                    entry.process.take();
                    entry.state = PluginState::Failed;
                }
            }
        }
        outputs
    }
    pub fn output(&self, id: &str) -> Option<&PluginOutput> {
        self.entries.get(id).map(|e| &e.last_output)
    }

    /// Show a newly resolved catalog tag to every plugin that declares it can
    /// localize its own views.
    ///
    /// Plugins that were never told about `localized_views_v1` are skipped
    /// rather than sent a message they cannot parse: the capability is the
    /// contract, and an installed plugin built before the message existed keeps
    /// running exactly as it did.
    ///
    /// The tag is remembered even when no plugin can use it, so a plugin
    /// started later receives it on its own `Start`.
    pub fn set_locale(&mut self, locale: &str) {
        if self.locale.as_deref() == Some(locale) {
            return;
        }
        self.locale = Some(locale.to_owned());
        for entry in self.entries.values_mut() {
            deliver_locale(entry, Some(locale));
        }
    }

    pub fn shutdown(&mut self) {
        for entry in self.entries.values_mut() {
            if let Some(mut process) = entry.process.take()
                && let Ok(output) = process.stop()
            {
                entry.config = output.config.clone();
                entry.last_output = output;
                let _ = persist(entry);
            }
        }
    }

    pub fn resume_enabled(&mut self) -> Result<(), RegistryError> {
        let ids: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.enabled && entry.process.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        let mut failure = None;
        for id in ids {
            if let Err(error) = self.enable(&id) {
                if let Some(entry) = self.entries.get_mut(&id) {
                    entry.state = PluginState::Failed;
                }
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub fn keyboard_activity(&mut self, elapsed_ms: u64) {
        for entry in self.entries.values_mut() {
            if !entry
                .metadata
                .capabilities
                .contains(&bongocat_plugin_sdk::Capability::KeyboardActivityV1)
            {
                continue;
            }
            let Some(process) = entry.process.as_mut() else {
                continue;
            };
            match process.exchange(HostMessage::KeyboardActivity { elapsed_ms }) {
                Ok(output) => entry.last_output = output,
                Err(_) => {
                    entry.process.take();
                    entry.state = PluginState::Failed;
                }
            }
        }
    }
}

/// Hand the host's current catalog tag to one running plugin.
///
/// A plugin that never declared [`Capability::LocalizedViewsV1`], or that is not
/// running, is skipped: the capability is the contract that says the plugin
/// understands the message, so an installed plugin built before it existed is
/// never sent a frame it would fail to parse.
///
/// A plugin that declared the capability and then refuses the exchange is a
/// fault like any other failed tick — the process is dropped and the card shows
/// as failed — because a plugin that cannot answer a message it advertised is
/// not one the host can keep driving.
fn deliver_locale(entry: &mut Entry, locale: Option<&str>) {
    let Some(locale) = locale else {
        return;
    };
    if !entry
        .metadata
        .capabilities
        .contains(&bongocat_plugin_sdk::Capability::LocalizedViewsV1)
    {
        return;
    }
    let Some(process) = entry.process.as_mut() else {
        return;
    };
    match process.exchange(HostMessage::Locale {
        locale: locale.to_owned(),
    }) {
        Ok(output) => entry.last_output = output,
        Err(_) => {
            entry.process.take();
            entry.state = PluginState::Failed;
        }
    }
}

fn persist(entry: &Entry) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(
        entry
            .data_file
            .parent()
            .ok_or_else(|| std::io::Error::other("invalid data root"))?,
    )?;
    let mut file = atomic_write_file::AtomicWriteFile::options().open(&entry.data_file)?;
    serde_json::to_writer(
        &mut file,
        &SavedSession {
            schema_version: 1,
            config: entry.config.clone(),
            state: entry.last_output.state.clone(),
            enabled: entry.enabled,
        },
    )?;
    file.flush()?;
    file.commit()
}
impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
