use super::*;

/// Build one frame of the Mver conversion-mode dialog.
///
/// The builder runs whenever the window's dialog layer paints, which happens
/// inside `SettingsView::render`. It therefore must not read the
/// `SettingsView`: a `read_with` here would re-borrow the entity that
/// `render` already holds. Its inputs are passed in instead — the inspected
/// modes and current checks live in a shared [`MverDialogSnapshot`], and the
/// language is captured once when the dialog opens.
///
/// The snapshot is the controlled source for the frame. A checkbox writes its
/// new value into the snapshot and then calls
/// [`SettingsView::set_mver_mode_checked`], so the draft the confirm callback
/// eventually reads and the boxes the next frame draws from agree.
///
/// The options are exactly the modes inspection reported, in the store's order.
/// The title, the description and the button copy come from the page's own
/// catalog. The footer is built by hand because the stock button pair has no
/// disabled state: with every box cleared, confirm has to read and behave as
/// disabled, which a footer button can express. Enter still routes through
/// [`Dialog::on_ok`], so the keyboard path reads the same predicate.
pub(super) fn build_mver_mode_dialog(
    snapshot: Rc<RefCell<MverDialogSnapshot>>,
    locale: &'static str,
    view: WeakEntity<SettingsView>,
    dialog: Dialog,
    _window: &mut Window,
    _cx: &mut App,
) -> Dialog {
    let available = snapshot.borrow().available().to_vec();
    let text = |key: &str| SharedString::from(bongocat_i18n::text(locale, key));
    let title = text("models.import.mver_dialog.title");
    let description = text("models.import.mver_dialog.description");
    let confirm_label = text("models.import.mver_dialog.import");
    let cancel_label = text("actions.cancel");

    let on_ok_view = view.clone();
    let on_cancel_view = view.clone();
    let footer_confirm_view = view.clone();
    let footer_cancel_view = view.clone();
    let check_view = view.clone();
    let confirm_snapshot = snapshot.clone();
    let footer_snapshot = snapshot.clone();
    let confirm_enabled = snapshot.borrow().can_confirm();

    let viewport_height = _window.viewport_size().height;
    // GPUI Kit's Dialog positions from a top inset, before the surface has
    // measured its content. The Mver surface is a fixed width with one
    // checkbox per available mode; use the measured height of the same
    // content in gpui-kit 0.6.6 to derive a centered inset.
    let dialog_height = px(132.5 + available.len() as f32 * 32.);
    let centered_margin_top = ((viewport_height - dialog_height) / 2.).max(px(16.));

    dialog
        .title(title)
        .w(px(440.))
        .margin_top(centered_margin_top)
        .button_props(
            DialogButtonProps::default()
                .ok_text(confirm_label.clone())
                .cancel_text(cancel_label.clone())
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    // Enter and the footer's confirm read the same shared
                    // snapshot, so an empty selection can never start a run.
                    let modes = confirm_snapshot.borrow().checked_in_order();
                    if modes.is_empty() {
                        return false;
                    }
                    let _ =
                        on_ok_view.update(cx, |view, cx| view.confirm_mver_mode_import(modes, cx));
                    window.close_dialog(cx);
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
                    Button::new("mver-mode-cancel")
                        .label(cancel_label.clone())
                        .ghost()
                        .tab_stop(true)
                        .on_click(move |_, window, cx| {
                            let _ = footer_cancel_view
                                .update(cx, |view, cx| view.cancel_mver_mode_dialog(cx));
                            window.close_dialog(cx);
                        }),
                )
                .child(
                    Button::new("mver-mode-confirm")
                        .label(confirm_label.clone())
                        .disabled(!confirm_enabled)
                        .tab_stop(confirm_enabled)
                        .on_click(move |_, window, cx| {
                            let modes = footer_snapshot.borrow().checked_in_order();
                            if modes.is_empty() {
                                return;
                            }
                            let _ = footer_confirm_view
                                .update(cx, |view, cx| view.confirm_mver_mode_import(modes, cx));
                            window.close_dialog(cx);
                        }),
                ),
        )
        .content(move |content, _window, cx| {
            let muted = cx.theme().muted_foreground;
            let options = available.iter().copied().map(|mode| {
                let label: SharedString =
                    bongocat_i18n::text(locale, mver_mode_label_key(mode)).into();
                let checked = snapshot.borrow().is_checked(mode);
                let toggled_view = check_view.clone();
                let toggled_snapshot = snapshot.clone();
                Checkbox::new(mver_mode_checkbox_id(mode))
                    .label(label)
                    .checked(checked)
                    .on_change(move |checked, _window, cx| {
                        toggled_snapshot.borrow_mut().set(mode, *checked);
                        let _ = toggled_view.update(cx, |view, cx| {
                            view.set_mver_mode_checked(mode, *checked, cx)
                        });
                    })
                    .into_any_element()
            });
            content.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_3()
                    .child(div().text_sm().text_color(muted).child(description.clone()))
                    .children(options),
            )
        })
}

