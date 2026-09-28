//! What the update window does, as opposed to how it looks.
//!
//! Every phase offers different actions, so the view's own methods are mostly
//! the checks and installs: the window's job is to make sure the action a phase
//! offers is the one that phase can actually carry out.

use super::*;

impl UpdateView {
    pub(crate) fn new(
        client: UpdateClient,
        settings_client: SettingsClient,
        language: SettingsLanguage,
        appearance_theme: SettingsTheme,
        start: UpdateWindowStart,
        cx: &mut Context<Self>,
    ) -> Self {
        let snapshot = client.snapshot();
        let mut view = Self {
            client,
            settings_client,
            language,
            appearance_theme,
            observed_revision: Some(snapshot.revision),
            pending_check: PendingCheck::default(),
            snapshot,
            applied_theme: None,
            content_height: Rc::new(ContentHeight::default()),
            primary_focus: cx.focus_handle().tab_index(1).tab_stop(true),
            close_focus: cx.focus_handle().tab_index(2).tab_stop(true),
            link_focus: cx.focus_handle().tab_index(3).tab_stop(true),
        };
        // The request is made here, while the window is still being built. A window
        // that asked for its check after the first frame had been painted is a window
        // whose first frame shows the answer to the previous question.
        if start == UpdateWindowStart::Check {
            view.check(cx);
        }
        view
    }

    /// The state currently rendered.
    pub fn snapshot(&self) -> &UpdateSnapshot {
        &self.snapshot
    }

    pub fn phase(&self) -> &UpdatePhase {
        &self.snapshot.phase
    }

    pub fn language(&self) -> SettingsLanguage {
        self.language
    }

    /// Start a check from outside the window (the system menu and the About page), or
    /// from the window's own action.
    ///
    /// Asking does not wait for the worker, and it does not have to: the view renders
    /// the check it asked for until the worker publishes a newer revision, so the
    /// progress is there from the next frame rather than a poll later.
    pub fn check(&mut self, cx: &mut Context<Self>) {
        if !asks_for_a_new_check(&self.snapshot.phase, self.pending_check.is_pending()) {
            return;
        }
        if self.client.request_check().is_err() {
            // The worker cannot take the command, so there is no check to show, and a
            // check nobody is running must not be rendered as one that is.
            return;
        }
        // The revision this request is keyed on is read from the shared state now, not
        // reused from the snapshot this view last polled. The worker may have published
        // something in between, and a request keyed on a revision it has already moved
        // past would be answered by that stale result instead of by its own.
        let mut snapshot = self.client.snapshot();
        self.pending_check.begin(&mut snapshot);
        self.observed_revision = Some(snapshot.revision);
        self.snapshot = snapshot;
        cx.notify();
    }

    pub(crate) fn install(&mut self, cx: &mut Context<Self>) {
        if !self.snapshot.phase.offers_install() {
            return;
        }
        let _ = self.client.request_install();
        cx.notify();
    }

    pub(crate) fn restart(&mut self, cx: &mut Context<Self>) {
        let _ = self.client.request_restart();
        cx.notify();
    }

    pub(crate) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.remove_window();
        cx.notify();
    }

    pub(crate) fn start_polling(&self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(UPDATE_STATE_POLL_INTERVAL).await;
                if this.update(cx, |view, cx| view.poll_state(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Keeps the window on the settings the product is configured with.
    ///
    /// The window opens on the values the application cached, and this poll is what
    /// makes a change made while it is open take effect: the display language and the
    /// appearance, which the update window used to read as a constant.
    ///
    /// The revision is probed before the snapshot is read. This window only cares
    /// about two fields, and building a snapshot walks the model store on disk, so
    /// asking for a whole snapshot every second spent a directory scan per second on
    /// a window that almost never had anything new to show.
    pub(crate) fn start_settings_polling(&self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let mut last_revision: Option<u64> = None;
            loop {
                executor.timer(SETTINGS_POLL_INTERVAL).await;
                let client = match this.update(cx, |view, _| view.settings_client.clone()) {
                    Ok(client) => client,
                    Err(_) => break,
                };
                let Ok(revision) = client.read_snapshot_revision().await else {
                    continue;
                };
                if last_revision == Some(revision) {
                    continue;
                }
                let Ok(snapshot) = client.read_snapshot().await else {
                    continue;
                };
                if this
                    .update(cx, |view, cx| {
                        let language_changed = view.language != snapshot.resolved_language;
                        let theme_changed = view.appearance_theme != snapshot.appearance_theme;
                        if language_changed || theme_changed {
                            view.language = snapshot.resolved_language;
                            view.appearance_theme = snapshot.appearance_theme;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
                last_revision = Some(snapshot.revision);
            }
        })
        .detach();
    }

    pub(crate) fn poll_state(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.client.snapshot();
        if self.pending_check.is_pending() {
            // A check this view asked for is the newer fact, so the result of the
            // previous one is not adopted until the worker answers. The worker has not
            // answered while the revision is the one it was asked from.
            if !self.pending_check.answers(snapshot.revision) {
                return;
            }
            self.pending_check.settle();
        } else if self.observed_revision == Some(snapshot.revision) {
            return;
        }
        self.observed_revision = Some(snapshot.revision);
        self.snapshot = snapshot;
        // A restart-requiring install is *not* driven from here: the window can be
        // closed, and then nothing would replace the process. The application watches
        // the published phase itself, and the restart action below only shortens the
        // wait for a user who is looking at it.
        cx.notify();
    }

    pub(crate) fn sync_component_theme(
        &mut self,
        theme: SettingsTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.applied_theme == Some(theme) {
            return;
        }
        apply_component_theme(theme, window, cx);
        self.applied_theme = Some(theme);
    }
}
