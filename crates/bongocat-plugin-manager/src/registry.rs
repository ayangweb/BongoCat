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
}
pub struct PluginRegistry {
    entries: BTreeMap<String, Entry>,
}
impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
    pub fn register(
        &mut self,
        metadata: PluginMetadata,
        root: PathBuf,
    ) -> Result<(), RegistryError> {
        if self.entries.contains_key(&metadata.id) {
            return Ok(());
        }
        self.entries.insert(
            metadata.id.clone(),
            Entry {
                metadata,
                root,
                state: PluginState::Discovered,
                process: None,
                config: Configuration::new(),
                last_output: PluginOutput::default(),
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
    pub fn enable(&mut self, id: &str) -> Result<PluginOutput, RegistryError> {
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
            state: Vec::new(),
        })?;
        entry.last_output = output.clone();
        entry.process = Some(process);
        entry.state = PluginState::Enabled;
        Ok(output)
    }
    pub fn disable(&mut self, id: &str) -> Result<(), RegistryError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.into()))?;
        if let Some(mut process) = entry.process.take() {
            process.stop()?;
        }
        entry.state = PluginState::Disabled;
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
        entry.config = config.clone();
        let Some(process) = entry.process.as_mut() else {
            return Ok(entry.last_output.clone());
        };
        let output = process.exchange(HostMessage::Configure { config })?;
        entry.last_output = output.clone();
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
    pub fn shutdown(&mut self) {
        for entry in self.entries.values_mut() {
            if let Some(mut process) = entry.process.take() {
                let _ = process.stop();
            }
        }
    }
}
impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
