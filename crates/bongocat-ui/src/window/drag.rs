//! Dropping a model onto the window.
//!
//! A drop is a path the user dragged, not a path the window chose, so it is
//! validated as carefully as one the picker returned and refused with the same
//! codes. The drag state is cleared on every exit from this path, including the
//! rejected ones: an overlay left showing a drop target after the user let go of
//! something invalid is a window that looks stuck.

use super::*;

impl SettingsView {
    /// Reflect an external file drag over the settings window.
    ///
    /// GPUI translates Finder/Explorer file drags into [`ExternalPaths`]. The
    /// number of paths is enough to decide whether the drop can enter the
    /// one-folder import contract without touching the filesystem on the UI
    /// thread. A regular file is rejected by the same canonical directory
    /// validation as the native picker once it is released.
    pub(super) fn update_model_drag(&mut self, path_count: usize, cx: &mut Context<Self>) {
        let next = if path_count != 1 {
            ModelDragOverlayState::InvalidSelection
        } else if !self.model_source_command_available() {
            ModelDragOverlayState::Busy
        } else {
            ModelDragOverlayState::Ready
        };
        if self.model_drag != Some(next) {
            self.model_drag = Some(next);
            cx.notify();
        }
    }
}

impl SettingsView {
    /// Remove the temporary drag affordance when the platform drag leaves or is
    /// submitted. It is presentation state and must never survive the gesture.
    pub(super) fn clear_model_drag(&mut self, cx: &mut Context<Self>) {
        if self.model_drag.take().is_some() {
            cx.notify();
        }
    }
}

impl SettingsView {
    /// Accept one dropped model folder and hand it to the existing inspection
    /// and import flow.
    ///
    /// The path first goes through the picker adapter's absolute/directory/
    /// canonicalization check on a background executor. Only the canonical
    /// directory crosses into the settings command, where package validation,
    /// conversion-mode selection, transactional import and cover capture already
    /// live. The UI never walks or copies the model package itself.
    pub(super) fn accept_model_folder_drop(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        self.model_drag = None;
        if !self.model_source_command_available() {
            cx.notify();
            return;
        }
        let Some(source_root) = paths.first().filter(|_| paths.len() == 1).cloned() else {
            self.pending_notification = Some(SettingsError::new(
                SettingsErrorCode::ModelImportDropInvalid,
            ));
            cx.notify();
            return;
        };

        self.model_import.state = ModelImportState::ValidatingDrop;
        cx.notify();
        let validation = cx
            .background_executor()
            .spawn(async move { validate_model_folder(source_root) });
        cx.spawn(async move |this, cx| {
            let result = validation.await;
            let _ = this.update(cx, |view, cx| {
                // A newer gesture or a failed view teardown may have reset the
                // draft while the tiny validation task was in flight. Never let
                // a stale path resurrect an import the user no longer owns.
                if !view.model_import.is_validating_drop() {
                    return;
                }
                match result {
                    Ok(ModelSourcePickerOutcome::Selected(source_root)) => {
                        if let Some(source_root) = view.apply_model_source_result(Ok(
                            ModelSourcePickerOutcome::Selected(source_root),
                        )) {
                            view.inspect_model_source(source_root, cx);
                        }
                    }
                    Ok(ModelSourcePickerOutcome::Cancelled) => view.model_import.reset(),
                    Err(_) => {
                        view.model_import.reset();
                        view.pending_notification = Some(SettingsError::new(
                            SettingsErrorCode::ModelImportDropInvalid,
                        ));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
