//! Model selection shares the conversion dialog and queues existing imports.

use super::*;

pub(crate) struct ModelSelectionDialog {
    pub(crate) models: Vec<SettingsModelSourceCandidate>,
    pub(crate) open: bool,
    pub(crate) checked: BTreeSet<usize>,
}

impl SettingsView {
    pub(super) fn apply_model_source_content(
        &mut self,
        content: SettingsModelSourceContent,
        cx: &mut Context<Self>,
    ) {
        self.model_import.state = ModelImportState::Idle;
        match content {
            SettingsModelSourceContent::Package => self.start_model_import(cx),
            SettingsModelSourceContent::Mver { modes } => {
                self.model_import.mver_mode_dialog = Some(MverModeDialog::from_available(modes));
            }
            SettingsModelSourceContent::Folder { models } => {
                if models.len() == 1 {
                    self.model_import.queued_sources.extend(models);
                    self.advance_model_import_queue(cx);
                } else if !models.is_empty() {
                    self.model_import.model_selection_dialog = Some(ModelSelectionDialog {
                        checked: (0..models.len()).collect(),
                        models,
                        open: false,
                    });
                } else {
                    self.model_import.reset();
                }
            }
        }
    }

    pub(super) fn advance_model_import_queue(&mut self, cx: &mut Context<Self>) {
        if let Some(model) = self.model_import.queued_sources.pop_front() {
            self.model_import.source_root = Some(model.source_root.clone());
            self.inspect_model_source(model.source_root, cx);
        }
    }

    pub(super) fn confirm_model_selection(&mut self, indices: Vec<usize>, cx: &mut Context<Self>) {
        let Some(dialog) = self.model_import.model_selection_dialog.as_ref() else {
            return;
        };
        // Only inspected candidates can enter the queue, once each in display order.
        let selected = dialog
            .models
            .iter()
            .enumerate()
            .filter(|(index, _)| indices.contains(index))
            .map(|(_, model)| model.clone())
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return;
        }
        self.model_import.model_selection_dialog = None;
        self.model_import.queued_sources.extend(selected);
        self.advance_model_import_queue(cx);
    }

    pub(super) fn sync_model_selection_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.model_import.model_selection_dialog.as_mut() else {
            return;
        };
        if dialog.open {
            if !window.has_active_dialog(cx) {
                self.model_import.reset();
            }
            return;
        }
        dialog.open = true;
        let choices = dialog
            .models
            .iter()
            .enumerate()
            .map(|(index, model)| (model.label.clone(), dialog.checked.contains(&index)))
            .collect::<Vec<_>>();
        let locale = self.display_language().catalog_locale();
        let title = bongocat_i18n::text(locale, "models.import.selection_dialog.title").to_owned();
        let description = self.model_import.title.clone();
        let view = cx.entity().downgrade();
        // Install the shared snapshot once. Root rebuilds the dialog each frame;
        // it must not re-borrow SettingsView or reset the user's checks.
        let snapshot = Rc::new(RefCell::new(ChoicesSnapshot::new(choices)));
        window.open_dialog(cx, move |dialog, window, _cx| {
            build_choices_frame(
                snapshot.clone(),
                title.clone(),
                description.clone(),
                locale,
                view.clone(),
                dialog,
                window,
                Rc::new(|view, index, checked, _| {
                    if let Some(dialog) = view.model_import.model_selection_dialog.as_mut() {
                        if checked {
                            dialog.checked.insert(index);
                        } else {
                            dialog.checked.remove(&index);
                        }
                    }
                }),
                Rc::new(|view, indices, cx| view.confirm_model_selection(indices, cx)),
            )
        });
    }
}

struct ChoicesSnapshot {
    labels: Vec<String>,
    checked: BTreeSet<usize>,
}

impl ChoicesSnapshot {
    fn new(choices: Vec<(String, bool)>) -> Self {
        let checked = choices
            .iter()
            .enumerate()
            .filter(|(_, (_, checked))| *checked)
            .map(|(index, _)| index)
            .collect();
        Self {
            labels: choices.into_iter().map(|(label, _)| label).collect(),
            checked,
        }
    }
}

type ToggleChoice = Rc<dyn Fn(&mut SettingsView, usize, bool, &mut Context<SettingsView>)>;
type ConfirmChoices = Rc<dyn Fn(&mut SettingsView, Vec<usize>, &mut Context<SettingsView>)>;

