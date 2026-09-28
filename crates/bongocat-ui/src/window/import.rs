//! Running the import, and watching it.
//!
//! An import outlives the click that started it, so the view observes it rather
//! than awaiting it. Cancelling is a request, not a kill: the store is given the
//! chance to finish and clean up, and the window is told when it actually has.

use super::*;

impl SettingsView {
    /// Start a package import, which converts nothing and so names no modes.
    pub(super) fn start_model_import(&mut self, cx: &mut Context<Self>) {
        self.start_model_import_with_modes(Vec::new(), cx);
    }
}

impl SettingsView {
    /// Start the run for the chosen folder and the conversions it should carry.
    ///
    /// A package names no modes; a Mver source names exactly the checked ones,
    /// in the order the dialog showed them. The draft's dialog state is
    /// cleared here so a later snapshot or failure cannot re-apply a stale
    /// selection to a second run.
    pub(super) fn start_model_import_with_modes(
        &mut self,
        selected_mver_modes: Vec<SettingsMverMode>,
        cx: &mut Context<Self>,
    ) {
        if !self.model_import.can_import() || self.pending.is_some() {
            return;
        }
        self.model_import.mver_mode_dialog = None;
        let request = SettingsModelImportRequest {
            title: self.model_import.title.clone(),
            source_root: self
                .model_import
                .source_root
                .clone()
                .expect("importable draft has a source directory"),
            selected_mver_modes,
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
                    view.model_import.mver_mode_dialog = None;
                    view.pending_notification = Some(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

impl SettingsView {
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
                        if view.begin_model_reveal() {
                            view.model_import_success_pending = true;
                        }
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
}

impl SettingsView {
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
}
