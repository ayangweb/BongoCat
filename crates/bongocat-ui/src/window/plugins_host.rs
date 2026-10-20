//! The settings window's view of the plugin service.
//!
//! The plugins themselves are owned by `bongocat-plugin-service`, which outlives
//! every window: a timer has to keep counting while the settings window is closed.
//! What is here is only the window's side of that conversation — the cache the
//! page renders and the typed commands the controls send. Nothing in this module
//! starts, ticks or stops a plugin, so the page cannot become the reason a plugin
//! runs or stops running.

use bongocat_plugin_sdk::PluginCopy;
use bongocat_plugin_service::{
    PluginCard, PluginCenterSnapshot, PluginCommandError, PluginServiceClient,
};
use std::{
    collections::{BTreeMap, BTreeSet},
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
            && self.has_updates()
    }

    /// Whether the catalog has a newer version for an installed plugin.
    ///
    /// This is what the page needs rather than "the batch may start now": a
    /// command already running blocks the button without emptying the catalog, so
    /// the two conditions decide different things.
    pub(crate) fn has_updates(&self) -> bool {
        self.cards.iter().any(PluginCardView::pending_update)
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
            loading: snapshot.loading,
            ..Default::default()
        }
    }
}

/// One failed plugin command, as the window reports it.
///
/// The service speaks in stable error codes rather than sentences, so the
/// window is where a code becomes localized copy. The operation travels with
/// the error because "the plugin operation failed" is not actionable: an
/// install, an update and an uninstall each need to name what failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PluginFailure {
    Refresh,
    Load {
        plugin: String,
        running: bool,
    },
    Reported {
        plugin: String,
        id: String,
        message: String,
    },
    Preview {
        plugin: String,
        key: String,
    },
    Install {
        plugin: String,
        update: bool,
    },
    Enable {
        plugin: String,
        enabled: bool,
    },
    Uninstall {
        plugin: String,
    },
    Action {
        plugin: String,
        action: String,
    },
    Configure {
        plugin: String,
        key: String,
    },
    UpdateAll {
        failed: Vec<String>,
    },
}

impl PluginFailure {
    /// The one plugin a command was aimed at, if the failure names one.
    ///
    /// A snapshot produced by a successful command is allowed to carry that
    /// plugin's new failed state without also becoming a separate "load failed"
    /// notification. The same target is remembered when the command itself
    /// fails, because that command returns no snapshot.
    fn command_target(&self) -> Option<&str> {
        match self {
            Self::Install { plugin, .. }
            | Self::Enable { plugin, .. }
            | Self::Uninstall { plugin }
            | Self::Action { plugin, .. }
            | Self::Configure { plugin, .. }
            | Self::Preview { plugin, .. } => Some(plugin),
            Self::Refresh | Self::Load { .. } | Self::Reported { .. } | Self::UpdateAll { .. } => {
                None
            }
        }
    }
}

/// A failure and the stable code the service returned for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginFailureReport {
    pub(crate) failure: PluginFailure,
    pub(crate) code: PluginCommandError,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginCardView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    /// The plugin's own name and description per locale, and the fallback above.
    ///
    /// The card carries the plugin's own map rather than a resolved pair because
    /// the language on screen is a property of the window: a user switching
    /// language repaints from the same cards instead of asking every plugin to
    /// republish metadata.
    pub(crate) localizations: BTreeMap<String, PluginCopy>,
    pub(crate) icon_url: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) failed: bool,
    pub(crate) version: Option<String>,
    pub(crate) available_version: Option<String>,
    pub(crate) update_available: bool,
    pub(crate) actions: Vec<bongocat_plugin_sdk::UiNode>,
    pub(crate) settings: Vec<bongocat_plugin_sdk::UiNode>,
    pub(crate) errors: Vec<bongocat_plugin_sdk::PluginError>,
    pub(crate) interaction_failure: Option<bongocat_plugin_service::PluginInteractionFailure>,
}

