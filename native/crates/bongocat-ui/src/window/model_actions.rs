use super::*;

impl SettingsView {
    pub(super) fn choose_model_directory(&mut self, cx: &mut Context<Self>) {
        if self.model_import.is_running() || self.model_import.is_picker_open() {
            return;
        }
        self.model_import.state = ModelImportState::Picking;
        cx.notify();

        let (sender, receiver) = async_channel::bounded(1);
        if let Err(error) = pick_model_directory(move |result| {
            let _ = sender.try_send(result);
        }) {
            self.apply_model_directory_result(Err(error));
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = receiver
                .recv()
                .await
                .unwrap_or(Err(DirectoryPickerError::BackendUnavailable));
            let _ = this.update(cx, |view, cx| {
                view.apply_model_directory_result(result);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn apply_model_directory_result(
        &mut self,
        result: Result<DirectoryPickerOutcome, DirectoryPickerError>,
    ) {
        match result {
            Ok(DirectoryPickerOutcome::Selected(source_root)) => {
                self.model_import.id = suggested_model_id(&source_root);
                self.model_import.source_root = Some(source_root);
                self.model_import.state = ModelImportState::Ready;
            }
            Ok(DirectoryPickerOutcome::Cancelled) => {
                self.model_import.state = ModelImportState::PickerCancelled;
            }
            Err(error) => {
                self.model_import.state = ModelImportState::PickerFailed(error);
            }
        }
    }

    pub(super) fn start_model_import(&mut self, cx: &mut Context<Self>) {
        if !self.model_import.can_import() || self.pending.is_some() {
            return;
        }
        let request = SettingsModelImportRequest {
            id: self.model_import.id.clone(),
            source_root: self
                .model_import
                .source_root
                .clone()
                .expect("importable draft has a source directory"),
        };
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
                    view.model_import.state = ModelImportState::Failed(error);
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
                    Ok(snapshot)
                        if view
                            .snapshot
                            .as_ref()
                            .is_none_or(|current| snapshot.revision >= current.revision) =>
                    {
                        view.snapshot = Some(snapshot);
                        view.model_import.source_root = None;
                        view.model_import.state = ModelImportState::Succeeded;
                    }
                    Ok(_) => {
                        view.model_import.source_root = None;
                        view.model_import.state = ModelImportState::Succeeded;
                    }
                    Err(error) if error.code() == SettingsErrorCode::ModelImportCancelled => {
                        view.model_import.state = ModelImportState::Cancelled;
                    }
                    Err(error) => view.model_import.state = ModelImportState::Failed(error),
                }
                cx.notify();
            });
        })
        .detach();

        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(100)).await;
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
            .is_some_and(|model| !model_delete_confirmation_is_valid(entries, active_model, model))
        {
            self.model_delete_confirmation = None;
        }
        let keys = entries
            .iter()
            .map(|entry| ModelRowKey::new(entry.origin, &entry.id))
            .collect::<BTreeSet<_>>();
        self.model_row_focus.retain(|key, _| keys.contains(key));
        for (index, entry) in entries.iter().enumerate() {
            let key = ModelRowKey::new(entry.origin, &entry.id);
            let model = SettingsModelKey {
                id: entry.id.clone(),
                origin: entry.origin,
            };
            let actions = model_row_actions(entry, active_model, commands_blocked);
            let confirming_delete = self.model_delete_confirmation.as_ref() == Some(&model);
            let offset = isize::try_from(index)
                .unwrap_or(isize::MAX / 3)
                .saturating_mul(3);
            let tab_index = 40_isize.saturating_add(offset);
            let action_tabs = model_row_action_tab_indices(tab_index, confirming_delete);
            let focus = self
                .model_row_focus
                .entry(key)
                .or_insert_with(|| ModelRowFocus {
                    activate: cx.focus_handle(),
                    delete: cx.focus_handle(),
                    cancel_delete: cx.focus_handle(),
                });
            focus.activate = focus
                .activate
                .clone()
                .tab_index(action_tabs.activate)
                .tab_stop(actions.can_activate);
            focus.delete = focus
                .delete
                .clone()
                .tab_index(action_tabs.delete)
                .tab_stop(actions.can_delete);
            focus.cancel_delete = focus
                .cancel_delete
                .clone()
                .tab_index(action_tabs.cancel_delete)
                .tab_stop(actions.can_delete && confirming_delete);
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
        self.error = None;
        self.model_delete_confirmation = None;
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
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn request_model_delete(&mut self, model: SettingsModelKey, cx: &mut Context<Self>) {
        if self.pending.is_some() || self.model_import.is_running() {
            return;
        }
        if self.model_delete_confirmation.as_ref() == Some(&model) {
            self.delete_model(model, cx);
        } else {
            self.model_delete_confirmation = Some(model);
            self.error = None;
            cx.notify();
        }
    }

    pub(super) fn cancel_model_delete(&mut self, model: &SettingsModelKey, cx: &mut Context<Self>) {
        if self.model_delete_confirmation.as_ref() == Some(model) {
            self.model_delete_confirmation = None;
            cx.notify();
        }
    }

    pub(super) fn delete_model(&mut self, model: SettingsModelKey, cx: &mut Context<Self>) {
        self.pending = Some(PendingOperation::ModelDeletion);
        self.error = None;
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
                    Err(error) => view.error = Some(error),
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
                window.focus(&focus.activate);
                self.select_model(model, cx);
            }
            ModelRowAction::Delete if actions.can_delete => {
                window.focus(&focus.delete);
                self.request_model_delete(model, cx);
            }
            ModelRowAction::CancelDelete
                if self.model_delete_confirmation.as_ref() == Some(&model) =>
            {
                window.focus(&focus.cancel_delete);
                self.cancel_model_delete(&model, cx);
            }
            _ => {}
        }
    }

}
