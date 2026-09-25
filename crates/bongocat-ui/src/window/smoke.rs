use super::about::ABOUT_LOCALIZED_KEYS;
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
        if let Err(error) = self.show_appearance_page_for_smoke(cx) {
            failures.push(error);
        }
        if let Err(error) = self.show_navigation_pages_for_smoke(cx) {
            failures.push(error);
        }
        if let Err(error) = self.show_app_system_for_smoke(cx) {
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

    pub fn show_navigation_pages_for_smoke(
        &mut self,
        _cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let language = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "settings navigation has not received a settings snapshot".to_owned())?
            .resolved_language;
        for page in SettingsNavigationPage::ALL {
            if page.title(language).is_empty() {
                return Err(format!("page {} has no localized title", page.title_key()));
            }
        }
        for key in [
            "settings.models.behavior.mirror_model.label",
            "settings.models.behavior.motion_audio.label",
            "settings.models.behavior.random_behavior_enabled.label",
            "settings.models.behavior.random_behavior_interval.label",
            "settings.input_interaction.mouse.title",
            "settings.input_interaction.mouse.ignore_mouse_input.label",
            "settings.input_interaction.keyboard.title",
            "settings.input_interaction.keyboard.ignore_keyboard_input.label",
            "settings.input_interaction.gamepad.title",
            "settings.input_interaction.gamepad.ignore_gamepad_input.label",
            "settings.app_system.desktop.title",
            "settings.app_system.updates.title",
            "settings.app_system.logging.title",
        ] {
            if bongocat_i18n::text(language.catalog_locale(), key).is_empty() {
                return Err(format!(
                    "settings navigation is missing localized text for {key}"
                ));
            }
        }
        Ok(())
    }

    pub fn show_app_system_for_smoke(&mut self, _cx: &mut Context<Self>) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "app & system has not received a settings snapshot".to_owned())?;
        let locale = snapshot.resolved_language.catalog_locale();
        for key in [
            "settings.app_system.desktop.title",
            "settings.app_system.open_at_login.label",
            "settings.app_system.updates.title",
            "settings.app_system.logging.title",
            "settings.app_system.logging.level.label",
            "settings.app_system.logging.retention_days.label",
            "settings.app_system.auto_update.label",
            "settings.app_system.auto_update.interval.label",
        ] {
            if bongocat_i18n::text(locale, key).is_empty() {
                return Err(format!("app & system is missing localized text for {key}"));
            }
        }
        if bongocat_i18n::platform_text(locale, "settings.app_system.status_icon.label").is_empty()
        {
            return Err("app & system is missing the platform status-icon label".to_owned());
        }
        let options = logging_level_options(snapshot.resolved_language);
        if options.len() != SettingsLogLevel::ALL.len()
            || options.iter().any(|option| option.is_empty())
            || options.iter().copied().collect::<BTreeSet<_>>().len() != options.len()
        {
            return Err(
                "app & system logging does not expose one unique label per level".to_owned(),
            );
        }
        for level in SettingsLogLevel::ALL {
            let display = logging_level_display_name(level, snapshot.resolved_language);
            if logging_level_from_display_name(display, snapshot.resolved_language) != Some(level) {
                return Err("app & system logging level labels are not reversible".to_owned());
            }
        }
        if !(1..=bongocat_config::MAXIMUM_LOG_RETENTION_DAYS)
            .contains(&snapshot.logging.retention_days)
        {
            return Err("app & system logging retention is outside 1..=30 days".to_owned());
        }
        if !(1..=bongocat_config::MAXIMUM_CHECK_FOR_UPDATES_INTERVAL_HOURS)
            .contains(&snapshot.check_for_updates_interval_hours)
        {
            return Err("automatic update interval is outside 1..=8760 hours".to_owned());
        }
        Ok(())
    }

    pub fn show_appearance_page_for_smoke(
        &mut self,
        _cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "appearance page has not received a settings snapshot".to_owned())?;
        for key in [
            "settings.appearance.theme.label",
            "settings.appearance.language.label",
        ] {
            if bongocat_i18n::text(snapshot.resolved_language.catalog_locale(), key).is_empty() {
                return Err(format!(
                    "appearance page is missing localized text for {key}"
                ));
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

    pub fn show_model_library_page_for_smoke(
        &mut self,
        _cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "model library page has not received a runtime snapshot".to_owned())?;
        if snapshot.model_catalog.error.is_some() {
            return Err("model library page received a catalog error".to_owned());
        }
        let active_model = snapshot
            .active_model
            .as_ref()
            .ok_or_else(|| "model library page has no active model identity".to_owned())?;
        let active_entry = snapshot
            .model_catalog
            .entries
            .iter()
            .find(|entry| entry.origin == active_model.origin && entry.id == active_model.id)
            .ok_or_else(|| {
                "model library page active model is absent from the catalog".to_owned()
            })?;
        let active_actions = model_row_actions(active_entry, Some(active_model), false);
        if !active_actions.active || active_actions.can_activate {
            return Err("model library page did not protect the active model row".to_owned());
        }

        let mut has_activation_target = false;
        let mut has_location_target = false;
        let mut has_edit_target = false;
        for entry in &snapshot.model_catalog.entries {
            let actions = model_row_actions(entry, Some(active_model), false);
            if entry.origin == SettingsModelOrigin::BuiltIn && actions.can_delete {
                return Err("model library page exposed deletion for a preset model".to_owned());
            }
            if matches!(
                &entry.availability,
                SettingsModelAvailability::Invalid { .. }
            ) && actions.can_activate
            {
                return Err("model library page exposed activation for an invalid model".to_owned());
            }
            has_activation_target |= actions.can_activate;
            has_location_target |= actions.can_open_location;
            has_edit_target |= entry.origin == SettingsModelOrigin::BuiltIn && actions.can_edit;
        }
        if !has_activation_target {
            return Err("model library page has no ready inactive activation target".to_owned());
        }
        if !has_location_target {
            return Err("model library page has no model whose folder can be opened".to_owned());
        }
        // Renaming and re-covering a model is offered for both origins, so a
        // build whose presets are all read-only would be shipping the page
        // without half of its edit affordances — the case this section exists to
        // catch.
        if !has_edit_target {
            return Err("model library page exposed no editable preset model".to_owned());
        }
        if active_entry
            .cover
            .as_ref()
            .is_none_or(|cover| !cover.is_file())
        {
            return Err("model library page active model has no cover file".to_owned());
        }
        Ok(())
    }

    pub fn show_model_library_localization_for_smoke(
        &mut self,
        _cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "model library page has not received a settings snapshot".to_owned())?;
        let entry = snapshot
            .model_catalog
            .entries
            .first()
            .ok_or_else(|| "model library page catalog is empty".to_owned())?;
        let language = snapshot.resolved_language;
        if let Some(status) = model_availability_status(entry, language) {
            let expected_origin = bongocat_i18n::text(
                language.catalog_locale(),
                match entry.origin {
                    SettingsModelOrigin::BuiltIn => "models.identity.source.built_in",
                    SettingsModelOrigin::Imported => "models.identity.source.imported",
                },
            );
            if !status.contains(expected_origin) {
                return Err("model library page did not localize the model status".to_owned());
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
        let locale = language.catalog_locale();
        if ABOUT_LOCALIZED_KEYS
            .iter()
            .any(|key| bongocat_i18n::text(locale, key).is_empty())
        {
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
