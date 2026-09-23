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
    let confirm_label = text("models.import.mver_dialog.confirm");
    let cancel_label = text("models.import.mver_dialog.cancel");

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
