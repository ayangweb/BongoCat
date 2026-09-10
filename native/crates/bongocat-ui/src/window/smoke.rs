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
        self.verify_models_localization_for_smoke(snapshot, active_entry, true)?;

        let mut has_activation_target = false;
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
        }
        if !has_activation_target {
            return Err("models page has no ready inactive activation target".to_owned());
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
        let active = snapshot
            .active_model
            .as_ref()
            .is_some_and(|active| active.origin == entry.origin && active.id == entry.id);
        self.verify_models_localization_for_smoke(snapshot, entry, active)
    }

    fn verify_models_localization_for_smoke(
        &self,
        snapshot: &SettingsSnapshot,
        entry: &SettingsModelEntry,
        active: bool,
    ) -> Result<(), String> {
        let language = snapshot.resolved_language;
        let status = model_availability_status(entry, active, language);
        let expected_origin = bongocat_i18n::text(
            language.catalog_locale(),
            match entry.origin {
                SettingsModelOrigin::Preset => "models.identity.source.preset",
                SettingsModelOrigin::Installed => "models.identity.source.installed",
            },
        );
        if !status.contains(expected_origin)
            || (active
                && !status.contains(bongocat_i18n::text(
                    language.catalog_locale(),
                    "models.identity.status.active",
                )))
        {
            return Err("models page did not localize the model status".to_owned());
        }
        let (import_status, import_failed) = model_import_status(&self.model_import, language);
        if import_failed
            || import_status
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
            for (id, label) in [
                (
                    ACCESSIBILITY_MODELS,
                    bongocat_i18n::text(language.catalog_locale(), "navigation.models.title"),
                ),
                (
                    ACCESSIBILITY_REFRESH,
                    bongocat_i18n::text(language.catalog_locale(), "actions.refresh"),
                ),
                (
                    ACCESSIBILITY_QUIT,
                    bongocat_i18n::text(language.catalog_locale(), "actions.quit"),
                ),
            ] {
                let node = tree
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .ok_or_else(|| "models page omitted a shell accessibility node".to_owned())?;
                if node.label != label {
                    return Err(
                        "models page and shell accessibility labels were not localized".to_owned(),
                    );
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
        let expected_theme_mode =
            component_theme_mode(snapshot.appearance_theme, cx.window_appearance());
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
                || theme.description.as_deref()
                    != Some(bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.appearance.theme.description",
                    ))
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
                || language.description.as_deref()
                    != Some(bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.appearance.language.description",
                    ))
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
                || startup.value.as_deref() != Some(presentation.description)
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
                    != bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.application.status_icon.label",
                    )
                || status_icon.value.as_deref()
                    != Some(bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.application.status_icon.description",
                    ))
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
                    || taskbar_icon.value.as_deref()
                        != Some(bongocat_i18n::text(
                            snapshot.resolved_language.catalog_locale(),
                            "settings.application.taskbar_icon.description",
                        ))
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
                            "settings.overlay.release_fallback_timeout.description",
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
            for (id, label, value, toggled) in [
                (
                    ACCESSIBILITY_OVERLAY_TOPMOST,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.always_on_top.label",
                    ),
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.always_on_top.description",
                    ),
                    snapshot.overlay.always_on_top,
                ),
                (
                    ACCESSIBILITY_OVERLAY_CLICK_THROUGH,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.click_through.label",
                    ),
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.click_through.description",
                    ),
                    snapshot.overlay.click_through,
                ),
                (
                    ACCESSIBILITY_OVERLAY_KEEP_INSIDE_WORK_AREA,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.keep_inside_work_area.label",
                    ),
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.overlay.keep_inside_work_area.description",
                    ),
                    snapshot.overlay.keep_inside_work_area,
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
                    || node.value.as_deref() != Some(value)
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
                    ACCESSIBILITY_BEHAVIOR_SHORTCUTS,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.behavior_shortcuts.label",
                    ),
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.behavior_shortcuts.description",
                    ),
                    snapshot.behavior_shortcuts_enabled,
                ),
                (
                    ACCESSIBILITY_MIRROR,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.mirror_model.label",
                    ),
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.mirror_model.description",
                    ),
                    snapshot.model_settings.mirror,
                ),
                (
                    ACCESSIBILITY_MIRROR_POINTER,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.mirror_pointer_tracking.label",
                    ),
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.mirror_pointer_tracking.description",
                    ),
                    snapshot.model_settings.mirror_pointer_tracking,
                ),
                (
                    ACCESSIBILITY_IGNORE_POINTER,
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.ignore_pointer_input.label",
                    ),
                    bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "settings.model_interaction.ignore_pointer_input.description",
                    ),
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
                    || node.value.as_deref() != Some(value)
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

    pub fn show_diagnostics_page_for_smoke(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.page = SettingsPage::Diagnostics;
        cx.notify();
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "diagnostics page has not received a settings snapshot".to_owned())?;
        let language = snapshot.resolved_language;
        let metrics = input_diagnostic_metrics(language, snapshot.input_diagnostics);
        if metrics.len() != 26 {
            return Err("diagnostics page did not project every input counter".to_owned());
        }
        let recovery = config_recovery_presentation(
            snapshot.configuration_status,
            snapshot.config_recovery,
            language,
        );
        if recovery.title.is_empty() || recovery.detail.is_empty() {
            return Err("diagnostics page did not project configuration recovery".to_owned());
        }
        if language == SettingsLanguage::ChineseSimplified
            && (bongocat_i18n::text(
                language.catalog_locale(),
                "navigation.diagnostics.description",
            ) == bongocat_i18n::text(
                SettingsLanguage::EnglishUnitedStates.catalog_locale(),
                "navigation.diagnostics.description",
            ) || metrics[0].0
                == input_diagnostic_metrics(
                    SettingsLanguage::EnglishUnitedStates,
                    snapshot.input_diagnostics,
                )[0]
                .0)
        {
            return Err("diagnostics visible text was not localized".to_owned());
        }
        let open_backups = self
            .accessibility_tree()
            .nodes
            .into_iter()
            .find(|node| node.id == ACCESSIBILITY_OPEN_BACKUPS)
            .ok_or_else(|| "diagnostics omitted the accessible backup location".to_owned())?;
        if open_backups.role != AccessibilityRole::Button
            || open_backups.label
                != bongocat_i18n::text(
                    language.catalog_locale(),
                    "diagnostics.configuration.open_backups_folder",
                )
            || open_backups.value.as_deref()
                != Some(bongocat_i18n::text(
                    language.catalog_locale(),
                    "diagnostics.configuration.open_backups_folder_description",
                ))
            || open_backups.disabled
            || !open_backups.supports_click
            || !open_backups.supports_focus
        {
            return Err("backup location accessibility semantics are invalid".to_owned());
        }
        let export = self
            .accessibility_tree()
            .nodes
            .into_iter()
            .find(|node| node.id == ACCESSIBILITY_EXPORT_DIAGNOSTICS)
            .ok_or_else(|| "diagnostics omitted the accessible export action".to_owned())?;
        if export.role != AccessibilityRole::Button
            || export.label
                != bongocat_i18n::text(language.catalog_locale(), "diagnostics.export.action")
            || export.value.as_deref()
                != Some(bongocat_i18n::text(
                    language.catalog_locale(),
                    "diagnostics.export.description",
                ))
            || export.disabled
            || !export.supports_click
            || !export.supports_focus
        {
            return Err("diagnostics export accessibility semantics are invalid".to_owned());
        }
        let clear_shortcuts = self
            .accessibility_tree()
            .nodes
            .into_iter()
            .find(|node| node.id == ACCESSIBILITY_CLEAR_SHORTCUTS)
            .ok_or_else(|| "diagnostics omitted the accessible shortcut clear action".to_owned())?;
        let shortcuts_present = !snapshot.shortcuts.commands.is_empty()
            || !snapshot.shortcuts.model_behaviors.is_empty();
        if clear_shortcuts.role != AccessibilityRole::Button
            || clear_shortcuts.label
                != bongocat_i18n::text(language.catalog_locale(), "shortcuts.actions.clear_all")
            || clear_shortcuts.value.as_deref()
                != Some(bongocat_i18n::text(
                    language.catalog_locale(),
                    "shortcuts.actions.clear_all_description",
                ))
            || clear_shortcuts.disabled != !shortcuts_present
            || clear_shortcuts.supports_click != shortcuts_present
            || clear_shortcuts.supports_focus != shortcuts_present
        {
            return Err("shortcut clear accessibility semantics are invalid".to_owned());
        }
        let capture_nodes = self
            .accessibility_tree()
            .nodes
            .into_iter()
            .filter(|node| {
                node.id.get() >= ACCESSIBILITY_SHORTCUT_CAPTURE_BASE
                    && node.id.get() < ACCESSIBILITY_SHORTCUT_CLEAR_BASE
            })
            .collect::<Vec<_>>();
        let expected_capture_rows = shortcut_accessibility_rows(
            &snapshot.shortcuts,
            snapshot.active_model.as_ref(),
            &snapshot.model_catalog.entries,
            language,
        );
        let shortcut_count = expected_capture_rows.len();
        if capture_nodes.len() != shortcut_count
            || capture_nodes
                .iter()
                .zip(expected_capture_rows)
                .any(|(node, (_, label, value))| {
                    node.role != AccessibilityRole::Button
                        || node.label != label
                        || node.value.as_deref() != Some(value.as_str())
                        || node.disabled
                        || !node.supports_click
                        || !node.supports_focus
                })
        {
            return Err("shortcut capture accessibility semantics are invalid".to_owned());
        }
        let expected_clear_rows = shortcut_clear_accessibility_rows(
            &snapshot.shortcuts,
            snapshot.active_model.as_ref(),
            &snapshot.model_catalog.entries,
            language,
        );
        let clear_nodes = self
            .accessibility_tree()
            .nodes
            .into_iter()
            .filter(|node| node.id.get() >= ACCESSIBILITY_SHORTCUT_CLEAR_BASE)
            .collect::<Vec<_>>();
        if clear_nodes.len() != expected_clear_rows.len()
            || clear_nodes
                .iter()
                .zip(expected_clear_rows)
                .any(|(node, (_, label))| {
                    node.role != AccessibilityRole::Button
                        || node.label != label
                        || node.disabled
                        || !node.supports_click
                        || !node.supports_focus
                })
        {
            return Err("shortcut binding clear accessibility semantics are invalid".to_owned());
        }
        if matches!(
            snapshot.configuration_status,
            SettingsConfigurationStatus::RecoveryRequired { .. }
        ) {
            if !recovery.attention || !recovery.can_restore {
                return Err("recovery diagnostics omitted the restore action".to_owned());
            }
            let restore = self
                .accessibility_tree()
                .nodes
                .into_iter()
                .find(|node| node.id == ACCESSIBILITY_RESTORE_DEFAULTS)
                .ok_or_else(|| {
                    "recovery diagnostics omitted the accessible restore action".to_owned()
                })?;
            if restore.role != AccessibilityRole::Button
                || restore.label
                    != bongocat_i18n::text(
                        language.catalog_locale(),
                        "diagnostics.configuration.restore_defaults",
                    )
                || restore.value.as_deref()
                    != Some(bongocat_i18n::text(
                        language.catalog_locale(),
                        "diagnostics.configuration.restore_defaults_description",
                    ))
                || restore.disabled
                || !restore.supports_click
                || !restore.supports_focus
            {
                return Err("recovery restore accessibility semantics are invalid".to_owned());
            }
        }
        Ok(())
    }

    pub fn show_shortcuts_page_for_smoke(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.page = SettingsPage::Shortcuts;
        self.shortcut_tab = ShortcutSettingsTab::Window;
        cx.notify();
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| "shortcuts page has not received a settings snapshot".to_owned())?;
        let window_rows = window_shortcut_rows(&snapshot.shortcuts);
        if window_rows.len() != 5
            || window_rows[0].target != ShortcutCaptureTarget::Command("toggle_overlay".to_owned())
            || window_rows[1].target != ShortcutCaptureTarget::Command("open_settings".to_owned())
        {
            return Err("shortcuts page omitted fixed window shortcut targets".to_owned());
        }
        self.shortcut_tab = ShortcutSettingsTab::Model;
        let model_rows = shortcut_behavior_rows(
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
                || node.value.as_deref()
                    != Some(bongocat_i18n::text(
                        snapshot.resolved_language.catalog_locale(),
                        "navigation.shortcuts.description",
                    ))
                || !node.supports_click
                || !node.supports_focus
            {
                return Err(
                    "shortcuts page navigation accessibility semantics are invalid".to_owned(),
                );
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
                || node.value.as_deref()
                    != Some(bongocat_i18n::text(
                        language.catalog_locale(),
                        "navigation.about.description",
                    ))
                || !node.supports_click
                || !node.supports_focus
            {
                return Err("about page navigation accessibility semantics are invalid".to_owned());
            }
        }
        Ok(())
    }

    pub fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        bongocat_platform::show_native_window(window).map_err(|error| error.to_string())?;
        self.window_hidden = false;
        self.refresh(cx);
        window.activate_window();
        Ok(())
    }

    pub fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        self.cancel_shortcut_capture(cx);
        self.flush_pending_settings(cx);

        #[cfg(target_os = "windows")]
        {
            bongocat_platform::hide_native_window(window).map_err(|error| error.to_string())?;
            self.window_hidden = true;
            cx.notify();
        }

        #[cfg(target_os = "macos")]
        window.remove_window();

        Ok(())
    }
}
