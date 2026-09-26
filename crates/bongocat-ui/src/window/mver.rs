//! The conversion-mode dialog, and the import it gates.
//!
//! A `.mver` source converts through more than one mode, so inspecting one does
//! not produce a model — it produces a question, and the import cannot start
//! until the question is answered. That is why inspection and import are one
//! module here even though they are two commands: the dialog is what stands
//! between them, and splitting them would put half of that handshake in each.

use super::*;

impl SettingsView {
    /// Classify the chosen folder, then start a package's import at once or hold
    /// a Mver source for its conversion-mode choices.
    ///
    /// The store decides what the folder is from its bytes, so this is the one
    /// place the page learns whether there is a choice to put to the user.
    /// Reaching the source's title here rather than at pick time keeps a folder
    /// the user picked but never got to import through its own failure path: an
    /// inspection error resets the card and reports like any import error, sharing
    /// the notification vocabulary rather than inventing a second one.
    pub(super) fn inspect_model_source(&mut self, source_root: PathBuf, cx: &mut Context<Self>) {
        self.model_import.title = suggested_model_title(&source_root);
        self.model_import.state = ModelImportState::Inspecting;
        self.model_import.mver_mode_dialog = None;
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.inspect_model_source(source_root).await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(SettingsModelSourceContent::Package) => {
                        view.model_import.mver_mode_dialog = None;
                        view.model_import.state = ModelImportState::Idle;
                        view.start_model_import(cx);
                    }
                    Ok(SettingsModelSourceContent::Mver { modes }) => {
                        view.model_import.mver_mode_dialog =
                            Some(MverModeDialog::from_available(modes));
                        view.model_import.state = ModelImportState::Idle;
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
    }
}

impl SettingsView {
    /// Keep the window's dialog surface and the draft's selection in step.
    ///
    /// The checkboxes are drawn from a render-time snapshot of the draft while
    /// toggles write the draft back, so the frame after a toggle is the frame
    /// with the new value. Opening and closing mirror the same split: the draft
    /// says what should be on screen, and this is the one place that reads it
    /// onto the window. A dialog dismissed by the overlay or Escape never reaches
    /// a button callback, so its close is noticed the next frame and the draft
    /// is dropped then, leaving the card at its prompt.
    pub(super) fn sync_mver_mode_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let has_dialog = window.has_active_dialog(cx);
        let status = self
            .model_import
            .mver_mode_dialog
            .as_ref()
            .map(|dialog| (dialog.open, has_dialog));
        match status {
            // The draft asked for a surface that was dismissed without a button:
            // the overlay, Escape, or the window's own close. Dropping the
            // draft is the same reset cancel performs, so the card returns to
            // its prompt with nothing half-chosen.
            Some((true, false)) => self.model_import.mver_mode_dialog = None,
            Some((false, _)) => self.open_mver_mode_dialog(window, cx),
            _ => {}
        }
    }
}

impl SettingsView {
    /// Put the conversion-mode choices to the user.
    ///
    /// The dialog is `gpui-kit`'s, and each option is one of its checkboxes:
    /// no hand-built surface stands in for the component. The values the
    /// dialog *renders* come from an [`MverDialogSnapshot`] taken here rather
    /// than a later read of the draft: `Root` owns the dialog layer inside
    /// `SettingsView::render`, so a `read_with` there would
    /// re-borrow the entity `render` already holds. The snapshot is the
    /// frame's controlled state; toggles write back to it and to the draft,
    /// the confirm callback reads the draft, and a cancel simply drops it.
    pub(super) fn open_mver_mode_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .model_import
            .mver_mode_dialog
            .as_ref()
            .is_none_or(|dialog| dialog.open)
        {
            return;
        }
        // The view's own language and draft are the only per-frame inputs the
        // dialog needs, and both are read here rather than inside the builder:
        // the builder runs during this render, when the view cannot be read.
        let locale = self.display_language().catalog_locale();
        let dialog = self
            .model_import
            .mver_mode_dialog
            .as_mut()
            .expect("the dialog draft was just checked");
        dialog.open = true;
        let snapshot = Rc::new(RefCell::new(MverDialogSnapshot::from_dialog(dialog)));
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            // The builder runs again on every frame the dialog is on screen.
            // `snapshot` is the frame's rendered state (never a read back of
            // `SettingsView`); toggles update it in place, and the notify in
            // the same callback redraws from the new values.
            build_mver_mode_dialog(snapshot.clone(), locale, view.clone(), dialog, window, cx)
        });
    }
}

impl SettingsView {
    /// Apply a checked value the dialog's checkbox reported.
    ///
    /// A value outside the inspected set is refused rather than trusted: the
    /// dialog may only ever offer what the source actually carries.
    pub(super) fn set_mver_mode_checked(
        &mut self,
        mode: SettingsMverMode,
        checked: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.model_import.mver_mode_dialog.as_mut() else {
            return;
        };
        dialog.toggle(mode, checked);
        cx.notify();
    }
}

impl SettingsView {
    /// Confirm the dialog's selection by starting the run for its checked modes.
    ///
    /// The callback returns whether the dialog should close; confirm is
    /// disabled for an empty selection, so it can never close into a run with
    /// nothing to convert.
    pub(super) fn confirm_mver_mode_import(
        &mut self,
        modes: Vec<SettingsMverMode>,
        cx: &mut Context<Self>,
    ) {
        self.model_import.mver_mode_dialog = None;
        self.start_model_import_with_modes(modes, cx);
    }
}

impl SettingsView {
    /// Drop the dialog's selection and return the card to its prompt.
    pub(super) fn cancel_mver_mode_dialog(&mut self, cx: &mut Context<Self>) {
        self.model_import.reset();
        cx.notify();
    }
}
