use super::*;

/// Which set of shortcut bindings one group of the page renders.
///
/// The two sets used to be tabs. They are titled groups now: `SettingPage`
/// cannot host child pages, but the settings component renders every titled
/// group of a page that has more than one group as a second-level sidebar
/// entry, so the sidebar lists both scopes under "Shortcuts" while the body
/// renders them one after the other.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ShortcutScope {
    Window,
    Model,
}

impl ShortcutScope {
    /// Whether this scope's bindings may reach the platform table.
    ///
    /// Both gates are configured positively and rendered as "enable …"
    /// switches, so the switch, the accessibility node and the command it sends
    /// all read the same configuration field — there is no inversion to keep in
    /// step anywhere.
    pub(super) fn is_enabled(self, snapshot: &SettingsSnapshot) -> bool {
        match self {
            Self::Window => snapshot.command_shortcuts_enabled,
            Self::Model => snapshot.behavior_shortcuts_enabled,
        }
    }

    /// The label of this scope's gate.
    ///
    /// The visible row and the accessibility node read the same string, so they
    /// cannot drift apart, and the catalog scans find the literal because it is
    /// written out in full rather than assembled from a suffix. The row carries
    /// no description: "enable …" already says what the switch does, and a
    /// second line would only be the label in other words.
    pub(super) fn gate_label(self, language: SettingsLanguage) -> &'static str {
        match self {
            Self::Window => bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.switches.enable_window_shortcuts.label",
            ),
            Self::Model => bongocat_i18n::text(
                language.catalog_locale(),
                "shortcuts.switches.enable_model_shortcuts.label",
            ),
        }
    }

    /// The switch that turns this scope's bindings off and on again.
    ///
    /// It is the first row of the scope's group, above the bindings it gates,
    /// because the gate used to live on the Interaction page and a list of
    /// chords that silently does nothing is the state that page produced.
    /// Switching it back on never needs the chords to be recorded again: no
    /// gate rewrites the bindings, they only stay out of the platform table.
    fn gate_item(
        self,
        language: SettingsLanguage,
        view: Entity<SettingsView>,
        disabled: bool,
    ) -> SettingItem {
        SettingItem::new(
            self.gate_label(language),
            SettingField::switch(
                {
                    let view = view.clone();
                    move |app| {
                        view.read(app)
                            .snapshot
                            .as_ref()
                            .is_some_and(|snapshot| self.is_enabled(snapshot))
                    }
                },
                {
                    let view = view.clone();
                    move |enabled, app| {
                        view.update(app, |view, cx| match self {
                            Self::Window => view.set_command_shortcuts_enabled(enabled, cx),
                            Self::Model => view.set_behavior_shortcuts_enabled(enabled, cx),
                        });
                    }
                },
            ),
        )
        .disabled(disabled)
    }

    /// The name of this scope, used as its group heading and its sidebar entry.
    pub(super) fn title(self, language: SettingsLanguage) -> &'static str {
        bongocat_i18n::text(
            language.catalog_locale(),
            match self {
                Self::Window => "shortcuts.scopes.window",
                Self::Model => "shortcuts.scopes.model",
            },
        )
    }

    /// The rows this scope binds.
    ///
    /// The window scope always lists every application command, so only the
    /// model scope can come back empty — when the active model declares no
    /// behavior.
    pub(super) fn rows(
        self,
        shortcuts: &SettingsShortcuts,
        active_model: Option<&SettingsModelKey>,
        entries: &[SettingsModelEntry],
    ) -> Vec<ShortcutRow> {
        match self {
            Self::Window => window_shortcut_rows(shortcuts),
            Self::Model => shortcut_behavior_rows(shortcuts, active_model, entries),
        }
    }

    /// Where this scope's rows start in the page-wide row order.
    ///
    /// Both scopes are one page, so their capture and clear controls share one
    /// keyboard tab order and the accessibility nodes are numbered from the
    /// same combined list of rows.
    pub(super) fn row_index_offset(self, shortcuts: &SettingsShortcuts) -> usize {
        match self {
            Self::Window => 0,
            Self::Model => window_shortcut_rows(shortcuts).len(),
        }
    }

    /// The message to show while this scope has no rows at all.
    pub(super) fn empty_message(self, language: SettingsLanguage) -> Option<&'static str> {
        match self {
            Self::Window => None,
            Self::Model => Some(bongocat_i18n::text(
                language.catalog_locale(),
                "models.behaviors.empty",
            )),
        }
    }
}

/// One scope of the shortcut page as a titled group.
pub(super) fn group(
    scope: ShortcutScope,
    language: SettingsLanguage,
    view: Entity<SettingsView>,
    disabled: bool,
) -> SettingGroup {
    // The group heading names the scope in the body and in the sidebar, so the
    // custom item below carries no label of its own: a `SettingItem` with a
    // title would print the scope name a second time, directly under the
    // heading. Search matches an item by its title or keywords, and a custom
    // element has no title, so the page and scope names are passed as keywords
    // to keep the page reachable through the sidebar's search box.
    let keywords = [
        bongocat_i18n::text(language.catalog_locale(), "navigation.shortcuts.title").to_owned(),
        scope.title(language).to_owned(),
    ];
    SettingGroup::new()
        .title(scope.title(language))
        .item(scope.gate_item(language, view.clone(), disabled))
        .item(
            SettingItem::render({
                let view = view.clone();
                move |_: &RenderOptions, window: &mut Window, app: &mut App| {
                    let snapshot = view.read(app).snapshot.clone();
                    let tokens = Tokens::from_theme(app);
                    view.update(app, move |view, cx| {
                        view.page = SettingsPage::Shortcuts;
                        content(view, window, cx, snapshot.as_ref(), scope, disabled, tokens)
                    })
                    .into_any_element()
                }
            })
            .keywords(keywords),
        )
}

