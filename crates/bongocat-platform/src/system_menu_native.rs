use crate::{SystemMenuAction, SystemMenuError, SystemMenuPresentation};
use image::ImageReader;
use muda::{
    CheckMenuItem, ContextMenu, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem,
    Submenu,
};
#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    io::Cursor,
    sync::mpsc::{self, Receiver, Sender},
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconId};
#[cfg(target_os = "windows")]
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

const TRAY_ICON_ID: &str = "bongocat.system-menu";
#[cfg(target_os = "windows")]
const TRAY_ICON_GUID: u128 = 0x123f3c6f_7d2a_4ca3_b8cb_9b1d1eaf2f10;
const OPEN_SETTINGS_ID: &str = "bongocat.open-settings";
const MODEL_WINDOW_SUBMENU_ID: &str = "bongocat.model-window";
const TOGGLE_OVERLAY_VISIBILITY_ID: &str = "bongocat.toggle-overlay";
const TOGGLE_CLICK_THROUGH_ID: &str = "bongocat.toggle-click-through";
const TOGGLE_ALWAYS_ON_TOP_ID: &str = "bongocat.toggle-always-on-top";
const TOGGLE_HIDE_ON_POINTER_HOVER_ID: &str = "bongocat.toggle-hide-on-pointer-hover";
const CHECK_FOR_UPDATES_ID: &str = "bongocat.check-for-updates";
const QUIT_ID: &str = "bongocat.quit";

#[cfg(target_os = "macos")]
const STATUS_ICON_BYTES: &[u8] = include_bytes!("../../../resources/icons/tray-macos.png");
#[cfg(target_os = "windows")]
const STATUS_ICON_BYTES: &[u8] = include_bytes!("../../../resources/icons/tray-windows.png");

/// The platform owner for both native menu surfaces.
///
/// The tray and model-window context surfaces intentionally share one native
/// menu tree and one set of item instances. The owner keeps the menu rooted
/// with the tray icon while also allowing the overlay to present that same
/// menu at its own HWND/`NSView`.
pub struct SystemMenu {
    // Keep the tray icon first: the native menu handles must be released after
    // the status item stops using them.
    tray_icon: TrayIcon,
    menu: Menu,
    model_window: Submenu,
    items: NativeMenuItems,
    #[cfg(target_os = "windows")]
    tray_icon_id: TrayIconId,
    sender: Sender<SystemMenuAction>,
    receiver: Receiver<SystemMenuAction>,
    visible: bool,
}

