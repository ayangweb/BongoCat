//! The settings-side owner of the plugin processes.
//!
//! Discovery, lifecycle and protocol live in `bongocat-plugin-manager`; this is
//! the GPUI-facing cache and the poll that turns ticks into a redraw.

use super::*;
use bongocat_config::{BUNDLE_ID, BuildEnvironment};
use bongocat_plugin_manager::{PluginManager, PluginState};
use std::sync::{Arc, Mutex};

/// State the plugin center renders. It holds no GPUI entity and no business rule:
/// a plugin's own output is the only source of its controls.
#[derive(Clone, Debug, Default)]
pub(crate) struct PluginHubState {
    pub(crate) cards: Vec<PluginCardView>,
    pub(crate) error: Option<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct PluginCardView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) icon: bongocat_plugin_sdk::PluginIcon,
    pub(crate) enabled: bool,
    pub(crate) failed: bool,
    pub(crate) actions: Vec<bongocat_plugin_sdk::UiNode>,
    pub(crate) settings: Vec<bongocat_plugin_sdk::UiNode>,
}
/// The plugin directory for this build. Development and Production never share
/// one: the storage root already separates them.
pub(crate) fn plugin_directory(environment: BuildEnvironment) -> std::path::PathBuf {
    if let Some(directory) = std::env::var_os("BONGCAT_PLUGIN_DIR") {
        return PathBuf::from(directory);
    }
    bongocat_config::platform_layout(environment).map_or_else(
        |_| std::env::temp_dir().join(BUNDLE_ID).join("plugins"),
        |layout| layout.root.join("plugins"),
    )
}

#[derive(Default)]
struct Hub {
    state: PluginHubState,
    manager: Option<PluginManager>,
    last_tick_ms: u64,
}
#[derive(Clone, Default)]
pub(crate) struct PluginHost {
    hub: Arc<Mutex<Hub>>,
}
impl PluginHost {
    pub(crate) fn initialize(&self, environment: BuildEnvironment) {
        let mut hub = self.hub.lock().unwrap();
        if hub.manager.is_some() {
            return;
        }
        match PluginManager::new(plugin_directory(environment)).and_then(|mut manager| {
            manager.discover_plugins()?;
            Ok(manager)
        }) {
            Ok(manager) => hub.manager = Some(manager),
            Err(error) => hub.state.error = Some(error.to_string()),
        }
        refresh(&mut hub);
    }
    pub(crate) fn snapshot(&self) -> PluginHubState {
        self.hub.lock().unwrap().state.clone()
    }
    pub(crate) fn tick(&self, elapsed_ms: u64) {
        let mut hub = self.hub.lock().unwrap();
        let delta = elapsed_ms.saturating_sub(hub.last_tick_ms);
        hub.last_tick_ms = elapsed_ms;
        let Some(manager) = hub.manager.as_mut() else {
            return;
        };
        let updates = manager.tick(delta);
        for (id, output) in updates {
            if let Some(card) = hub.state.cards.iter_mut().find(|card| card.id == id) {
                card.actions = output.model.nodes;
                card.settings = output.settings.nodes;
            }
        }
        let hub = &mut *hub;
        refresh(hub);
    }
    pub(crate) fn toggle(&self, id: &str) {
        let mut hub = self.hub.lock().unwrap();
        let enabled = hub
            .state
            .cards
            .iter()
            .any(|card| card.id == id && card.enabled);
        let result = match hub.manager.as_mut() {
            Some(manager) if enabled => manager.disable_plugin(id),
            Some(manager) => manager.enable_plugin(id),
            None => return,
        };
        if let Err(error) = result {
            hub.state.error = Some(error.to_string());
        }
        refresh(&mut hub);
    }

    pub(crate) fn action(&self, id: &str, action: &str) {
        let mut hub = self.hub.lock().unwrap();
        let config = manager_config(&hub.state, id);
        let result = match hub.manager.as_mut() {
            Some(manager) => {
                let _ = manager.tick(0);
                manager.configure_plugin(id, config)
            }
            None => return,
        };
        if let Err(error) = result {
            hub.state.error = Some(error.to_string());
        }
        let _ = action;
        refresh(&mut hub);
    }
}
fn manager_config(state: &PluginHubState, id: &str) -> bongocat_plugin_sdk::Configuration {
    let mut config = bongocat_plugin_sdk::Configuration::new();
    if let Some(card) = state.cards.iter().find(|card| card.id == id) {
        for node in &card.settings {
            match node {
                bongocat_plugin_sdk::UiNode::Toggle { key, value, .. } => {
                    config.insert(
                        key.clone(),
                        bongocat_plugin_sdk::ConfigValue::Boolean(*value),
                    );
                }
                bongocat_plugin_sdk::UiNode::Integer { key, value, .. } => {
                    config.insert(
                        key.clone(),
                        bongocat_plugin_sdk::ConfigValue::Integer(*value),
                    );
                }
                _ => {}
            }
        }
    }
    config
}
fn refresh(hub: &mut Hub) {
    let Some(manager) = hub.manager.as_ref() else {
        return;
    };
    hub.state.cards = manager
        .list_plugins()
        .into_iter()
        .map(|(metadata, state)| PluginCardView {
            id: metadata.id.clone(),
            name: metadata.name,
            description: metadata.description,
            icon: metadata.icon,
            enabled: state == PluginState::Enabled,
            failed: state == PluginState::Failed,
            actions: manager
                .plugin_output(&metadata.id)
                .map(|output| output.model.nodes.clone())
                .unwrap_or_default(),
            settings: manager
                .plugin_output(&metadata.id)
                .map(|output| output.settings.nodes.clone())
                .unwrap_or_default(),
        })
        .collect();
}