/// One stable element id per option, so each checkbox keeps its own focus ring
/// and `gpui-kit` can tell the rows apart.
fn mver_mode_checkbox_id(mode: SettingsMverMode) -> gpui_kit::ElementId {
    match mode {
        SettingsMverMode::Standard => "mver-mode-standard".into(),
        SettingsMverMode::Keyboard => "mver-mode-keyboard".into(),
        SettingsMverMode::Gamepad => "mver-mode-gamepad".into(),
    }
}

// The conversion-mode dialog's own state.
//
// A `.mver` model converts through more than one mode, so the import cannot
// start until the user has picked one. The state is a value the view owns rather
// than something the dialog's rendering decides, because the page has to know
// which mode is preselected before it opens the layer at all.

/// The BongoCat Mver conversion-mode dialog's own state.
///
/// The dialog only exists after inspection reported the modes a source actually
/// carries, so `available` is exactly what the checkboxes show. `checked` is
/// the user's selection and reaches a request in `available` order; the set has
/// no default of the whole list, only the single priority mode
/// [`default_checked_mver_mode`] names.
pub(crate) struct MverModeDialog {
    /// The conversions inspection reported, in the store's report order.
    pub(crate) available: Vec<SettingsMverMode>,
    /// The conversions the user has checked; always a subset of `available`.
    pub(crate) checked: BTreeSet<SettingsMverMode>,
    /// Set when the dialog is built during render; the window's dialog system
    /// owns the surface itself, and this records what the view put into it.
    pub(crate) open: bool,
}

impl MverModeDialog {
    pub(crate) fn from_available(available: Vec<SettingsMverMode>) -> Self {
        let checked = default_checked_mver_mode(&available).into_iter().collect();
        Self {
            available,
            checked,
            open: false,
        }
    }

    /// The checked modes in the order inspection reported them.
    ///
    /// The request is built from this rather than `checked` itself so the
    /// selection reads top to bottom and the checkbox order decides, not
    /// [`SettingsMverMode`]'s declaration order.
    #[cfg(test)]
    pub(crate) fn checked_in_order(&self) -> Vec<SettingsMverMode> {
        self.available
            .iter()
            .copied()
            .filter(|mode| self.checked.contains(mode))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn can_confirm(&self) -> bool {
        !self.checked.is_empty()
    }

    pub(crate) fn toggle(&mut self, mode: SettingsMverMode, checked: bool) {
        if !self.available.contains(&mode) {
            return;
        }
        if checked {
            self.checked.insert(mode);
        } else {
            self.checked.remove(&mode);
        }
    }
}

/// A render-safe copy of one Mver conversion dialog.
///
/// A dialog builder runs while `SettingsView` is borrowed for rendering, so it
/// must not read the entity back through `App`. This snapshot carries exactly the
/// state the pane needs: the options to draw, the current checks, and whether
/// confirmation is available.
#[derive(Clone)]
pub(super) struct MverDialogSnapshot {
    pub(crate) available: Vec<SettingsMverMode>,
    pub(crate) checked: BTreeSet<SettingsMverMode>,
}

impl MverDialogSnapshot {
    pub(crate) fn from_dialog(dialog: &MverModeDialog) -> Self {
        Self {
            available: dialog.available.clone(),
            checked: dialog.checked.clone(),
        }
    }

    pub(super) fn available(&self) -> &[SettingsMverMode] {
        &self.available
    }

    pub(super) fn is_checked(&self, mode: SettingsMverMode) -> bool {
        self.checked.contains(&mode)
    }

    pub(super) fn set(&mut self, mode: SettingsMverMode, checked: bool) {
        if !self.available.contains(&mode) {
            return;
        }
        if checked {
            self.checked.insert(mode);
        } else {
            self.checked.remove(&mode);
        }
    }

    pub(super) fn checked_in_order(&self) -> Vec<SettingsMverMode> {
        self.available
            .iter()
            .copied()
            .filter(|mode| self.checked.contains(mode))
            .collect()
    }

    pub(super) fn can_confirm(&self) -> bool {
        !self.checked.is_empty()
    }
}

/// The one mode checked when the conversion dialog first opens.
///
/// The order here is the user's priority, not the enum's: standard first, then
/// keyboard, then gamepad, and `None` only when the source has no convertible
/// mode at all (which inspection never reports, but the page does not assume it).
pub(crate) fn default_checked_mver_mode(
    available: &[SettingsMverMode],
) -> Option<SettingsMverMode> {
    [
        SettingsMverMode::Standard,
        SettingsMverMode::Keyboard,
        SettingsMverMode::Gamepad,
    ]
    .into_iter()
    .find(|mode| available.contains(mode))
}

/// The catalog key naming one BongoCat Mver conversion mode.
///
/// The mode keys are shared with the legacy model list, so the dialog says
/// "Keyboard mode" in the same words the card for a converted model will.
pub(crate) fn mver_mode_label_key(mode: SettingsMverMode) -> &'static str {
    match mode {
        SettingsMverMode::Standard => "models.mver.mode.standard",
        SettingsMverMode::Keyboard => "models.mver.mode.keyboard",
        SettingsMverMode::Gamepad => "models.mver.mode.gamepad",
    }
}
