use super::*;

impl SettingsView {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn report_service_error(&mut self, error: SettingsError, cx: &mut Context<Self>) {
        self.pending = None;
        self.error = Some(error);
        cx.notify();
    }

    pub fn snapshot_revision(&self) -> Option<u64> {
        self.snapshot.as_ref().map(|snapshot| snapshot.revision)
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

        let mut has_activation_target = false;
        for entry in &snapshot.model_catalog.entries {
            let actions = model_row_actions(entry, Some(active_model), false);
            if entry.origin == SettingsModelOrigin::Preset && actions.can_delete {
                return Err("models page exposed deletion for a preset model".to_owned());
            }
            if matches!(
                entry.availability,
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
        let presentation = startup_item_presentation(Some(snapshot.startup_item), false);
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
            for (id, theme, label) in [
                (ACCESSIBILITY_THEME_SYSTEM, SettingsTheme::System, "System"),
                (ACCESSIBILITY_THEME_LIGHT, SettingsTheme::Light, "Light"),
                (ACCESSIBILITY_THEME_DARK, SettingsTheme::Dark, "Dark"),
            ] {
                let node = tree
                    .nodes
                    .iter()
                    .find(|node| node.id == id)
                    .ok_or_else(|| {
                        "general accessibility tree omitted an appearance theme".to_owned()
                    })?;
                if node.role != AccessibilityRole::RadioButton
                    || node.label != label
                    || node.disabled != controls_disabled
                    || node.supports_click != !controls_disabled
                    || node.supports_focus != !controls_disabled
                    || node.toggled
                        != Some(if snapshot.appearance_theme == theme {
                            AccessibilityToggle::On
                        } else {
                            AccessibilityToggle::Off
                        })
                {
                    return Err(
                        "theme accessibility semantics diverged from the visible control"
                            .to_owned(),
                    );
                }
            }
            let startup = tree
                .nodes
                .iter()
                .find(|node| node.id == ACCESSIBILITY_STARTUP)
                .ok_or_else(|| "accessibility tree omitted the startup item".to_owned())?;
            if startup.role != AccessibilityRole::Switch
                || startup.label != "Open at login"
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
            for (id, label, value, toggled) in [
                (
                    ACCESSIBILITY_OVERLAY_TOPMOST,
                    "Always on top",
                    "Keep the Live2D overlay above other windows",
                    snapshot.overlay.always_on_top,
                ),
                (
                    ACCESSIBILITY_OVERLAY_CLICK_THROUGH,
                    "Click-through overlay",
                    "Let pointer input pass through the Live2D overlay",
                    snapshot.overlay.click_through,
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
                    ACCESSIBILITY_MIRROR,
                    "Mirror model",
                    "Render the model mirrored horizontally",
                    snapshot.model_settings.mirror,
                ),
                (
                    ACCESSIBILITY_MIRROR_POINTER,
                    "Mirror pointer tracking",
                    "Mirror horizontal pointer tracking with the model",
                    snapshot.model_settings.mirror_pointer_tracking,
                ),
                (
                    ACCESSIBILITY_IGNORE_POINTER,
                    "Ignore pointer input",
                    "Do not apply pointer movement to the model",
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
                    "Decrease overlay scale",
                    snapshot.overlay.scale_percent <= 25,
                ),
                (
                    ACCESSIBILITY_OVERLAY_SCALE_INCREASE,
                    "Increase overlay scale",
                    snapshot.overlay.scale_percent >= 400,
                ),
                (
                    ACCESSIBILITY_OVERLAY_OPACITY_DECREASE,
                    "Decrease overlay opacity",
                    snapshot.overlay.opacity_percent <= 1,
                ),
                (
                    ACCESSIBILITY_OVERLAY_OPACITY_INCREASE,
                    "Increase overlay opacity",
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
        if input_diagnostic_metrics(snapshot.input_diagnostics).len() != 25 {
            return Err("diagnostics page did not project every input counter".to_owned());
        }
        let recovery =
            config_recovery_presentation(snapshot.configuration_status, snapshot.config_recovery);
        if recovery.title.is_empty() || recovery.detail.is_empty() {
            return Err("diagnostics page did not project configuration recovery".to_owned());
        }
        let open_backups = self
            .accessibility_tree()
            .nodes
            .into_iter()
            .find(|node| node.id == ACCESSIBILITY_OPEN_BACKUPS)
            .ok_or_else(|| "diagnostics omitted the accessible backup location".to_owned())?;
        if open_backups.role != AccessibilityRole::Button
            || open_backups.label != "Open configuration backups folder"
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
            || export.label != "Export diagnostics"
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
            || clear_shortcuts.label != "Clear all shortcuts"
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
            .filter(|node| node.id.get() >= ACCESSIBILITY_SHORTCUT_CAPTURE_BASE)
            .collect::<Vec<_>>();
        let shortcut_count =
            snapshot.shortcuts.commands.len() + snapshot.shortcuts.model_behaviors.len();
        if capture_nodes.len() != shortcut_count
            || capture_nodes.iter().any(|node| {
                node.role != AccessibilityRole::Button
                    || node.disabled
                    || !node.supports_click
                    || !node.supports_focus
            })
        {
            return Err("shortcut capture accessibility semantics are invalid".to_owned());
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
                || restore.label != "Restore default configuration"
                || restore.disabled
                || !restore.supports_click
                || !restore.supports_focus
            {
                return Err("recovery restore accessibility semantics are invalid".to_owned());
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
}
