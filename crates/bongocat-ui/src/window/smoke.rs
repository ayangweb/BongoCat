use super::*;

impl SettingsView {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
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

    pub fn window_hidden(&self) -> bool {
        self.window_hidden
    }

    pub fn show_models_page_for_smoke(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.page = SettingsPage::Models;
        cx.notify();
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "models page has not received a runtime snapshot".to_owned())?;
        if snapshot.model_catalog.error.is_some() {
            return Err("models page received a catalog error".to_owned());
        }
        if snapshot.model_catalog.entries.is_empty() {
            return Err("models page catalog is empty".to_owned());
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
        if !active_actions.active || active_actions.can_activate || active_actions.can_delete {
            return Err("models page did not protect the active model row".to_owned());
        }
        self.verify_models_localization_for_smoke(snapshot, active_entry)?;

        let mut has_activation_target = false;
        let mut has_location_target = false;
        for entry in &snapshot.model_catalog.entries {
            let actions = model_row_actions(entry, Some(active_model), false);
            if entry.origin == SettingsModelOrigin::Preset && actions.can_delete {
                return Err("models page exposed deletion for a preset model".to_owned());
            }
            // Preset names and covers are app-bundled content, so the page must
            // not offer to edit them.
            if entry.origin == SettingsModelOrigin::Preset && actions.can_edit {
                return Err("models page exposed editing for a preset model".to_owned());
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
        }
        if !has_activation_target {
            return Err("models page has no ready inactive activation target".to_owned());
        }
        if !has_location_target {
            return Err("models page has no model whose folder can be opened".to_owned());
        }
        // The page renders the package's own cover, so a catalog entry the user
        // can see must point at a real image rather than a guessed path.
        let active_cover = active_entry
            .cover
            .as_ref()
            .ok_or_else(|| "models page active model has no cover".to_owned())?;
        if !active_cover.is_file() {
            return Err("models page cover does not point at a file".to_owned());
        }
        Ok(())
    }

    pub fn show_models_localization_for_smoke(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.page = SettingsPage::Models;
        cx.notify();
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "models page has not received a settings snapshot".to_owned())?;
        if snapshot.model_catalog.error.is_some() {
            return Err("models page received a catalog error".to_owned());
        }
        let entry = snapshot
            .model_catalog
            .entries
            .first()
            .ok_or_else(|| "models page catalog is empty".to_owned())?;
        self.verify_models_localization_for_smoke(snapshot, entry)
    }

    fn verify_models_localization_for_smoke(
        &self,
        snapshot: &SettingsSnapshot,
        entry: &SettingsModelEntry,
    ) -> Result<(), String> {
        let language = snapshot.resolved_language;
        // Only a diagnostic earns a status line; a ready model card shows no
        // status at all, so there is nothing to localize for it.
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
        let import_status = model_import_status(&self.model_import, language);
        if import_status
            != bongocat_i18n::text(
                language.catalog_locale(),
                "models.import.folder.none_selected",
            )
        {
            return Err("models page did not localize the initial import status".to_owned());
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let tree = self.accessibility_tree();
            let node = tree
                .nodes
                .iter()
                .find(|node| node.id == ACCESSIBILITY_MODELS)
                .ok_or_else(|| "models page omitted a shell accessibility node".to_owned())?;
            if node.label
                != bongocat_i18n::text(language.catalog_locale(), "navigation.models.title")
            {
                return Err(
                    "models page and shell accessibility labels were not localized".to_owned(),
                );
            }
        }
        Ok(())
    }

    /// Whether a frame has applied the appearance preference yet.
    ///
    /// `applied_theme` is only assigned by `sync_component_theme` from inside
    /// `render`, so asserting on it before the first frame is drawn fails. On a
    /// loaded machine the smoke's fixed start-up delay did not always cover
    /// that frame; callers wait on this instead of guessing how long to sleep.
    pub fn appearance_applied_for_smoke(&self) -> bool {
        self.applied_theme.is_some()
    }

    /// Runs every page assertion and reports all of them together.
    ///
    /// The caller used to chain these with `?`, so the first failure hid the
    /// rest and one reported failure looked like the whole smoke had been
    /// exercised. A page that fails still leaves the view usable: each helper
    /// only sets `page` and asserts on what the product projected.
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

