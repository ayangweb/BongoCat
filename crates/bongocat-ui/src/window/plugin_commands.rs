//! The plugin center's commands.
//!
//! Five actions, all immediate and all revision-free: a plugin install changes no
//! configuration, and a plugin switch is a preference the service owns. They do not
//! go through the window's pending/debounce machinery, which exists for controls
//! that send a command per keystroke — a plugin button is pressed once, and a plugin's
//! own settings go the whole way at once rather than one field per press.
//!
//! Each one marks the page busy through [`PendingOperation::PluginOperation`]
//! rather than a separate flag, so a second press while an install is in flight is
//! refused the same way every other in-flight operation is.

use super::model_actions::file_picker_error;
use super::*;
use bongocat_platform::pick_audio_file;

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

    /// Press one of the controls a plugin offered the host to draw.
    ///
    /// Goes through [`Self::run_plugin_operation`] rather than the fire-and-forget a
    /// field change uses, and the difference is the answer: a press is what makes a
    /// timer say "Pause" instead of "Start", and the snapshot that comes back is what
    /// tells this page the button's own label changed. A fire-and-forget here would
    /// leave the card offering a control whose label no longer says what it will do —
    /// the one wrong answer this whole mechanism exists to prevent.
    ///
    /// The id is sent as it arrived rather than checked here. The host checks it against
    /// what the plugin is *currently* offering, which is the only side that knows: a
    /// button on this page was drawn from a snapshot, and a plugin may have withdrawn
    /// the control since, so a check here would answer from a value already stale.
    pub(super) fn press_plugin_action(
        &mut self,
        plugin: String,
        action: String,
        cx: &mut Context<Self>,
    ) {
        self.run_plugin_operation(
            move |client| Box::pin(async move { client.press_plugin_action(plugin, action).await }),
            cx,
        );
    }

    /// Whether one plugin's own settings are expanded under its card.
    pub(super) fn plugin_settings_are_open(&self, id: &str) -> bool {
        self.plugin_settings
            .as_ref()
            .is_some_and(|draft| draft.plugin == id)
    }

    /// The labels this plugin's open form shows, in order.
    ///
    /// Read by the page's own tests rather than by painting them: the decision worth
    /// asserting is *which rows the form offers and in what order*, and a rendered frame
    /// cannot say that as plainly as a list can.
    #[cfg(test)]
    pub(super) fn plugin_settings_row_labels(&self, id: &str) -> Vec<String> {
        let Some(entry) = self.plugin_entry(id) else {
            return Vec::new();
        };
        if !self.plugin_settings_are_open(id) {
            return Vec::new();
        }
        plugin_settings::row_labels(entry, self.plugin_language())
    }

    /// The language the user reads, as the page's own copy is resolved against it.
    #[cfg(test)]
    pub(super) fn plugin_language(&self) -> SettingsLanguage {
        self.snapshot
            .as_ref()
            .map_or(SettingsLanguage::default(), |snapshot| {
                snapshot.resolved_language
            })
    }

    /// Whether the page is waiting for a stopped plugin to start so it can open its
    /// settings form.
    ///
    /// Read by the card's settings button, which is the one control whose meaning
    /// differs between a running plugin and a stopped one — so it says so rather than
    /// looking the same either way, and marks itself unpressable rather than queueing a
    /// second enable behind the first.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "read by the card's own tests, which pin that a waiting card says so"
        )
    )]
    pub(super) fn plugin_awaiting_settings(&self, id: &str) -> bool {
        self.plugin_settings_pending.as_deref() == Some(id)
    }

    /// Expand or collapse one plugin's own settings.
    ///
    /// Under the card rather than in a dialog, and for the reason a card is a card: a
    /// dialog has to be opened, sized, closed and reopened, and a settings form that
    /// appears where the user pressed the button is a form they can see the rest of
    /// the page beside. The rows come from the schema the *running* plugin declared,
    /// so a plugin that improved its settings in a later version is configured against
    /// the version that is actually running.
    ///
    /// **A plugin that is switched off is turned on rather than refused.** The form is
    /// the running process's to declare, so there is nothing to open — and the old
    /// answer, which was to do nothing at all, is what left a card with a delete button
    /// and no way to configure anything. Enabling is the only action that can make the
    /// form exist, it is what pressing a settings button is asking for, and it is
    /// reversible with the switch the card is showing. The panel opens when the plugin's
    /// handshake arrives rather than here, because the handshake is the moment the schema
    /// exists — opening a form now would open an empty one.
    pub(super) fn toggle_plugin_settings(&mut self, plugin: String, cx: &mut Context<Self>) {
        if self.plugin_settings_are_open(&plugin) {
            // Flushed before the draft goes, because the draft is where the send reads its
            // values from. Collapsing the form is not discarding what the user typed into it.
            self.send_pending_plugin_settings(cx);
            self.plugin_settings = None;
            cx.notify();
            return;
        }
        let Some(entry) = self.plugin_entry(&plugin).cloned() else {
            // A press for a plugin the page has stopped listing is a press that
            // arrived after an uninstall; ignoring it is the same answer the host
            // gives for a press on a panel that is gone.
            return;
        };
        if entry.settings_available {
            self.plugin_settings = Some(PluginSettingsDraft {
                plugin,
                values: entry.values.clone(),
            });
            cx.notify();
            return;
        }
        if entry.enabled {
            // Enabled and still no form: the process is running and declared no
            // settings, which is a plugin with nothing to configure. There is nothing
            // to open and nothing to say — an empty panel would be a heading with no
            // fields under it.
            return;
        }
        self.open_settings_by_enabling(plugin, cx);
    }

    /// Turn a plugin on so its settings form becomes something that can be opened.
    ///
    /// The request is marked before the command goes out, because the panel cannot open
    /// until the plugin's handshake answers and the page has to remember it was asked.
    /// [`Self::refresh_after_plugin_snapshot`] is what opens it.
    fn open_settings_by_enabling(&mut self, plugin: String, cx: &mut Context<Self>) {
        if self.plugin_operation_in_flight() {
            return;
        }
        self.plugin_settings_pending = Some(plugin.clone());
        self.run_plugin_operation(
            move |client| Box::pin(async move { client.set_plugin_enabled(plugin, true).await }),
            cx,
        );
    }

    /// Open a settings form that was waiting on a plugin to start, now that it can be.
    ///
    /// Called from the snapshot poll rather than from the enable command's own reply,
    /// because the answer to "did the plugin start" is not in that reply — it is in
    /// whatever the plugin's own handshake produces afterwards, which may be a moment
    /// later or never. Polling is what makes both cases work without a second command
    /// whose only job would be to ask a question the snapshot already answers.
    pub(super) fn refresh_after_plugin_snapshot(&mut self) {
        let Some(plugin) = self.plugin_settings_pending.clone() else {
            return;
        };
        // The values are taken before the request is cleared, because the draft owns a
        // copy and clearing the request is what lets a later press start over. Cloning
        // here rather than holding the borrow is the cost of a page that owns its own
        // state, and it is the same copy every other draft in this file makes.
        let Some(values) = self
            .plugin_entry(&plugin)
            .filter(|entry| entry.settings_available)
            .map(|entry| entry.values.clone())
        else {
            // Either the plugin has not answered yet — the ordinary case, and the reason
            // this runs on every snapshot — or it went away while it was starting, which
            // is the one case worth clearing: waiting for a handshake that is never
            // coming would leave the card claiming to be opening a form forever.
            if self.plugin_entry(&plugin).is_none() {
                self.plugin_settings_pending = None;
            }
            return;
        };
        self.plugin_settings_pending = None;
        self.plugin_settings = Some(PluginSettingsDraft { plugin, values });
    }

    /// Set one of a plugin's own settings.
    ///
    /// The draft is kept here and sent as a whole document, because the plugin writes
    /// its own file atomically and a patch would have to be merged by a side that does
    /// not own the file. The value is fitted to the field the plugin declared before
    /// it goes out, so the window cannot put a number where a menu belongs.
    /// Open the platform's own file dialog for one plugin's file field.
    ///
    /// Refused while another dialog is up, because the platform has one modal panel at a
    /// time and a second request would open a panel behind a panel. Refused for a form
    /// that is not open, and for a field that is not in it, because a dialog that came
    /// back with a file and nowhere to put it would have to be thrown away — which is
    /// exactly what the user would experience as a dialog that ate their click.
    ///
    /// Whether the request was taken. A refusal is the one outcome here a caller has to be
    /// able to *see*, because a button that does nothing and a button that was refused are
    /// the same thing to a user.
    ///
    /// The gate and nothing else — it does not open a panel. Opening one is
    /// [`Self::open_plugin_file_picker`], and the split is not tidiness: a machine with no
    /// file panel answers the *open* with an error, so a test that could only reach the
    /// behaviour by opening one would be a test of whether the machine has a desktop. What
    /// is worth checking here is which requests are taken, and that is a question about
    /// the form rather than about the platform.
    #[must_use]
    pub(super) fn choose_plugin_file(
        &mut self,
        plugin: &str,
        key: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.pending.is_some() || self.plugin_file_picking.is_some() {
            return false;
        }
        let belongs_to_open_form = self
            .plugin_settings
            .as_ref()
            .is_some_and(|draft| draft.plugin == plugin && draft.values.contains_key(key));
        if !belongs_to_open_form {
            return false;
        }
        self.plugin_file_picking = Some((plugin.to_string(), key.to_string()));
        cx.notify();
        true
    }

    /// Open the platform's file panel for a field that has already been accepted.
    ///
    /// The extensions come from the plugin's own schema rather than from here, so what the
    /// dialog offers is the plugin's declaration and this is only where it becomes a
    /// filter. The plugin and key are *not* taken again: the accepted request is already
    /// recorded in [`Self::plugin_file_picking`], and a second copy of it would be a
    /// second thing that could disagree about which field is waiting.
    pub(super) fn open_plugin_file_picker(&mut self, accept: Vec<String>, cx: &mut Context<Self>) {
        let (sender, receiver) = async_channel::bounded(1);
        let picking = move |result| {
            let _ = sender.try_send(result);
        };
        if let Err(error) = pick_audio_file(&accept, picking) {
            self.apply_plugin_file_result(Err(error), cx);
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = receiver
                .recv()
                .await
                .unwrap_or(Err(FilePickerError::BackendUnavailable));
            let _ = this.update(cx, |view, cx| {
                view.apply_plugin_file_result(result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Apply what the file dialog answered, for the field that asked.
    ///
    /// Split from the press so a test can say what the dialog returned without opening
    /// one — the same split the model cover picker has, and for the same reason: the
    /// interesting behaviour is what happens to the *value*, and a test that had to open a
    /// real panel to reach it would be a test of the operating system.
    pub(super) fn apply_plugin_file_result(
        &mut self,
        result: Result<FilePickerOutcome, FilePickerError>,
        cx: &mut Context<Self>,
    ) {
        // Taken first, so a second application is a no-op rather than a second write, and
        // so a result that arrives after the form was closed has nowhere to go.
        let Some((plugin, key)) = self.plugin_file_picking.take() else {
            return;
        };
        match result {
            Ok(FilePickerOutcome::Selected(path)) => {
                let path = path.to_string_lossy().into_owned();
                let Some(draft) = self.plugin_settings.as_ref() else {
                    return;
                };
                // The form may have closed while the dialog was up, in which case the
                // value goes nowhere rather than into the next plugin's settings.
                if draft.plugin != plugin || !draft.values.contains_key(&key) {
                    return;
                }
                self.set_plugin_field(&plugin, &key, SettingsFieldValue::Text(path), cx);
            }
            // A cancel is not a failure and gets no message: the user closed the panel
            // because they read what was already set and were happy with it.
            Ok(FilePickerOutcome::Cancelled) => {}
            Err(error) => self.pending_notification = Some(file_picker_error(error)),
        }
    }

    pub(super) fn set_plugin_field(
        &mut self,
        plugin: &str,
        key: &str,
        value: SettingsFieldValue,
        cx: &mut Context<Self>,
    ) {
        // The field is read first and the draft second, because the two live in
        // different places: a field belongs to the snapshot and a value to the draft,
        // and borrowing both at once would be the borrow checker telling the truth
        // about a design that has one too many owners.
        let Some(kind) = self
            .plugin_entry(plugin)
            .and_then(|entry| entry.fields.iter().find(|field| field.key == key))
            .map(|field| field.kind.clone())
        else {
            // A key this build's schema does not name is a control the window drew
            // from a stale snapshot; the next snapshot will not draw it.
            return;
        };
        if !value.fits(&kind) {
            return;
        }
        let Some(draft) = self
            .plugin_settings
            .as_mut()
            .filter(|draft| draft.plugin == plugin)
        else {
            return;
        };
        draft.values.insert(key.to_string(), value);
        let plugin = draft.plugin.clone();
        let values = draft.values.clone();
        // The first change of a burst goes out at once and the rest wait, which is the
        // window's rule for every bounded setting and is the right one here too: a switch
        // the user taps once should take effect when they tap it, and a number they are
        // still dragging should not write the plugin's file once per step.
        match self
            .plugin_settings_debouncer
            .observe(values, Instant::now())
        {
            Some(sending) => {
                self.plugin_settings_pending_send = None;
                self.send_plugin_config(&plugin, sending, cx);
            }
            None => {
                self.plugin_settings_pending_send = Some(plugin);
                self.schedule_plugin_settings_send(cx);
            }
        }
    }

    /// Send one plugin's settings once the user has stopped changing them.
    ///
    /// The whole document goes out rather than the one field that moved, because the file
    /// belongs to the plugin and the plugin writes it whole — so a number being dragged or a
    /// text field being typed into is one document per step, each of them crossing a process
    /// boundary and each of them making the plugin write its own file.
    ///
    /// The generation is bumped **only when a timer is started**. Bumping it on every change
    /// would leave the timer that is already running stale the moment a second change
    /// arrived: it would find a generation that was not its own, decline to send, and
    /// nothing would replace it. That is the whole of a coalescer that coalesces the sends
    /// and then loses the last one.
    pub(super) fn schedule_plugin_settings_send(&mut self, cx: &mut Context<Self>) {
        self.plugin_settings_send_generation =
            self.plugin_settings_send_generation.saturating_add(1);
        let generation = self.plugin_settings_send_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(crate::SETTINGS_PATCH_DEBOUNCE).await;
            let _ = this.update(cx, |view, cx| {
                if view.plugin_settings_send_generation != generation {
                    return;
                }
                view.send_pending_plugin_settings(cx);
            });
        })
        .detach();
    }

    /// Send whatever the coalescer is holding, if it is still holding it.
    ///
    /// Separate from [`Self::schedule_plugin_settings_send`] so that closing the window can
    /// flush the same way it does for every other setting: a value the user typed and then
    /// closed the window over must still reach the plugin, and "the timer had not fired yet"
    /// is not a reason to lose it.
    pub(super) fn send_pending_plugin_settings(&mut self, cx: &mut Context<Self>) {
        let Some(values) = self.plugin_settings_debouncer.flush(Instant::now()) else {
            return;
        };
        let Some(plugin) = self.plugin_settings_pending_send.take() else {
            return;
        };
        self.send_plugin_config(&plugin, values, cx);
    }

    /// Hand the plugin its whole settings document.
    ///
    /// The reply is ignored on purpose — and now it is *only* whether the command was
    /// accepted, because the service does not build a settings snapshot to answer it: the
    /// plugin writes its own file, the form is drawing the draft this view already holds,
    /// and what the card says about this plugin next is on the snapshot the window's own
    /// revision poll fetches. So a value that changed costs one command rather than one
    /// command and a walk of the model store.
    ///
    /// Not gated on the page's pending operation, unlike an install or a press. Two reasons
    /// and they are the reason this is a fire-and-forget at all: a settings change is a
    /// change to the *user's* document rather than to the product's, so the next one is not
    /// stale the way a second press after an install is; and each send carries the whole
    /// document, so a send that arrives after a newer one is harmless rather than a revision
    /// to be resolved. The one thing that is gated is *how often* it is sent — see
    /// [`Self::schedule_plugin_settings_send`].
    fn send_plugin_config(
        &mut self,
        plugin: &str,
        values: BTreeMap<String, SettingsFieldValue>,
        cx: &mut Context<Self>,
    ) {
        let plugin = plugin.to_string();
        let client = self.client.clone();
        cx.spawn(async move |_this, _cx| {
            let _ = client.set_plugin_config(plugin, values).await;
        })
        .detach();
    }

    /// Move one plugin's panel to another corner of the model window.
    ///
    /// A press through the same operation gate as everything else on this page, and
    /// fire-and-forget like a configuration change rather than a plugin action: the panel
    /// moves because the host applied it, and the position the plugin *actually* got is on
    /// the next snapshot — which is the answer, because another plugin may already hold the
    /// corner that was asked for.
    pub(super) fn set_plugin_position(
        &mut self,
        plugin: &str,
        position: &str,
        cx: &mut Context<Self>,
    ) {
        let plugin = plugin.to_string();
        let position = position.to_string();
        let client = self.client.clone();
        cx.spawn(async move |_this, _cx| {
            let _ = client.set_plugin_position(plugin, position).await;
        })
        .detach();
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