struct NativeMenuItems {
    open_settings: MenuItem,
    toggle_overlay: CheckMenuItem,
    toggle_click_through: CheckMenuItem,
    toggle_always_on_top: CheckMenuItem,
    toggle_hide_on_pointer_hover: CheckMenuItem,
    check_for_updates: Option<MenuItem>,
    quit: MenuItem,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuEntry {
    OpenSettings,
    Separator,
    ModelWindow,
    ToggleOverlay,
    ToggleClickThrough,
    ToggleAlwaysOnTop,
    ToggleHideOnPointerHover,
    CheckForUpdates,
    Quit,
}

const MENU_ENTRIES: &[MenuEntry] = &[
    MenuEntry::OpenSettings,
    MenuEntry::Separator,
    MenuEntry::ModelWindow,
    MenuEntry::Separator,
    MenuEntry::CheckForUpdates,
    MenuEntry::Separator,
    MenuEntry::Quit,
];

const MODEL_WINDOW_ENTRIES: &[MenuEntry] = &[
    MenuEntry::ToggleOverlay,
    MenuEntry::ToggleClickThrough,
    MenuEntry::ToggleAlwaysOnTop,
    MenuEntry::ToggleHideOnPointerHover,
];

impl NativeMenuItems {
    fn new(presentation: &SystemMenuPresentation, include_update: bool) -> Self {
        Self {
            open_settings: MenuItem::with_id(
                OPEN_SETTINGS_ID,
                &presentation.open_settings,
                true,
                None,
            ),
            // Keep one fixed "Hide model window" row. Its check state is the
            // inverse of runtime visibility, so there is no separate Show row.
            toggle_overlay: CheckMenuItem::with_id(
                TOGGLE_OVERLAY_VISIBILITY_ID,
                &presentation.hide_overlay,
                true,
                visibility_checked(presentation.overlay_visible),
                None,
            ),
            toggle_click_through: CheckMenuItem::with_id(
                TOGGLE_CLICK_THROUGH_ID,
                &presentation.click_through,
                true,
                presentation.click_through_enabled,
                None,
            ),
            toggle_always_on_top: CheckMenuItem::with_id(
                TOGGLE_ALWAYS_ON_TOP_ID,
                &presentation.always_on_top,
                true,
                presentation.always_on_top_enabled,
                None,
            ),
            toggle_hide_on_pointer_hover: CheckMenuItem::with_id(
                TOGGLE_HIDE_ON_POINTER_HOVER_ID,
                &presentation.hide_on_pointer_hover,
                true,
                presentation.hide_on_pointer_hover_enabled,
                None,
            ),
            // Update availability is a build/channel fact and does not change
            // during the lifetime of this owner. Do not leave a permanently
            // disabled row in a production tray menu when the channel cannot
            // produce a release.
            check_for_updates: include_update.then(|| {
                MenuItem::with_id(
                    CHECK_FOR_UPDATES_ID,
                    &presentation.check_for_updates,
                    presentation.update_check_available,
                    None,
                )
            }),
            quit: MenuItem::with_id(QUIT_ID, &presentation.quit, true, None),
        }
    }

    fn update(&self, presentation: &SystemMenuPresentation) {
        self.open_settings.set_text(&presentation.open_settings);
        self.toggle_overlay.set_text(&presentation.hide_overlay);
        self.toggle_overlay
            .set_checked(visibility_checked(presentation.overlay_visible));
        self.toggle_click_through
            .set_text(&presentation.click_through);
        self.toggle_click_through
            .set_checked(presentation.click_through_enabled);
        self.toggle_always_on_top
            .set_text(&presentation.always_on_top);
        self.toggle_always_on_top
            .set_checked(presentation.always_on_top_enabled);
        self.toggle_hide_on_pointer_hover
            .set_text(&presentation.hide_on_pointer_hover);
        self.toggle_hide_on_pointer_hover
            .set_checked(presentation.hide_on_pointer_hover_enabled);
        if let Some(check_for_updates) = &self.check_for_updates {
            check_for_updates.set_text(&presentation.check_for_updates);
            check_for_updates.set_enabled(presentation.update_check_available);
        }
        self.quit.set_text(&presentation.quit);
    }
}

impl SystemMenu {
    pub fn start_with_presentation(
        visible: bool,
        presentation: SystemMenuPresentation,
    ) -> Result<Self, SystemMenuError> {
        #[cfg(target_os = "macos")]
        MainThreadMarker::new().ok_or(SystemMenuError::WrongThread)?;

        let icon = load_status_icon()?;
        let items = NativeMenuItems::new(&presentation, presentation.update_check_available);
        let model_window = Submenu::with_id(
            MenuId::new(MODEL_WINDOW_SUBMENU_ID),
            &presentation.model_window,
            true,
        );
        append_model_window_entries(&model_window, &items)?;

        // One menu tree serves both the tray and the model-window context
        // surface. The optional update row is omitted when its channel cannot
        // produce a release.
        let menu = Menu::new();
        append_menu_entries(&menu, &items, &model_window, MENU_ENTRIES)?;

        let tray_icon_id = TrayIconId::new(TRAY_ICON_ID);
        let tray_icon_builder = TrayIconBuilder::new()
            .with_id(tray_icon_id.clone())
            .with_menu(Box::new(menu.clone()))
            .with_icon(icon)
            .with_tooltip(&presentation.tooltip)
            .with_icon_as_template(cfg!(target_os = "macos"))
            .with_menu_on_left_click(cfg!(target_os = "macos"))
            .with_menu_on_right_click(true);
        #[cfg(target_os = "windows")]
        let tray_icon_builder = tray_icon_builder.with_guid(TRAY_ICON_GUID);
        let tray_icon = tray_icon_builder.build().map_err(|error| {
            #[cfg(target_os = "macos")]
            if matches!(error, tray_icon::Error::NotMainThread) {
                return SystemMenuError::WrongThread;
            }
            #[cfg(not(target_os = "macos"))]
            let _ = error;
            SystemMenuError::StatusItemCreateFailed
        })?;

        if !visible {
            tray_icon
                .set_visible(false)
                .map_err(|_| SystemMenuError::StatusItemUpdateFailed)?;
        }

        let (sender, receiver) = mpsc::channel();
        Ok(Self {
            tray_icon,
            menu,
            model_window,
            items,
            #[cfg(target_os = "windows")]
            tray_icon_id,
            sender,
            receiver,
            visible,
        })
    }

