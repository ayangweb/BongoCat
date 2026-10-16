//! Process-backed plugin registry. A failed child only removes that entry.
use crate::loader::Process;
use bongocat_plugin_sdk::{Configuration, HostMessage, PluginMetadata, PluginOutput, UiNode};
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
    #[error("plugin did not publish this preview option")]
    InvalidPreview,
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
        let bytes = crate::install::read_bounded(&data_file, MAX_SESSION_BYTES);
        let saved = match bytes {
            Ok(bytes) if bytes.len() <= MAX_SESSION_BYTES => {
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
        let started = (|| -> std::io::Result<_> {
            let mut process = Process::start(&entry.root, &entry.metadata)?;
            let mut output = process.exchange(HostMessage::Start {
                config: entry.last_output.config.clone(),
                state: entry.last_output.state.clone(),
            })?;
            if let Some(locale) = locale
                && entry
                    .metadata
                    .capabilities
                    .contains(&bongocat_plugin_sdk::Capability::LocalizedViewsV1)
            {
                output = process.exchange(HostMessage::Locale {
                    locale: entry.metadata.view_locale(&locale).to_owned(),
                })?;
            }
            persist_output(entry, &output, true)?;
            Ok((process, output))
        })();
        match started {
            Ok((process, output)) => {
                entry.last_output = output.clone();
                entry.process = Some(process);
                entry.state = PluginState::Enabled;
                entry.enabled = true;
                Ok(output)
            }
            Err(error) => {
                entry.state = PluginState::Failed;
                Err(error.into())
            }
        }
    }

    pub fn disable(&mut self, id: &str) -> Result<(), RegistryError> {
        self.stop(id, false)
    }

    /// Stop for replacement without persisting a user-requested disable.
    pub(crate) fn suspend(&mut self, id: &str) -> Result<(), RegistryError> {
        let enabled = self
            .entries
            .get(id)
            .ok_or_else(|| RegistryError::NotFound(id.into()))?
            .enabled;
        self.stop(id, enabled)
    }

    fn stop(&mut self, id: &str, enabled: bool) -> Result<(), RegistryError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.into()))?;
        let stopped = entry
            .process
            .take()
            .map(|mut process| process.stop())
            .transpose();
        if let Ok(Some(output)) = &stopped {
            entry.last_output = output.clone();
        }
        entry.enabled = enabled;
        entry.state = if stopped.is_err() {
            PluginState::Failed
        } else {
            PluginState::Disabled
        };
        // Persist the requested enable flag even after a refused/failed Stop,
        // or a plugin explicitly disabled by the user would restart next boot.
        persist(entry)?;
        stopped.map(|_| ()).map_err(Into::into)
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
        let previous = entry.last_output.clone();
        let output = entry.exchange(HostMessage::Configure { config })?;
        if let Err(error) = persist(entry) {
            // A process that applied an uncommitted config must not keep running
            // with settings different from disk. Retry starts the last good state.
            entry.fail();
            entry.last_output = previous;
            return Err(error.into());
        }
        Ok(output)
    }
    pub fn action(&mut self, id: &str, action: &str) -> Result<PluginOutput, RegistryError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.into()))?;
        entry.exchange(HostMessage::Action { id: action.into() })
    }
    pub fn preview(
        &mut self,
        id: &str,
        key: &str,
        value: &str,
    ) -> Result<PluginOutput, RegistryError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.into()))?;
        if entry.state != PluginState::Enabled {
            return Err(RegistryError::NotRunning(id.into()));
        }
        if !entry.last_output.settings.nodes.iter().any(|node| {
            matches!(node, UiNode::Select { key: published_key, options, preview: true, .. }
                if published_key == key && options.iter().any(|option| option.value == value))
        }) {
            return Err(RegistryError::InvalidPreview);
        }
        entry.exchange(HostMessage::Preview {
            key: key.into(),
            value: value.into(),
        })
    }
    pub fn tick(&mut self, elapsed_ms: u64) -> Vec<(String, PluginOutput)> {
        let mut outputs = Vec::new();
        for (id, entry) in &mut self.entries {
            if entry.process.is_none() {
                continue;
            }
            if let Ok(output) = entry.required_exchange(HostMessage::Tick { elapsed_ms }) {
                outputs.push((id.clone(), output));
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
        let locale = bongocat_plugin_sdk::plugin_locale(locale);
        if self.locale.as_deref() == Some(locale) {
            return;
        }
        self.locale = Some(locale.to_owned());
        for entry in self.entries.values_mut() {
            deliver_locale(entry, Some(locale));
        }
    }

    pub fn shutdown(&mut self) -> Result<(), RegistryError> {
        let ids = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.process.is_some())
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut failure = None;
        for id in ids {
            if let Err(error) = self.suspend(&id) {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
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

    /// Hand every running plugin the device edges counted since the last poll.
    ///
    /// Which message a plugin gets is the capability it declared, not the
    /// protocol version: one that asked for `input_activity_v1` is told the edges
    /// of both devices and both directions, and one that only ever asked for
    /// `keyboard_activity_v1` keeps receiving the single keyboard total it was
    /// built against. A plugin that declared the newer capability is never sent
    /// the older message as well, because it did not ask for two signals and the
    /// duplicate would double every sound it plays.
    pub fn input_activity(
        &mut self,
        elapsed_ms: u64,
        activity: bongocat_plugin_sdk::InputActivity,
    ) {
        for entry in self.entries.values_mut() {
            let Some(message) =
                activity_message(&entry.metadata.capabilities, elapsed_ms, activity)
            else {
                continue;
            };
            if entry.process.is_some() {
                let _ = entry.required_exchange(message);
            }
        }
    }

    /// Establish the cumulative activity baseline for one newly started plugin.
    ///
    /// This is intentionally separate from [`Self::input_activity`]: sending a
    /// baseline to every plugin while enabling one plugin could make an already
    /// running sound plugin replay edges that occurred during the enable action.
    pub fn initialize_input_activity(
        &mut self,
        id: &str,
        elapsed_ms: u64,
        activity: bongocat_plugin_sdk::InputActivity,
    ) {
        let Some(entry) = self.entries.get_mut(id) else {
            return;
        };
        if !entry
            .metadata
            .capabilities
            .contains(&bongocat_plugin_sdk::Capability::InputActivityV1)
        {
            return;
        }
        if entry.process.is_some() {
            let _ = entry.required_exchange(HostMessage::InputActivity {
                elapsed_ms,
                activity,
            });
        }
    }
}

/// The activity message one plugin's declared capabilities entitle it to.
///
/// The capability is the gate, not the protocol version, and the newer one wins
/// rather than adding to the older: a plugin that asked for both signals did not
/// ask for two, and sending it the keyboard total as well would double every
/// sound it plays. A plugin that declared neither is not running an activity
/// plugin at all, so there is nothing to send it.
fn activity_message(
    capabilities: &[bongocat_plugin_sdk::Capability],
    elapsed_ms: u64,
    activity: bongocat_plugin_sdk::InputActivity,
) -> Option<HostMessage> {
    use bongocat_plugin_sdk::Capability;
    if capabilities.contains(&Capability::InputActivityV1) {
        Some(HostMessage::InputActivity {
            elapsed_ms,
            activity,
        })
    } else if capabilities.contains(&Capability::KeyboardActivityV1) {
        Some(HostMessage::KeyboardActivity { elapsed_ms })
    } else {
        None
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
    if entry.process.is_some() {
        let _ = entry.required_exchange(HostMessage::Locale {
            locale: entry.metadata.view_locale(locale).to_owned(),
        });
    }
}

impl Entry {
    fn fail(&mut self) {
        self.process.take();
        self.state = PluginState::Failed;
    }

    fn exchange(&mut self, message: HostMessage) -> Result<PluginOutput, RegistryError> {
        let process = self
            .process
            .as_mut()
            .ok_or_else(|| RegistryError::NotRunning(self.metadata.id.clone()))?;
        match process.exchange(message) {
            Ok(output) => {
                self.last_output = output.clone();
                Ok(output)
            }
            Err(error) => {
                if error.kind() != std::io::ErrorKind::InvalidInput {
                    self.fail();
                }
                Err(error.into())
            }
        }
    }

    fn required_exchange(&mut self, message: HostMessage) -> Result<PluginOutput, RegistryError> {
        let result = self.exchange(message);
        if result.is_err() {
            self.fail();
        }
        result
    }
}

// State bytes expand to JSON integers; a valid 64 KiB state can exceed the
// protocol's frame limit once wrapped in a durable session.
const MAX_SESSION_BYTES: usize = 512 * 1024;

fn persist(entry: &Entry) -> std::io::Result<()> {
    persist_output(entry, &entry.last_output, entry.enabled)
}

fn persist_output(entry: &Entry, output: &PluginOutput, enabled: bool) -> std::io::Result<()> {
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
            config: output.config.clone(),
            state: output.state.clone(),
            enabled,
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

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_sdk::{Capability, InputActivity};

    fn metadata() -> PluginMetadata {
        serde_json::from_str::<crate::PluginManifest>(include_str!(
            "../../../plugins/pomodoro/plugin.json"
        ))
        .unwrap()
        .metadata
    }

    #[test]
    fn durable_sessions_round_trip_full_state_and_reject_invalid_config() {
        let store = tempfile::tempdir().unwrap();
        let metadata = metadata();
        let root = store
            .path()
            .join("packages")
            .join(&metadata.id)
            .join(&metadata.version);
        let data = store.path().join("data").join(&metadata.id);
        std::fs::create_dir_all(&data).unwrap();
        let file = data.join("session.json");
        let mut saved = SavedSession {
            state: vec![255; 64 * 1024],
            config: [(
                "minutes".into(),
                bongocat_plugin_sdk::ConfigValue::Integer(5),
            )]
            .into(),
            enabled: true,
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&saved).unwrap();
        assert!(bytes.len() > bongocat_plugin_sdk::MAX_MESSAGE_BYTES);
        std::fs::write(&file, bytes).unwrap();
        let mut registry = PluginRegistry::new();
        registry.register(metadata.clone(), root.clone()).unwrap();
        assert_eq!(registry.output(&metadata.id).unwrap().state, saved.state);
        assert_eq!(registry.output(&metadata.id).unwrap().config, saved.config);
        assert!(registry.entries[&metadata.id].enabled);
        persist(&registry.entries[&metadata.id]).unwrap();
        let mut reopened = PluginRegistry::new();
        reopened.register(metadata.clone(), root.clone()).unwrap();
        assert_eq!(reopened.output(&metadata.id).unwrap().state, saved.state);

        saved.config.insert(
            "../escape".into(),
            bongocat_plugin_sdk::ConfigValue::Boolean(true),
        );
        std::fs::write(&file, serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(PluginRegistry::new().register(metadata, root).is_err());
    }

    #[test]
    fn configuring_a_stopped_plugin_is_refused_without_changing_its_output() {
        let store = tempfile::tempdir().unwrap();
        let metadata = metadata();
        let mut registry = PluginRegistry::new();
        registry
            .register(
                metadata.clone(),
                store
                    .path()
                    .join("packages")
                    .join(&metadata.id)
                    .join(&metadata.version),
            )
            .unwrap();
        assert!(matches!(
            registry.configure(
                &metadata.id,
                [(
                    "minutes".into(),
                    bongocat_plugin_sdk::ConfigValue::Integer(5)
                )]
                .into()
            ),
            Err(RegistryError::NotRunning(_))
        ));
        assert!(registry.output(&metadata.id).unwrap().config.is_empty());
    }

    #[test]
    fn only_running_plugins_with_opted_in_published_options_receive_previews() {
        let store = tempfile::tempdir().unwrap();
        let metadata = metadata();
        let mut registry = PluginRegistry::new();
        registry
            .register(
                metadata.clone(),
                store
                    .path()
                    .join("packages")
                    .join(&metadata.id)
                    .join(&metadata.version),
            )
            .unwrap();
        assert!(matches!(
            registry.preview(&metadata.id, "pack", "one"),
            Err(RegistryError::NotRunning(_))
        ));
        let entry = registry.entries.get_mut(&metadata.id).unwrap();
        entry.state = PluginState::Enabled;
        let select = |preview| UiNode::Select {
            key: "pack".into(),
            label: "Sound".into(),
            value: "one".into(),
            options: vec![bongocat_plugin_sdk::SelectOption {
                value: "one".into(),
                label: "One".into(),
            }],
            preview,
        };
        entry.last_output.settings.nodes = vec![select(false)];
        assert!(matches!(
            registry.preview(&metadata.id, "pack", "one"),
            Err(RegistryError::InvalidPreview)
        ));
        registry
            .entries
            .get_mut(&metadata.id)
            .unwrap()
            .last_output
            .settings
            .nodes = vec![select(true)];
        for (key, value) in [("other", "one"), ("pack", "other")] {
            assert!(matches!(
                registry.preview(&metadata.id, key, value),
                Err(RegistryError::InvalidPreview)
            ));
        }
        // A valid request reaches exchange (there is deliberately no child).
        assert!(matches!(
            registry.preview(&metadata.id, "pack", "one"),
            Err(RegistryError::NotRunning(_))
        ));
        assert_eq!(registry.entries[&metadata.id].state, PluginState::Enabled);
    }

    const ACTIVITY: InputActivity = InputActivity {
        keyboard_down: 3,
        keyboard_up: 2,
        mouse_down: 1,
        mouse_up: 1,
    };

    /// A plugin is sent the activity contract it asked for, and only that one.
    #[test]
    fn the_declared_capability_picks_the_activity_message() {
        let message = |capabilities: &[Capability]| activity_message(capabilities, 500, ACTIVITY);

        assert!(matches!(
            message(&[Capability::InputActivityV1]),
            Some(HostMessage::InputActivity {
                elapsed_ms: 500,
                activity,
            }) if activity == ACTIVITY
        ));
        assert!(matches!(
            message(&[Capability::KeyboardActivityV1]),
            Some(HostMessage::KeyboardActivity { elapsed_ms: 500 })
        ));
        // The newer capability supersedes the older one rather than joining it:
        // a plugin that declared both is still sent one message, or it would
        // play every sound twice.
        assert!(matches!(
            message(&[Capability::KeyboardActivityV1, Capability::InputActivityV1,]),
            Some(HostMessage::InputActivity {
                elapsed_ms: 500,
                activity,
            }) if activity == ACTIVITY
        ));
        // A plugin that asked for neither is not an activity plugin at all.
        assert!(message(&[]).is_none());
        assert!(message(&[Capability::ModelUi]).is_none());
    }
}