    /// Walk the pages that were split out of the general page.
    ///
    /// These four carry no controls the general page smoke does not already
    /// assert, so what this proves is the split itself: each one is a
    /// navigation target of its own, the accessibility tree focuses it while it
    /// is the current page, and its label is the localized page title. Without
    /// this, a page that failed to render would only show up as a sidebar entry
    /// nobody could open.
    pub fn show_split_pages_for_smoke(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let language = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "split pages have not received a settings snapshot".to_owned())?
            .resolved_language;
        for (page, node_id, title_key) in [
            (
                SettingsPage::Overlay,
                ACCESSIBILITY_OVERLAY_PAGE,
                "navigation.overlay.title",
            ),
            (
                SettingsPage::Interaction,
                ACCESSIBILITY_INTERACTION,
                "navigation.interaction.title",
            ),
            (
                SettingsPage::Input,
                ACCESSIBILITY_INPUT,
                "navigation.input.title",
            ),
            (
                SettingsPage::Application,
                ACCESSIBILITY_APPLICATION,
                "navigation.application.title",
            ),
        ] {
            self.page = page;
            cx.notify();
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                let tree = self.accessibility_tree();
                tree.validate().map_err(|error| error.to_string())?;
                if tree.focus != node_id {
                    return Err(format!(
                        "page {title_key} did not expose the active accessibility focus"
                    ));
                }
                let node = tree
                    .nodes
                    .iter()
                    .find(|node| node.id == node_id)
                    .ok_or_else(|| {
                        format!("page {title_key} omitted its navigation accessibility node")
                    })?;
                if node.role != AccessibilityRole::Button
                    || node.label != bongocat_i18n::text(language.catalog_locale(), title_key)
                    || node.value.is_some()
                    || !node.supports_click
                    || !node.supports_focus
                {
                    return Err(format!(
                        "page {title_key} navigation accessibility semantics are invalid"
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn show_general_page_for_smoke(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.page = SettingsPage::General;
        cx.notify();
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "general page has not received a settings snapshot".to_owned())?;
        let controls_disabled = self.pending.is_some()
            || self.model_import.is_running()
            || snapshot.configuration_status != SettingsConfigurationStatus::Ready;
        // Derived through the product's own resolution rather than a second copy of the
        // formula. The smoke exists to prove what the product does, so re-deriving the
        // expected value independently would let the two drift apart unnoticed — and on
        // macOS they already had: this assertion used gpui's appearance while the render
        // path had moved to the platform query.
        let expected_theme_mode = resolved_theme_mode(snapshot.appearance_theme, None, cx);
        if self.applied_theme != Some(snapshot.appearance_theme)
            || cx.theme().mode != expected_theme_mode
        {
            return Err("general page did not apply the configured appearance theme".to_owned());
        }
        let presentation = startup_item_presentation(
            Some(snapshot.startup_item),
            false,
            snapshot.resolved_language,
        );
        match snapshot.startup_item {
            SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(_)) => {
                if presentation.action != StartupItemAction::None {
                    return Err("unsupported startup item exposed a mutation".to_owned());
                }
            }
            SettingsStartupItemStatus::ReadError(_) => {
                if presentation.action != StartupItemAction::Retry {
                    return Err("startup item read error did not expose retry".to_owned());
                }
            }
            SettingsStartupItemStatus::State(_) => {
                if !matches!(presentation.action, StartupItemAction::SetEnabled(_)) {
                    return Err("actionable startup item did not expose a mutation".to_owned());
                }
            }
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let tree = self.accessibility_tree();
            tree.validate().map_err(|error| error.to_string())?;
            let theme = tree
                .nodes
                .iter()
                .find(|node| node.id == ACCESSIBILITY_THEME)
                .ok_or_else(|| "general accessibility tree omitted the theme setting".to_owned())?;
            if theme.role != AccessibilityRole::ComboBox
                || theme.label
                    != bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.appearance.theme.label",
                    )
                || theme.description.is_some()
                || theme.value.as_deref()
                    != Some(theme_display_name(
                        snapshot.appearance_theme,
                        snapshot.resolved_language,
                    ))
                || theme.disabled != controls_disabled
                || theme.supports_click != !controls_disabled
                || theme.supports_focus != !controls_disabled
            {
                return Err(
                    "theme accessibility semantics diverged from the visible control".to_owned(),
                );
            }
            let language = tree
                .nodes
                .iter()
                .find(|node| node.id == ACCESSIBILITY_LANGUAGE)
                .ok_or_else(|| "accessibility tree omitted the language setting".to_owned())?;
            if language.role != AccessibilityRole::ComboBox
                || language.label
                    != bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.appearance.language.label",
                    )
                || language.description.is_some()
                || language.value.as_deref()
                    != Some(snapshot.language.display_name(snapshot.resolved_language))
                || language.disabled != controls_disabled
                || language.supports_click != !controls_disabled
                || language.supports_focus != !controls_disabled
            {
                return Err(
                    "language accessibility semantics diverged from the visible control".to_owned(),
                );
            }
            let startup = tree
                .nodes
                .iter()
                .find(|node| node.id == ACCESSIBILITY_STARTUP)
                .ok_or_else(|| "accessibility tree omitted the startup item".to_owned())?;
            if startup.role != AccessibilityRole::Switch
                || startup.label
                    != bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.application.open_at_login.label",
                    )
                || startup.value.as_deref() != presentation.description
                || startup.toggled
                    != Some(if presentation.enabled {
                        AccessibilityToggle::On
                    } else {
                        AccessibilityToggle::Off
                    })
                || startup.disabled != (presentation.action == StartupItemAction::None)
                || startup.supports_click != (presentation.action != StartupItemAction::None)
                || startup.supports_focus != (presentation.action != StartupItemAction::None)
            {
                return Err(
                    "startup accessibility semantics diverged from the visible control".to_owned(),
                );
            }
            let status_icon = tree
                .nodes
                .iter()
                .find(|node| node.id == ACCESSIBILITY_STATUS_ICON)
                .ok_or_else(|| "accessibility tree omitted the status icon setting".to_owned())?;
            if status_icon.role != AccessibilityRole::Switch
                || status_icon.label
                    != bongocat_i18n::platform_text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.application.status_icon.label",
                    )
                || status_icon.value.is_some()
                || status_icon.toggled
                    != Some(if snapshot.status_icon_visible {
                        AccessibilityToggle::On
                    } else {
                        AccessibilityToggle::Off
                    })
                || status_icon.disabled != controls_disabled
                || status_icon.supports_click != !controls_disabled
                || status_icon.supports_focus != !controls_disabled
            {
                return Err(
                    "status icon accessibility semantics diverged from the visible control"
                        .to_owned(),
                );
            }
            let automatic_update_check = tree
                .nodes
                .iter()
                .find(|node| node.id == ACCESSIBILITY_AUTOMATIC_UPDATE_CHECK)
                .ok_or_else(|| {
                    "accessibility tree omitted the automatic update setting".to_owned()
                })?;
            if automatic_update_check.role != AccessibilityRole::Switch
                || automatic_update_check.label
                    != bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.application.auto_update.label",
                    )
                || automatic_update_check.value.as_deref()
                    != Some(bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.application.auto_update.description",
                    ))
                || automatic_update_check.toggled
                    != Some(if snapshot.check_for_updates_automatically {
                        AccessibilityToggle::On
                    } else {
                        AccessibilityToggle::Off
                    })
                || automatic_update_check.disabled != controls_disabled
                || automatic_update_check.supports_click != !controls_disabled
                || automatic_update_check.supports_focus != !controls_disabled
            {
                return Err(
                    "automatic update accessibility semantics diverged from the visible control"
                        .to_owned(),
                );
            }
            #[cfg(target_os = "windows")]
            {
                let taskbar_icon = tree
                    .nodes
                    .iter()
                    .find(|node| node.id == ACCESSIBILITY_TASKBAR_ICON)
                    .ok_or_else(|| {
                        "accessibility tree omitted the taskbar icon setting".to_owned()
                    })?;
                if taskbar_icon.role != AccessibilityRole::Switch
                    || taskbar_icon.label
                        != bongocat_i18n::text(
                            snapshot.resolved_language.catalog_locale(),
                            "settings.application.taskbar_icon.label",
                        )
                    || taskbar_icon.value.is_some()
                    || taskbar_icon.toggled
                        != Some(if snapshot.taskbar_icon_visible {
                            AccessibilityToggle::On
                        } else {
                            AccessibilityToggle::Off
                        })
                    || taskbar_icon.disabled != controls_disabled
                    || taskbar_icon.supports_click != !controls_disabled
                    || taskbar_icon.supports_focus != !controls_disabled
                {
                    return Err(
                        "taskbar icon accessibility semantics diverged from the visible control"
                            .to_owned(),
                    );
                }
            }
            #[cfg(target_os = "macos")]
            if tree
                .nodes
                .iter()
                .any(|node| matches!(node.label.as_str(), "Show taskbar icon" | "显示任务栏图标"))
            {
                return Err("macOS exposed the Windows taskbar icon setting".to_owned());
            }
            for (id, label) in [
                (
                    ACCESSIBILITY_RELEASE_FALLBACK_DECREASE,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "shortcuts.actions.decrease_release_fallback_timeout",
                    ),
                ),
                (
                    ACCESSIBILITY_RELEASE_FALLBACK_INCREASE,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "shortcuts.actions.increase_release_fallback_timeout",
                    ),
                ),
            ] {
                let node = tree
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .ok_or_else(|| {
                        "accessibility tree omitted the release fallback setting".to_owned()
                    })?;
                if node.role != AccessibilityRole::Button
                    || node.label != label
                    || node.description.as_deref()
                        != Some(bongocat_i18n::text(
                            snapshot.resolved_language.catalog_locale(),
                            "settings.input.release_fallback_timeout.description",
                        ))
                    || node.value.as_deref()
                        != Some(snapshot.release_fallback_timeout_ms.to_string().as_str())
                    || node.disabled != controls_disabled
                    || node.supports_click != !controls_disabled
                    || node.supports_focus != !controls_disabled
                {
                    return Err(
                        "release fallback accessibility semantics diverged from the visible control"
                            .to_owned(),
                    );
                }
            }
            for (id, label, bound_disabled) in [
                (
                    ACCESSIBILITY_OVERLAY_HOVER_DELAY_DECREASE,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "shortcuts.actions.decrease_hide_on_pointer_hover_delay",
                    ),
                    snapshot.overlay.hide_on_pointer_hover_delay_seconds == 0,
                ),
                (
                    ACCESSIBILITY_OVERLAY_HOVER_DELAY_INCREASE,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "shortcuts.actions.increase_hide_on_pointer_hover_delay",
                    ),
                    snapshot.overlay.hide_on_pointer_hover_delay_seconds
                        >= bongocat_config::MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_SECONDS,
                ),
            ] {
                let node = tree
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .ok_or_else(|| {
                        "accessibility tree omitted the hover hide delay setting".to_owned()
                    })?;
                // The delay buttons disable themselves at the ends of the range and
                // while the hide-on-hover switch above the row is off, so the expected
                // state is the shared control state, the switch, or the per-button
                // bound rather than the shared state alone.
                let node_disabled =
                    controls_disabled || !snapshot.overlay.hide_on_pointer_hover || bound_disabled;
                if node.role != AccessibilityRole::Button
                    || node.label != label
                    || node.description.as_deref()
                        != Some(bongocat_i18n::text(
                            snapshot.resolved_language.catalog_locale(),
                            "settings.overlay.hide_on_pointer_hover_delay.description",
                        ))
                    || node.value.as_deref()
                        != Some(
                            format!("{}s", snapshot.overlay.hide_on_pointer_hover_delay_seconds)
                                .as_str(),
                        )
                    || node.disabled != node_disabled
                    || node.supports_click != !node_disabled
                    || node.supports_focus != !node_disabled
                {
                    return Err(
                        "hover hide delay accessibility semantics diverged from the visible control"
                            .to_owned(),
                    );
                }
            }
            for (id, label, value, toggled) in [
                (
                    ACCESSIBILITY_OVERLAY_TOPMOST,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.always_on_top.label",
                    ),
                    None,
                    snapshot.overlay.always_on_top,
                ),
                (
                    ACCESSIBILITY_OVERLAY_CLICK_THROUGH,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.click_through.label",
                    ),
                    None,
                    snapshot.overlay.click_through,
                ),
                (
                    ACCESSIBILITY_OVERLAY_KEEP_INSIDE_WORK_AREA,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.keep_inside_screen.label",
                    ),
                    Some(bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.keep_inside_screen.description",
                    )),
                    snapshot.overlay.keep_inside_screen,
                ),
                (
                    ACCESSIBILITY_OVERLAY_HIDE_ON_POINTER_HOVER,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.hide_on_pointer_hover.label",
                    ),
                    Some(bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.hide_on_pointer_hover.description",
                    )),
                    snapshot.overlay.hide_on_pointer_hover,
                ),
            ] {
                let node = tree
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .ok_or_else(|| {
                        "general accessibility tree omitted an overlay setting".to_owned()
                    })?;
                if node.role != AccessibilityRole::Switch
                    || node.label != label
                    || node.value.as_deref() != value
                    || node.disabled != controls_disabled
                    || node.supports_click != !controls_disabled
                    || node.supports_focus != !controls_disabled
                    || node.toggled
                        != Some(if toggled {
                            AccessibilityToggle::On
                        } else {
                            AccessibilityToggle::Off
                        })
                {
                    return Err(
                        "overlay accessibility semantics diverged from the visible control"
                            .to_owned(),
                    );
                }
            }
            for (id, label, value, toggled) in [
                (
                    ACCESSIBILITY_MIRROR,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.mirror_model.label",
                    ),
                    None,
                    snapshot.model_settings.mirror,
                ),
                (
                    ACCESSIBILITY_MIRROR_POINTER,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.mirror_pointer_tracking.label",
                    ),
                    None,
                    snapshot.model_settings.mirror_pointer_tracking,
                ),
                (
                    ACCESSIBILITY_IGNORE_POINTER,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.ignore_pointer_input.label",
                    ),
                    None,
                    snapshot.model_settings.ignore_pointer,
                ),
            ] {
                let node = tree
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .ok_or_else(|| {
                        "general accessibility tree omitted a model setting".to_owned()
                    })?;
                if node.role != AccessibilityRole::Switch
                    || node.label != label
                    || node.value.as_deref() != value
                    || node.disabled != controls_disabled
                    || node.supports_click != !controls_disabled
                    || node.supports_focus != !controls_disabled
                    || node.toggled
                        != Some(if toggled {
                            AccessibilityToggle::On
                        } else {
                            AccessibilityToggle::Off
                        })
                {
                    return Err(
                        "model accessibility semantics diverged from the visible control"
                            .to_owned(),
                    );
                }
            }
            for (id, label, unavailable) in [
                (
                    ACCESSIBILITY_OVERLAY_SCALE_DECREASE,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "shortcuts.actions.decrease_overlay_scale",
                    ),
                    snapshot.overlay.scale_percent <= 25,
                ),
                (
                    ACCESSIBILITY_OVERLAY_SCALE_INCREASE,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "shortcuts.actions.increase_overlay_scale",
                    ),
                    snapshot.overlay.scale_percent >= 400,
                ),
                (
                    ACCESSIBILITY_OVERLAY_OPACITY_DECREASE,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "shortcuts.actions.decrease_overlay_opacity",
                    ),
                    snapshot.overlay.opacity_percent <= 1,
                ),
                (
                    ACCESSIBILITY_OVERLAY_OPACITY_INCREASE,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "shortcuts.actions.increase_overlay_opacity",
                    ),
                    snapshot.overlay.opacity_percent >= 100,
                ),
            ] {
                let node = tree
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .ok_or_else(|| {
                        "general accessibility tree omitted an overlay stepper".to_owned()
                    })?;
                if node.role != AccessibilityRole::Button
                    || node.label != label
                    || node
                        .value
                        .as_deref()
                        .is_none_or(|value| !value.ends_with('%'))
                    || node.disabled != (controls_disabled || unavailable)
                    || node.supports_click != !(controls_disabled || unavailable)
                    || node.supports_focus != !(controls_disabled || unavailable)
                {
                    return Err(
                        "overlay stepper accessibility semantics diverged from the visible control"
                            .to_owned(),
                    );
                }
            }
            #[cfg(target_os = "macos")]
            self.accessibility
                .as_ref()
                .ok_or_else(|| "settings accessibility bridge is unavailable".to_owned())?
                .verify_startup_control(
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.application.open_at_login.label",
                    ),
                    if presentation.enabled {
                        AccessibilityToggle::On
                    } else {
                        AccessibilityToggle::Off
                    },
                    presentation.action != StartupItemAction::None,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// Verify the configuration-recovery notice the General page renders.
    ///
    /// This cannot ride on `show_general_page_for_smoke`: that smoke asserts the whole page
    /// against a configuration it treats as usable, and the window this runs in deliberately
    /// has no usable configuration. The notice is what that page shows instead of the
    /// settings, so it is asserted on its own.
    pub fn verify_configuration_recovery_for_smoke(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.page = SettingsPage::General;
        cx.notify();
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "recovery notice has not received a settings snapshot".to_owned())?;
        let recovery = config_recovery_presentation(
            snapshot.configuration_status,
            snapshot.config_recovery,
            snapshot.resolved_language,
        );
        if recovery.title.is_empty() || recovery.detail.is_empty() {
            return Err("general page did not project configuration recovery".to_owned());
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            if matches!(
                snapshot.configuration_status,
                SettingsConfigurationStatus::RecoveryRequired { .. }
            ) {
                if !recovery.attention || !recovery.can_restore {
                    return Err("recovery notice omitted the restore action".to_owned());
                }
                let restore = self
                    .accessibility_tree()
                    .nodes
                    .into_iter()
                    .find(|node| node.id == ACCESSIBILITY_RESTORE_DEFAULTS)
                    .ok_or_else(|| {
                        "recovery notice omitted the accessible restore action".to_owned()
                    })?;
                if restore.role != AccessibilityRole::Button
                    || restore.label != config_recovery_restore_label(snapshot.resolved_language)
                    || restore.value.as_deref()
                        != Some(bongocat_i18n::text(
                            snapshot.resolved_language.catalog_locale(),
                            "diagnostics.configuration.restore_defaults_description",
                        ))
                    || restore.disabled
                    || !restore.supports_click
                    || !restore.supports_focus
                {
                    return Err("recovery restore accessibility semantics are invalid".to_owned());
                }
                // The notice is visible on every page and the restore action is reachable from
                // every page, so the notice's own text has to be readable too. Without this node
                // a screen reader met the button and never learned what it was about.
                let notice = self
                    .accessibility_tree()
                    .nodes
                    .into_iter()
                    .find(|node| node.id == ACCESSIBILITY_CONFIG_RECOVERY)
                    .ok_or_else(|| "recovery notice did not expose its own text".to_owned())?;
                if notice.role != AccessibilityRole::Status
                    || notice.label != recovery.title
                    || notice.value.as_deref() != Some(recovery.detail.as_str())
                    || notice.supports_click
                    || notice.supports_focus
                {
                    return Err("recovery notice accessibility text is invalid".to_owned());
                }
            }
        }
        Ok(())
    }

    pub fn show_shortcuts_page_for_smoke(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.page = SettingsPage::Shortcuts;
        cx.notify();
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "shortcuts page has not received a settings snapshot".to_owned())?;
        // The page's two scopes are groups, not tabs. A group only becomes a
        // second-level sidebar entry when it carries a title, so the titles have
        // to exist and have to be distinct: two scopes sharing one name would
        // read as one entry and hide the other scope.
        let scope_titles = [
            shortcuts_page::ShortcutScope::Window.title(snapshot.resolved_language),
            shortcuts_page::ShortcutScope::Model.title(snapshot.resolved_language),
        ];
        if scope_titles.iter().any(|title| title.is_empty()) || scope_titles[0] == scope_titles[1] {
            return Err("shortcuts page scopes lost their localized titles".to_owned());
        }
        let window_rows = shortcuts_page::ShortcutScope::Window.rows(
            &snapshot.shortcuts,
            snapshot.active_model.as_ref(),
            &snapshot.model_catalog.entries,
        );
        if window_rows.len() != 5
            || window_rows[0].target != ShortcutCaptureTarget::Command("toggle_overlay".to_owned())
            || window_rows[1].target != ShortcutCaptureTarget::Command("open_settings".to_owned())
        {
            return Err("shortcuts page omitted fixed window shortcut targets".to_owned());
        }
        if window_rows
            .iter()
            .any(|row| !matches!(row.target, ShortcutCaptureTarget::Command(_)))
        {
            return Err("shortcuts page mixed model targets into window shortcuts".to_owned());
        }
        let model_rows = shortcuts_page::ShortcutScope::Model.rows(
            &snapshot.shortcuts,
            snapshot.active_model.as_ref(),
            &snapshot.model_catalog.entries,
        );
        if model_rows
            .iter()
            .any(|row| !matches!(row.target, ShortcutCaptureTarget::ModelBehavior { .. }))
        {
            return Err("shortcuts page mixed window targets into model shortcuts".to_owned());
        }
        // The scopes are two halves of one page: the accessibility nodes and the
        // keyboard tab order are numbered from the combined row list, so the
        // rendered order has to stay window rows first and model rows at the
        // offset the model scope reports.
        let combined = shortcut_rows(
            &snapshot.shortcuts,
            snapshot.active_model.as_ref(),
            &snapshot.model_catalog.entries,
        );
        let scopes_partition_combined = combined.len() == window_rows.len() + model_rows.len()
            && combined
                .iter()
                .zip(window_rows.iter().chain(model_rows.iter()))
                .all(|(combined, row)| {
                    combined.target == row.target && combined.shortcut == row.shortcut
                });
        if !scopes_partition_combined
            || shortcuts_page::ShortcutScope::Model.row_index_offset(&snapshot.shortcuts)
                != window_rows.len()
        {
            return Err("shortcuts page scopes do not share one row order".to_owned());
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let tree = self.accessibility_tree();
            let node = tree
                .nodes
                .iter()
                .find(|node| node.id == ACCESSIBILITY_SHORTCUTS)
                .ok_or_else(|| {
                    "shortcuts page omitted its navigation accessibility node".to_owned()
                })?;
            if node.role != AccessibilityRole::Button
                || node.label
                    != bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "navigation.shortcuts.title",
                    )
                || node.value.is_some()
                || !node.supports_click
                || !node.supports_focus
            {
                return Err(
                    "shortcuts page navigation accessibility semantics are invalid".to_owned(),
                );
            }
            // The generated capture rows used to be asserted from the diagnostics page smoke,
            // which was the only smoke that walked the accessibility tree this far. They
            // describe this page's controls, so they assert here instead.
            let editing_disabled =
                snapshot.configuration_status != SettingsConfigurationStatus::Ready;
            // A row's availability is not the global editing state alone. Under
            // ADR-0053's unified gate rule a row whose scope's switch is off
            // reports itself disabled and drops its click and focus support, so
            // an expectation that only knew about `editing_disabled` would call
            // every row of a switched-off scope wrong — and the model scope
            // starts switched off.
            let row_disabled = |target: &ShortcutCaptureTarget| {
                editing_disabled
                    || !shortcuts_page::ShortcutScope::for_target(target).is_enabled(snapshot)
            };
            // Both gates are the first row of their scope's group. They must
            // read the same label as the visible switch and report the same
            // configuration field, and they must be listed directly above the
            // rows they gate — the gate used to sit on the Interaction page,
            // where the chords it governs were a page away from it.
            for (scope, id) in [
                (
                    shortcuts_page::ShortcutScope::Window,
                    ACCESSIBILITY_COMMAND_SHORTCUTS,
                ),
                (
                    shortcuts_page::ShortcutScope::Model,
                    ACCESSIBILITY_BEHAVIOR_SHORTCUTS,
                ),
            ] {
                let node = tree
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .ok_or_else(|| "shortcuts page omitted one of its scope gates".to_owned())?;
                if node.role != AccessibilityRole::Switch
                    || node.label != scope.gate_label(snapshot.resolved_language)
                    || node.value.is_some()
                    || node.disabled != editing_disabled
                    || node.supports_click != !editing_disabled
                    || node.supports_focus != !editing_disabled
                    || node.toggled
                        != Some(if scope.is_enabled(snapshot) {
                            AccessibilityToggle::On
                        } else {
                            AccessibilityToggle::Off
                        })
                {
                    return Err(
                        "shortcut gate accessibility semantics diverged from the visible switch"
                            .to_owned(),
                    );
                }
            }
            let root = tree
                .nodes
                .iter()
                .find(|node| node.id == tree.root)
                .ok_or_else(|| "accessibility tree omitted its root node".to_owned())?;
            let window_gate_index = root
                .children
                .iter()
                .position(|id| *id == ACCESSIBILITY_COMMAND_SHORTCUTS)
                .ok_or_else(|| "shortcuts page omitted the window shortcut gate".to_owned())?;
            let model_gate_index = root
                .children
                .iter()
                .position(|id| *id == ACCESSIBILITY_BEHAVIOR_SHORTCUTS)
                .ok_or_else(|| "shortcuts page omitted the model shortcut gate".to_owned())?;
            let first_window_row = root
                .children
                .iter()
                .position(|id| *id == shortcut_accessibility_node_id(0))
                .ok_or_else(|| "shortcuts page omitted its command shortcut rows".to_owned())?;
            let model_gate_precedes_its_rows = model_rows.is_empty()
                || root
                    .children
                    .iter()
                    .position(|id| *id == shortcut_accessibility_node_id(window_rows.len()))
                    .is_some_and(|first_model_row| model_gate_index < first_model_row);
            if !(window_gate_index < first_window_row
                && first_window_row < model_gate_index
                && model_gate_precedes_its_rows)
            {
                return Err(
                    "shortcut gates are not listed directly above the rows they gate".to_owned(),
                );
            }
            let capture_nodes = tree
                .nodes
                .iter()
                .filter(|node| {
                    node.id.get() >= ACCESSIBILITY_SHORTCUT_CAPTURE_BASE
                        && node.id.get() < ACCESSIBILITY_SHORTCUT_CLEAR_BASE
                })
                .collect::<Vec<_>>();
            let expected_capture_rows = shortcut_accessibility_rows(
                &snapshot.shortcuts,
                snapshot.active_model.as_ref(),
                &snapshot.model_catalog.entries,
                snapshot.resolved_language,
            );
            if capture_nodes.len() != expected_capture_rows.len()
                || capture_nodes.iter().zip(expected_capture_rows).any(
                    |(node, (target, label, value))| {
                        let disabled = row_disabled(&target);
                        node.role != AccessibilityRole::Button
                            || node.label != *label
                            || node.value.as_deref() != Some(value.as_str())
                            || node.disabled != disabled
                            || node.supports_click != !disabled
                            || node.supports_focus != !disabled
                    },
                )
            {
                return Err("shortcut capture accessibility semantics are invalid".to_owned());
            }
            let expected_clear_rows = shortcut_clear_accessibility_rows(
                &snapshot.shortcuts,
                snapshot.active_model.as_ref(),
                &snapshot.model_catalog.entries,
                snapshot.resolved_language,
            );
            let clear_nodes = tree
                .nodes
                .iter()
                .filter(|node| {
                    // Generated node ids are allocated in 1_000-wide blocks, so a shortcut block
                    // ends where the next base would begin.
                    node.id.get() >= ACCESSIBILITY_SHORTCUT_CLEAR_BASE
                        && node.id.get() < ACCESSIBILITY_SHORTCUT_CLEAR_BASE + 1_000
                })
                .collect::<Vec<_>>();
            if clear_nodes.len() != expected_clear_rows.len()
                || clear_nodes
                    .iter()
                    .zip(expected_clear_rows)
                    .any(|(node, (target, label))| {
                        let disabled = row_disabled(&target);
                        node.role != AccessibilityRole::Button
                            || node.label != *label
                            || node.disabled != disabled
                            || node.supports_click != !disabled
                            || node.supports_focus != !disabled
                    })
            {
                return Err("shortcut binding clear accessibility semantics are invalid".to_owned());
            }
        }
        Ok(())
    }

    pub fn show_about_page_for_smoke(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.page = SettingsPage::About;
        cx.notify();
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
        if language == SettingsLanguage::ChineseSimplified
            && bongocat_i18n::text(language.catalog_locale(), "about.privacy.description")
                == bongocat_i18n::text(
                    SettingsLanguage::EnglishUnitedStates.catalog_locale(),
                    "about.privacy.description",
                )
        {
            return Err("about page privacy text was not localized".to_owned());
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let tree = self.accessibility_tree();
            tree.validate().map_err(|error| error.to_string())?;
            if tree.focus != ACCESSIBILITY_ABOUT {
                return Err("about page did not expose the active accessibility focus".to_owned());
            }
            let node = tree
                .nodes
                .iter()
                .find(|node| node.id == ACCESSIBILITY_ABOUT)
                .ok_or_else(|| "about page omitted its navigation accessibility node".to_owned())?;
            if node.role != AccessibilityRole::Button
                || node.label
                    != bongocat_i18n::text(language.catalog_locale(), "navigation.about.title")
                || node.value.is_some()
                || !node.supports_click
                || !node.supports_focus
            {
                return Err("about page navigation accessibility semantics are invalid".to_owned());
            }
        }
        Ok(())
    }

    /// Show the pre-rendered window again.
    ///
    /// Both platforms keep one window for the whole product lifetime, so opening
    /// settings after a close re-shows this same view and its current runtime
    /// snapshot rather than building a second one.
    pub fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        bongocat_platform::show_native_window(window).map_err(|error| error.to_string())?;
        self.window_hidden = false;
        self.refresh(cx);
        window.activate_window();
        Ok(())
    }

    /// Hide the window while keeping it alive.
    ///
    /// Closing settings is not a product event: the runtime, the input pipeline and the
    /// overlay keep running, and the view keeps whatever the user had in flight. Only the
    /// native window leaves the screen, which is the same `SW_HIDE`/`orderOut:` pair on
    /// Windows and macOS.
    pub fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        self.cancel_shortcut_capture(cx);
        self.flush_pending_settings(cx);
        bongocat_platform::hide_native_window(window).map_err(|error| error.to_string())?;
        self.window_hidden = true;
        cx.notify();
        Ok(())
    }
}
