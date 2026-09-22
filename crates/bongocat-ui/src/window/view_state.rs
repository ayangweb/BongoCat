use super::*;

impl SettingsView {
    /// The display language for the frame being rendered.
    ///
    /// The snapshot answers as soon as it exists; before that the window renders the
    /// language it was opened with, so the first frame is already the user's. See
    /// [`SettingsWindowSeed`].
    pub(super) fn display_language(&self) -> SettingsLanguage {
        self.snapshot
            .as_ref()
            .map_or(self.seed.language, |snapshot| snapshot.resolved_language)
    }

    /// The appearance for the frame being rendered, with the same rule as
    /// [`Self::display_language`].
    pub(super) fn display_appearance_theme(&self) -> SettingsTheme {
        self.snapshot
            .as_ref()
            .map_or(self.seed.appearance_theme, |snapshot| {
                snapshot.appearance_theme
            })
    }

    pub(super) fn sync_component_theme(
        &mut self,
        theme: SettingsTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.applied_theme == Some(theme) {
            return;
        }
        apply_component_theme(theme, window, cx);
        self.applied_theme = Some(theme);
    }

    pub(super) fn sync_component_inputs(
        &mut self,
        snapshot: &SettingsSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.set_window_title(bongocat_i18n::text(
            snapshot.resolved_language.catalog_locale(),
            "navigation.settings.title",
        ));
        self.syncing_component_inputs = true;
        self.model_id_input.update(cx, |input, cx| {
            input.set_placeholder(
                bongocat_i18n::text(
                    snapshot.resolved_language.catalog_locale(),
                    "models.identity.title",
                ),
                window,
                cx,
            );
            input.set_value(&self.model_import.title, window, cx)
        });
        self.language_select.update(cx, |select, cx| {
            select.set_items(
                SearchableVec::new(
                    SettingsLanguage::ALL
                        .into_iter()
                        .map(|language| language.display_name(snapshot.resolved_language))
                        .collect::<Vec<_>>(),
                ),
                window,
                cx,
            );
            select.set_selected_value(
                &snapshot.language.display_name(snapshot.resolved_language),
                window,
                cx,
            )
        });
        self.theme_select.update(cx, |select, cx| {
            select.set_items(
                SearchableVec::new(theme_options(snapshot.resolved_language)),
                window,
                cx,
            );
            select.set_selected_value(
                &theme_display_name(snapshot.appearance_theme, snapshot.resolved_language),
                window,
                cx,
            )
        });
        self.syncing_component_inputs = false;
    }

    pub(super) fn new(
        client: SettingsClient,
        seed: SettingsWindowSeed,
        request_quit: Rc<dyn Fn(&mut App)>,
        request_update: SettingsWindowRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Every component built here is seeded with the window's language for the same
        // reason as the frame itself: they are constructed before the first snapshot
        // exists, and a component built in the default language would be replaced —
        // visibly — by the values the first snapshot carries.
        let model_id_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(bongocat_i18n::text(
                seed.language.catalog_locale(),
                "models.identity.title",
            ))
        });
        let language_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    SettingsLanguage::ALL
                        .into_iter()
                        .map(|language| language.display_name(seed.language))
                        .collect::<Vec<_>>(),
                ),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let theme_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(theme_options(seed.language)),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        cx.subscribe(&model_id_input, |view, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                if view.syncing_component_inputs || view.model_import.is_running() {
                    return;
                }
                let value = input.read(cx).value();
                view.model_import.title = sanitize_model_title_input(&value);
                view.model_import.reset_result_state();
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(
            &language_select,
            |view, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if view.syncing_component_inputs {
                    return;
                }
                let display_language = view.display_language();
                if let SelectEvent::Confirm(Some(name)) = event
                    && let Some(language) =
                        SettingsLanguage::from_display_name(name, display_language)
                {
                    view.set_language(language, cx);
                }
            },
        )
        .detach();
        cx.subscribe(
            &theme_select,
            |view, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if view.syncing_component_inputs {
                    return;
                }
                let display_language = view.display_language();
                if let SelectEvent::Confirm(Some(name)) = event
                    && let Some(theme) = theme_from_display_name(name, display_language)
                {
                    view.set_appearance_theme(theme, cx);
                }
            },
        )
        .detach();
        Self {
            client,
            seed,
            snapshot: None,
            pending: None,
            pending_notification: None,
            page: SettingsPage::General,
            model_import: ModelImportDraft::default(),
            overlay_scale_debouncer: crate::SettingsPatchDebouncer::default(),
            overlay_scale_timer_generation: 0,
            overlay_opacity_debouncer: crate::SettingsPatchDebouncer::default(),
            overlay_opacity_timer_generation: 0,
            overlay_corner_radius_debouncer: crate::SettingsPatchDebouncer::default(),
            overlay_corner_radius_timer_generation: 0,
            overlay_hover_hide_delay_debouncer: crate::SettingsPatchDebouncer::default(),
            overlay_hover_hide_delay_timer_generation: 0,
            gamepad_dead_zone_debouncer: crate::SettingsPatchDebouncer::default(),
            gamepad_dead_zone_timer_generation: 0,
            maximum_fps_debouncer: crate::SettingsPatchDebouncer::default(),
            maximum_fps_timer_generation: 0,
            release_fallback_timeout_debouncer: crate::SettingsPatchDebouncer::default(),
            release_fallback_timeout_timer_generation: 0,
            flush_pending_requested: false,
            quit_after_flush: false,
            model_delete_confirmation: None,
            model_row_focus: BTreeMap::new(),
            model_edit: None,
            model_catalog_error_reported: false,
            shortcut_capture: None,
            shortcut_capture_blur_subscription: None,
            shortcut_row_focus: BTreeMap::new(),
            shortcut_clear_focus: BTreeMap::new(),
            window_hidden: true,
            applied_theme: None,
            language_select,
            theme_select,
            request_quit,
            request_update,
            overlay_focus: cx.focus_handle().tab_index(10).tab_stop(true),
            model_id_focus: cx.focus_handle().tab_index(20).tab_stop(true),
            choose_model_focus: cx.focus_handle().tab_index(21).tab_stop(true),
            choose_archive_focus: cx.focus_handle().tab_index(22).tab_stop(true),
            import_model_focus: cx.focus_handle().tab_index(23).tab_stop(true),
            model_id_input,
            syncing_component_inputs: false,
        }
    }
}
