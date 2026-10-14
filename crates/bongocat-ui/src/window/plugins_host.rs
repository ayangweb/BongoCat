//! The settings window's view of the plugin service.
//!
//! The plugins themselves are owned by `bongocat-plugin-service`, which outlives
//! every window: a timer has to keep counting while the settings window is closed.
//! What is here is only the window's side of that conversation — the cache the
//! page renders and the typed commands the controls send. Nothing in this module
//! starts, ticks or stops a plugin, so the page cannot become the reason a plugin
//! runs or stops running.

use bongocat_plugin_sdk::PluginIcon;
use bongocat_plugin_service::{
    PluginCard, PluginCenterSnapshot, PluginCommandError, PluginServiceClient,
};
use std::sync::{Arc, Mutex};

/// The center state the page renders.
///
/// It is a copy of what the service last answered, published on the GPUI thread
/// so a frame never waits on a plugin round trip. The service is the source of
/// truth; this is only what the window has already read.
#[derive(Clone, Debug, PartialEq)]
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

impl From<PluginCenterSnapshot> for PluginHubState {
    fn from(snapshot: PluginCenterSnapshot) -> Self {
        Self {
            cards: snapshot
                .cards
                .into_iter()
                .map(PluginCardView::from)
                .collect(),
            error: snapshot.error,
            loading: snapshot.loading,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginCardView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) icon: PluginIcon,
    pub(crate) enabled: bool,
    pub(crate) failed: bool,
    pub(crate) version: Option<String>,
    pub(crate) actions: Vec<bongocat_plugin_sdk::UiNode>,
    pub(crate) settings: Vec<bongocat_plugin_sdk::UiNode>,
}

impl From<PluginCard> for PluginCardView {
    fn from(card: PluginCard) -> Self {
        Self {
            id: card.id,
            name: card.name,
            description: card.description,
            icon: card.icon,
            enabled: card.enabled,
            failed: card.failed,
            version: card.version,
            actions: card.actions,
            settings: card.settings,
        }
    }
}

/// The window's handle on the plugin service.
///
/// It carries the service client and the published center state, and it is shared
/// by every view of one settings window. Two windows each hold their own copy of
/// the same state, which is why the service — not this cache — is the thing that
/// decides what a plugin is doing.
#[derive(Clone, Default)]
pub(crate) struct PluginHost {
    client: Option<PluginServiceClient>,
    published: Arc<Mutex<PluginHubState>>,
}

impl PluginHost {
    /// Attach a window to a running plugin service.
    pub(crate) fn new(client: PluginServiceClient) -> Self {
        Self {
            client: Some(client),
            published: Arc::new(Mutex::new(PluginHubState::default())),
        }
    }

