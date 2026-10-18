//! The settings window's view of the plugin service.
//!
//! The plugins themselves are owned by `bongocat-plugin-service`, which outlives
//! every window: a timer has to keep counting while the settings window is closed.
//! What is here is only the window's side of that conversation — the cache the
//! page renders and the typed commands the controls send. Nothing in this module
//! starts, ticks or stops a plugin, so the page cannot become the reason a plugin
//! runs or stops running.

use bongocat_plugin_sdk::{PluginCopy, PluginIcon};
use bongocat_plugin_service::{
    PluginCard, PluginCenterSnapshot, PluginCommandError, PluginServiceClient,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

/// One package operation the center itself runs for a plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PluginPackageOperation {
    Install,
    /// Turning a plugin on or off. The switch is not a button, so it shows the
    /// in-flight state by staying inert rather than by spinning.
    Enable,
    Uninstall,
}

/// One command this window has sent to the plugin service.
///
/// It exists so the control a user pressed can show that the command it started
/// is still running: the entry is dropped when the reply lands, whether it
/// carried a new center state or a refusal.
///
/// The count is what keeps two overlapping sends apart: the same command can be
/// sent twice before the first reply arrives, and one reply must not report the
/// other as finished.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PluginInFlight {
    Refresh,
    UpdateAll,
    Package {
        plugin: String,
        operation: PluginPackageOperation,
    },
    Action {
        plugin: String,
        action: String,
    },
    Configure {
        plugin: String,
        key: String,
    },
}

impl PluginInFlight {
    pub(crate) fn package(plugin: &str, operation: PluginPackageOperation) -> Self {
        Self::Package {
            plugin: plugin.to_owned(),
            operation,
        }
    }

    pub(crate) fn action(plugin: &str, action: &str) -> Self {
        Self::Action {
            plugin: plugin.to_owned(),
            action: action.to_owned(),
        }
    }

    /// The key one configuration write is in flight under.
    ///
    /// A key is per field rather than per plugin because two fields in one
    /// panel can be written at the same time, and only the field that was
    /// edited is busy.
    pub(crate) fn configure(plugin: &str, key: &str) -> Self {
        Self::Configure {
            plugin: plugin.to_owned(),
            key: key.to_owned(),
        }
    }
}

/// The center state the page renders.
///
/// It is a copy of what the service last answered, published on the GPUI thread
/// so a frame never waits on a plugin round trip. The service is the source of
/// truth; this is only what the window has already read.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PluginHubState {
    pub(crate) cards: Vec<PluginCardView>,
    pub(crate) error: Option<String>,
    /// True until the first discovery has finished.
    pub(crate) loading: bool,
    /// Commands this window has sent whose reply has not arrived yet.
    ///
    /// It belongs to the window rather than to the service: two windows can have
    /// different commands outstanding, and a state the service publishes while
    /// one is running must not read as though the command had finished.
    pub(crate) in_flight: BTreeMap<PluginInFlight, usize>,
}

impl PluginHubState {
    /// The state a center is in before it has read anything.
    pub(crate) fn loading() -> Self {
        Self {
            loading: true,
            ..Default::default()
        }
    }

    /// Whether a package operation on one plugin is still waiting for its reply.
    pub(crate) fn pending_package(&self, plugin: &str, operation: PluginPackageOperation) -> bool {
        self.in_flight
            .contains_key(&PluginInFlight::package(plugin, operation))
    }

    /// Whether one plugin-published action is still waiting for its reply.
    pub(crate) fn pending_action(&self, plugin: &str, action: &str) -> bool {
        self.in_flight
            .contains_key(&PluginInFlight::action(plugin, action))
    }

    /// Whether one configuration key of one plugin is still being written.
    pub(crate) fn pending_key(&self, plugin: &str, key: &str) -> bool {
        self.in_flight
            .contains_key(&PluginInFlight::configure(plugin, key))
    }

    pub(crate) fn updating_all(&self) -> bool {
        self.in_flight.contains_key(&PluginInFlight::UpdateAll)
    }

    pub(crate) fn refreshing(&self) -> bool {
        self.in_flight.contains_key(&PluginInFlight::Refresh)
    }

    pub(crate) fn package_busy(&self) -> bool {
        self.in_flight
            .keys()
            .any(|command| matches!(command, PluginInFlight::Package { .. }))
    }

    pub(crate) fn can_update_all(&self) -> bool {
        !self.loading
            && !self.updating_all()
            && !self.refreshing()
            && !self.package_busy()
            && self
                .cards
                .iter()
                .any(|card| card.version.is_some() && card.update_available)
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
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginCardView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    /// The plugin's own name and description per locale, and the fallback above.
    pub(crate) localizations: BTreeMap<String, PluginCopy>,
    pub(crate) icon: PluginIcon,
    pub(crate) enabled: bool,
    pub(crate) failed: bool,
    pub(crate) version: Option<String>,
    pub(crate) available_version: Option<String>,
    pub(crate) update_available: bool,
    pub(crate) actions: Vec<bongocat_plugin_sdk::UiNode>,
    pub(crate) settings: Vec<bongocat_plugin_sdk::UiNode>,
}

impl PluginCardView {
    /// The name and description the plugin wrote for a resolved catalog tag.
    ///
    /// The card resolves the tag rather than the service because the language on
    /// screen is a property of the window: a user switching language repaints
    /// from the same cards instead of asking every plugin to republish metadata.
    pub(crate) fn copy(&self, locale: &str) -> (&str, &str) {
        bongocat_plugin_sdk::localized_copy(
            &self.localizations,
            locale,
            (self.name.as_str(), self.description.as_str()),
        )
    }
}

impl From<PluginCard> for PluginCardView {
    fn from(card: PluginCard) -> Self {
        Self {
            id: card.id,
            name: card.name,
            description: card.description,
            localizations: card.localizations,
            icon: card.icon,
            enabled: card.enabled,
            failed: card.failed,
            version: card.version,
            available_version: card.available_version,
            update_available: card.update_available,
            actions: card.actions,
            settings: card.settings,
        }
    }
}

/// Hover is latest-value UI state, not a reliable command queue. At most one
/// preview is in flight and one is pending, so fast pointer movement cannot
/// replay a backlog or reorder the final hovered option.
#[derive(Default)]
struct PreviewMailbox {
    pending: Option<PreviewRequest>,
    running: bool,
}

struct PreviewRequest {
    plugin: String,
    key: String,
    value: String,
    lifetime: Weak<()>,
}

impl PreviewMailbox {
    fn enqueue(&mut self, request: PreviewRequest, hovered: bool) -> bool {
        if !hovered {
            if self.pending.as_ref().is_some_and(|pending| {
                pending.plugin == request.plugin
                    && pending.key == request.key
                    && pending.value == request.value
            }) {
                self.pending = None;
            }
            return false;
        }
        self.pending = Some(request);
        if self.running {
            return false;
        }
        self.running = true;
        true
    }

