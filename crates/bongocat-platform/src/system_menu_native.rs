use crate::{SystemMenuAction, SystemMenuError, SystemMenuPresentation};
use image::ImageReader;
use muda::{CheckMenuItem, ContextMenu, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
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
const TOGGLE_OVERLAY_VISIBILITY_ID: &str = "bongocat.toggle-overlay";
const TOGGLE_CLICK_THROUGH_ID: &str = "bongocat.toggle-click-through";
const CHECK_FOR_UPDATES_ID: &str = "bongocat.check-for-updates";
const OPEN_SOURCE_ID: &str = "bongocat.open-source";
const RESTART_ID: &str = "bongocat.restart";
const QUIT_ID: &str = "bongocat.quit";

#[cfg(target_os = "macos")]
const STATUS_ICON_BYTES: &[u8] = include_bytes!("../../../resources/icons/tray-macos.png");
#[cfg(target_os = "windows")]
const STATUS_ICON_BYTES: &[u8] = include_bytes!("../../../resources/icons/tray-windows.png");

pub struct SystemMenu {
    tray_icon: TrayIcon,
    menu: Menu,
    open_settings_item: MenuItem,
    toggle_overlay_item: MenuItem,
    toggle_click_through_item: CheckMenuItem,
    check_for_updates_item: MenuItem,
    open_source_item: MenuItem,
    version_item: MenuItem,
    restart_item: MenuItem,
    quit_item: MenuItem,
    #[cfg(target_os = "windows")]
    tray_icon_id: TrayIconId,
    sender: Sender<SystemMenuAction>,
    receiver: Receiver<SystemMenuAction>,
    visible: bool,
    presentation: SystemMenuPresentation,
}

impl SystemMenu {
    pub fn start_with_presentation(
        visible: bool,
        presentation: SystemMenuPresentation,
    ) -> Result<Self, SystemMenuError> {
        #[cfg(target_os = "macos")]
        MainThreadMarker::new().ok_or(SystemMenuError::WrongThread)?;

        let icon = load_status_icon()?;
        let menu = Menu::new();
        let open_settings_item =
            MenuItem::with_id(OPEN_SETTINGS_ID, &presentation.open_settings, true, None);
        let toggle_overlay_item = MenuItem::with_id(
            TOGGLE_OVERLAY_VISIBILITY_ID,
            overlay_label(&presentation),
            true,
            None,
        );
        let toggle_click_through_item = CheckMenuItem::with_id(
            TOGGLE_CLICK_THROUGH_ID,
            &presentation.click_through,
            true,
            presentation.click_through_enabled,
            None,
        );
        let check_for_updates_item = MenuItem::with_id(
            CHECK_FOR_UPDATES_ID,
            &presentation.check_for_updates,
            presentation.update_check_available,
            None,
        );
        let open_source_item =
            MenuItem::with_id(OPEN_SOURCE_ID, &presentation.open_source, true, None);
        let version_item =
            MenuItem::with_id("bongocat.version", &presentation.version, false, None);
        let restart_item = MenuItem::with_id(RESTART_ID, &presentation.restart, true, None);
        let quit_item = MenuItem::with_id(QUIT_ID, &presentation.quit, true, None);

        menu.append(&open_settings_item)
            .and_then(|()| menu.append(&toggle_overlay_item))
            .and_then(|()| menu.append(&PredefinedMenuItem::separator()))
            .and_then(|()| menu.append(&toggle_click_through_item))
            .and_then(|()| menu.append(&PredefinedMenuItem::separator()))
            .and_then(|()| menu.append(&check_for_updates_item))
            .and_then(|()| menu.append(&open_source_item))
            .and_then(|()| menu.append(&PredefinedMenuItem::separator()))
            .and_then(|()| menu.append(&version_item))
            .and_then(|()| menu.append(&restart_item))
            .and_then(|()| menu.append(&quit_item))
            .map_err(|_| SystemMenuError::MenuCreateFailed)?;

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
            open_settings_item,
            toggle_overlay_item,
            toggle_click_through_item,
            check_for_updates_item,
            open_source_item,
            version_item,
            restart_item,
            quit_item,
            #[cfg(target_os = "windows")]
            tray_icon_id,
            sender,
            receiver,
            visible,
            presentation,
        })
    }

    pub fn set_presentation(
        &mut self,
        presentation: SystemMenuPresentation,
    ) -> Result<(), SystemMenuError> {
        #[cfg(target_os = "macos")]
        MainThreadMarker::new().ok_or(SystemMenuError::WrongThread)?;

        self.tray_icon
            .set_tooltip(Some(&presentation.tooltip))
            .map_err(|_| SystemMenuError::StatusItemUpdateFailed)?;
        self.open_settings_item
            .set_text(&presentation.open_settings);
        self.toggle_overlay_item
            .set_text(overlay_label(&presentation));
        self.toggle_click_through_item
            .set_text(&presentation.click_through);
        self.toggle_click_through_item
            .set_checked(presentation.click_through_enabled);
        self.check_for_updates_item
            .set_text(&presentation.check_for_updates);
        self.check_for_updates_item
            .set_enabled(presentation.update_check_available);
        self.open_source_item.set_text(&presentation.open_source);
        self.version_item.set_text(&presentation.version);
        self.restart_item.set_text(&presentation.restart);
        self.quit_item.set_text(&presentation.quit);
        self.presentation = presentation;
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

    /// Present the same action menu used by the tray icon at the current pointer location.
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

fn overlay_label(presentation: &SystemMenuPresentation) -> &str {
    if presentation.overlay_visible {
        &presentation.hide_overlay
    } else {
        &presentation.show_overlay
    }
}

fn action_for_menu_id(id: &MenuId) -> Option<SystemMenuAction> {
    Some(match id.0.as_str() {
        OPEN_SETTINGS_ID => SystemMenuAction::OpenSettings,
        TOGGLE_OVERLAY_VISIBILITY_ID => SystemMenuAction::ToggleOverlayVisibility,
        TOGGLE_CLICK_THROUGH_ID => SystemMenuAction::ToggleClickThrough,
        CHECK_FOR_UPDATES_ID => SystemMenuAction::CheckForUpdates,
        OPEN_SOURCE_ID => SystemMenuAction::OpenSource,
        RESTART_ID => SystemMenuAction::Restart,
        QUIT_ID => SystemMenuAction::Quit,
        _ => return None,
    })
}
