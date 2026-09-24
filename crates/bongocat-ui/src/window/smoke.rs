use super::*;

impl SettingsView {
    pub fn report_service_error(&mut self, error: SettingsError, cx: &mut Context<Self>) {
        self.pending = None;
        self.pending_notification = Some(error);
        cx.notify();
    }

    pub fn snapshot_revision(&self) -> Option<u64> {
        self.snapshot.as_ref().map(|snapshot| snapshot.revision)
    }

    pub fn resolved_language_for_smoke(&self) -> Option<SettingsLanguage> {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.resolved_language)
    }

    /// The language the current frame renders with.
    ///
    /// Unlike [`Self::resolved_language_for_smoke`] this answers before the first
    /// snapshot exists, which covers the frame the user sees while the service is
    /// still building it.
    pub fn display_language_for_smoke(&self) -> SettingsLanguage {
        self.display_language()
    }

    pub fn window_hidden(&self) -> bool {
        self.window_hidden
    }

    /// Wait until the first settings frame has applied the selected theme.
    pub fn appearance_applied_for_smoke(&self) -> bool {
        self.applied_theme.is_some()
    }

    pub fn run_page_smoke(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let mut failures = Vec::new();
        if let Err(error) = self.show_general_page_for_smoke(cx) {
            failures.push(error);
        }
        if let Err(error) = self.show_split_pages_for_smoke(cx) {
            failures.push(error);
        }
        if let Err(error) = self.show_shortcuts_page_for_smoke(cx) {
            failures.push(error);
        }
        if let Err(error) = self.show_about_page_for_smoke(cx) {
            failures.push(error);
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    pub fn show_split_pages_for_smoke(&mut self, _cx: &mut Context<Self>) -> Result<(), String> {
        let language = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "split pages have not received a settings snapshot".to_owned())?
            .resolved_language;
        for title_key in [
            "navigation.overlay.title",
            "navigation.interaction.title",
            "navigation.input.title",
            "navigation.application.title",
        ] {
            if bongocat_i18n::text(language.catalog_locale(), title_key).is_empty() {
                return Err(format!("page {title_key} has no localized title"));
            }
        }
        Ok(())
    }

    pub fn show_general_page_for_smoke(&mut self, _cx: &mut Context<Self>) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "general page has not received a settings snapshot".to_owned())?;
        for key in [
            "settings.appearance.theme.label",
            "settings.appearance.language.label",
            "settings.overlay.visibility.label",
        ] {
            if bongocat_i18n::text(snapshot.resolved_language.catalog_locale(), key).is_empty() {
                return Err(format!("general page is missing localized text for {key}"));
            }
        }
        Ok(())
    }

    pub fn show_shortcuts_page_for_smoke(&mut self, _cx: &mut Context<Self>) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "shortcuts page has not received a settings snapshot".to_owned())?;
        let rows = shortcut_rows(
            &snapshot.shortcuts,
            snapshot.active_model.as_ref(),
            &snapshot.model_catalog.entries,
        );
        for row in rows {
            if row.name(snapshot.resolved_language).is_empty() {
                return Err("shortcuts page contains an unnamed row".to_owned());
            }
        }
        Ok(())
    }

    pub fn show_models_page_for_smoke(&mut self, _cx: &mut Context<Self>) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "models page has not received a runtime snapshot".to_owned())?;
        if snapshot.model_catalog.error.is_some() {
            return Err("models page received a catalog error".to_owned());
        }
        let active_model = snapshot
            .active_model
            .as_ref()
            .ok_or_else(|| "models page has no active model identity".to_owned())?;
        let active_entry = snapshot
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.origin == active_model.origin && entry.id == active_model.id)
            .ok_or_else(|| "models page active model is absent from the catalog".to_owned())?;
        let active_actions = model_row_actions(active_entry, Some(active_model), false);
        if !active_actions.active || active_actions.can_activate {
            return Err("models page did not protect the active model row".to_owned());
        }

        let mut has_activation_target = false;
        let mut has_location_target = false;
        let mut has_edit_target = false;
        for entry in &snapshot.model_catalog.entries {
            let actions = model_row_actions(entry, Some(active_model), false);
            if entry.origin == SettingsModelOrigin::Preset && actions.can_delete {
                return Err("models page exposed deletion for a preset model".to_owned());
            }
            if matches!(
                &entry.availability,
                SettingsModelAvailability::Invalid { .. }
            ) && actions.can_activate
            {
                return Err("models page exposed activation for an invalid model".to_owned());
            }
            has_activation_target |= actions.can_activate;
            has_location_target |= actions.can_open_location;
            has_edit_target |= entry.origin == SettingsModelOrigin::Preset && actions.can_edit;
        }
        if !has_activation_target {
            return Err("models page has no ready inactive activation target".to_owned());
        }
        if !has_location_target {
            return Err("models page has no model whose folder can be opened".to_owned());
        }
        // Renaming and re-covering a model is offered for both origins, so a
        // build whose presets are all read-only would be shipping the page
        // without half of its edit affordances — the case this section exists to
        // catch.
        if !has_edit_target {
            return Err("models page exposed no editable preset model".to_owned());
        }
        if active_entry
            .cover
            .as_ref()
            .is_none_or(|cover| !cover.is_file())
        {
            return Err("models page active model has no cover file".to_owned());
        }
        Ok(())
    }

    pub fn show_models_localization_for_smoke(
        &mut self,
        _cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "models page has not received a settings snapshot".to_owned())?;
        let entry = snapshot
            .model_catalog
            .entries
            .first()
            .ok_or_else(|| "models page catalog is empty".to_owned())?;
        let language = snapshot.resolved_language;
        if let Some(status) = model_availability_status(entry, language) {
            let expected_origin = bongocat_i18n::text(
                language.catalog_locale(),
                match entry.origin {
                    SettingsModelOrigin::Preset => "models.identity.source.preset",
                    SettingsModelOrigin::Installed => "models.identity.source.installed",
                },
            );
            if !status.contains(expected_origin) {
                return Err("models page did not localize the model status".to_owned());
            }
        }
        Ok(())
    }

    pub fn show_about_page_for_smoke(&mut self, _cx: &mut Context<Self>) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "about page has not received a settings snapshot".to_owned())?;
        let language = snapshot.resolved_language;
        if ABOUT_SECTIONS.iter().any(|section| {
            bongocat_i18n::text(language.catalog_locale(), section.title).is_empty()
                || bongocat_i18n::text(language.catalog_locale(), section.description).is_empty()
        }) {
            return Err("about page has incomplete localized content".to_owned());
        }
        Ok(())
    }

    /// Show the pre-rendered window again.
    pub fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        bongocat_platform::show_native_window(window).map_err(|error| error.to_string())?;
        self.window_hidden = false;
        self.refresh(cx);
        window.activate_window();
        Ok(())
    }

    /// Hide the window while keeping it alive.
    pub fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        self.cancel_shortcut_capture(cx);
        self.clear_model_drag(cx);
        self.flush_pending_settings(cx);
        self.window_hidden = true;
        bongocat_platform::hide_native_window(window).map_err(|error| error.to_string())
    }
}
