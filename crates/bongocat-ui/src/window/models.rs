use super::*;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{Sizable as _, Size, StyleSized as _};
use gpui_kit::{AnyElement, Rems};

/// The narrowest column the catalog allows. The number of columns is the width
/// divided by this floor, up to [`MODEL_GRID_MAX_COLUMNS`]; the grid then
/// stretches every column equally, so every row ends at the container's edge.
pub(super) const MODEL_CARD_MIN_WIDTH: f32 = 240.0;
/// The catalog never spreads past this many columns, even on a very wide window.
pub(super) const MODEL_GRID_MAX_COLUMNS: usize = 5;
/// Horizontal chrome between the settings window and the model grid: the
/// resizable sidebar and the page and group padding the grid sits inside. Column
/// selection only needs the grid's approximate width; the final row grid still
/// stretches exactly to the space it is given.
const MODEL_GRID_WINDOW_CHROME: f32 = 284.0;
/// Height of the cover area. A cover is cropped into this box rather than
/// resized around it, so one unusual image cannot resize the whole page.
const MODEL_CARD_COVER_HEIGHT: f32 = 140.0;
/// The height a model card occupies when its summary is one line: the card's own
/// border and padding, the cover, the title row, the action row, and the gaps
/// between them.
///
/// It is not the card's height — a row is as tall as its tallest cell, and a card
/// carrying a status line is taller than this — but it is the height the import
/// card floors itself at, so the grid's first cell keeps the shape of the cells
/// beside it even when it is the only cell on its row. Every row it sums is
/// fixed, so the number is exact rather than nominal, and
/// `opening_a_models_editor_does_not_change_the_card` holds it to that.
pub(super) const MODEL_CARD_MIN_HEIGHT: f32 = 238.0;
/// The component size the title row and the field that edits it are both built
/// from.
///
/// The row takes its height from `input_h(size)` and the field from
/// `Input::with_size(size)`; that pair is the whole reason the card measures the
/// same with its editor open as without it, so the two must not be given
/// different sizes.
const MODEL_TITLE_SIZE: Size = Size::Medium;
/// The line box the title reserves for its text, in the field's own terms.
///
/// The field sets `1.25 rem` on its text and this is the same length: a row of a
/// fixed height centres whatever line box its text has, so the two faces only
/// put the name at the same place if the line box is the same one.
const MODEL_TITLE_LINE_HEIGHT: Rems = Rems(1.25);
/// How far the cover picker sits from the cover's own corner.
const MODEL_COVER_PICKER_INSET: Pixels = px(8.0);

/// How many columns the catalog uses at a usable width.
///
/// Two at the narrowest desktop window, then one more per
/// [`MODEL_CARD_MIN_WIDTH`]
/// of room until the cap. Keeping the count derived from the same width the
/// column floor came from means the columns stay roughly square-necked: they do
/// not balloon on a wide window, and they do not fall below the model card's
/// former 240px.
pub(super) fn model_grid_columns(width: Pixels) -> usize {
    let fit = (f32::from(width) / MODEL_CARD_MIN_WIDTH).floor().max(2.0);
    (fit as usize).clamp(2, MODEL_GRID_MAX_COLUMNS)
}

/// Select the column count from the settings window's available width.
pub(super) fn model_grid_columns_for_window(width: Pixels) -> usize {
    model_grid_columns(px((f32::from(width) - MODEL_GRID_WINDOW_CHROME).max(0.0)))
}

/// The scrolling catalog body. The window width picks a column count, then the
/// cells are split into rows here. Every row keeps that full column template so
/// a short final row leaves its unused columns empty instead of stretching one
/// card across the whole width.
pub(super) fn model_grid(
    columns: usize,
    children: impl IntoIterator<Item = AnyElement>,
) -> Stateful<Div> {
    let columns = columns.clamp(2, MODEL_GRID_MAX_COLUMNS);
    let mut children = children.into_iter();
    let mut rows = Vec::new();
    loop {
        let row = children.by_ref().take(columns).collect::<Vec<_>>();
        if row.is_empty() {
            break;
        }
        rows.push(
            div()
                .w_full()
                .grid()
                .grid_cols(u16::try_from(columns).expect("the column cap fits in u16"))
                .gap_3()
                .children(row)
                .into_any_element(),
        );
    }

    div()
        .id("model-catalog")
        .min_w_0()
        .min_h_0()
        .flex_1()
        .overflow_y_scroll()
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_3()
                .pb_3()
                .children(rows),
        )
}

