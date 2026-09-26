//! What one row of the list can do, apart from renaming it.
//!
//! A row is selected, revealed on disk, and deleted. Those share the row's focus
//! and its pending-action state, which is what makes them one thing rather than
//! three — and the row action dispatcher at the end is what the page's controls
//! all route through, so what a row can do is decided in exactly one place.

use super::*;

impl SettingsView {
    /// Keep the per-card focus handles, the open edit and the open delete
    /// question matching the catalog.
    ///
    /// The catalog is re-projected on every snapshot, so this is also where a
    /// deleted model's handles are dropped, an edit whose model is no longer in
    /// the catalog is abandoned — the origin and the id together are the model's
    /// identity, so either origin's row keeps its own edit alive — and a delete
    /// question whose control the card would no longer draw is dropped.
    pub(super) fn sync_model_row_focus(
        &mut self,
        entries: &[SettingsModelEntry],
        active_model: Option<&SettingsModelKey>,
        commands_blocked: bool,
        cx: &mut Context<Self>,
    ) {
        if self
            .model_delete_confirmation
            .as_ref()
            .is_some_and(|model| {
                !model_delete_confirmation_is_valid(entries, active_model, commands_blocked, model)
            })
        {
            self.model_delete_confirmation = None;
        }
        if self.model_edit.as_ref().is_some_and(|draft| {
            !entries
                .iter()
                .any(|entry| entry.origin == draft.model.origin && entry.id == draft.model.id)
        }) {
            self.model_edit = None;
        }
        let keys = entries
            .iter()
            .map(|entry| ModelRowKey::new(entry.origin, &entry.id))
            .collect::<BTreeSet<_>>();
        self.model_row_focus.retain(|key, _| keys.contains(key));
        for (index, entry) in entries.iter().enumerate() {
            let key = ModelRowKey::new(entry.origin, &entry.id);
            let actions = model_row_actions(entry, active_model, commands_blocked);
            let offset = isize::try_from(index)
                .unwrap_or(isize::MAX / 5)
                .saturating_mul(5);
            let tab_index = 40_isize.saturating_add(offset);
            let action_tabs = model_row_action_tab_indices(tab_index);
            let focus = self
                .model_row_focus
                .entry(key)
                .or_insert_with(|| ModelRowFocus {
                    activate: cx.focus_handle(),
                    open_location: cx.focus_handle(),
                    edit: cx.focus_handle(),
                    delete: cx.focus_handle(),
                });
            focus.activate = focus
                .activate
                .clone()
                .tab_index(action_tabs.activate)
                .tab_stop(actions.can_activate);
            focus.open_location = focus
                .open_location
                .clone()
                .tab_index(action_tabs.open_location)
                .tab_stop(actions.can_open_location);
            focus.edit = focus
                .edit
                .clone()
                .tab_index(action_tabs.edit)
                .tab_stop(actions.can_edit);
            focus.delete = focus
                .delete
                .clone()
                .tab_index(action_tabs.delete)
                .tab_stop(actions.can_delete);
        }
    }
}