    /// Applies a new settings snapshot to both native menu surfaces.
    ///
    /// `SystemMenuPresentation::tooltip` is a creation-time input: it is applied once by
    /// [`Self::start_with_presentation`] and deliberately not re-applied here. `tray-icon 0.25.0`
    /// cannot update the tooltip of a GUID-registered Windows icon, because its `set_tooltip`
    /// issues `NIM_MODIFY` without `NIF_GUID`. The shell ignores `uID` for an icon identified by
    /// `guidItem` and requires the same GUID in every later call, so that call always fails
    /// (<https://learn.microsoft.com/windows/win32/api/shellapi/ns-shellapi-notifyicondataw#troubleshooting>).
    /// The tooltip must therefore stay invariant for the lifetime of this owner; changing it
    /// requires replacing the tray owner, which ADR-0031 forbids while the app is running.
    pub fn set_presentation(
        &mut self,
        presentation: SystemMenuPresentation,
    ) -> Result<(), SystemMenuError> {
        #[cfg(target_os = "macos")]
        MainThreadMarker::new().ok_or(SystemMenuError::WrongThread)?;

        self.items.update(&presentation);
        self.model_window.set_text(&presentation.model_window);
        Ok(())
    }

    pub fn try_recv(&self) -> Option<SystemMenuAction> {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = action_for_menu_id(&event.id) {
                let _ = self.sender.send(action);
            }
        }

        #[cfg(target_os = "windows")]
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                id,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
                && id == self.tray_icon_id
            {
                let _ = self.sender.send(SystemMenuAction::OpenSettings);
            }
        }

        self.receiver.try_recv().ok()
    }

    /// Present the model-window context menu at the current pointer location.
    pub fn show_context_menu_for_window(
        &self,
        window: &impl HasWindowHandle,
    ) -> Result<(), SystemMenuError> {
        let window = window
            .window_handle()
            .map_err(|_| SystemMenuError::WindowHandleUnavailable)?;

        match window.as_raw() {
            #[cfg(target_os = "windows")]
            RawWindowHandle::Win32(handle) => {
                // SAFETY: `window` keeps the HWND alive for the duration of this
                // synchronous popup, and `muda` only tracks the owned menu.
                let _ = unsafe {
                    self.menu
                        .show_context_menu_for_hwnd(handle.hwnd.get(), None)
                };
                Ok(())
            }
            #[cfg(target_os = "macos")]
            RawWindowHandle::AppKit(handle) => {
                let _main_thread = MainThreadMarker::new().ok_or(SystemMenuError::WrongThread)?;
                // SAFETY: the overlay's `HasWindowHandle` implementation keeps its
                // content NSView alive for the duration of this synchronous popup.
                let _ = unsafe {
                    self.menu
                        .show_context_menu_for_nsview(handle.ns_view.as_ptr(), None)
                };
                Ok(())
            }
            _ => Err(SystemMenuError::UnsupportedWindowHandle),
        }
    }

    #[doc(hidden)]
    pub fn request_action_for_smoke(
        &self,
        action: SystemMenuAction,
    ) -> Result<(), SystemMenuError> {
        self.sender
            .send(action)
            .map_err(|_| SystemMenuError::EventQueueClosed)
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn set_visible(&mut self, visible: bool) -> Result<(), SystemMenuError> {
        if visible == self.visible {
            return Ok(());
        }
        self.tray_icon
            .set_visible(visible)
            .map_err(|_| SystemMenuError::StatusItemUpdateFailed)?;
        self.visible = visible;
        Ok(())
    }

    pub fn shutdown(mut self) -> Result<(), SystemMenuError> {
        let result = if self.visible {
            self.tray_icon
                .set_visible(false)
                .map_err(|_| SystemMenuError::StatusItemUpdateFailed)
        } else {
            Ok(())
        };
        self.visible = false;
        result
    }
}

