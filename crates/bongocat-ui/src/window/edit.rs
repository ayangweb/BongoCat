//! Renaming a model, and the cover that goes with the new name.
//!
//! A rename is a draft until it is saved: the title is editable, the cover is
//! picked, and neither reaches the store until the user commits both. The cover
//! capture is a session rather than a function because it has to wait for a frame
//! that was actually drawn, and a function that returned before the frame arrived
//! would save an empty image.

use super::model_actions::model_source_picker_error;
use super::*;

impl SettingsView {
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
}

impl SettingsView {
    pub(super) fn cancel_model_edit(&mut self, cx: &mut Context<Self>) {
        if self.model_edit.take().is_some() {
            cx.notify();
        }
    }
}

impl SettingsView {
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
}

impl SettingsView {
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
}

impl SettingsView {
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
}

impl SettingsView {
    /// Report the outcome of a model's cover capture.
    ///
    /// The import result lands before the product has rendered the model's own
    /// cover, so the cards the run installed are withheld from the grid until
    /// this call. It comes from the capture on the window thread — the settings
    /// worker is a different one.
    ///
    /// A capture that produced a cover publishes the model. One that did not
    /// abandons it: the capture renders the model through the same GPU path the
    /// overlay uses, so a model that cannot be captured cannot be activated
    /// either, and a card the user can only fail to select is worse than no
    /// card. The caller removes the model from the store before calling this;
    /// the key is still recorded here so the reveal gate stops waiting for a
    /// signal that has already arrived.
    pub fn finish_model_cover_capture(
        &mut self,
        model: &SettingsModelKey,
        captured: bool,
        cx: &mut Context<Self>,
    ) {
        let key = ModelRowKey::new(model.origin, &model.id);
        if captured {
            self.invalidate_model_cover(model, cx);
        } else {
            self.model_import_failed_pending = true;
        }
        if self.pending_model_reveal.remove(&key) {
            if self.pending_model_reveal.is_empty()
                && matches!(self.model_import.state, ModelImportState::Capturing)
            {
                self.model_import.reset();
                if captured {
                    self.model_import_success_pending = true;
                }
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
}