impl SettingsView {
    pub(super) fn select_model(&mut self, model: SettingsModelKey, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        self.pending = Some(PendingOperation::ModelSelection);
        self.model_delete_confirmation = None;
        self.model_edit = None;
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.select_model(expected_config_revision, model).await;
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
                    Ok(_) => {}
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl SettingsView {
    /// Open the model's own folder in the system file manager.
    ///
    /// This is the escape hatch for everything the page deliberately does not
    /// do: replacing artwork by hand, inspecting what a package contains, or
    /// removing a model outright.
    pub(super) fn open_model_location(&mut self, model: SettingsModelKey, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        self.pending = Some(PendingOperation::ModelLocation);
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.open_model_location(model).await;
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
                    Ok(_) => {}
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl SettingsView {
    /// Hold the models the run just installed back until their covers are done.
    ///
    /// A successful import always queues a cover capture, so the run's own
    /// cards become visible and its success notification is enabled when that
    /// capture reports back rather than the moment the package lands. Nothing
    /// is published when the run installed nothing, which is also how a
    /// rejected snapshot revision is handled: the newer snapshot republishes
    /// the catalog with the card already in it, so the notification can be
    /// enabled immediately.
    pub(super) fn begin_model_reveal(&mut self) -> bool {
        let baseline = std::mem::take(&mut self.model_import.baseline_models);
        let installed = self
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .model_catalog
                    .entries
                    .iter()
                    .filter(|entry| entry.origin == SettingsModelOrigin::Imported)
                    .map(|entry| ModelRowKey::new(entry.origin, &entry.id))
                    .filter(|key| !baseline.contains(key))
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let mut pending = installed;
        for key in std::mem::take(&mut self.completed_model_cover_captures) {
            pending.remove(&key);
        }
        let complete = pending.is_empty();
        if complete {
            self.model_import.reset();
        } else {
            self.pending_model_reveal = pending;
            self.model_import.state = ModelImportState::Capturing;
        }
        complete
    }
}

impl SettingsView {
    /// Drop the cached cover image of a model whose cover just changed.
    ///
    /// The replacement keeps the package's own file name, so the image cache is
    /// keyed by a path that still resolves to the *old* bytes until it is
    /// dropped.
    pub(crate) fn invalidate_model_cover(&self, model: &SettingsModelKey, cx: &mut App) {
        let cover = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .model_catalog
                .entries
                .iter()
                .find(|entry| entry.origin == model.origin && entry.id == model.id)
                .and_then(|entry| entry.cover.clone())
        });
        if let Some(cover) = cover {
            ImageSource::from(cover).remove_asset(cx);
        }
    }
}

impl SettingsView {
    /// Open the delete confirmation for `model`.
    ///
    /// The confirmation is a surface the card's delete control owns, so this
    /// records which model it belongs to and the card draws itself open. It does
    /// not delete anything: that is [`SettingsView::delete_model`], which only
    /// the confirmation's accept button reaches.
    pub(super) fn request_model_delete(&mut self, model: SettingsModelKey, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        self.model_delete_confirmation = Some(model);
        cx.notify();
    }
}

impl SettingsView {
    /// Close the delete confirmation for `model`.
    ///
    /// Guarded on the model so a surface that reports its own close after the
    /// confirmation already moved on — a click outside during a delete, say —
    /// cannot cancel somebody else's.
    pub(super) fn cancel_model_delete(&mut self, model: &SettingsModelKey, cx: &mut Context<Self>) {
        if self.model_delete_confirmation.as_ref() == Some(model) {
            self.model_delete_confirmation = None;
            cx.notify();
        }
    }
}

impl SettingsView {
    pub(super) fn delete_model(&mut self, model: SettingsModelKey, cx: &mut Context<Self>) {
        self.pending = Some(PendingOperation::ModelDeletion);
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.delete_model(model).await;
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
                        view.model_delete_confirmation = None;
                    }
                    Ok(_) => view.model_delete_confirmation = None,
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl SettingsView {
    pub(super) fn run_model_row_action(
        &mut self,
        action: ModelRowAction,
        model: SettingsModelKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .model_catalog
                .entries
                .iter()
                .find(|entry| entry.origin == model.origin && entry.id == model.id)
        }) else {
            return;
        };
        let actions = model_row_actions(
            entry,
            self.snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.active_model.as_ref()),
            self.pending.is_some()
                || self.model_import.is_running()
                || self.model_import.is_source_surface_open(),
        );
        let Some(focus) = self
            .model_row_focus
            .get(&ModelRowKey::new(model.origin, &model.id))
            .cloned()
        else {
            return;
        };
        match action {
            ModelRowAction::Activate if actions.can_activate => {
                window.focus(&focus.activate, cx);
                self.select_model(model, cx);
            }
            ModelRowAction::OpenLocation if actions.can_open_location => {
                window.focus(&focus.open_location, cx);
                self.open_model_location(model, cx);
            }
            ModelRowAction::Edit if actions.can_edit => {
                window.focus(&focus.edit, cx);
                self.begin_model_edit(model, window, cx);
            }
            ModelRowAction::Delete if actions.can_delete => {
                self.delete_model(model, cx);
            }
            _ => {}
        }
    }
}
