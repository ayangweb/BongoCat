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
#[derive(Clone, Debug)]
pub(crate) struct PluginHubState {
    pub(crate) cards: Vec<PluginCardView>,
    pub(crate) error: Option<String>,
    pub(crate) loading: bool,
}
impl Default for PluginHubState {
    fn default() -> Self {
        Self {
            cards: Vec::new(),
            error: None,
            loading: true,
        }
    }
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
    bongocat_config::platform_layout(environment).map_or_else(
        |_| {
            std::env::temp_dir()
                .join(BUNDLE_ID)
                .join(environment.directory_name())
                .join("plugins")
        },
        |layout| layout.root.join("plugins"),
    )
}

#[derive(Default)]
struct Hub {
    state: PluginHubState,
    manager: Option<PluginManager>,
}
#[derive(Clone, Default)]
pub(crate) struct PluginHost {
    hub: Arc<Mutex<Hub>>,
    published: Arc<Mutex<PluginHubState>>,
}
impl PluginHost {
    pub(crate) fn initialize(&self, environment: BuildEnvironment) {
        self.initialize_directory(plugin_directory(environment));
    }
    fn initialize_directory(&self, directory: PathBuf) {
        {
            let mut hub = self.hub.lock().unwrap();
            hub.state.error = None;
            if hub.manager.is_some() {
                self.publish(&hub);
                return;
            }
            hub.state.loading = true;
            self.publish(&hub);
        }
        let result = PluginManager::new(directory).and_then(|mut manager| {
            manager.discover_plugins()?;
            Ok(manager)
        });
        let mut hub = self.hub.lock().unwrap();
        hub.state.loading = false;
        hub.state.error = None;
        match result {
            Ok(manager) => hub.manager = Some(manager),
            Err(error) => hub.state.error = Some(error.to_string()),
        }
        refresh(&mut hub);
        self.publish(&hub);
    }
    pub(crate) fn snapshot(&self) -> PluginHubState {
        self.published.lock().unwrap().clone()
    }
    fn publish(&self, hub: &Hub) {
        *self.published.lock().unwrap() = hub.state.clone();
    }
    pub(crate) fn tick(&self, elapsed_ms: u64) {
        let mut hub = self.hub.lock().unwrap();
        let Some(manager) = hub.manager.as_mut() else {
            return;
        };
        let updates = manager.tick(elapsed_ms);
        for (id, output) in updates {
            if let Some(card) = hub.state.cards.iter_mut().find(|card| card.id == id) {
                card.actions = output.model.nodes;
                card.settings = output.settings.nodes;
            }
        }
        let hub = &mut *hub;
        refresh(hub);
        self.publish(hub);
    }
    pub(crate) fn toggle(&self, id: &str) {
        let mut hub = self.hub.lock().unwrap();
        hub.state.error = None;
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
        self.publish(&hub);
    }

    pub(crate) fn action(&self, id: &str, action: &str) {
        let mut hub = self.hub.lock().unwrap();
        hub.state.error = None;
        let result = match hub.manager.as_mut() {
            Some(manager) => manager.action_plugin(id, action),
            None => return,
        };
        if let Err(error) = result {
            hub.state.error = Some(error.to_string());
        }
        refresh(&mut hub);
        self.publish(&hub);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, test::TestWindowExt as _};

    #[test]
    fn plugin_directories_are_environment_isolated() {
        assert_ne!(
            plugin_directory(BuildEnvironment::Development),
            plugin_directory(BuildEnvironment::Production)
        );
    }

    #[test]
    fn discovery_publishes_real_cards_instead_of_the_initial_empty_state() {
        let host = PluginHost::default();
        assert!(host.snapshot().loading);
        // Read the actual local package without executing it or using user storage.
        host.initialize_directory(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins"),
        );
        let state = host.snapshot();
        assert!(!state.loading);
        assert!(state.error.is_none());
        assert_eq!(state.cards.len(), 1);
        assert_eq!(state.cards[0].name, "Pomodoro / 番茄钟");
        assert_eq!(state.cards[0].id, "com.bongocat.countdown");
        assert!(!state.cards[0].enabled);
    }

    #[test]
    fn rendering_reads_a_published_snapshot_without_waiting_for_plugin_io() {
        let host = PluginHost::default();
        let _io_in_progress = host.hub.lock().unwrap();
        assert!(host.snapshot().loading);
    }

    #[test]
    fn discovery_failure_is_not_empty_and_can_be_retried() {
        let host = PluginHost::default();
        let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        host.initialize_directory(crate_root.join("Cargo.toml"));
        let failed = host.snapshot();
        assert!(!failed.loading);
        assert!(failed.error.is_some());
        host.initialize_directory(crate_root.join("../../plugins"));
        let recovered = host.snapshot();
        assert!(!recovered.loading);
        assert!(recovered.error.is_none());
        assert_eq!(recovered.cards.len(), 1);
    }

    struct PluginPageHarness(Entity<SettingsView>);

    impl Render for PluginPageHarness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.0.update(cx, |view, cx| {
                super::super::plugins_page::content(view, SettingsLanguage::ChineseSimplified, cx)
            })
        }
    }

    #[gpui_kit::test]
    fn discovered_plugin_card_is_rendered_instead_of_an_empty_page(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (client, _endpoint) = SettingsClient::bounded(4);
        let (_, visual) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| {
                let view = SettingsView::new(
                    client,
                    SettingsWindowSeed {
                        language: SettingsLanguage::ChineseSimplified,
                        appearance_theme: SettingsTheme::System,
                        environment: BuildEnvironment::Development,
                    },
                    Rc::new(|_| {}),
                    Rc::new(|_| {}),
                    window,
                    cx,
                );
                view.plugin_host.initialize_directory(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins"),
                );
                view
            });
            Root::new(cx.new(|_| PluginPageHarness(view)), window, cx)
        });
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        visual.update(|window, _| {
            let card = window
                .try_find("plugin-card-com.bongocat.countdown")
                .expect("the discovered plugin must be painted")
                .bounds();
            assert!(card.size.width > px(0.) && card.size.height > px(0.));
        });
        assert_eq!(
            bongocat_i18n::text("zh-CN", "navigation.plugins.title"),
            "插件中心"
        );
    }
}
