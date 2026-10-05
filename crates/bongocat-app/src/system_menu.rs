//! The system tray menu: its items, their labels and what each one does.
//!
//! The labels are read from the settings snapshot rather than translated here, so
//! the menu and the settings window cannot disagree about the product's language.

use super::*;

pub(crate) fn system_menu_presentation(
    snapshot: &SettingsSnapshot,
    palette: SystemMenuPalette,
) -> SystemMenuPresentation {
    // `catalog_locale` is the protocol's own answer to "which catalog does this
    // resolved language read from". Restating that mapping here is what left the
    // tray menu in English for every language other than Chinese.
    let locale = snapshot.resolved_language.catalog_locale();
    let text = |key| bongocat_i18n::text(locale, key).to_owned();
    SystemMenuPresentation {
        title: text("system_menu.title"),
        tooltip: text("system_menu.title"),
        open_settings: text("system_menu.open_settings"),
        model_window: text("navigation.model_window.title"),
        hide_overlay: text("settings.overlay.hide_model_window.label"),
        click_through: text("settings.overlay.click_through.label"),
        always_on_top: text("settings.overlay.always_on_top.label"),
        hide_on_pointer_hover: text("settings.overlay.hide_on_mouse_hover.label"),
        check_for_updates: text("update.about.label"),
        quit: text("system_menu.quit"),
        overlay_visible: snapshot.overlay_visible,
        click_through_enabled: snapshot.overlay.click_through,
        always_on_top_available: snapshot.overlay_capabilities.always_on_top,
        always_on_top_enabled: snapshot.overlay.always_on_top,
        hide_on_pointer_hover_available: snapshot.overlay_capabilities.pointer_hover,
        hide_on_pointer_hover_enabled: snapshot.overlay.hide_on_pointer_hover,
        // A Production build stays gated on its channel and release signing key
        // (`bongocat_app::update_check_available`): without them a check can only fail.
        // A Development build can never update either, but the update window is where
        // that is *explained* (`Unavailable · DevelopmentBuild`), so its entry stays
        // clickable and clicking it shows the development-build explanation.
        update_check_available: bongocat_app::update_check_available()
            || matches!(
                bongocat_app::BUILD_ENVIRONMENT,
                bongocat_config::BuildEnvironment::Development
            ),
        palette,
    }
}

pub(crate) async fn refresh_system_menu_presentation(
    client: &SettingsClient,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let snapshot = client
        .read_snapshot()
        .await
        .map_err(|error| error.to_string())?;
    cx.update(|cx| {
        let presentation = system_menu_presentation(
            &snapshot,
            system_menu_palette(snapshot.appearance_theme, cx),
        );
        if !cx.has_global::<ProductCoordinator>() {
            return Err("system menu owner is unavailable".to_owned());
        }
        let coordinator = cx.global_mut::<ProductCoordinator>();
        #[cfg(target_os = "linux")]
        if let Some(overlay) = coordinator.overlay.as_mut() {
            overlay.set_system_menu_presentation(presentation.clone());
        }
        let Some(system_menu) = coordinator.system_menu.as_mut() else {
            return Ok(());
        };
        system_menu
            .set_presentation(presentation)
            .map_err(|error| error.to_string())
    })
}

pub(crate) async fn apply_system_menu_overlay_action(
    client: SettingsClient,
    action: SystemMenuAction,
) -> Result<bool, String> {
    let snapshot = client
        .read_snapshot()
        .await
        .map_err(|error| error.to_string())?;
    let revision = snapshot
        .config_revision
        .ok_or_else(|| "system menu cannot update configuration during recovery".to_owned())?;
    match action {
        SystemMenuAction::ToggleOverlayVisibility => {
            client
                .set_overlay_visible(revision, !snapshot.overlay_visible)
                .await
        }
        SystemMenuAction::ToggleClickThrough => {
            client
                .set_overlay_settings(
                    revision,
                    SettingsOverlay {
                        click_through: !snapshot.overlay.click_through,
                        ..snapshot.overlay
                    },
                )
                .await
        }
        SystemMenuAction::ToggleAlwaysOnTop => {
            client
                .set_overlay_settings(
                    revision,
                    SettingsOverlay {
                        always_on_top: !snapshot.overlay.always_on_top,
                        ..snapshot.overlay
                    },
                )
                .await
        }
        SystemMenuAction::ToggleHideOnPointerHover => {
            client
                .set_overlay_settings(
                    revision,
                    SettingsOverlay {
                        hide_on_pointer_hover: !snapshot.overlay.hide_on_pointer_hover,
                        ..snapshot.overlay
                    },
                )
                .await
        }
        _ => return Err("invalid system menu overlay action".to_owned()),
    }
    .map(|_| true)
    .map_err(|error| error.to_string())
}