#[expect(
    clippy::too_many_arguments,
    reason = "shared dialog inputs and typed callbacks"
)]
pub(super) fn build_import_choices_dialog(
    choices: Vec<(String, bool)>,
    title: String,
    description: String,
    locale: &'static str,
    view: WeakEntity<SettingsView>,
    dialog: Dialog,
    window: &mut Window,
    toggle: impl Fn(&mut SettingsView, usize, bool, &mut Context<SettingsView>) + 'static,
    confirm: impl Fn(&mut SettingsView, Vec<usize>, &mut Context<SettingsView>) + 'static,
) -> Dialog {
    build_choices_frame(
        Rc::new(RefCell::new(ChoicesSnapshot::new(choices))),
        title,
        description,
        locale,
        view,
        dialog,
        window,
        Rc::new(toggle),
        Rc::new(confirm),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared dialog inputs and typed callbacks"
)]
fn build_choices_frame(
    snapshot: Rc<RefCell<ChoicesSnapshot>>,
    title: String,
    description: String,
    locale: &'static str,
    view: WeakEntity<SettingsView>,
    dialog: Dialog,
    window: &mut Window,
    toggle: ToggleChoice,
    confirm: ConfirmChoices,
) -> Dialog {
    let enabled = !snapshot.borrow().checked.is_empty();
    let labels = snapshot.borrow().labels.clone();
    let confirm_label = bongocat_i18n::text(locale, "models.import.mver_dialog.import");
    let cancel_label = bongocat_i18n::text(locale, "actions.cancel");
    let on_ok_snapshot = snapshot.clone();
    let on_ok_view = view.clone();
    let on_ok_confirm = confirm.clone();
    let on_cancel_view = view.clone();
    let footer_cancel_view = view.clone();
    let footer_view = view.clone();
    let footer_snapshot = snapshot.clone();
    let dialog_height =
        px(132.5 + labels.len() as f32 * 32.).min(window.viewport_size().height - px(48.));
    let list_height = (dialog_height - px(140.)).max(px(32.));
    dialog
        .title(title)
        .w(px(440.))
        .margin_top(((window.viewport_size().height - dialog_height) / 2.).max(px(16.)))
        .button_props(
            DialogButtonProps::default()
                .ok_text(confirm_label)
                .cancel_text(cancel_label)
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let indices = on_ok_snapshot
                        .borrow()
                        .checked
                        .iter()
                        .copied()
                        .collect::<Vec<_>>();
                    if indices.is_empty() {
                        return false;
                    }
                    window.close_dialog(cx);
                    let _ = on_ok_view.update(cx, |view, cx| on_ok_confirm(view, indices, cx));
                    true
                })
                .on_cancel(move |_, window, cx| {
                    let _ = on_cancel_view.update(cx, |view, cx| view.cancel_mver_mode_dialog(cx));
                    window.close_dialog(cx);
                    true
                }),
        )
        .footer(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("import-choice-cancel")
                        .label(cancel_label)
                        .ghost()
                        .tab_stop(true)
                        .on_click(move |_, window, cx| {
                            let _ = footer_cancel_view
                                .update(cx, |view, cx| view.cancel_mver_mode_dialog(cx));
                            window.close_dialog(cx);
                        }),
                )
                .child(
                    Button::new("import-choice-confirm")
                        .label(confirm_label)
                        .disabled(!enabled)
                        .tab_stop(enabled)
                        .on_click(move |_, window, cx| {
                            let indices = footer_snapshot
                                .borrow()
                                .checked
                                .iter()
                                .copied()
                                .collect::<Vec<_>>();
                            if indices.is_empty() {
                                return;
                            }
                            window.close_dialog(cx);
                            let _ = footer_view.update(cx, |view, cx| confirm(view, indices, cx));
                        }),
                ),
        )
        .content(move |content, _window, cx| {
            let options = labels.iter().enumerate().map(|(index, label)| {
                let checked = snapshot.borrow().checked.contains(&index);
                let snapshot = snapshot.clone();
                let view = view.clone();
                let toggle = toggle.clone();
                Checkbox::new(("import-choice", index))
                    .label(label.clone())
                    .checked(checked)
                    .on_change(move |checked, _, cx| {
                        if *checked {
                            snapshot.borrow_mut().checked.insert(index);
                        } else {
                            snapshot.borrow_mut().checked.remove(&index);
                        }
                        let _ = view.update(cx, |view, cx| {
                            toggle(view, index, *checked, cx);
                            cx.notify();
                        });
                    })
                    .into_any_element()
            });
            content.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(description.clone()),
                    )
                    .child(
                        div()
                            .id("import-choice-list")
                            .v_flex()
                            .gap_3()
                            .max_h(list_height)
                            .overflow_y_scroll()
                            .children(options),
                    ),
            )
        })
}
