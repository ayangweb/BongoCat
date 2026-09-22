use super::*;

/// Map any dialog failure to the one code the page reports.
///
/// The model folder picker and the cover picker share their result vocabulary
/// because their failures are properties of opening a native dialog, not of what
/// was being chosen; the page therefore reports them the same way too.
fn model_source_picker_error(_error: ModelSourcePickerError) -> SettingsError {
    SettingsError::new(SettingsErrorCode::ModelSourcePickerUnavailable)
}

impl SettingsView {
    /// Open the native picker for the folder to import.
    ///
    /// One dialog at a time is the whole flow: the folder the user picks is
    /// revalidated by the platform layer, and a valid one starts the import
    /// directly — choosing the folder *is* the decision.
    pub(super) fn choose_model_source(&mut self, cx: &mut Context<Self>) {
        // The card stays drawn as interactive while a command is in flight —
        // `pending` never feeds a visual gate (ADR-0053) — so the refusal of a
        // second command lives here rather than in the paint.
        if self.pending.is_some()
            || self.model_import.is_running()
            || self.model_import.is_picker_open()
        {
            return;
        }
        self.model_import.state = ModelImportState::Picking;
        cx.notify();

        let (sender, receiver) = async_channel::bounded(1);
        let picking = move |result| {
            let _ = sender.try_send(result);
        };
        if let Err(error) = pick_model_folder(picking) {
            let _ = self.apply_model_source_result(Err(error));
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = receiver
                .recv()
                .await
                .unwrap_or(Err(ModelSourcePickerError::BackendUnavailable));
            let _ = this.update(cx, |view, cx| {
                // Choosing is the whole decision: a selected folder starts the
                // import immediately, so there is no second button to press.
                if view.apply_model_source_result(result) {
                    view.start_model_import(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Apply a folder dialog outcome to the import draft.
    ///
    /// Returns whether the outcome left a source ready to import, which is the
    /// signal the caller uses to start the run. Cancelling and a failed dialog
    /// both return the card to its prompt: an error keeps nothing, because the
    /// folder the user was choosing is the thing the error is about, and the
    /// failure is reported as a notification rather than as inline text, so the
    /// page reports every error one way.
    ///
    /// The selected folder is not classified here. What it contains is what the
    /// store detects from the bytes, and the title the run starts with is derived
    /// from the folder's name.
    pub(super) fn apply_model_source_result(
        &mut self,
        result: Result<ModelSourcePickerOutcome, ModelSourcePickerError>,
    ) -> bool {
        match result {
            Ok(ModelSourcePickerOutcome::Selected(source_root)) => {
                self.model_import.title = suggested_model_title(&source_root);
                self.model_import.source_root = Some(source_root);
                self.model_import.state = ModelImportState::Idle;
                true
            }
            Ok(ModelSourcePickerOutcome::Cancelled) => {
                self.model_import.reset();
                false
            }
            Err(error) => {
                self.model_import.reset();
                self.pending_notification = Some(model_source_picker_error(error));
                false
            }
        }
    }

    pub(super) fn start_model_import(&mut self, cx: &mut Context<Self>) {
        if !self.model_import.can_import() || self.pending.is_some() {
            return;
        }
        let request = SettingsModelImportRequest {
            title: self.model_import.title.clone(),
            source_root: self
                .model_import
                .source_root
                .clone()
                .expect("importable draft has a source directory"),
        };
        // The catalog as it stands is what tells the newly installed cards apart
        // from the ones that were already there, so it is recorded before the
        // run can change it.
        self.model_import.baseline_models = self
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .model_catalog
                    .entries
                    .iter()
                    .map(|entry| ModelRowKey::new(entry.origin, &entry.id))
                    .collect()
            })
            .unwrap_or_default();
        self.model_import.state = ModelImportState::Starting {
            cancel_requested: false,
        };
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.start_model_import(request).await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(operation) => {
                    view.model_import.apply_starting_cancellation(&operation);
                    view.observe_model_import(operation, cx);
                }
                Err(error) => {
                    view.pending_notification = Some(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn observe_model_import(
        &mut self,
        operation: SettingsModelImportOperation,
        cx: &mut Context<Self>,
    ) {
        let monitor = operation.monitor();
        let operation_id = monitor.operation_id();
        self.model_import.state = ModelImportState::Running(monitor);
        cx.notify();

        cx.spawn(async move |this, cx| {
            let final_result = operation.final_result().await;
            let _ = this.update(cx, |view, cx| {
                if view.model_import.running_operation_id() != Some(final_result.operation_id) {
                    return;
                }
                match final_result.result {
                    Ok(snapshot) => {
                        if view
                            .snapshot
                            .as_ref()
                            .is_none_or(|current| snapshot.revision >= current.revision)
                        {
                            view.snapshot = Some(snapshot);
                        }
                        view.begin_model_reveal();
                    }
                    // Nothing was installed: the card goes back to its prompt
                    // and only a real failure has anything to report.
                    Err(error) if error.code() == SettingsErrorCode::ModelImportCancelled => {
                        view.model_import.reset();
                    }
                    Err(error) => {
                        view.model_import.reset();
                        view.pending_notification = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let keep_polling = this
                    .update(cx, |view, cx| {
                        let keep_polling =
                            view.model_import.running_operation_id() == Some(operation_id);
                        if keep_polling {
                            cx.notify();
                        }
                        keep_polling
                    })
                    .unwrap_or(false);
                if !keep_polling {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn cancel_model_import(&mut self, cx: &mut Context<Self>) {
        match &mut self.model_import.state {
            ModelImportState::Starting { cancel_requested } => {
                *cancel_requested = true;
                cx.notify();
            }
            ModelImportState::Running(monitor) => {
                monitor.cancel();
                cx.notify();
            }
            _ => {}
        }
    }

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

    /// Start editing one model's title and cover.
    ///
    /// The card owns the whole edit, so the field is created here with the
    /// current title already in it: there is no separate "edit view" that could
    /// disagree with the catalog it was opened from. Editing keeps no origin
    /// exception — a preset is renamed and re-covered through the same records
    /// as an installed model — so the only reason to refuse is a command already
    /// in flight.
    pub(super) fn begin_model_edit(
        &mut self,
        model: SettingsModelKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let Some(title) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .model_catalog
                .entries
                .iter()
                .find(|entry| entry.origin == model.origin && entry.id == model.id)
                .map(|entry| entry.title.clone())
        }) else {
            return;
        };
        let language = self
            .snapshot
            .as_ref()
            .map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
                snapshot.resolved_language
            });
        let placeholder = bongocat_i18n::text(language.catalog_locale(), "models.identity.title");
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(title.clone())
        });
        let input_focus = input.read(cx).focus_handle(cx);
        cx.subscribe(&input, |view, input, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let value = input.read(cx).value();
            if let Some(draft) = view.model_edit.as_mut() {
                draft.title = sanitize_model_title_input(&value);
                cx.notify();
            }
        })
        .detach();
        self.model_delete_confirmation = None;
        window.focus(&input_focus, cx);
        self.model_edit = Some(ModelEditDraft {
            model,
            title,
            cover: None,
            input,
            input_focus,
            cover_focus: cx.focus_handle(),
            save_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            picking: false,
        });
        cx.notify();
    }

    pub(super) fn cancel_model_edit(&mut self, cx: &mut Context<Self>) {
        if self.model_edit.take().is_some() {
            cx.notify();
        }
    }

    /// Open the cover picker for the card that is being edited.
    pub(super) fn choose_model_cover(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let Some(draft) = self.model_edit.as_mut() else {
            return;
        };
        if draft.picking {
            return;
        }
        draft.picking = true;
        cx.notify();

        let (sender, receiver) = async_channel::bounded(1);
        let picking = move |result| {
            let _ = sender.try_send(result);
        };
        if let Err(error) = pick_model_cover(picking) {
            self.apply_model_cover_result(Err(error));
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = receiver
                .recv()
                .await
                .unwrap_or(Err(ModelSourcePickerError::BackendUnavailable));
            let _ = this.update(cx, |view, cx| {
                view.apply_model_cover_result(result);
                cx.notify();
            });
        })
        .detach();
    }

    /// Apply a cover dialog outcome to the open draft.
    ///
    /// Cancelling keeps whatever the draft already holds — the dialog is also
    /// how a user changes their mind about a choice they just made — and so does
    /// a backend failure, which only reports itself: dropping a cover the user
    /// really did choose because a later dialog misfired would lose work.
    pub(super) fn apply_model_cover_result(
        &mut self,
        result: Result<ModelSourcePickerOutcome, ModelSourcePickerError>,
    ) {
        let Some(draft) = self.model_edit.as_mut() else {
            return;
        };
        draft.picking = false;
        match result {
            Ok(ModelSourcePickerOutcome::Selected(cover)) => draft.cover = Some(cover),
            Ok(ModelSourcePickerOutcome::Cancelled) => {}
            Err(error) => self.pending_notification = Some(model_source_picker_error(error)),
        }
    }

    /// Commit the open edit: the title first, then the cover it was paired with.
    ///
    /// A cover is display artwork and does not move the configuration revision,
    /// while the title does, so the title goes first and the revision it reads
    /// is the one the snapshot reported when the card was opened.
    pub(super) fn save_model_edit(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        let Some(draft) = self.model_edit.as_ref() else {
            return;
        };
        let model = draft.model.clone();
        let title = draft.title.clone();
        let cover = draft.cover.clone();
        let current_title = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .model_catalog
                .entries
                .iter()
                .find(|entry| entry.origin == model.origin && entry.id == model.id)
                .map(|entry| entry.title.clone())
        });
        let expected_config_revision = if current_title.as_deref() != Some(title.as_str()) {
            match self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.config_revision)
            {
                Some(revision) => Some(revision),
                None => return,
            }
        } else {
            None
        };
        if expected_config_revision.is_none() && cover.is_none() {
            // Nothing changed: closing the card is the whole result.
            self.model_edit = None;
            cx.notify();
            return;
        }
        self.pending = Some(PendingOperation::ModelMetadata);
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let mut failure = None;
            let mut cover_written = false;
            if let Some(expected_config_revision) = expected_config_revision {
                match client
                    .set_model_title(expected_config_revision, model.clone(), title)
                    .await
                {
                    Ok(snapshot) => {
                        let _ = this.update(cx, |view, cx| {
                            if view
                                .snapshot
                                .as_ref()
                                .is_none_or(|current| snapshot.revision >= current.revision)
                            {
                                view.snapshot = Some(snapshot);
                            }
                            cx.notify();
                        });
                    }
                    Err(error) => failure = Some(error),
                }
            }
            if failure.is_none()
                && let Some(cover) = cover
            {
                match client.set_model_cover(model.clone(), cover).await {
                    Ok(_) => cover_written = true,
                    Err(error) => failure = Some(error),
                }
            }
            let _ = this.update(cx, |view, cx| {
                view.pending = None;
                match failure {
                    Some(error) => view.pending_notification = Some(error),
                    None => {
                        view.model_edit = None;
                        if cover_written {
                            view.invalidate_model_cover(&model, cx);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Publish a model whose cover capture just finished.
    ///
    /// The import result lands before the product has rendered the model's own
    /// cover, so the cards the run installed are withheld from the grid until
    /// this call. It comes from the capture on the window thread — the settings
    /// worker is a different one — and arrives whether the capture produced a
    /// cover or not, so a failed capture publishes the model with the cover its
    /// source shipped instead of leaving it hidden.
    pub fn finish_model_cover_capture(
        &mut self,
        model: &SettingsModelKey,
        captured: bool,
        cx: &mut Context<Self>,
    ) {
        let key = ModelRowKey::new(model.origin, &model.id);
        if captured {
            self.invalidate_model_cover(model, cx);
        }
        if self.pending_model_reveal.remove(&key) {
            if self.pending_model_reveal.is_empty()
                && matches!(self.model_import.state, ModelImportState::Capturing)
            {
                self.model_import.reset();
            }
        } else if matches!(
            self.model_import.state,
            ModelImportState::Starting { .. } | ModelImportState::Running(_)
        ) {
            // The capture won the race against the import reply. The reveal gate
            // consults this set when it opens, so the model is not left waiting
            // for a signal that already happened.
            self.completed_model_cover_captures.insert(key);
        }
        cx.notify();
    }

    /// Hold the models the run just installed back until their covers are done.
    ///
    /// A successful import always queues a cover capture, so the run's own
    /// cards become visible when that capture reports back rather than the
    /// moment the package lands. Nothing is published when the run installed
    /// nothing, which is also how a rejected snapshot revision is handled: the
    /// newer snapshot republishes the catalog with the card already in it.
    pub(super) fn begin_model_reveal(&mut self) {
        let baseline = std::mem::take(&mut self.model_import.baseline_models);
        let installed = self
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .model_catalog
                    .entries
                    .iter()
                    .filter(|entry| entry.origin == SettingsModelOrigin::Installed)
                    .map(|entry| ModelRowKey::new(entry.origin, &entry.id))
                    .filter(|key| !baseline.contains(key))
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let mut pending = installed;
        for key in std::mem::take(&mut self.completed_model_cover_captures) {
            pending.remove(&key);
        }
        if pending.is_empty() {
            self.model_import.reset();
        } else {
            self.pending_model_reveal = pending;
            self.model_import.state = ModelImportState::Capturing;
        }
    }

    /// Drop the cached cover image of a model whose cover just changed.
    ///
    /// The replacement keeps the package's own file name, so the image cache is
    /// keyed by a path that still resolves to the *old* bytes until it is
    /// dropped.
    fn invalidate_model_cover(&self, model: &SettingsModelKey, cx: &mut App) {
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
            self.pending.is_some() || self.model_import.is_running(),
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