pub(super) fn content(
    view: &SettingsView,
    window: &Window,
    cx: &mut Context<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    scope: ShortcutScope,
    disabled: bool,
    tokens: Tokens,
) -> Stateful<Div> {
    let language = snapshot.map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
        snapshot.resolved_language
    });
    div()
        .min_w_0()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .text_color(tokens.text)
        .id(match scope {
            ShortcutScope::Window => "window-shortcuts-content",
            ShortcutScope::Model => "model-shortcuts-content",
        })
        .when_some(snapshot, |content, snapshot| {
            let rows = scope.rows(
                &snapshot.shortcuts,
                snapshot.active_model.as_ref(),
                &snapshot.model_catalog.entries,
            );
            let row_index_offset = scope.row_index_offset(&snapshot.shortcuts);
            let empty_message = scope.empty_message(language).filter(|_| rows.is_empty());
            content
                .when_some(empty_message, |content, message| {
                    content.child(div().text_sm().text_color(tokens.muted).child(message))
                })
                .children(rows.into_iter().enumerate().map(|(index, row)| {
                    shortcut_row(
                        view,
                        window,
                        cx,
                        language,
                        disabled,
                        tokens,
                        scope,
                        row,
                        row_index_offset + index,
                    )
                }))
        })
}

#[allow(clippy::too_many_arguments)]
fn shortcut_row(
    view: &SettingsView,
    window: &Window,
    cx: &mut Context<SettingsView>,
    language: SettingsLanguage,
    disabled: bool,
    tokens: Tokens,
    scope: ShortcutScope,
    row: ShortcutRow,
    row_index: usize,
) -> Div {
    let target_name = row.name(language);
    let target = row.target;
    let capture = view
        .shortcut_capture
        .as_ref()
        .filter(|capture| capture.target == target);
    let capturing = capture.is_some();
    let focus = view
        .shortcut_row_focus
        .get(&target)
        .expect("shortcut row focus is synchronized")
        .clone();
    let clear_focus = view
        .shortcut_clear_focus
        .get(&target)
        .expect("shortcut clear focus is synchronized")
        .clone();
    let clear_key_focus = clear_focus.clone();
    let keyboard_target = target.clone();
    let clear_target = target.clone();
    let clear_key_target = target.clone();
    // The two scopes are one page and their element ids live in one namespace,
    // so the scope prefix keeps the ids apart without folding the row index
    // (which is already unique across the page) into the name.
    let (capture_id, clear_id) = match scope {
        ShortcutScope::Window => (
            ("capture-window-shortcut", row_index),
            ("clear-window-shortcut", row_index),
        ),
        ShortcutScope::Model => (
            ("capture-model-shortcut", row_index),
            ("clear-model-shortcut", row_index),
        ),
    };
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .text_sm()
        .child(div().min_w_0().flex_1().child(target_name))
        .child(
            div()
                .key_context("SettingsControl")
                .track_focus(&focus)
                .tab_index(shortcut_capture_tab_index(row_index))
                .id(capture_id)
                .h(px(32.))
                .min_w(px(180.))
                .flex()
                .items_center()
                .justify_center()
                .px_3()
                .border_1()
                .border_color(if capturing {
                    tokens.accent
                } else {
                    tokens.border
                })
                .rounded_md()
                .when(capturing, |this| this.focus_ring_style(window, cx))
                .cursor_pointer()
                .text_color(if capturing || row.shortcut.is_some() {
                    tokens.text
                } else {
                    tokens.muted
                })
                .when(disabled, |this| this.opacity(0.5).cursor_default())
                .child(if let Some(capture) = capture {
                    shortcut_capture_preview(&capture.modifiers, &capture.keys)
                        .map(|shortcut| shortcut_display(&shortcut))
                        .unwrap_or_else(|| {
                            bongocat_i18n::text(
                                language.catalog_locale(),
                                "shortcuts.capture.press_to_record",
                            )
                            .to_owned()
                        })
                } else if let Some(shortcut) = row.shortcut.clone() {
                    shortcut_display(&shortcut)
                } else {
                    bongocat_i18n::text(
                        language.catalog_locale(),
                        "shortcuts.capture.click_to_record",
                    )
                    .to_owned()
                })
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.begin_shortcut_capture(target.clone(), window, cx);
                }))
                .when(capturing, |this| {
                    this.on_mouse_down_out(cx.listener(|view, _, window, cx| {
                        view.cancel_shortcut_capture(cx);
                        window.blur(cx);
                    }))
                })
                .on_key_down(cx.listener(move |view, event, window, cx| {
                    if view.shortcut_capture.is_none() && is_activation_key(event) {
                        cx.stop_propagation();
                        view.begin_shortcut_capture(keyboard_target.clone(), window, cx);
                    }
                })),
        )
        .when(row.shortcut.is_some(), |actions| {
            actions.child(
                command_button(
                    bongocat_i18n::text(language.catalog_locale(), "shortcuts.actions.clear"),
                    &clear_focus,
                    shortcut_clear_tab_index(row_index),
                    window,
                    tokens,
                    disabled,
                )
                .id(clear_id)
                .on_click(cx.listener(move |view, _, window, cx| {
                    window.focus(&clear_focus, cx);
                    view.clear_shortcut(clear_target.clone(), cx);
                }))
                .on_key_down(cx.listener(move |view, event, window, cx| {
                    if is_activation_key(event) {
                        cx.stop_propagation();
                        window.focus(&clear_key_focus, cx);
                        view.clear_shortcut(clear_key_target.clone(), cx);
                    }
                })),
            )
        })
}