pub(super) fn content(
    view: &mut SettingsView,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    tokens: Tokens,
) -> Stateful<Div> {
    let language = snapshot.map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
        snapshot.resolved_language
    });
    // One gate predicate for every page (`SettingsView::editing_blocked`,
    // ADR-0053): structural blocking only, never the transient in-flight
    // `pending` flag — gating on it disabled and re-enabled every button on
    // the page, and each activate, reveal or delete read as the page
    // refreshing. Re-entrancy is guarded by the command methods in
    // `model_actions.rs` instead; the page just no longer flickers while a
    // command waits.
    let model_commands_blocked = view.editing_blocked(snapshot);
    // A model the running import just installed is withheld until its cover
    // capture reports back, so the grid only ever shows the finished card: the
    // alternative is a card that appears with the source package's placeholder
    // picture and swaps it a moment later.
    let model_entries = snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .model_catalog
                .entries
                .iter()
                .filter(|entry| {
                    !view
                        .pending_model_reveal
                        .contains(&ModelRowKey::new(entry.origin, &entry.id))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let active_model = snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.active_model.as_ref());
    view.sync_model_row_focus(&model_entries, active_model, model_commands_blocked, cx);
    let catalog_error = snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.model_catalog.error.is_some());
    // An unreadable catalog is an error, and an error on this page is a
    // notification. It is pushed on the transition into the failure so a
    // catalog that stays unreadable does not repeat the same message on every
    // snapshot; the grid's placeholder stays neutral for the same reason.
    if catalog_error && !view.model_catalog_error_reported {
        view.model_catalog_error_reported = true;
        window.push_notification(
            Notification::new()
                .id::<ModelCatalogErrorNotification>()
                .message(bongocat_i18n::text(
                    language.catalog_locale(),
                    "models.catalog.load_failed",
                ))
                .with_type(NotificationType::Error),
            cx,
        );
    } else if !catalog_error {
        view.model_catalog_error_reported = false;
    }
    let model_cards = model_entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let model = SettingsModelKey {
                id: entry.id.clone(),
                origin: entry.origin,
            };
            let actions = model_row_actions(&entry, active_model, model_commands_blocked);
            let confirming_delete = view.model_delete_confirmation.as_ref() == Some(&model);
            // The card is the same card whether or not it is open for editing:
            // the title row is one box and the action row is one box in both
            // faces, and the cover picker is drawn over the cover instead of in
            // a row of its own. A cell that grew here would drag its whole grid
            // row with it and shift every card below, so nothing the editor adds
            // is allowed to take up room the catalog did not already take.
            let editing = view
                .model_edit
                .as_ref()
                .filter(|draft| draft.model == model);
            let focus = view
                .model_row_focus
                .get(&ModelRowKey::new(entry.origin, &entry.id))
                .expect("model card focus is synchronized")
                .clone();
            let tab_index = 40_isize.saturating_add(
                isize::try_from(index)
                    .unwrap_or(isize::MAX / 5)
                    .saturating_mul(5),
            );
            let action_tabs = model_row_action_tab_indices(tab_index);
            // The status line belongs to the card, and the confirmation is a
            // surface anchored to the delete control, so the diagnostic a broken
            // package carries stays visible while the question is on screen — and
            // while the card is being edited, where dropping it would make the
            // editor's card shorter than the catalog's.
            let status = model_availability_status(&entry, language);
            let activate_label = if actions.active {
                bongocat_i18n::text(language.catalog_locale(), "models.identity.status.active")
            } else if matches!(
                &entry.availability,
                SettingsModelAvailability::Invalid { .. }
            ) {
                bongocat_i18n::text(
                    language.catalog_locale(),
                    "models.identity.status.unavailable",
                )
            } else {
                bongocat_i18n::text(language.catalog_locale(), "models.actions.activate")
            };
            let cover = editing
                .and_then(|draft| draft.cover.clone())
                .or_else(|| entry.cover.clone());
            // The card is observed like the import card is, so a test can read the
            // two cells' geometry: the grid's whole job is to make them match.
            let card = div()
                .id(("model-card", index))
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .rounded_lg()
                .border_1()
                .border_color(if actions.active {
                    tokens.accent
                } else {
                    tokens.border
                })
                .bg(tokens.canvas)
                .child(model_card_cover(
                    cover, editing, window, cx, language, tokens,
                ))
                .child(model_card_identity(
                    &entry,
                    actions.active,
                    status,
                    editing,
                    index,
                    cx,
                    tokens,
                ));
            match editing {
                Some(draft) => card
                    .child(edit_model_card_actions(draft, window, cx, language, tokens))
                    .test_support(),
                None => card
                    .child(model_card_actions(
                        window,
                        cx,
                        index,
                        &model,
                        activate_label,
                        actions,
                        confirming_delete,
                        action_tabs,
                        &focus,
                        language,
                        tokens,
                    ))
                    .test_support(),
            }
        })
        .collect::<Vec<_>>();
    // The page shell renders the title, and the only place progress is drawn is
    // the import card itself — buttons carry their own disabled/cancel state and
    // errors go through notifications — so the content is just the grid, and
    // nothing outside it grows or shrinks as a command comes and goes.
    div()
        .min_w_0()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .text_color(tokens.text)
        .id("models-content")
        .child(model_grid(
            model_grid_columns_for_window(window.viewport_size().width),
            {
                let mut grid_children = Vec::with_capacity(model_cards.len() + 1);
                grid_children.push(model_import_card(view, cx, language).into_any_element());
                grid_children.extend(model_cards.into_iter().map(IntoElement::into_any_element));
                grid_children
            },
        ))
}