impl Drop for SystemMenu {
    fn drop(&mut self) {
        let _ = self.tray_icon.set_visible(false);
    }
}

fn append_menu_entries(
    menu: &Menu,
    items: &NativeMenuItems,
    model_window: &Submenu,
    entries: &[MenuEntry],
) -> Result<(), SystemMenuError> {
    let mut previous_was_separator = false;
    for entry in entries {
        let entry = *entry;
        if entry == MenuEntry::CheckForUpdates && items.check_for_updates.is_none() {
            continue;
        }
        if entry == MenuEntry::Separator {
            // The update row is optional. Collapse the two separators around it
            // instead of leaving an empty section in an unavailable channel.
            if previous_was_separator {
                continue;
            }
            append_item(menu, &PredefinedMenuItem::separator())?;
            previous_was_separator = true;
            continue;
        }
        previous_was_separator = false;
        match entry {
            MenuEntry::OpenSettings => append_item(menu, &items.open_settings)?,
            MenuEntry::Separator => unreachable!("separator handled above"),
            MenuEntry::ModelWindow => append_item(menu, model_window)?,
            MenuEntry::ToggleOverlay => append_item(menu, &items.toggle_overlay)?,
            MenuEntry::ToggleClickThrough => append_item(menu, &items.toggle_click_through)?,
            MenuEntry::ToggleAlwaysOnTop => append_item(menu, &items.toggle_always_on_top)?,
            MenuEntry::ToggleHideOnPointerHover => {
                append_item(menu, &items.toggle_hide_on_pointer_hover)?
            }
            MenuEntry::CheckForUpdates => append_item(
                menu,
                items
                    .check_for_updates
                    .as_ref()
                    .ok_or(SystemMenuError::MenuCreateFailed)?,
            )?,
            MenuEntry::Quit => append_item(menu, &items.quit)?,
        }
    }
    Ok(())
}

fn append_model_window_entries(
    submenu: &Submenu,
    items: &NativeMenuItems,
) -> Result<(), SystemMenuError> {
    for entry in MODEL_WINDOW_ENTRIES {
        let item: &dyn IsMenuItem = match *entry {
            MenuEntry::ToggleOverlay => &items.toggle_overlay,
            MenuEntry::ToggleClickThrough => &items.toggle_click_through,
            MenuEntry::ToggleAlwaysOnTop => &items.toggle_always_on_top,
            MenuEntry::ToggleHideOnPointerHover => &items.toggle_hide_on_pointer_hover,
            _ => return Err(SystemMenuError::MenuCreateFailed),
        };
        submenu
            .append(item)
            .map_err(|_| SystemMenuError::MenuCreateFailed)?;
    }
    Ok(())
}

fn append_item(menu: &Menu, item: &dyn IsMenuItem) -> Result<(), SystemMenuError> {
    menu.append(item)
        .map_err(|_| SystemMenuError::MenuCreateFailed)
}

