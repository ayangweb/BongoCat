//! Where a model is chosen from, and what the picker refused.
//!
//! The folder picker and the cover picker share one failure vocabulary, because
//! their failures are properties of opening a native dialog rather than of what
//! was being chosen. The page therefore reports them the same way too, and the
//! helper that maps them is shared rather than duplicated per picker.

use super::model_actions::model_source_picker_error;
use super::*;

impl SettingsView {
    /// Whether a new model source may enter the import flow.
    ///
    /// The picker, a dropped path and the visible drop affordance all read this
    /// one predicate, so a second source cannot enter through a different
    /// gesture while a command, import or source surface is already active.
    pub(super) fn model_source_command_available(&self) -> bool {
        self.pending.is_none()
            && !self.model_import.is_running()
            && !self.model_import.is_source_surface_open()
    }
}

impl SettingsView {
    /// Open the native picker for the folder to import.
    ///
    /// One dialog at a time is the whole flow: the folder the user picks is
    /// inspected before anything starts, and what it contains decides whether the
    /// run begins on the spot or waits behind the conversion-mode dialog.
    pub(super) fn choose_model_source(&mut self, cx: &mut Context<Self>) {
        // The card stays drawn as interactive while a command is in flight —
        // `pending` never feeds a visual gate (ADR-0053) — so the refusal of a
        // second command lives here rather than in the paint.
        if !self.model_source_command_available() {
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
                if let Some(source_root) = view.apply_model_source_result(result) {
                    // Classification comes before conversion: a package starts
                    // its import now, while a Mver source has to wait for the
                    // user to pick which modes this run converts.
                    view.inspect_model_source(source_root, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl SettingsView {
    /// Apply a folder dialog outcome to the import draft.
    ///
    /// The selected folder is not classified here; [`Self::inspect_model_source`]
    /// asks the store what it contains. A cancelled or failed dialog both return
    /// the card to its prompt, because the folder the user was choosing is the
    /// thing the error is about, and the failure is reported as a notification
    /// rather than as inline text, so the page reports every error one way. A
    /// chosen folder returns its path without touching the card: the state is not
    /// decided until the store answers.
    pub(super) fn apply_model_source_result(
        &mut self,
        result: Result<ModelSourcePickerOutcome, ModelSourcePickerError>,
    ) -> Option<PathBuf> {
        match result {
            Ok(ModelSourcePickerOutcome::Selected(source_root)) => {
                self.model_import.source_root = Some(source_root.clone());
                Some(source_root)
            }
            Ok(ModelSourcePickerOutcome::Cancelled) => {
                self.model_import.reset();
                None
            }
            Err(error) => {
                self.model_import.reset();
                self.pending_notification = Some(model_source_picker_error(error));
                None
            }
        }
    }
}