    fn take(&mut self) -> Option<PreviewRequest> {
        let pending = self.pending.take();
        if pending.is_none() {
            self.running = false;
        }
        pending
    }
}

/// The window's handle on the plugin service.
///
/// It carries the service client and the published center state, and it is shared
/// by every view of one settings window. Two windows each hold their own copy of
/// the same state, which is why the service — not this cache — is the thing that
/// decides what a plugin is doing.
#[derive(Clone)]
pub(crate) struct PluginHost {
    client: Option<PluginServiceClient>,
    published: Arc<Mutex<PluginHubState>>,
    requests: Arc<Mutex<()>>,
    previews: Arc<Mutex<PreviewMailbox>>,
    update_report: Arc<Mutex<Option<Vec<String>>>>,
}

impl Default for PluginHost {
    fn default() -> Self {
        Self {
            client: None,
            published: Arc::new(Mutex::new(PluginHubState::loading())),
            requests: Arc::default(),
            previews: Arc::default(),
            update_report: Arc::default(),
        }
    }
}

impl PluginHost {
    /// Attach a window to a running plugin service.
    pub(crate) fn new(client: PluginServiceClient) -> Self {
        Self {
            client: Some(client),
            published: Arc::new(Mutex::new(PluginHubState::loading())),
            requests: Arc::default(),
            previews: Arc::default(),
            update_report: Arc::default(),
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
        let mut published = self
            .published
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // What is still running is this window's own bookkeeping: a state the
        // service published while a command is in flight describes the plugins,
        // not the command, so it must not clear the control that is waiting.
        let in_flight = published.in_flight.clone();
        *published = PluginHubState { in_flight, ..state };
    }

    /// Record that this window is about to send one command.
    ///
    /// The page calls this on the GPUI thread before it hands the command to the
    /// background executor, because nothing between the click and the reply would
    /// otherwise redraw: the control a user pressed would jump from idle to idle
    /// and never show that it was working.
    pub(crate) fn begin(&self, command: PluginInFlight) {
        self.published
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .in_flight
            .entry(command)
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
    }

    /// Reserve the whole batch on the UI thread, before any background work.
    pub(crate) fn begin_update_all(&self) -> Vec<String> {
        let mut state = self
            .published
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.can_update_all() {
            return Vec::new();
        }
        let targets = state
            .cards
            .iter()
            .filter(|card| card.version.is_some() && card.update_available)
            .map(|card| card.id.clone())
            .collect::<Vec<_>>();
        state.in_flight.insert(PluginInFlight::UpdateAll, 1);
        targets
    }

    pub(crate) fn update_all(&self, targets: Vec<String>) {
        let mut failed = Vec::new();
        for id in targets {
            // The batch marks each package as it starts, so the card being
            // replaced shows its own progress while the batch control also spins.
            self.begin(PluginInFlight::package(
                &id,
                PluginPackageOperation::Install,
            ));
            if !self.install_available(&id) {
                failed.push(id);
            }
        }
        *self
            .update_report
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(failed);
        self.finish(&PluginInFlight::UpdateAll);
    }

    pub(crate) fn take_update_report(&self) -> Option<Vec<String>> {
        self.update_report
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    fn finish(&self, command: &PluginInFlight) {
        let mut published = self
            .published
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let finished = published
            .in_flight
            .get_mut(command)
            .map(|count| {
                *count = count.saturating_sub(1);
                *count == 0
            })
            .unwrap_or(true);
        if finished {
            published.in_flight.remove(command);
        }
    }

    /// Report an error that did not come from the service, such as a failed file
    /// read or a dialog backend failure.
    pub(crate) fn report_error(&self, message: String) {
        let mut state = self
            .published
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.error = Some(message);
        state.loading = false;
    }

    /// Read the center's current state and publish it for the next frame.
    pub(crate) fn refresh(&self) {
        let _request = self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        self.apply_command(PluginInFlight::Refresh, |client| client.refresh_blocking());
    }

    pub(crate) fn install_available(&self, id: &str) -> bool {
        self.apply_command(
            PluginInFlight::package(id, PluginPackageOperation::Install),
            |client| {
                let mut snapshot = client.install_available_blocking(id)?;
                let expected = snapshot
                    .cards
                    .iter()
                    .find(|card| card.id == id)
                    .and_then(|card| card.available_version.clone());
                // InstallAvailable acknowledges job submission, not installation.
                // Wait off the UI thread before retiring loading or starting the next ZIP.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                while snapshot.loading {
                    self.publish(snapshot.into());
                    if std::time::Instant::now() >= deadline {
                        return Err(PluginCommandError::Operation);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    snapshot = client.read_center_blocking()?;
                }
                let installed = expected.is_some()
                    && snapshot.cards.iter().any(|card| {
                        card.id == id && card.version == expected && !card.update_available
                    });
                if !installed {
                    self.publish(snapshot.into());
                    return Err(PluginCommandError::Operation);
                }
                Ok(snapshot)
            },
        )
    }

    pub(crate) fn set_enabled(&self, id: &str, enabled: bool) {
        self.apply_command(
            PluginInFlight::package(id, PluginPackageOperation::Enable),
            |client| client.set_enabled_blocking(id, enabled),
        );
    }

    pub(crate) fn uninstall(&self, id: &str) {
        self.apply_command(
            PluginInFlight::package(id, PluginPackageOperation::Uninstall),
            |client| client.uninstall_blocking(id),
        );
    }

    pub(crate) fn action(&self, plugin: &str, action: &str) {
        self.apply_command(PluginInFlight::action(plugin, action), |client| {
            client.action_blocking(plugin, action)
        });
    }

    pub(crate) fn queue_preview(
        &self,
        plugin: &str,
        key: &str,
        value: &str,
        hovered: bool,
        lifetime: Weak<()>,
    ) -> bool {
        self.previews
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .enqueue(
                PreviewRequest {
                    plugin: plugin.to_owned(),
                    key: key.to_owned(),
                    value: value.to_owned(),
                    lifetime,
                },
                hovered,
            )
    }

    pub(crate) fn cancel_preview(&self, plugin: &str, key: &str) {
        let mut previews = self
            .previews
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if previews
            .pending
            .as_ref()
            .is_some_and(|pending| pending.plugin == plugin && pending.key == key)
        {
            previews.pending = None;
        }
    }

    pub(crate) fn drain_previews(&self) {
        loop {
            // Take the latest option only after earlier reliable commands finish.
            // Release this lock between previews so configure/stop/poll can run.
            let _request = self
                .requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(preview) = self
                .previews
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            else {
                return;
            };
            if preview.lifetime.upgrade().is_none() {
                continue;
            }
            let Ok(client) = self.client() else {
                continue;
            };
            match client.preview_blocking(&preview.plugin, &preview.key, &preview.value) {
                Ok(snapshot) => self.publish(snapshot.into()),
                Err(error) => self.report_command_error(error),
            }
        }
    }

    pub(crate) fn configure(
        &self,
        plugin: &str,
        key: &str,
        value: bongocat_plugin_sdk::ConfigValue,
    ) {
        self.apply_command(PluginInFlight::configure(plugin, key), |client| {
            client.configure_blocking(plugin, key, value)
        });
    }

    fn apply_command(
        &self,
        command_key: PluginInFlight,
        command: impl FnOnce(&PluginServiceClient) -> Result<PluginCenterSnapshot, PluginCommandError>,
    ) -> bool {
        // Polling and command replies publish in request order. A slow poll
        // must not overwrite the newer state a configuration command returned.
        let _request = self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The page marked this command before it dispatched it; this call only
        // retires that mark.
        let Ok(client) = self.client() else {
            self.finish(&command_key);
            self.report_error("the plugin service is unavailable".to_owned());
            return false;
        };
        let result = command(&client);
        self.finish(&command_key);
        match result {
            Ok(snapshot) => {
                self.publish(snapshot.into());
                true
            }
            Err(error) => {
                self.report_command_error(error);
                false
            }
        }
    }

    fn report_command_error(&self, error: PluginCommandError) {
        self.report_error(error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::*;
    use bongocat_plugin_sdk::{PluginCopy, UiNode};
    use bongocat_plugin_service::{
        PluginCommand, PluginService, PluginServiceClient, PluginServiceEndpoint, plugin_directory,
    };
    use gpui_kit::{TestAppContext, VisualTestContext, test::TestWindowExt as _};
    use std::io::Write as _;

    #[test]
    fn preview_mailbox_coalesces_hover_and_cancels_stale_options() {
        let lifetime = Arc::new(());
        let request = |value: &str| PreviewRequest {
            plugin: "plugin".into(),
            key: "pack".into(),
            value: value.into(),
            lifetime: Arc::downgrade(&lifetime),
        };
        let mut mailbox = PreviewMailbox::default();
        assert!(mailbox.enqueue(request("first"), true));
        assert!(!mailbox.enqueue(request("second"), true));
        assert!(!mailbox.enqueue(request("first"), false));
        assert_eq!(mailbox.take().unwrap().value, "second");
        assert!(!mailbox.enqueue(request("third"), true));
        assert!(!mailbox.enqueue(request("third"), false));
        assert!(mailbox.take().is_none());
        assert!(!mailbox.running);
        assert!(mailbox.enqueue(request("fourth"), true));
        let pending = mailbox.take().unwrap();
        drop(lifetime);
        assert!(
            pending.lifetime.upgrade().is_none(),
            "a closed panel invalidates pending work"
        );
        assert!(mailbox.take().is_none());

        let host = PluginHost::default();
        let lifetime = Arc::new(());
        assert!(host.queue_preview("plugin", "pack", "first", true, Arc::downgrade(&lifetime)));
        host.cancel_preview("plugin", "other");
        assert!(host.previews.lock().unwrap().pending.is_some());
        host.cancel_preview("plugin", "pack");
        assert!(host.previews.lock().unwrap().pending.is_none());
        host.drain_previews();
        assert!(!host.previews.lock().unwrap().running);
    }

    #[cfg(target_os = "windows")]
    const BINARY_FILE: &str = "bongocat-pomodoro.exe";
    #[cfg(not(target_os = "windows"))]
    const BINARY_FILE: &str = "bongocat-pomodoro";

    const PLUGIN_ID: &str = "com.bongocat.pomodoro";

    /// A plugin store holding the checked-in pomodoro package, installed the same
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

    /// Plugin cards support only Chinese and English, even when the manifest
    /// offers additional translations.
    #[test]
    fn a_card_speaks_the_language_the_window_shows() {
        let card = sample_card();
        assert_eq!(card.copy("en-US").0, "A plugin with a longer title");
        assert_eq!(card.copy("zh-CN").0, "一个插件");
        assert_eq!(card.copy("zh-CN").1, "由插件自己书写的描述");
        assert_eq!(card.copy("zh-TW").0, "A plugin with a longer title");
        assert_eq!(card.copy("fr-FR").0, "A plugin with a longer title");
    }

    #[test]
    fn bulk_updates_wait_for_completion_continue_after_failure_and_can_retry() {
        let (client, endpoint) = PluginServiceClient::bounded(8);
        let host = PluginHost::new(client);
        let mut cards = (0..3)
            .map(|index| {
                let mut card = sample_card();
                card.id = format!("plugin-{index}");
                card
            })
            .collect::<Vec<_>>();
        let mut available = sample_card();
        available.id = "not-installed".into();
        available.version = None;
        let mut current = sample_card();
        current.id = "already-current".into();
        current.update_available = false;
        cards.extend([available, current]);
        host.publish(PluginHubState {
            cards: cards.clone(),
            ..Default::default()
        });
        let targets = host.begin_update_all();
        assert_eq!(targets, ["plugin-0", "plugin-1", "plugin-2"]);
        assert!(
            host.begin_update_all().is_empty(),
            "a batch cannot be queued twice"
        );
        assert!(host.snapshot().updating_all());

        let responder = std::thread::spawn(move || {
            let mut observed = Vec::new();
            let mut active = None;
            let mut reads = 0;
            loop {
                let command = loop {
                    if let Ok(Some(command)) = endpoint.try_recv() {
                        break command;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                };
                match command {
                    PluginCommand::InstallAvailable { id, reply } => {
                        assert!(active.is_none(), "updates must not overlap catalog jobs");
                        observed.push(id.clone());
                        active = Some(id);
                        reads = 0;
                        let mut snapshot = plugin_center_snapshots(cards.clone());
                        snapshot.loading = true;
                        reply.respond(Ok(snapshot)).unwrap();
                    }
                    PluginCommand::ReadCenter { reply } => {
                        reads += 1;
                        if reads >= 2 {
                            let id = active.take().unwrap();
                            if id != "plugin-1" {
                                let card = cards.iter_mut().find(|card| card.id == id).unwrap();
                                card.version = card.available_version.clone();
                                card.update_available = false;
                            }
                        }
                        let mut snapshot = plugin_center_snapshots(cards.clone());
                        snapshot.loading = active.is_some();
                        reply.respond(snapshot).unwrap();
                        if observed.len() == 3 && active.is_none() {
                            return observed;
                        }
                    }
                    _ => panic!("bulk updates only reuse install and read commands"),
                }
            }
        });
        host.update_all(targets);
        assert_eq!(
            responder.join().unwrap(),
            ["plugin-0", "plugin-1", "plugin-2"]
        );
        let state = host.snapshot();
        assert!(state.in_flight.is_empty());
        assert_eq!(host.take_update_report(), Some(vec!["plugin-1".to_owned()]));
        assert_eq!(host.take_update_report(), None);
        assert_eq!(host.begin_update_all(), ["plugin-1"]);
    }

    #[test]
    fn bulk_updates_release_all_controls_when_the_service_is_unavailable() {
        let host = PluginHost::default();
        host.publish(PluginHubState {
            cards: vec![sample_card()],
            ..Default::default()
        });
        let targets = host.begin_update_all();
        host.update_all(targets);
        assert!(host.snapshot().in_flight.is_empty());
        assert_eq!(host.take_update_report(), Some(vec![sample_card().id]));
    }

    /// What a window is still waiting for is the window's own bookkeeping: a state
    /// the service publishes while a command runs must not clear the control that
    /// is waiting for it.
    #[test]
    fn a_published_state_keeps_the_commands_this_window_is_waiting_for() {
        let host = PluginHost::default();
        host.publish(PluginHubState::loading());
        host.begin(PluginInFlight::package(
            PLUGIN_ID,
            PluginPackageOperation::Uninstall,
        ));
        host.begin(PluginInFlight::action(PLUGIN_ID, "toggle"));
        host.begin(PluginInFlight::configure(PLUGIN_ID, "minutes"));
        host.publish(PluginHubState {
            cards: Vec::new(),
            error: None,
            loading: false,
            ..Default::default()
        });
        let state = host.snapshot();
        assert!(!state.loading);
        assert!(state.pending_package(PLUGIN_ID, PluginPackageOperation::Uninstall));
        assert!(state.pending_action(PLUGIN_ID, "toggle"));
        assert!(state.pending_key(PLUGIN_ID, "minutes"));
        // Another plugin's controls are not held by this one's commands.
        assert!(!state.pending_action("com.example.other", "toggle"));
        assert!(!state.pending_key(PLUGIN_ID, "volume"));
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
        service.shutdown().unwrap();
    }

    /// The installed plugin's card carries the copy its manifest declares, so the
    /// center reads a Chinese name from the plugin rather than from the host.
    #[test]
    fn an_installed_card_carries_the_plugins_own_localizations() {
        let Some((service, _store)) = store_with_pomodoro() else {
            return;
        };
        let host = PluginHost::new(service.client());
        host.refresh();
        let state = host.snapshot();
        let card = &state.cards[0];
        assert_eq!(card.copy("en-US").0, "Pomodoro");
        assert_eq!(card.copy("zh-CN").0, "番茄钟");
        assert_eq!(card.copy("zh-TW").0, "Pomodoro");
        service.shutdown().unwrap();
    }

    /// The center hands the plugin's own panels to the plugin's current locale,
    /// which is what makes the plugin's own labels follow a language change.
    #[test]
    fn telling_the_service_about_a_locale_relabels_the_running_plugin() {
        let Some((service, _store)) = store_with_pomodoro() else {
            return;
        };
        let host = PluginHost::new(service.client());
        host.refresh();
        host.set_enabled(PLUGIN_ID, true);
        service.client().set_locale_blocking("zh-CN").unwrap();
        host.refresh();
        let state = host.snapshot();
        let labels = state.cards[0]
            .actions
            .iter()
            .filter_map(|node| match node {
                bongocat_plugin_sdk::UiNode::Button { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            labels.contains(&"开始") && labels.contains(&"重新计时"),
            "the plugin's own control reads Simplified Chinese: {labels:?}"
        );
        for locale in ["zh-TW", "fr-FR"] {
            service.client().set_locale_blocking(locale).unwrap();
            host.refresh();
            let state = host.snapshot();
            let card = &state.cards[0];
            assert_eq!(card.copy(locale).0, "Pomodoro");
            let labels = card
                .actions
                .iter()
                .filter_map(|node| match node {
                    UiNode::Button { label, .. } => Some(label.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert!(
                labels.contains(&"Start") && labels.contains(&"Restart"),
                "unsupported plugin locale {locale} reads English: {labels:?}"
            );
        }
        service.shutdown().unwrap();
    }

    struct PluginPageHarness {
        view: Entity<SettingsView>,
    }

    impl Render for PluginPageHarness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.view.update(cx, |view, cx| {
                // The real window reconciles the configuration panel from its own
                // render, so the harness does too: a surface the user dismissed
                // without a callback has to be noticed on the next frame here as
                // well, or the test would only cover the button that closes it.
                view.sync_plugin_settings_dialog(window, cx);
                div().w_full().child(super::super::plugins_page::content(
                    view,
                    window,
                    SettingsLanguage::ChineseSimplified,
                    cx,
                ))
            })
        }
    }

    #[gpui_kit::test]
    fn discovered_plugin_card_is_rendered_instead_of_an_empty_page(cx: &mut TestAppContext) {
        let Some((service, _store)) = store_with_pomodoro() else {
            return;
        };
        cx.update(gpui_kit::init);
        let (client, _settings_endpoint) = SettingsClient::bounded(4);
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
            Root::new(cx.new(|_| PluginPageHarness { view }), window, cx)
        });
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        visual.update(|window, _| {
            let card = window
                .try_find("plugin-card-com.bongocat.pomodoro")
                .expect("the discovered plugin must be painted")
                .bounds();
            assert!(card.size.width > px(0.) && card.size.height > px(0.));
        });
        assert_eq!(
            bongocat_i18n::text("zh-CN", "navigation.plugins.title"),
            "插件中心"
        );
    }

    /// One card with copy in the languages the requirements name, so a test reads
    /// a translation from the plugin's own map rather than from the host.
    fn sample_card() -> PluginCardView {
        PluginCardView {
            id: "com.example.card".into(),
            name: "A plugin with a longer title".into(),
            description: "A description that wraps at narrow widths without covering the controls."
                .into(),
            localizations: BTreeMap::from([
                (
                    "zh-CN".to_owned(),
                    PluginCopy {
                        name: "一个插件".into(),
                        description: "由插件自己书写的描述".into(),
                    },
                ),
                (
                    "zh-TW".to_owned(),
                    PluginCopy {
                        name: "一個外掛".into(),
                        description: "由外掛自己書寫的描述".into(),
                    },
                ),
            ]),
            icon: PluginIcon::Emoji("🐱".into()),
            enabled: true,
            failed: false,
            version: Some("1.0.0".into()),
            available_version: Some("1.1.0".into()),
            update_available: true,
            actions: vec![
                UiNode::Text {
                    text: "25:00".into(),
                },
                UiNode::Button {
                    id: "first".into(),
                    label: "Start or pause".into(),
                    icon: Some(bongocat_plugin_sdk::ActionIcon::Play),
                    loading: false,
                },
                UiNode::Button {
                    id: "second".into(),
                    label: "Restart the session".into(),
                    icon: Some(bongocat_plugin_sdk::ActionIcon::Restart),
                    loading: false,
                },
            ],
            settings: vec![
                UiNode::Integer {
                    key: "duration".into(),
                    label: "A longer setting label that may need more than one line".into(),
                    value: 25,
                    min: 1,
                    max: 180,
                },
                UiNode::Toggle {
                    key: "sound".into(),
                    label: "Sound".into(),
                    value: true,
                },
                UiNode::Button {
                    id: "preview".into(),
                    label: "Preview sound".into(),
                    icon: Some(bongocat_plugin_sdk::ActionIcon::Volume),
                    loading: false,
                },
                UiNode::Select {
                    key: "keyboard_pack".into(),
                    label: "Keyboard sound".into(),
                    value: "skcm-blue".into(),
                    options: vec![
                        bongocat_plugin_sdk::SelectOption {
                            value: "auto".into(),
                            label: "Follow the keyboard sound".into(),
                            group: None,
                        },
                        bongocat_plugin_sdk::SelectOption {
                            value: "skcm-blue".into(),
                            label: "SKCM Blue · Alps".into(),
                            group: None,
                        },
                    ],
                    preview: false,
                },
            ],
        }
    }

    /// A window rendering one card of the center, at a chosen width.
    ///
    /// The page picks its column count from the window's viewport, so the test
    /// window is opened at the width under test rather than maximized: a maximized
    /// test window would report the wide-window layout for every width, and a
    /// responsive grid test that cannot observe the narrow layouts proves nothing.
    fn card_page(
        cx: &mut TestAppContext,
        cards: Vec<PluginCardView>,
        width: f32,
    ) -> (Entity<SettingsView>, &mut VisualTestContext) {
        let PluginPage {
            view,
            visual,
            endpoint: _,
        } = plugin_page(cx, cards, width);
        (view, visual)
    }

    struct PluginPage {
        view: Entity<SettingsView>,
        visual: &'static mut VisualTestContext,
        endpoint: PluginServiceEndpoint,
    }

    /// The same page, with the plugin-service endpoint a test can inspect.
    fn plugin_page(cx: &mut TestAppContext, cards: Vec<PluginCardView>, width: f32) -> PluginPage {
        cx.update(gpui_kit::init);
        let (client, _endpoint) = SettingsClient::bounded(4);
        let (plugin_client, endpoint) = PluginServiceClient::bounded(8);
        let capture = Rc::new(RefCell::new(None));
        let built = capture.clone();
        let window = cx.open_window(size(px(width), px(900.0)), move |window, cx| {
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
                view.plugin_host.publish(PluginHubState {
                    cards,
                    error: None,
                    loading: false,
                    ..Default::default()
                });
                view
            });
            built.borrow_mut().replace(view.clone());
            Root::new(cx.new(|_| PluginPageHarness { view }), window, cx)
        });
        let view = capture.borrow_mut().take().unwrap();
        let visual = VisualTestContext::from_window(*window, cx).into_mut();
        visual.run_until_parked();
        PluginPage {
            view,
            visual,
            endpoint,
        }
    }

    #[gpui_kit::test]
    fn the_center_lays_its_cards_out_in_a_responsive_grid(cx: &mut TestAppContext) {
        // The same window at several widths: the cards multiply into the row as
        // the window grows, by the same rule the model library's catalog uses.
        // Six cards are enough to reach a second row at every width below.
        for width in [360., 800., 1_200., 1_600., 2_000.] {
            let columns = models::grid_columns_for_window(
                px(width),
                super::super::plugins_page::PLUGIN_CARD_MIN_WIDTH,
            );
            let cards = (0..6)
                .map(|index| {
                    let mut card = sample_card();
                    card.id = format!("com.example.card-{index}");
                    card
                })
                .collect::<Vec<_>>();
            let (_, visual) = card_page(cx, cards, width);
            let rows = visual.update(|window, _| {
                (0..6)
                    .map(|index| {
                        window
                            .find(SharedString::from(format!(
                                "plugin-card-com.example.card-{index}"
                            )))
                            .bounds()
                    })
                    .collect::<Vec<_>>()
            });
            let first_row = rows.iter().take(columns).collect::<Vec<_>>();
            assert_eq!(
                first_row.len(),
                columns,
                "at {width}px the grid holds {columns} columns"
            );
            // Every cell on the first row shares a line and has the same width:
            // the grid stretches its columns, so the row ends at the edge.
            for cell in &first_row {
                assert_eq!(
                    cell.center().y,
                    first_row[0].center().y,
                    "at {width}px every card of the first row shares one line"
                );
                assert!(
                    (cell.size.width - first_row[0].size.width).abs() <= px(1.0),
                    "at {width}px the columns are equal, got {:?} and {:?}",
                    cell.size.width,
                    first_row[0].size.width
                );
            }
            // The card past the first row starts below it, which is what makes
            // the count a row capacity rather than a total.
            assert!(
                rows[columns].origin.y > first_row[0].origin.y,
                "at {width}px the seventh card starts a second row"
            );
        }
    }

    #[gpui_kit::test]
    fn update_actions_stay_inside_cards_and_wrap_as_a_whole_footer(cx: &mut TestAppContext) {
        for width in [360., 480., 800., 1_200., 1_600.] {
            // Pomodoro publishes three model-window controls on top of the
            // lifecycle commands, which is the widest footer the center draws.
            let mut card = sample_card();
            card.actions.push(UiNode::Button {
                id: "third".into(),
                label: "Reset the timer".into(),
                icon: Some(bongocat_plugin_sdk::ActionIcon::Restart),
                loading: false,
            });
            let (_, visual) = card_page(cx, vec![card], width);
            visual.update(|window, _| {
                let card = window.find("plugin-card-com.example.card").bounds();
                let toggle = window.find("plugin-toggle-com.example.card").bounds();
                for id in ["first", "second", "third", "install", "uninstall"] {
                    let id = if matches!(id, "first" | "second" | "third") {
                        format!("plugin-action-com.example.card-{id}")
                    } else {
                        format!("plugin-{id}-com.example.card")
                    };
                    let button = window.find(SharedString::from(id)).bounds();
                    assert!(
                        button.left() >= card.left() && button.right() <= card.right(),
                        "at {width}px each action must fit inside its card: {button:?} in {card:?}"
                    );
                    assert!(button.bottom() <= card.bottom());
                    assert!(
                        button.right() <= toggle.left()
                            || button.left() >= toggle.right()
                            || button.top() >= toggle.bottom()
                            || button.bottom() <= toggle.top(),
                        "actions must not overlap the switch"
                    );
                }
                assert!(
                    window
                        .try_find("plugin-update-badge-com.example.card")
                        .is_some()
                );
                assert!(window.try_find("plugin-update-all").is_some());
            });
        }
    }

    #[gpui_kit::test]
    fn every_action_button_carries_the_glyph_its_plugin_declared(cx: &mut TestAppContext) {
        use bongocat_plugin_sdk::ActionIcon;
        use gpui_kit::assets::IconName;
        for (icon, expected) in [
            (Some(ActionIcon::Play), IconName::Play),
            (Some(ActionIcon::Pause), IconName::Pause),
            (Some(ActionIcon::Restart), IconName::RotateCcw),
            (Some(ActionIcon::Volume), IconName::Volume2),
            (None, IconName::MousePointerClick),
        ] {
            assert_eq!(super::super::plugins_page::action_icon(icon), expected);
        }
        let (_, visual) = card_page(cx, vec![sample_card()], 720.);
        visual.update(|window, _| {
            // The plugin named a glyph per action, so the buttons are told apart
            // by what they do rather than all drawing the same pointer.
            let play = window.find("plugin-action-com.example.card-first").bounds();
            let restart = window
                .find("plugin-action-com.example.card-second")
                .bounds();
            assert_eq!(play.size.width, play.size.height, "actions are icon-only");
            assert_eq!(restart.size.width, restart.size.height);
            // Each installed plugin offers exactly one settings entry, beside the
            // identity it configures rather than inside the lifecycle row.
            assert!(
                window
                    .try_find("plugin-settings-com.example.card")
                    .is_some()
            );
            let actions = window.find("plugin-actions-com.example.card").bounds();
            let settings = window.find("plugin-settings-com.example.card").bounds();
            assert!(settings.bottom() <= actions.top());
        });
    }

    #[gpui_kit::test]
    fn a_disabled_card_keeps_lifecycle_controls_but_hides_plugin_commands(cx: &mut TestAppContext) {
        let mut card = sample_card();
        card.enabled = false;
        let (_, visual) = card_page(cx, vec![card], 480.);
        visual.update(|window, _| {
            assert!(window.try_find("plugin-toggle-com.example.card").is_some());
            assert!(window.try_find("plugin-reset-com.example.card").is_none());
            assert!(
                window
                    .try_find("plugin-uninstall-com.example.card")
                    .is_some()
            );
            // A plugin that is not running has no model-window controls and
            // nothing to configure, so neither entry is offered.
            assert!(
                window
                    .try_find("plugin-action-com.example.card-first")
                    .is_none()
            );
            assert!(
                window
                    .try_find("plugin-settings-com.example.card")
                    .is_none()
            );
        });
    }

    #[gpui_kit::test]
    fn an_available_card_only_offers_installation(cx: &mut TestAppContext) {
        let mut card = sample_card();
        card.version = None;
        card.enabled = false;
        card.update_available = false;
        let (_, visual) = card_page(cx, vec![card], 480.);
        visual.update(|window, _| {
            assert!(window.try_find("plugin-install-com.example.card").is_some());
            assert!(window.try_find("plugin-toggle-com.example.card").is_none());
            assert!(window.try_find("plugin-reset-com.example.card").is_none());
            assert!(
                window
                    .try_find("plugin-uninstall-com.example.card")
                    .is_none()
            );
            assert!(
                window
                    .try_find("plugin-settings-com.example.card")
                    .is_none()
            );
        });
    }

    /// A plugin that reports an action still running draws that on the button
    /// itself, rather than publishing a line of placeholder text beside it.
    #[gpui_kit::test]
    fn a_plugin_owned_action_state_is_drawn_on_its_own_button(cx: &mut TestAppContext) {
        let mut card = sample_card();
        card.actions = vec![UiNode::Button {
            id: "preview".into(),
            label: "Preview".into(),
            icon: Some(bongocat_plugin_sdk::ActionIcon::Volume),
            loading: true,
        }];
        card.settings = Vec::new();
        let (_, visual) = card_page(cx, vec![card], 720.);
        visual.update(|window, _| {
            let button = window
                .find("plugin-action-com.example.card-preview")
                .bounds();
            assert!(button.size.width > px(0.));
            // The plugin published no text node for the action, so nothing on the
            // card reads as a placeholder for it.
            assert!(
                window
                    .try_find("plugin-action-com.example.card-preview")
                    .is_some()
            );
        });
    }

    /// One plugin's settings entry opens that plugin's own panel, and the panel
    /// draws the controls that plugin published.
    #[gpui_kit::test]
    fn a_card_opens_the_plugin_owned_settings_panel(cx: &mut TestAppContext) {
        let (view, visual) = card_page(cx, vec![sample_card()], 720.);
        visual.update(|window, cx| {
            window.click("plugin-settings-com.example.card", cx);
        });
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        visual.update(|window, cx| assert!(window.has_active_dialog(cx)));
        view.read_with(visual, |view, _| {
            let draft = view
                .plugin_settings
                .as_ref()
                .expect("the panel records the plugin it belongs to");
            assert_eq!(draft.plugin, "com.example.card");
        });
        // The panel is the plugin's own tree: its fields are drawn by the plugin's
        // keys and labels, not by a host-side schema.
        for id in [
            "plugin-setting-integer-duration-input",
            "plugin-setting-toggle-sound",
            "plugin-setting-button-preview",
        ] {
            visual.update(|window, _| {
                assert!(
                    window.try_find(SharedString::from(id)).is_some(),
                    "{id} must be painted"
                );
            });
        }
        // The integer row is the component's spinbutton, not a pair of host-owned
        // step buttons.
        visual.update(|window, _| {
            assert!(
                window
                    .try_find("plugin-setting-integer-duration-increase")
                    .is_none(),
                "the old increase button must be replaced by NumberInput"
            );
            assert!(
                window
                    .try_find("plugin-setting-integer-duration-decrease")
                    .is_none(),
                "the old decrease button must be replaced by NumberInput"
            );
        });
        // A select draws the component's own trigger, and its bounds prove the
        // plugin's options reached a real dropdown with room to draw in. The row
        // itself is published by the panel, and the panel would have panicked
        // while building it if the dropdown the plugin's options created were
        // missing.
        let row = visual.update(|window, _| {
            window
                .try_find(ElementId::from(("plugin-setting-select", 3usize)))
                .expect("the plugin's select row must be painted")
                .bounds()
        });
        assert!(
            row.size.width > px(0.) && row.size.height > px(0.),
            "the plugin dropdown has to occupy its row"
        );
        // Closing the panel drops the draft, so the next open is a fresh read of
        // whatever the plugin is publishing then.
        // The dialog slides in on frame time, so it has to settle before pointer
        // hit testing: otherwise a click computes its target from one frame's
        // bounds and lands after the next frame has moved the control.
        std::thread::sleep(std::time::Duration::from_millis(200));
        visual.update(|window, cx| window.click("plugin-settings-close", cx));
        // A dialog leaves the screen through its own control or through the
        // overlay swallowing a click outside it, and both are a close. The frame
        // that follows is what reconciles the draft, exactly as it does in the
        // settings window, so this asserts on the frame rather than on the click.
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        view.read_with(visual, |view, _| {
            assert!(view.plugin_settings.is_none());
        });
    }

    /// The number row is a real spinbutton: typed values and keyboard steps
    /// reach the plugin's typed configuration command, and leaving an
    /// out-of-range draft saves the clamped value.
    #[gpui_kit::test]
    fn a_plugin_integer_setting_is_edited_with_number_input(cx: &mut TestAppContext) {
        let PluginPage {
            view,
            visual,
            endpoint,
        } = plugin_page(cx, vec![sample_card()], 720.);
        let responder =
            std::thread::spawn(move || respond_to_integer_commands(endpoint, &[30, 31, 180, 42]));
        visual.update(|window, cx| window.click("plugin-settings-com.example.card", cx));
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        let input_focus = visual.update(|_, cx| {
            let draft = view.read(cx).plugin_settings.as_ref().unwrap();
            draft.numbers.borrow().entities["duration"]
                .input
                .read(cx)
                .focus_handle(cx)
        });
        visual.update(|window, cx| window.focus(&input_focus, cx));
        visual.run_until_parked();
        let input = view.read_with(visual, |view, _| {
            view.plugin_settings
                .as_ref()
                .unwrap()
                .numbers
                .borrow()
                .entities["duration"]
                .input
                .clone()
        });
        visual.update(|window, cx| {
            input.update(cx, |input, cx| input.select_all(window, cx));
            window.input("30", cx);
        });
        visual.update(|_, cx| {
            assert_eq!(
                view.read(cx)
                    .plugin_settings
                    .as_ref()
                    .unwrap()
                    .numbers
                    .borrow()
                    .entities["duration"]
                    .input
                    .read(cx)
                    .value(),
                "30",
                "typing replaces the selected number"
            );
        });
        visual.run_until_parked();

        // Enter commits the field. The input propagates that action so a form can
        // submit, but the plugin panel must remain open instead of running the
        // dialog's confirm action.
        visual.update(|window, cx| window.press("enter", cx));
        visual.run_until_parked();
        visual.update(|window, cx| assert!(window.has_active_dialog(cx)));

        // Up/Down are the component's own step keys. The panel commits the
        // resulting value through the same typed command as Enter.
        visual.update(|window, cx| {
            input_focus.dispatch_action(&gpui_kit::base::Increment, window, cx);
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        assert_eq!(
            input.read_with(visual, |input, _| input.value()),
            "31",
            "a keyboard step writes the next value returned by the plugin"
        );

        // Out-of-range text is allowed while editing. Moving focus away commits
        // the clamped value rather than leaving it in the field.
        visual.update(|window, cx| {
            input.update(cx, |input, cx| input.select_all(window, cx));
            window.input("999", cx);
        });
        assert_eq!(
            input.read_with(visual, |input, _| input.value()),
            "999",
            "an out-of-range draft is accepted while the field has focus"
        );
        visual.update(|window, cx| {
            assert!(
                input.read(cx).focus_handle(cx).is_focused(window),
                "the number field must still own focus before blur"
            );
        });
        // A real click on a sibling control moves focus along the dialog's
        // rendered focus path, which is what delivers InputState::Blur. Moving
        // to a detached focus handle (or clearing window focus) would not.
        visual.update(|window, cx| window.click("plugin-setting-toggle-sound", cx));
        visual.update(|window, cx| {
            assert!(
                !input.read(cx).focus_handle(cx).is_focused(window),
                "clicking a sibling must move focus off the number field"
            );
        });
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(50));
        visual.run_until_parked();
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        assert_eq!(
            input.read_with(visual, |input, _| input.value()),
            "180",
            "blur commits the clamped maximum"
        );

        // An incomplete draft is put back to the last published value on blur,
        // and never becomes a configuration command in its own right.
        visual.update(|window, cx| window.focus(&input_focus, cx));
        visual.update(|window, cx| {
            input.update(cx, |input, cx| input.select_all(window, cx));
            window.press("backspace", cx);
        });
        visual.update(|window, cx| window.click("plugin-setting-toggle-sound", cx));
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        assert_eq!(
            input.read_with(visual, |input, _| input.value()),
            "180",
            "an empty draft returns to the latest accepted value"
        );
        // Closing immediately after typing must commit too. The close callback
        // can run before the next frame observes the number field's blur.
        visual.update(|window, cx| window.focus(&input_focus, cx));
        visual.update(|window, cx| {
            input.update(cx, |input, cx| input.select_all(window, cx));
            window.input("42", cx);
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.click("plugin-settings-close", cx));
        visual.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(200));
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        view.read_with(visual, |view, _| {
            assert!(view.plugin_settings.is_none());
        });
        let report = responder
            .join()
            .expect("the command responder must not panic");
        assert_eq!(report.observed, [30, 31, 180, 42]);
        assert!(
            report.unexpected.is_empty(),
            "an empty draft must not issue a configuration command: {:?}",
            report.unexpected
        );
    }

    #[derive(Debug, Eq, PartialEq)]
    struct IntegerCommandReport {
        observed: Vec<i64>,
        unexpected: Vec<i64>,
    }

    /// Stand in for the plugin service while the window's blocking command runs
    /// on the test executor. The service replies with the state it committed, so
    /// the panel continues from the same value it sent.
    ///
    /// The card it answers from is held for the whole run, exactly as a running
    /// plugin would hold its own configuration: a reply to some other key is
    /// then a fresh read of that same state rather than a reset to the values
    /// the fixture started with.
    fn respond_to_integer_commands(
        endpoint: PluginServiceEndpoint,
        expected: &[i64],
    ) -> IntegerCommandReport {
        let mut report = IntegerCommandReport {
            observed: Vec::new(),
            unexpected: Vec::new(),
        };
        let mut card = sample_card();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while report.observed.len() < expected.len() && std::time::Instant::now() < deadline {
            let Ok(Some(command)) = endpoint.try_recv() else {
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            };
            let Some(value) =
                reply_to_integer_command(command, expected[report.observed.len()], &mut card)
            else {
                continue;
            };
            report.observed.push(value);
        }

        // Give a stray command time to arrive, and answer it so the test
        // executor cannot remain blocked on an unhandled reply.
        let grace = std::time::Instant::now() + std::time::Duration::from_millis(250);
        while std::time::Instant::now() < grace {
            let Ok(Some(command)) = endpoint.try_recv() else {
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            };
            let Some(value) =
                reply_to_integer_command(command, *expected.last().unwrap(), &mut card)
            else {
                continue;
            };
            report.unexpected.push(value);
        }
        report
    }

    /// Answer one command with `expected` as the committed duration, reporting
    /// the integer the field actually sent. A command for another key is answered
    /// from the card as it stands, so it cannot undo the number just stored.
    fn reply_to_integer_command(
        command: PluginCommand,
        expected: i64,
        card: &mut PluginCardView,
    ) -> Option<i64> {
        let PluginCommand::Configure {
            plugin,
            key,
            value,
            reply,
        } = command
        else {
            return None;
        };
        if plugin != "com.example.card" || key != "duration" {
            reply
                .respond(Ok(plugin_center_snapshot(card.clone())))
                .unwrap();
            return None;
        }
        let bongocat_plugin_sdk::ConfigValue::Integer(value) = value else {
            panic!("the number field must write an integer");
        };
        let Some(UiNode::Integer {
            value: published, ..
        }) = card.settings.first_mut()
        else {
            unreachable!("the sample card's first setting is an integer");
        };
        *published = expected;
        reply
            .respond(Ok(plugin_center_snapshot(card.clone())))
            .unwrap();
        Some(value)
    }

    fn plugin_center_snapshot(
        card: PluginCardView,
    ) -> bongocat_plugin_service::PluginCenterSnapshot {
        plugin_center_snapshots(vec![card])
    }

    fn plugin_center_snapshots(cards: Vec<PluginCardView>) -> PluginCenterSnapshot {
        bongocat_plugin_service::PluginCenterSnapshot {
            cards: cards
                .into_iter()
                .map(|card| bongocat_plugin_service::PluginCard {
                    id: card.id,
                    name: card.name,
                    description: card.description,
                    localizations: card.localizations,
                    icon: card.icon,
                    enabled: card.enabled,
                    failed: card.failed,
                    version: card.version,
                    available_version: card.available_version,
                    update_available: card.update_available,
                    actions: card.actions,
                    settings: card.settings,
                })
                .collect(),
            error: None,
            loading: false,
        }
    }

    /// A command a user started is marked on the GPUI thread, so the button that
    /// started it draws as busy before the reply arrives, and the mark is retired
    /// once it does.
    #[gpui_kit::test]
    fn a_command_in_flight_is_drawn_on_the_control_that_started_it(cx: &mut TestAppContext) {
        let (view, visual) = card_page(cx, vec![sample_card()], 720.);
        view.update(visual, |view, _| {
            view.plugin_host.begin(PluginInFlight::package(
                "com.example.card",
                PluginPackageOperation::Uninstall,
            ));
            view.plugin_host
                .begin(PluginInFlight::action("com.example.card", "first"));
            view.plugin_host
                .begin(PluginInFlight::configure("com.example.card", "duration"));
        });
        visual.update(|window, cx| window.render_frame(cx));
        view.read_with(visual, |view, _| {
            let state = view.plugin_host.snapshot();
            assert!(state.pending_package("com.example.card", PluginPackageOperation::Uninstall));
            assert!(state.pending_action("com.example.card", "first"));
            assert!(state.pending_key("com.example.card", "duration"));
        });

        // Retiring one command clears only that command.
        view.update(visual, |view, _| {
            view.plugin_host
                .finish(&PluginInFlight::action("com.example.card", "first"));
        });
        view.read_with(visual, |view, _| {
            let state = view.plugin_host.snapshot();
            assert!(!state.pending_action("com.example.card", "first"));
            assert!(state.pending_package("com.example.card", PluginPackageOperation::Uninstall));
        });
    }
}