fn load_status_icon() -> Result<Icon, SystemMenuError> {
    let image = ImageReader::new(Cursor::new(STATUS_ICON_BYTES))
        .with_guessed_format()
        .map_err(|_| SystemMenuError::StatusIconImageLoadFailed)?
        .decode()
        .map_err(|_| SystemMenuError::StatusIconImageLoadFailed)?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height)
        .map_err(|_| SystemMenuError::StatusIconImageLoadFailed)
}

fn visibility_checked(overlay_visible: bool) -> bool {
    !overlay_visible
}

fn action_for_menu_id(id: &MenuId) -> Option<SystemMenuAction> {
    Some(match id.0.as_str() {
        OPEN_SETTINGS_ID => SystemMenuAction::OpenSettings,
        TOGGLE_OVERLAY_VISIBILITY_ID => SystemMenuAction::ToggleOverlayVisibility,
        TOGGLE_CLICK_THROUGH_ID => SystemMenuAction::ToggleClickThrough,
        TOGGLE_ALWAYS_ON_TOP_ID => SystemMenuAction::ToggleAlwaysOnTop,
        TOGGLE_HIDE_ON_POINTER_HOVER_ID => SystemMenuAction::ToggleHideOnPointerHover,
        CHECK_FOR_UPDATES_ID => SystemMenuAction::CheckForUpdates,
        QUIT_ID => SystemMenuAction::Quit,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        MENU_ENTRIES, MODEL_WINDOW_ENTRIES, MenuEntry, SystemMenuAction, action_for_menu_id,
        visibility_checked,
    };
    use muda::MenuId;

    #[test]
    fn visibility_is_rendered_as_a_check_state_meaning_hidden() {
        assert!(visibility_checked(false));
        assert!(!visibility_checked(true));
    }

    #[test]
    fn both_menu_surfaces_use_the_same_layout() {
        assert!(MENU_ENTRIES.contains(&MenuEntry::ModelWindow));
        assert!(MENU_ENTRIES.contains(&MenuEntry::CheckForUpdates));

        for entry in [
            MenuEntry::OpenSettings,
            MenuEntry::ModelWindow,
            MenuEntry::CheckForUpdates,
            MenuEntry::Quit,
        ] {
            assert!(MENU_ENTRIES.contains(&entry));
        }

        for entry in [
            MenuEntry::ToggleOverlay,
            MenuEntry::ToggleClickThrough,
            MenuEntry::ToggleAlwaysOnTop,
            MenuEntry::ToggleHideOnPointerHover,
        ] {
            assert!(MODEL_WINDOW_ENTRIES.contains(&entry));
        }
    }

    #[test]
    fn menu_action_mapping_keeps_only_the_current_action_set() {
        assert_eq!(
            action_for_menu_id(&MenuId::new("bongocat.open-settings")),
            Some(SystemMenuAction::OpenSettings)
        );
        assert_eq!(
            action_for_menu_id(&MenuId::new("bongocat.toggle-overlay")),
            Some(SystemMenuAction::ToggleOverlayVisibility)
        );
        assert_eq!(
            action_for_menu_id(&MenuId::new("bongocat.toggle-click-through")),
            Some(SystemMenuAction::ToggleClickThrough)
        );
        assert_eq!(
            action_for_menu_id(&MenuId::new("bongocat.toggle-always-on-top")),
            Some(SystemMenuAction::ToggleAlwaysOnTop)
        );
        assert_eq!(
            action_for_menu_id(&MenuId::new("bongocat.toggle-hide-on-pointer-hover")),
            Some(SystemMenuAction::ToggleHideOnPointerHover)
        );
        assert_eq!(
            action_for_menu_id(&MenuId::new("bongocat.check-for-updates")),
            Some(SystemMenuAction::CheckForUpdates)
        );
        assert_eq!(
            action_for_menu_id(&MenuId::new("bongocat.quit")),
            Some(SystemMenuAction::Quit)
        );

        for removed in [
            "bongocat.open-source",
            "bongocat.restart",
            "bongocat.version",
            "bongocat.model-window",
        ] {
            assert_eq!(action_for_menu_id(&MenuId::new(removed)), None);
        }
    }
}