    pub(crate) fn snapshot(&self) -> PluginHubState {
        self.published
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Whether this window has a plugin service to talk to at all.
    pub(crate) fn is_available(&self) -> bool {
        self.client.is_some()
    }

    fn client(&self) -> Result<PluginServiceClient, PluginCommandError> {
        self.client
            .clone()
            .ok_or(PluginCommandError::ServiceUnavailable)
    }

    fn publish(&self, state: PluginHubState) {
        *self
            .published
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
    }

    /// Report an error that did not come from the service, such as a failed file
    /// read or a dialog backend failure.
    pub(crate) fn report_error(&self, message: String) {
        let mut state = self.snapshot();
        state.error = Some(message);
        state.loading = false;
        self.publish(state);
    }

    /// Read the center's current state and publish it for the next frame.
    pub(crate) fn refresh(&self) {
        let Ok(client) = self.client() else {
            self.report_error("the plugin service is unavailable".to_owned());
            return;
        };
        match client.read_center_blocking() {
            Ok(snapshot) => self.publish(snapshot.into()),
            Err(error) => self.report_command_error(error),
        }
    }

    /// Scan the package store again, for the page's retry affordance.
    pub(crate) fn rescan(&self) {
        let Ok(client) = self.client() else {
            self.report_error("the plugin service is unavailable".to_owned());
            return;
        };
        match client.refresh_blocking() {
            Ok(snapshot) => self.publish(snapshot.into()),
            Err(error) => self.report_command_error(error),
        }
    }

    /// Install a plugin package the user chose, then republish the cards.
    pub(crate) fn install(&self, bytes: Vec<u8>) {
        self.apply_command(|client| client.install_blocking(bytes));
    }

    pub(crate) fn set_enabled(&self, id: &str, enabled: bool) {
        self.apply_command(|client| client.set_enabled_blocking(id, enabled));
    }

    pub(crate) fn uninstall(&self, id: &str) {
        self.apply_command(|client| client.uninstall_blocking(id));
    }

    pub(crate) fn reset(&self, id: &str) {
        self.apply_command(|client| client.reset_data_blocking(id));
    }

    pub(crate) fn action(&self, plugin: &str, action: &str) {
        self.apply_command(|client| client.action_blocking(plugin, action));
    }

    pub(crate) fn configure(
        &self,
        plugin: &str,
        key: &str,
        value: bongocat_plugin_sdk::ConfigValue,
    ) {
        self.apply_command(|client| client.configure_blocking(plugin, key, value));
    }

    fn apply_command(
        &self,
        command: impl FnOnce(&PluginServiceClient) -> Result<PluginCenterSnapshot, PluginCommandError>,
    ) {
        let Ok(client) = self.client() else {
            self.report_error("the plugin service is unavailable".to_owned());
            return;
        };
        match command(&client) {
            Ok(snapshot) => self.publish(snapshot.into()),
            Err(error) => self.report_command_error(error),
        }
    }

    fn report_command_error(&self, error: PluginCommandError) {
        let mut state = self.snapshot();
        state.error = Some(error.to_string());
        state.loading = false;
        self.publish(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::*;
    use bongocat_plugin_service::{PluginService, plugin_directory};
    use gpui_kit::{TestAppContext, test::TestWindowExt as _};
    use std::io::Write as _;

    #[cfg(target_os = "windows")]
    const BINARY_FILE: &str = "bongocat-countdown.exe";
    #[cfg(not(target_os = "windows"))]
    const BINARY_FILE: &str = "bongocat-countdown";

    const PLUGIN_ID: &str = "com.bongocat.countdown";

    /// A plugin store holding the checked-in countdown package, installed the same
    /// way a download is.
    fn store_with_pomodoro() -> Option<(PluginService, tempfile::TempDir)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("plugins/pomodoro");
        let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let binary = ["debug", "release"]
            .iter()
            .map(|profile| target.join(profile).join(BINARY_FILE))
            .find(|candidate| candidate.is_file())?;
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        archive.start_file("plugin.json", options).ok()?;
        archive
            .write_all(&std::fs::read(root.join("plugin.json")).ok()?)
            .ok()?;
        archive
            .start_file(BINARY_FILE, options.unix_permissions(0o755))
            .ok()?;
        archive.write_all(&std::fs::read(binary).ok()?).ok()?;
        let bytes = archive.finish().ok()?.into_inner();

        let store = tempfile::tempdir().ok()?;
        let (service, _handles) = PluginService::start(store.path(), 4).ok()?;
        service.client().install_blocking(bytes).ok()?;
        Some((service, store))
    }

    #[test]
    fn a_window_reads_the_cards_the_service_owns() {
        let Some((service, _store)) = store_with_pomodoro() else {
            return;
        };
        let host = PluginHost::new(service.client());
        assert!(host.snapshot().loading);
        host.refresh();
        let state = host.snapshot();
        assert!(!state.loading);
        assert!(state.error.is_none());
        assert_eq!(state.cards.len(), 1);
        assert_eq!(state.cards[0].id, PLUGIN_ID);
        assert_eq!(state.cards[0].version.as_deref(), Some("1.0.0"));
        assert!(!state.cards[0].enabled);
        service.shutdown().unwrap();
    }

    #[test]
    fn a_window_without_a_service_reports_it_instead_of_panicking() {
        let host = PluginHost::default();
        assert!(!host.is_available());
        host.refresh();
        assert!(host.snapshot().error.is_some());
    }

    #[test]
    fn enabling_and_uninstalling_republish_the_cards() {
        let Some((service, store)) = store_with_pomodoro() else {
            return;
        };
        let host = PluginHost::new(service.client());
        host.refresh();
        host.set_enabled(PLUGIN_ID, true);
        assert!(host.snapshot().cards[0].enabled);
        assert!(host.snapshot().cards[0].actions.iter().any(
            |node| matches!(node, bongocat_plugin_sdk::UiNode::Button { id, .. } if id == "toggle")
        ));

        let data = plugin_directory(store.path()).join("data").join(PLUGIN_ID);
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("state.json"), b"{}").unwrap();
        host.uninstall(PLUGIN_ID);
        assert!(host.snapshot().cards.is_empty());
        assert!(data.join("state.json").is_file());
        host.reset(PLUGIN_ID);
        assert!(!data.exists());
        service.shutdown().unwrap();
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
        let Some((service, _store)) = store_with_pomodoro() else {
            return;
        };
        cx.update(gpui_kit::init);
        let (client, _endpoint) = SettingsClient::bounded(4);
        let plugin_client = service.client();
        let (_, visual) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| {
                let mut view = SettingsView::new(
                    client,
                    SettingsWindowSeed {
                        language: SettingsLanguage::ChineseSimplified,
                        appearance_theme: SettingsTheme::System,
                    },
                    Rc::new(|_| {}),
                    Rc::new(|_| {}),
                    window,
                    cx,
                );
                view.set_plugin_host(PluginHost::new(plugin_client));
                view.plugin_host.refresh();
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