/// The cover area of a card: the image when the package ships one, and an
/// explicit placeholder when it does not. A package without a cover is an
/// ordinary package, so the placeholder is neutral rather than a warning.
///
/// While the card is being edited the picker joins the cover as a child of this
/// box, laid over the corner, rather than as a row under it — see
/// [`model_cover_picker`].
fn model_card_cover(
    cover: Option<PathBuf>,
    editing: Option<&ModelEditDraft>,
    window: &Window,
    cx: &mut Context<SettingsView>,
    language: SettingsLanguage,
    tokens: Tokens,
) -> Div {
    let frame = div()
        .relative()
        .flex_none()
        .w_full()
        .h(px(MODEL_CARD_COVER_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(tokens.border)
        .bg(tokens.canvas);
    let frame = frame.child(match cover {
        Some(cover) => img(cover)
            .w_full()
            .h_full()
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => div()
            .text_sm()
            .text_color(tokens.muted)
            .child(bongocat_i18n::text(
                language.catalog_locale(),
                "models.card.cover_missing",
            ))
            .into_any_element(),
    });
    match editing {
        Some(draft) => frame
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded_md()
                    .bg(Hsla::black().opacity(0.45))
                    .id("model-cover-edit-mask"),
            )
            .child(model_cover_picker(draft, window, cx, language, tokens)),
        None => frame,
    }
}

/// The control that replaces a card's cover.
///
/// It is drawn on the cover it acts on, in the corner, so choosing a cover is
/// one gesture on the thing being changed and costs the card no height — the
/// box it sits in is already there. The button sits over a black translucent mask
/// because the component lightens its surface on hover; the mask keeps the
/// label readable over whatever artwork the package shipped.
fn model_cover_picker(
    draft: &ModelEditDraft,
    window: &Window,
    cx: &mut Context<SettingsView>,
    language: SettingsLanguage,
    tokens: Tokens,
) -> impl IntoElement {
    let cover_focus = draft.cover_focus.clone();
    let cover_key_focus = cover_focus.clone();
    command_button(
        bongocat_i18n::text(language.catalog_locale(), "models.edit.cover.label"),
        &cover_focus,
        MODEL_EDIT_COVER_TAB_INDEX,
        window,
        tokens,
        draft.picking,
    )
    .absolute()
    .right(MODEL_COVER_PICKER_INSET)
    .bottom(MODEL_COVER_PICKER_INSET)
    .id("choose-model-cover")
    .on_click(cx.listener(move |view, _, window, cx| {
        if !view.model_edit.as_ref().is_some_and(|draft| draft.picking) {
            window.focus(&cover_focus, cx);
            view.choose_model_cover(cx);
        }
    }))
    .on_key_down(cx.listener(move |view, event, window, cx| {
        if is_activation_key(event) && !view.model_edit.as_ref().is_some_and(|draft| draft.picking)
        {
            cx.stop_propagation();
            window.focus(&cover_key_focus, cx);
            view.choose_model_cover(cx);
        }
    }))
    .test_support()
}

/// The card's identity block: its title row and the status line under it.
///
/// The block is the same two rows in both faces of the card. The title row is
/// built here, once, whatever the card is doing — the box, its height and the
/// line inside it are named in one place — and only its child changes, from the
/// name to the field that edits it. A row that came and went would make the
/// card, and with it its whole grid row, change height the moment the editor
/// opened.
fn model_card_identity(
    entry: &SettingsModelEntry,
    active: bool,
    status: Option<SharedString>,
    editing: Option<&ModelEditDraft>,
    index: usize,
    cx: &mut Context<SettingsView>,
    tokens: Tokens,
) -> Div {
    let title_row = div()
        .id(("model-card-title", index))
        .w_full()
        .flex()
        .items_center()
        .input_h(MODEL_TITLE_SIZE)
        .line_height(MODEL_TITLE_LINE_HEIGHT);
    let mut identity = div()
        .flex()
        .flex_col()
        .gap_1()
        .min_w_0()
        .w_full()
        .child(match editing {
            Some(draft) => title_row
                .child(model_card_title_field(draft, cx))
                .test_support(),
            None => title_row
                .text_color(if active { tokens.accent } else { tokens.text })
                .child(model_card_title(entry))
                .test_support(),
        });
    // The status line only exists to carry a diagnostic or a delete
    // confirmation; a healthy card shows nothing but its title.
    if let Some(status) = status {
        identity = identity.child(div().text_sm().text_color(tokens.muted).child(status));
    }
    identity
}

/// The name a card shows while it is not being edited.
fn model_card_title(entry: &SettingsModelEntry) -> Div {
    div()
        .min_w_0()
        .w_full()
        .truncate()
        .child(entry.title.clone())
}

/// The card's title as the field that edits it: the same row, the same height
/// and the same name, drawn as the input box it is.
///
/// The field is the component library's own input, in its own appearance — the
/// border, the surface behind it and the focus ring are what tell the user the
/// name is editable, and what tells them where to type. Nothing about it is
/// hand-styled except what the two faces have to agree on: its height comes from
/// [`MODEL_TITLE_SIZE`], the same value the row around it is dimensioned from,
/// and its line box from [`MODEL_TITLE_LINE_HEIGHT`], so the row keeps its height
/// and only the box appears. Its text keeps the size of the name it replaces, so
/// the row does not even change its type when the editor opens.
///
/// The field's own padding is what insets the name from the box it now sits in.
/// The row it replaces has none, so the name slides right by that padding as the
/// box appears; giving the row the padding instead would indent every card's
/// title from the cover's left edge for the sake of a state the card is in only
/// while it is being edited.
///
/// The text of an editable field is coloured by the input's own editor style
/// rather than by a row style, so a card whose title is accented — the active
/// model — shows its name in the ordinary text colour while the editor is open.
fn model_card_title_field(
    draft: &ModelEditDraft,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    div()
        .id("model-edit-title-input")
        .key_context("SettingsModelEditTitle")
        .track_focus(&draft.input_focus)
        .tab_index(MODEL_EDIT_TITLE_TAB_INDEX)
        .w_full()
        .on_click(cx.listener(move |view, _, window, cx| {
            if let Some(draft) = view.model_edit.as_ref() {
                let focus = draft.input_focus.clone();
                window.focus(&focus, cx);
            }
        }))
        .child(
            Input::new(&draft.input)
                .with_size(MODEL_TITLE_SIZE)
                .line_height(MODEL_TITLE_LINE_HEIGHT)
                .text_base(),
        )
        // Observed so a test can read the box the field really occupies: the
        // card keeping its height is only half of the claim, the other half
        // being that the field is the title row rather than something inside it.
        .test_support()
}

/// The action row of a card.
///
/// Activating is the only labelled control: it is the page's core action, and
/// the icon actions are the ones that leave the page. Deleting is the one that
/// cannot be undone, so its control owns a confirmation surface rather than
/// acting on the first press — the row keeps all four controls either way, and
/// the question appears next to the control that asked it.
#[allow(clippy::too_many_arguments)]
fn model_card_actions(
    window: &Window,
    cx: &mut Context<SettingsView>,
    index: usize,
    model: &SettingsModelKey,
    activate_label: &'static str,
    actions: ModelRowActions,
    confirming_delete: bool,
    action_tabs: ModelRowActionTabIndices,
    focus: &ModelRowFocus,
    language: SettingsLanguage,
    tokens: Tokens,
) -> Div {
    let activate_model = model.clone();
    let activate_key_model = model.clone();
    // The grid stretches a card to its row's tallest cell, so the controls are
    // pushed to the card's bottom edge: every card on a row then lines its
    // buttons up, whatever its own summary turned out to be worth.
    let mut row = div().flex().items_center().gap_1().mt_auto().child(
        command_button(
            activate_label,
            &focus.activate,
            action_tabs.activate,
            window,
            tokens,
            !actions.can_activate,
        )
        .flex_1()
        .id(("activate-model", index))
        .on_click(cx.listener(move |view, _, window, cx| {
            view.run_model_row_action(ModelRowAction::Activate, activate_model.clone(), window, cx);
        }))
        .on_key_down(cx.listener(move |view, event, window, cx| {
            if is_activation_key(event) {
                cx.stop_propagation();
                view.run_model_row_action(
                    ModelRowAction::Activate,
                    activate_key_model.clone(),
                    window,
                    cx,
                );
            }
        })),
    );
    let location_model = model.clone();
    let location_key_model = model.clone();
    row = row.child(
        icon_command_button(
            "open-model-location-control",
            bongocat_i18n::text(language.catalog_locale(), "models.actions.open_location"),
            IconName::FolderOpen,
            &focus.open_location,
            action_tabs.open_location,
            !actions.can_open_location,
        )
        .id(("open-model-location", index))
        .on_click(cx.listener(move |view, _, window, cx| {
            view.run_model_row_action(
                ModelRowAction::OpenLocation,
                location_model.clone(),
                window,
                cx,
            );
        }))
        .on_key_down(cx.listener(move |view, event, window, cx| {
            if is_activation_key(event) {
                cx.stop_propagation();
                view.run_model_row_action(
                    ModelRowAction::OpenLocation,
                    location_key_model.clone(),
                    window,
                    cx,
                );
            }
        })),
    );
    let edit_model = model.clone();
    let edit_key_model = model.clone();
    row = row.child(
        icon_command_button(
            "edit-model-control",
            bongocat_i18n::text(language.catalog_locale(), "models.actions.edit"),
            gpui_kit::assets::IconName::SquarePen,
            &focus.edit,
            action_tabs.edit,
            !actions.can_edit,
        )
        .id(("edit-model", index))
        .test_support()
        .on_click(cx.listener(move |view, _, window, cx| {
            view.run_model_row_action(ModelRowAction::Edit, edit_model.clone(), window, cx);
        }))
        .on_key_down(cx.listener(move |view, event, window, cx| {
            if is_activation_key(event) {
                cx.stop_propagation();
                view.run_model_row_action(ModelRowAction::Edit, edit_key_model.clone(), window, cx);
            }
        })),
    );
    let confirm_model = model.clone();
    let open_model = model.clone();
    let close_model = model.clone();
    let delete_label = bongocat_i18n::text(language.catalog_locale(), "models.actions.delete");
    // Presets have no delete affordance; only user-installed models render the
    // confirmation trigger (and only those can still be disabled during import).
    if model.origin == SettingsModelOrigin::Installed {
        row = row.child(
            PopConfirm::new(
                ("delete-model-confirmation", index),
                bongocat_i18n::text(language.catalog_locale(), "models.delete_confirmation"),
            )
            // The control sits at the trailing edge of the card, so the surface
            // grows back over the card instead of past the window edge.
            .anchor(Anchor::TopRight)
            .icon(gpui_kit::assets::IconName::TriangleAlert, tokens.danger)
            .confirm_label(bongocat_i18n::text(
                language.catalog_locale(),
                "actions.confirm",
            ))
            .cancel_label(bongocat_i18n::text(
                language.catalog_locale(),
                "actions.cancel",
            ))
            .trigger(
                icon_command_button(
                    "delete-model-control",
                    delete_label,
                    gpui_kit::assets::IconName::Trash,
                    &focus.delete,
                    action_tabs.delete,
                    !actions.can_delete,
                )
                // The wrapper owns the queryable id; the inner control gets a
                // derived one so the two registrations cannot be ambiguous.
                .id(("delete-model", index))
                .test_support(),
            )
            .open(confirming_delete)
            // The surface reports every transition — the control's press or
            // Enter, Escape, a press outside — and this is where the page decides
            // which of them are its business. The page's own `open` value is not
            // echoed back, so writing the state here cannot loop.
            .on_open_change(cx.listener(move |view, open: &bool, _, cx| {
                if *open {
                    view.request_model_delete(open_model.clone(), cx);
                } else {
                    view.cancel_model_delete(&close_model, cx);
                }
            }))
            .on_confirm({
                let view = cx.entity().downgrade();
                move |window, cx| {
                    let _ = view.update(cx, |view, cx| {
                        view.run_model_row_action(
                            ModelRowAction::Delete,
                            confirm_model.clone(),
                            window,
                            cx,
                        );
                    });
                }
            }),
        );
    }
    row
}

/// The row a card shows while it is being edited: the save/cancel pair, in the
/// place of the activate/edit/delete row it shows the rest of the time.
///
/// Editing stays inside the card it belongs to, so the model under edit is
/// always the model the card already shows and there is no separate edit view
/// that could disagree with the catalog it was opened from. Committing and
/// abandoning are the only two actions, so they are the only two controls: the
/// cover and the title each carry their own affordance now.
fn edit_model_card_actions(
    draft: &ModelEditDraft,
    window: &Window,
    cx: &mut Context<SettingsView>,
    language: SettingsLanguage,
    tokens: Tokens,
) -> Div {
    let save_focus = draft.save_focus.clone();
    let save_key_focus = save_focus.clone();
    let cancel_focus = draft.cancel_focus.clone();
    let cancel_key_focus = cancel_focus.clone();
    div()
        .flex()
        .items_center()
        .gap_1()
        // The grid stretches a card to its row's tallest cell, so the commit row
        // is pushed to the card's bottom edge exactly as the action row it
        // replaces is: the two faces of a stretched card line their rows up.
        .mt_auto()
        .child(
            command_button(
                bongocat_i18n::text(language.catalog_locale(), "models.actions.save"),
                &save_focus,
                MODEL_EDIT_SAVE_TAB_INDEX,
                window,
                tokens,
                false,
            )
            .flex_1()
            .id("save-model-edit")
            .on_click(cx.listener(move |view, _, window, cx| {
                window.focus(&save_focus, cx);
                view.save_model_edit(cx);
            }))
            .on_key_down(cx.listener(move |view, event, window, cx| {
                if is_activation_key(event) {
                    cx.stop_propagation();
                    window.focus(&save_key_focus, cx);
                    view.save_model_edit(cx);
                }
            })),
        )
        .child(
            command_button(
                bongocat_i18n::text(language.catalog_locale(), "actions.cancel"),
                &cancel_focus,
                MODEL_EDIT_CANCEL_TAB_INDEX,
                window,
                tokens,
                false,
            )
            .id("cancel-model-edit")
            .on_click(cx.listener(move |view, _, window, cx| {
                window.focus(&cancel_focus, cx);
                view.cancel_model_edit(cx);
            }))
            .on_key_down(cx.listener(move |view, event, window, cx| {
                if is_activation_key(event) {
                    cx.stop_propagation();
                    window.focus(&cancel_key_focus, cx);
                    view.cancel_model_edit(cx);
                }
            })),
        )
}

/// The import entry, as the first cell of the grid.
///
/// Importing is the only way a model reaches the page, so one card owns the
/// whole flow: a press opens the folder picker, the selection starts the run on
/// the spot, and the same card reports the step until the new model is ready to
/// be shown.
fn model_import_card(
    view: &SettingsView,
    cx: &mut Context<SettingsView>,
    language: SettingsLanguage,
) -> ModelImportCard {
    let locale = language.catalog_locale();
    // A native dialog or the import run itself leaves the card visible but
    // inert; during the run it shows progress instead of the prompt. An
    // in-flight page command does not touch the card: `pending` never feeds a
    // visual gate (ADR-0053), and `choose_model_source` refuses the press.
    let ready_for_input =
        !view.model_import.is_running() && !view.model_import.is_source_surface_open();
    let mut card = ModelImportCard::new(
        "model-import-card",
        bongocat_i18n::text(locale, "models.import.title"),
    )
    .hint(bongocat_i18n::text(locale, "models.import.hint"))
    .step(import_card_step(&view.model_import, language))
    .interactive(ready_for_input)
    .track_focus(Some(view.import_card_focus.clone()))
    .tab_index(20)
    .on_open({
        let view = cx.entity().downgrade();
        move |_, cx| {
            let _ = view.update(cx, |view, cx| view.choose_model_source(cx));
        }
    });
    if view.model_import.shows_cancel() {
        card = card.cancel(
            bongocat_i18n::text(locale, "actions.cancel"),
            !view.model_import.is_cancellable(),
            {
                let view = cx.entity().downgrade();
                move |_, cx| {
                    let _ = view.update(cx, |view, cx| view.cancel_model_import(cx));
                }
            },
        );
    }
    card
}

/// The step the import card shows, or `None` while the card is the upload prompt.
///
/// The phases are named rather than counted, so a wait with two halves says which
/// half it is in — but only the half that is running comes back. The capture
/// replaces the import line instead of being appended under it: the card is one
/// grid cell the size of a model card, and a growing list of finished steps would
/// turn a two-phase wait into a log that cell cannot hold.
pub(super) fn import_card_step(
    draft: &ModelImportDraft,
    language: SettingsLanguage,
) -> Option<SharedString> {
    let step = |key: &str| Some(bongocat_i18n::text(language.catalog_locale(), key).into());
    match &draft.state {
        ModelImportState::Idle => None,
        // The native dialog has not returned a path yet; once it does, the
        // inspection state below uses the shared folder-reading step.
        ModelImportState::Picking => step("models.import.step.choosing"),
        // Both source entrances converge on the same folder-reading step after
        // the user has provided a path. The picker adapter has already done its
        // filesystem check on its worker; the drop path performs the same check
        // in the background executor before entering the shared inspection.
        ModelImportState::ValidatingDrop | ModelImportState::Inspecting => {
            step("models.import.step.validating_source")
        }
        ModelImportState::Starting { .. } | ModelImportState::Running(_) => {
            step("models.import.step.importing")
        }
        ModelImportState::Capturing => step("models.import.step.capturing"),
    }
}

#[cfg(test)]
pub(super) fn empty_model_catalog_status(
    catalog: Option<&crate::SettingsModelCatalog>,
    language: SettingsLanguage,
) -> &'static str {
    match catalog {
        None => bongocat_i18n::text(language.catalog_locale(), "models.catalog.loading"),
        Some(catalog) if catalog.error.is_some() => {
            bongocat_i18n::text(language.catalog_locale(), "models.catalog.unavailable")
        }
        Some(_) => bongocat_i18n::text(language.catalog_locale(), "models.catalog.empty"),
    }
}
