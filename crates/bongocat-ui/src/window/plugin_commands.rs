//! The plugin center's commands.
//!
//! Four actions, all immediate and all revision-free: a plugin install changes no
//! configuration, and a plugin switch is a preference the service owns. They do not
//! go through the window's pending/debounce machinery, which exists for controls
//! that send a command per keystroke — a plugin button is pressed once.
//!
//! Each one marks the page busy through [`PendingOperation::PluginOperation`]
//! rather than a separate flag, so a second press while an install is in flight is
//! refused the same way every other in-flight operation is.

use super::*;

impl SettingsView {
    /// Re-read the catalog from whichever source this build uses.
    pub(super) fn refresh_plugin_catalog(&mut self, cx: &mut Context<Self>) {
        if self.plugin_operation_in_flight() {
            return;
        }
        self.pending = Some(PendingOperation::PluginOperation);
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let sent = client.refresh_plugin_catalog().await.is_ok();
            let _ = this.update(cx, |view, cx| {
                if sent {
                    view.pending = None;
                } else {
                    view.pending_notification =
                        Some(SettingsError::new(SettingsErrorCode::ServiceUnavailable));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether a plugin's panel is currently shown on the model window.
    ///
    /// `false` for a plugin the page has never heard of, which is the same answer
    /// the host would give: a press for a plugin that is no longer listed is
    /// ignored rather than turning something on.
    pub(super) fn plugin_is_enabled(&self, id: &str) -> bool {
        self.plugin_entry(id).is_some_and(|entry| entry.enabled)
    }

    /// Whether the model window still has room for one more panel.
    pub(super) fn plugin_switch_is_live(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.plugins.active < snapshot.plugins.maximum_active)
    }

    fn plugin_entry(&self, id: &str) -> Option<&SettingsPluginEntry> {
        self.snapshot
            .as_ref()?
            .plugins
            .entries
            .iter()
            .find(|entry| entry.id == id)
    }

    pub(super) fn install_plugin(&mut self, plugin: String, cx: &mut Context<Self>) {
        self.run_plugin_operation(
            move |client| Box::pin(async move { client.install_plugin(plugin).await }),
            cx,
        );
    }

    pub(super) fn uninstall_plugin(&mut self, plugin: String, cx: &mut Context<Self>) {
        self.run_plugin_operation(
            move |client| Box::pin(async move { client.uninstall_plugin(plugin).await }),
            cx,
        );
    }

    /// Turn a plugin's panel on the model window on or off.
    ///
    /// The switch already shows the state the snapshot has, and the snapshot is the
    /// only truth about it, so this reads the current value rather than being handed
    /// the one the press was aimed at. A refused press therefore needs no undo: the
    /// next snapshot puts the switch back where the host actually is.
    pub(super) fn toggle_plugin_enabled(&mut self, plugin: String, cx: &mut Context<Self>) {
        if self.plugin_operation_in_flight() {
            return;
        }
        let Some(entry) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .plugins
                .entries
                .iter()
                .find(|entry| entry.id == plugin)
        }) else {
            return;
        };
        let enabled = !entry.enabled;
        self.run_plugin_operation(
            move |client| Box::pin(async move { client.set_plugin_enabled(plugin, enabled).await }),
            cx,
        );
    }

    /// Whether another settings change is already in flight.
    ///
    /// The same gate every other page uses, deliberately: a plugin download is slow,
    /// and a second press during it would queue behind a reply nobody is waiting
    /// for. One plugin operation at a time is the whole rule.
    fn plugin_operation_in_flight(&self) -> bool {
        self.pending.is_some()
    }

    fn run_plugin_operation(
        &mut self,
        operation: impl FnOnce(
            SettingsClient,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<SettingsSnapshot, SettingsError>> + Send>,
        > + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() {
            return;
        }
        self.pending = Some(PendingOperation::PluginOperation);
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = operation(client).await;
            let _ = this.update(cx, |view, cx| {
                view.pending = None;
                match result {
                    Ok(snapshot)
                        if view
                            .snapshot
                            .as_ref()
                            .is_none_or(|current| snapshot.revision >= current.revision) =>
                    {
                        view.snapshot = Some(snapshot);
                    }
                    // An older snapshot is dropped rather than applied: the page
                    // already shows the newer one, and the worker's own revision
                    // will move the snapshot forward again on the next poll.
                    Ok(_) => {}
                    // A refused command is a notification as well as a row: the row
                    // goes back to the host's state, and the notification says why.
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