impl PluginCardView {
    /// Whether a newer version is waiting to be installed for this plugin.
    ///
    /// The badge, the per-card update button and the batch all answer from this
    /// one rule, so the center cannot show an update and refuse to run it.
    pub(crate) fn pending_update(&self) -> bool {
        self.version.is_some() && self.update_available
    }

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
            icon_url: card.icon_url,
            enabled: card.enabled,
            failed: card.failed,
            version: card.version,
            available_version: card.available_version,
            update_available: card.update_available,
            actions: card.actions,
            settings: card.settings,
            errors: card.errors,
            interaction_failure: card.interaction_failure,
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
    failures: Arc<Mutex<Vec<PluginFailureReport>>>,
    /// The plugin whose failed state was reported by the command that produced
    /// it, until a later snapshot shows that plugin healthy again.
    ///
    /// A failed enable returns no snapshot, so the card only learns that the
    /// plugin is now `Failed` on the next poll. That poll must not turn the same
    /// failure into a second "loading failed" notification.
    failed_command_targets: Arc<Mutex<BTreeSet<String>>>,
    discovery_failure_reported: Arc<Mutex<Option<String>>>,
    update_succeeded: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for PluginHost {
    fn default() -> Self {
        Self {
            client: None,
            published: Arc::new(Mutex::new(PluginHubState::loading())),
            requests: Arc::default(),
            previews: Arc::default(),
            failures: Arc::default(),
            failed_command_targets: Arc::default(),
            discovery_failure_reported: Arc::default(),
            update_succeeded: Arc::default(),
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
            failures: Arc::default(),
            failed_command_targets: Arc::default(),
            discovery_failure_reported: Arc::default(),
            update_succeeded: Arc::default(),
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

    /// Publish what the service answered and queue a notification for the
    /// center-level failure it carries, if any.
    ///
    /// The service keeps the last refusal in its snapshot so a window that
    /// opens after the failure can still explain it. The window remembers the
    /// text it already reported, so polling the same refusal does not repeat
    /// the notification on every frame; a different refusal is new and is
    /// reported once.
    fn publish_snapshot(&self, snapshot: PluginCenterSnapshot) {
        self.publish_reply(snapshot, None);
    }

    fn publish_reply(&self, snapshot: PluginCenterSnapshot, command: Option<&PluginFailure>) {
        let previous = self.snapshot();
        // A failed command returns no snapshot. Consume the marker on the next
        // publication, which is the first one that can show the resulting
        // `Failed` card; keeping it any longer could hide a later independent
        // failure after the plugin recovers.
        let failed_command_targets = std::mem::take(
            &mut *self
                .failed_command_targets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for card in &snapshot.cards {
            let old = previous.cards.iter().find(|old| old.id == card.id);
            let command_target = command.and_then(PluginFailure::command_target);
            if card.failed
                && !old.is_some_and(|old| old.failed)
                && command_target != Some(card.id.as_str())
                && !failed_command_targets.contains(&card.id)
            {
                self.record_failure(
                    PluginFailure::Load {
                        plugin: card.id.clone(),
                        running: old.is_some_and(|old| old.enabled),
                    },
                    PluginCommandError::Operation,
                );
            }
            for error in &card.errors {
                if !old.is_some_and(|old| old.errors.iter().any(|old| old.id == error.id)) {
                    self.record_failure(
                        PluginFailure::Reported {
                            plugin: card.id.clone(),
                            id: error.id.clone(),
                            message: error.message.clone(),
                        },
                        PluginCommandError::Operation,
                    );
                }
            }
            if card.interaction_failure != old.and_then(|old| old.interaction_failure.clone())
                && let Some(failure) = &card.interaction_failure
            {
                let (failure, code) = match failure {
                    bongocat_plugin_service::PluginInteractionFailure::Action { action, code } => (
                        PluginFailure::Action {
                            plugin: card.id.clone(),
                            action: action.clone(),
                        },
                        code,
                    ),
                    bongocat_plugin_service::PluginInteractionFailure::Configure { key, code } => (
                        PluginFailure::Configure {
                            plugin: card.id.clone(),
                            key: key.clone(),
                        },
                        code,
                    ),
                };
                self.record_failure(failure, code.clone());
            }
        }
        let error = snapshot.error.clone();
        self.publish(snapshot.into());
        if matches!(command, Some(PluginFailure::Install { .. })) {
            *self
                .discovery_failure_reported
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = error;
        } else {
            self.report_discovery_failure(error, PluginCommandError::Operation);
        }
    }

    fn report_discovery_failure(&self, error: Option<String>, code: PluginCommandError) {
        let mut reported = self
            .discovery_failure_reported
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if error.is_none() {
            *reported = None;
            return;
        }
        if *reported == error {
            return;
        }
        *reported = error;
        self.record_failure(PluginFailure::Refresh, code);
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
            .filter(|card| card.pending_update())
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
        if !failed.is_empty() {
            self.record_failure(
                PluginFailure::UpdateAll { failed },
                PluginCommandError::Operation,
            );
        } else {
            self.update_succeeded
                .store(true, std::sync::atomic::Ordering::Release);
        }
        self.finish(&PluginInFlight::UpdateAll);
    }

    pub(crate) fn take_failure(&self) -> Option<PluginFailureReport> {
        let mut failures = self
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (!failures.is_empty()).then(|| failures.remove(0))
    }

    fn record_failure(&self, failure: PluginFailure, code: PluginCommandError) {
        if matches!(failure, PluginFailure::Install { .. }) && self.snapshot().updating_all() {
            return;
        }
        let mut failures = self
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let report = PluginFailureReport { failure, code };
        if !failures.contains(&report) {
            failures.push(report);
        }
    }

    pub(crate) fn has_failures(&self) -> bool {
        !self
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }

    pub(crate) fn take_update_success(&self) -> bool {
        self.update_succeeded
            .swap(false, std::sync::atomic::Ordering::AcqRel)
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

    /// Read the center's current state and publish it for the next frame.
    pub(crate) fn refresh(&self) {
        let _request = self
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Ok(client) = self.client() else {
            self.published
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .loading = false;
            self.report_discovery_failure(
                Some(PluginCommandError::ServiceUnavailable.code().into()),
                PluginCommandError::ServiceUnavailable,
            );
            return;
        };
        match client.read_center_blocking() {
            Ok(snapshot) => self.publish_snapshot(snapshot),
            Err(error) => {
                self.published
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .loading = false;
                self.report_discovery_failure(Some(error.code().into()), error);
            }
        }
    }

    /// Scan the package store again, for the page's retry affordance.
    pub(crate) fn rescan(&self) {
        *self
            .discovery_failure_reported
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.apply_command(PluginInFlight::Refresh, PluginFailure::Refresh, |client| {
            client.refresh_blocking()
        });
    }

    pub(crate) fn install_available(&self, id: &str) -> bool {
        let update = self
            .snapshot()
            .cards
            .iter()
            .find(|card| card.id == id)
            .is_some_and(|card| card.version.is_some());
        self.apply_command(
            PluginInFlight::package(id, PluginPackageOperation::Install),
            PluginFailure::Install {
                plugin: id.to_owned(),
                update,
            },
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
                    self.publish_reply(
                        snapshot,
                        Some(&PluginFailure::Install {
                            plugin: id.to_owned(),
                            update,
                        }),
                    );
                    return Err(PluginCommandError::Operation);
                }
                Ok(snapshot)
            },
        )
    }

    pub(crate) fn set_enabled(&self, id: &str, enabled: bool) {
        self.apply_command(
            PluginInFlight::package(id, PluginPackageOperation::Enable),
            PluginFailure::Enable {
                plugin: id.to_owned(),
                enabled,
            },
            |client| client.set_enabled_blocking(id, enabled),
        );
    }

    pub(crate) fn uninstall(&self, id: &str) {
        self.apply_command(
            PluginInFlight::package(id, PluginPackageOperation::Uninstall),
            PluginFailure::Uninstall {
                plugin: id.to_owned(),
            },
            |client| client.uninstall_blocking(id),
        );
    }

    pub(crate) fn action(&self, plugin: &str, action: &str) {
        self.apply_command(
            PluginInFlight::action(plugin, action),
            PluginFailure::Action {
                plugin: plugin.to_owned(),
                action: action.to_owned(),
            },
            |client| client.action_blocking(plugin, action),
        );
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
            let failure = PluginFailure::Preview {
                plugin: preview.plugin.clone(),
                key: preview.key.clone(),
            };
            match self.client().and_then(|client| {
                client.preview_blocking(&preview.plugin, &preview.key, &preview.value)
            }) {
                Ok(snapshot) => self.publish_reply(snapshot, Some(&failure)),
                Err(error) => self.record_failure(failure, error),
            }
        }
    }

    pub(crate) fn configure(
        &self,
        plugin: &str,
        key: &str,
        value: bongocat_plugin_sdk::ConfigValue,
    ) {
        self.apply_command(
            PluginInFlight::configure(plugin, key),
            PluginFailure::Configure {
                plugin: plugin.to_owned(),
                key: key.to_owned(),
            },
            |client| client.configure_blocking(plugin, key, value),
        );
    }

    fn apply_command(
        &self,
        command_key: PluginInFlight,
        failure: PluginFailure,
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
            if matches!(failure, PluginFailure::Refresh) {
                self.report_discovery_failure(
                    Some(PluginCommandError::ServiceUnavailable.code().into()),
                    PluginCommandError::ServiceUnavailable,
                );
            } else {
                if let Some(plugin) = failure.command_target() {
                    self.failed_command_targets
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(plugin.to_owned());
                }
                self.record_failure(failure, PluginCommandError::ServiceUnavailable);
            }
            return false;
        };
        let result = command(&client);
        self.finish(&command_key);
        match result {
            Ok(snapshot) => {
                self.publish_reply(snapshot, Some(&failure));
                true
            }
            Err(error) => {
                if matches!(failure, PluginFailure::Refresh) {
                    // A failed explicit rescan is also visible to the polling
                    // path. Keep the same failure armed there so the next poll
                    // does not immediately emit a duplicate notification.
                    self.report_discovery_failure(Some(error.code().into()), error);
                } else {
                    if let Some(plugin) = failure.command_target() {
                        self.failed_command_targets
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .insert(plugin.to_owned());
                    }
                    self.record_failure(failure, error);
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::*;
    use bongocat_plugin_sdk::{PluginCopy, UiNode};
    use bongocat_plugin_service::{
        PluginCommand, PluginService, PluginServiceClient, PluginServiceEndpoint, PluginSource,
        plugin_directory,
    };
    use gpui_kit::{TestAppContext, VisualTestContext, http_client, test::TestWindowExt as _};
    use std::{future::Future, io::Write as _, pin::Pin, sync::Arc};

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

    /// A plugin store holding the checked-in pomodoro package, installed from a
    /// Development catalog the same way a downloaded package is.
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
        archive.start_file("resources/icon.png", options).ok()?;
        archive
            .write_all(&std::fs::read(root.join("resources/icon.png")).ok()?)
            .ok()?;
        archive
            .start_file(BINARY_FILE, options.unix_permissions(0o755))
            .ok()?;
        archive.write_all(&std::fs::read(binary).ok()?).ok()?;
        let bytes = archive.finish().ok()?.into_inner();

        // Installing is a catalog operation, so the package is published as the
        // Development catalog's single entry and installed from it.
        let catalog = tempfile::tempdir().ok()?;
        let manifest = bongocat_plugin_manager::install::inspect_package(&bytes).ok()?;
        std::fs::write(
            catalog.path().join(format!(
                "{}-{}.zip",
                manifest.metadata.id, manifest.metadata.version
            )),
            &bytes,
        )
        .ok()?;
        let icon = catalog.path().join(format!("{}.png", manifest.metadata.id));
        std::fs::copy(root.join("resources/icon.png"), &icon).ok()?;
        std::fs::write(
            catalog.path().join("catalog.json"),
            serde_json::to_vec(&bongocat_plugin_manager::Catalog {
                plugins: vec![bongocat_plugin_manager::CatalogEntry {
                    id: manifest.metadata.id,
                    name: manifest.metadata.name,
                    description: manifest.metadata.description,
                    localizations: manifest.metadata.localizations,
                    version: manifest.metadata.version,
                    app_versions: manifest.metadata.app_versions,
                    protocol_version: manifest.metadata.protocol_version,
                    icon: manifest.metadata.icon,
                    icon_url: icon.display().to_string(),
                    downloads: Default::default(),
                    sha256: Some(bongocat_plugin_manager::sha256_of(&bytes)),
                    homepage: None,
                }],
            })
            .ok()?,
        )
        .ok()?;

        let store = tempfile::tempdir().ok()?;
        // The package is installed against the digest its catalog entry
        // advertises, which is the only way a package is installed at all.
        let mut manager =
            bongocat_plugin_manager::PluginManager::new(plugin_directory(store.path())).ok()?;
        manager
            .install(&bytes, &bongocat_plugin_manager::sha256_of(&bytes))
            .ok()?;
        let source = PluginSource::Development {
            catalog: catalog.path().join("catalog.json"),
            packages: catalog.path().to_owned(),
        };
        let (service, _handles) =
            PluginService::start_with_source(store.path(), 4, source, None).ok()?;
        Some((service, store))
    }

    #[test]
    fn a_window_reads_the_cards_the_service_owns() {
        let Some((service, _store)) = store_with_pomodoro() else {
            return;
        };
        let host = PluginHost::new(service.client());
        assert!(host.snapshot().loading);
        // Discovery is the service's own background job; reading before it
        // finishes would report the center as still loading.
        for _ in 0..200 {
            host.refresh();
            if !host.snapshot().loading {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let state = host.snapshot();
        assert!(!state.loading);
        assert!(host.take_failure().is_none());
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
        assert_eq!(
            host.take_failure(),
            Some(PluginFailureReport {
                failure: PluginFailure::Refresh,
                code: PluginCommandError::ServiceUnavailable
            })
        );
        assert!(!host.snapshot().loading);
    }

    #[test]
    fn an_explicit_refresh_failure_is_not_repeated_by_polling() {
        let host = PluginHost::default();
        host.begin(PluginInFlight::Refresh);
        host.rescan();
        assert_eq!(
            host.take_failure(),
            Some(PluginFailureReport {
                failure: PluginFailure::Refresh,
                code: PluginCommandError::ServiceUnavailable,
            })
        );
        host.refresh();
        assert!(host.take_failure().is_none());
    }

    #[test]
    fn a_failed_enable_reports_once_when_the_next_poll_shows_the_failed_card() {
        let (client, endpoint) = PluginServiceClient::bounded(4);
        let host = PluginHost::new(client);
        let mut card = sample_card();
        card.enabled = false;
        host.publish(PluginHubState {
            cards: vec![card.clone()],
            ..Default::default()
        });

        let responder = std::thread::spawn(move || {
            loop {
                match endpoint.try_recv().unwrap() {
                    Some(PluginCommand::SetEnabled { reply, .. }) => {
                        // Enabling starts the process, records the failure on the
                        // card, and returns an error without a snapshot.
                        reply.respond(Err(PluginCommandError::Operation)).unwrap();
                        break;
                    }
                    Some(_) => panic!("only the enable command was sent"),
                    None => std::thread::sleep(Duration::from_millis(1)),
                }
            }
            let mut failed = card;
            failed.failed = true;
            loop {
                match endpoint.try_recv().unwrap() {
                    Some(PluginCommand::ReadCenter { reply }) => {
                        reply.respond(plugin_center_snapshot(failed)).unwrap();
                        break;
                    }
                    Some(_) => panic!("the window polls after the failed command"),
                    None => std::thread::sleep(Duration::from_millis(1)),
                }
            }
        });

        host.begin(PluginInFlight::package(
            "com.example.card",
            PluginPackageOperation::Enable,
        ));
        host.set_enabled("com.example.card", true);
        assert_eq!(
            host.take_failure(),
            Some(PluginFailureReport {
                failure: PluginFailure::Enable {
                    plugin: "com.example.card".into(),
                    enabled: true,
                },
                code: PluginCommandError::Operation,
            })
        );
        host.refresh();
        responder.join().unwrap();
        assert_eq!(
            host.take_failure(),
            None,
            "the failed card the enable produced must not become a second load failure"
        );
    }

    #[test]
    fn a_plugin_command_failure_keeps_operation_context_and_stable_code() {
        let (client, endpoint) = PluginServiceClient::bounded(2);
        let host = PluginHost::new(client);
        host.begin(PluginInFlight::action("com.example.card", "start"));
        let responder = std::thread::spawn(move || {
            let reply = loop {
                match endpoint.try_recv().unwrap() {
                    Some(PluginCommand::Action { reply, .. }) => break reply,
                    Some(_) => panic!("the action command must reach the plugin service"),
                    None => std::thread::sleep(Duration::from_millis(1)),
                }
            };
            reply.respond(Err(PluginCommandError::NotRunning)).unwrap();
        });
        host.action("com.example.card", "start");
        responder.join().unwrap();
        assert_eq!(
            host.take_failure(),
            Some(PluginFailureReport {
                failure: PluginFailure::Action {
                    plugin: "com.example.card".into(),
                    action: "start".into(),
                },
                code: PluginCommandError::NotRunning,
            })
        );
        assert!(host.snapshot().in_flight.is_empty());
    }

    #[gpui_kit::test]
    fn plugin_failures_are_painted_as_global_notifications(cx: &mut TestAppContext) {
        let (view, visual) = crate::window::tests::settings_view(cx);
        view.update(visual, |view, _| {
            let host = PluginHost::default();
            host.refresh();
            view.set_plugin_host(host);
        });
        visual.update(|window, cx| window.render_frame(cx));
        visual.run_until_parked();
        assert!(
            visual.update(|window, _| window.try_find("notification").is_some()),
            "plugin failures must use the global notification layer"
        );
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
        assert_eq!(
            host.take_failure(),
            Some(PluginFailureReport {
                failure: PluginFailure::UpdateAll {
                    failed: vec!["plugin-1".into()]
                },
                code: PluginCommandError::Operation
            })
        );
        assert_eq!(host.take_failure(), None);
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
        assert_eq!(
            host.take_failure(),
            Some(PluginFailureReport {
                failure: PluginFailure::UpdateAll {
                    failed: vec![sample_card().id]
                },
                code: PluginCommandError::Operation
            })
        );
        assert_eq!(host.take_failure(), None);
    }

    /// A batch only ever carries installed plugins the catalog has a newer version
    /// for, and the page asks the same rule before it draws the control.
    #[test]
    fn a_plugin_the_catalog_only_offers_is_not_an_update() {
        let mut catalog_only = sample_card();
        catalog_only.version = None;
        let mut current = sample_card();
        current.id = "com.example.current".into();
        current.update_available = false;
        let host = PluginHost::default();
        host.publish(PluginHubState {
            cards: vec![catalog_only.clone(), current],
            ..Default::default()
        });
        assert!(!host.snapshot().has_updates());
        assert!(host.begin_update_all().is_empty());

        let mut stale = sample_card();
        stale.id = "com.example.stale".into();
        host.publish(PluginHubState {
            cards: vec![catalog_only, stale],
            ..Default::default()
        });
        assert!(host.snapshot().has_updates());
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
        assert_eq!(card.copy("zh-CN").0, "番茄时钟");
        assert_eq!(card.copy("zh-TW").0, "Pomodoro");
        service.shutdown().unwrap();
    }

    /// A started plugin is told the tag the host is showing when its process
    /// starts, which is what makes its own labels carry the language on screen.
    #[test]
    fn a_plugin_started_while_the_host_shows_chinese_publishes_chinese_labels() {
        let Some((service, _store)) = store_with_pomodoro() else {
            return;
        };
        let host = PluginHost::new(service.client());
        host.refresh();
        service.client().set_locale_blocking("zh-CN").unwrap();
        host.set_enabled(PLUGIN_ID, true);
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
        // Only Simplified Chinese and English are served; the plugin follows the
        // application's resolved language like the rest of the product.
        for locale in ["zh-TW", "fr-FR"] {
            service.client().set_locale_blocking(locale).unwrap();
            host.refresh();
            let state = host.snapshot();
            let card = &state.cards[0];
            assert_eq!(card.copy("zh-CN").0, "番茄时钟");
            assert_eq!(card.copy("en-US").0, "Pomodoro");
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
            localizations: BTreeMap::from([(
                "zh-CN".to_owned(),
                PluginCopy {
                    name: "一个插件".into(),
                    description: "由插件自己书写的描述".into(),
                },
            )]),
            icon_url: None,
            enabled: true,
            failed: false,
            errors: Vec::new(),
            interaction_failure: None,
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
                    icon: bongocat_plugin_sdk::ActionIcon::Play,
                    loading: false,
                },
                UiNode::Button {
                    id: "second".into(),
                    label: "Restart the session".into(),
                    icon: bongocat_plugin_sdk::ActionIcon::Restart,
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
                    icon: bongocat_plugin_sdk::ActionIcon::Volume,
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
    fn a_plugin_icon_keeps_a_uniform_square_in_every_card(cx: &mut TestAppContext) {
        let mut remote = sample_card();
        remote.id = "com.example.remote-icon".into();
        remote.icon_url = Some("https://example.com/pomodoro.png".into());
        let mut missing = sample_card();
        missing.id = "com.example.missing-icon".into();
        let (_, visual) = card_page(cx, vec![remote, missing], 800.);
        visual.update(|window, _| {
            for id in ["com.example.remote-icon", "com.example.missing-icon"] {
                let icon = window
                    .find(SharedString::from(format!("plugin-icon-{id}")))
                    .bounds();
                assert_eq!(icon.size.width, px(48.));
                assert_eq!(icon.size.height, px(48.));
            }
        });
    }

    #[gpui_kit::test]
    fn a_release_icon_is_fetched_through_the_gpui_http_client(cx: &mut TestAppContext) {
        struct PngHttpClient;

        impl http_client::HttpClient for PngHttpClient {
            fn user_agent(&self) -> Option<&http_client::http::HeaderValue> {
                None
            }

            fn proxy(&self) -> Option<&http_client::Url> {
                None
            }

            fn send(
                &self,
                request: http_client::Request<http_client::AsyncBody>,
            ) -> Pin<
                Box<
                    dyn Future<
                            Output = http_client::Result<
                                http_client::Response<http_client::AsyncBody>,
                            >,
                        > + Send
                        + 'static,
                >,
            > {
                assert_eq!(
                    request.uri().to_string(),
                    "https://example.com/pomodoro.png"
                );
                Box::pin(async {
                    Ok(http_client::Response::builder()
                        .status(http_client::StatusCode::OK)
                        .header("content-type", "image/png")
                        .body(http_client::AsyncBody::from(
                            include_bytes!("../../../../plugins/pomodoro/resources/icon.png")
                                .as_slice(),
                        ))
                        .unwrap())
                })
            }
        }

        cx.update(|app| app.set_http_client(Arc::new(PngHttpClient)));
        let mut card = sample_card();
        card.icon_url = Some("https://example.com/pomodoro.png".into());
        let (_, visual) = card_page(cx, vec![card], 800.);
        visual.run_until_parked();
        visual.update(|window, _| {
            let icon = window
                .find(SharedString::from("plugin-icon-com.example.card"))
                .bounds();
            assert_eq!(icon.size.width, px(48.));
            assert_eq!(icon.size.height, px(48.));
        });
    }

    #[gpui_kit::test]
    fn a_development_icon_is_read_as_a_file_not_as_an_embedded_asset(cx: &mut TestAppContext) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("plugins/pomodoro/resources/icon.png");
        assert!(path.is_file());
        let mut card = sample_card();
        card.icon_url = Some(path.display().to_string());
        let (_, visual) = card_page(cx, vec![card], 800.);
        visual.run_until_parked();
        visual.update(|window, _| {
            let icon = window
                .find(SharedString::from("plugin-icon-com.example.card"))
                .bounds();
            assert_eq!(icon.size.width, px(48.));
            assert_eq!(icon.size.height, px(48.));
        });
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
                icon: bongocat_plugin_sdk::ActionIcon::Restart,
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
            (ActionIcon::Play, IconName::Play),
            (ActionIcon::Pause, IconName::Pause),
            (ActionIcon::Restart, IconName::RefreshCw),
            (ActionIcon::Volume, IconName::AudioLines),
        ] {
            assert_eq!(super::super::plugins_page::action_icon(icon), expected);
            // The glyph must be artwork the bundled assets ship, or the button
            // would draw nothing on a machine without that font.
            assert!(
                gpui_kit::assets::IconNamed::path(expected).as_ref() != "",
                "{expected:?} resolves to bundled artwork"
            );
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

    /// Nothing to update is an absent control rather than a dead one.
    #[gpui_kit::test]
    fn a_center_with_nothing_to_update_hides_the_batch_button(cx: &mut TestAppContext) {
        let mut up_to_date = sample_card();
        up_to_date.update_available = false;
        let mut catalog_only = sample_card();
        catalog_only.id = "com.example.available".into();
        catalog_only.version = None;
        catalog_only.update_available = false;
        let (_, visual) = card_page(cx, vec![up_to_date, catalog_only], 800.);
        visual.update(|window, _| {
            assert!(
                window.try_find("plugin-refresh").is_some(),
                "the header keeps the button that still does something"
            );
            assert!(
                window.try_find("plugin-update-all").is_none(),
                "no installed plugin is behind the catalog, so nothing can be updated"
            );
        });
    }

    /// One plugin behind the catalog is enough to offer the batch: hiding the
    /// control must not hide the work the page can still do.
    #[gpui_kit::test]
    fn a_center_with_a_stale_plugin_keeps_the_batch_button(cx: &mut TestAppContext) {
        let mut up_to_date = sample_card();
        up_to_date.update_available = false;
        let mut stale = sample_card();
        stale.id = "com.example.stale".into();
        let (_, visual) = card_page(cx, vec![up_to_date, stale], 800.);
        visual.update(|window, _| {
            assert!(window.try_find("plugin-update-all").is_some());
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
            icon: bongocat_plugin_sdk::ActionIcon::Volume,
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
                    icon_url: card.icon_url,
                    enabled: card.enabled,
                    failed: card.failed,
                    version: card.version,
                    available_version: card.available_version,
                    update_available: card.update_available,
                    actions: card.actions,
                    settings: card.settings,
                    errors: card.errors,
                    interaction_failure: card.interaction_failure,
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
