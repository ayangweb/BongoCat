use crate::{SystemMenuAction, SystemMenuError, SystemMenuPresentation};
use std::{
    mem::size_of,
    sync::mpsc::{self, Receiver, Sender},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NIN_SELECT,
                NOTIFYICON_VERSION_4, NOTIFYICONDATAW, NOTIFYICONDATAW_0, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CREATESTRUCTW, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
                DestroyMenu, DestroyWindow, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW,
                LoadIconW, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, PostMessageW,
                RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, TPM_BOTTOMALIGN,
                TPM_LEFTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, UnregisterClassW, WINDOW_EX_STYLE,
                WINDOW_STYLE, WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NCCREATE,
                WM_NCDESTROY, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
            },
        },
    },
    core::{HSTRING, PCWSTR, w},
};

const WINDOW_CLASS: PCWSTR = w!("BongoCatProductSystemMenuWindow");
const WINDOW_TITLE: PCWSTR = w!("BongoCat System Menu");
const CALLBACK_MESSAGE: u32 = WM_APP + 47;
const TRAY_ID: u32 = 1;
const STATUS_ICON_RESOURCE_ID: u16 = 102;
const OPEN_SETTINGS_ID: usize = 1;
const TOGGLE_OVERLAY_VISIBILITY_ID: usize = 2;
const TOGGLE_CLICK_THROUGH_ID: usize = 3;
const CHECK_FOR_UPDATES_ID: usize = 30;
const OPEN_SOURCE_ID: usize = 31;
const RESTART_ID: usize = 32;
const QUIT_ID: usize = 33;

struct WindowState {
    sender: Sender<SystemMenuAction>,
    menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
}

pub struct SystemMenu {
    instance: HINSTANCE,
    window: Option<HWND>,
    menu: Option<windows::Win32::UI::WindowsAndMessaging::HMENU>,
    state: Option<Box<WindowState>>,
    sender: Sender<SystemMenuAction>,
    receiver: Receiver<SystemMenuAction>,
    icon_added: bool,
    class_registered: bool,
    presentation: SystemMenuPresentation,
}

impl SystemMenu {
    pub fn start_with_presentation(
        visible: bool,
        presentation: SystemMenuPresentation,
    ) -> Result<Self, SystemMenuError> {
        // SAFETY: creation and cleanup occur on the GPUI owner thread; the boxed state outlives its HWND.
        unsafe { Self::start_inner(visible, presentation) }
    }

    unsafe fn start_inner(
        visible: bool,
        presentation: SystemMenuPresentation,
    ) -> Result<Self, SystemMenuError> {
        let module = unsafe { GetModuleHandleW(None) }
            .map_err(|_| SystemMenuError::WindowClassRegistrationFailed)?;
        let instance = HINSTANCE(module.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(system_menu_window_proc),
            hInstance: instance,
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(SystemMenuError::WindowClassRegistrationFailed);
        }
        let menu =
            unsafe { create_menu(&presentation) }.map_err(|_| SystemMenuError::MenuCreateFailed)?;
        let (sender, receiver) = mpsc::channel();
        let mut state = Box::new(WindowState { sender, menu });
        let state_ptr = (&mut *state) as *mut WindowState;
        let window = match unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                WINDOW_CLASS,
                WINDOW_TITLE,
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                Some(state_ptr.cast()),
            )
        } {
            Ok(window) => window,
            Err(_) => {
                let _ = unsafe { DestroyMenu(menu) };
                let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
                return Err(SystemMenuError::WindowCreateFailed);
            }
        };
        let sender = state.sender.clone();
        let mut result = Self {
            instance,
            window: Some(window),
            menu: Some(menu),
            state: Some(state),
            sender,
            receiver,
            icon_added: false,
            class_registered: true,
            presentation,
        };
        if visible && unsafe { result.add_icon() }.is_err() {
            let _ = result.cleanup();
            return Err(SystemMenuError::StatusItemCreateFailed);
        }
        Ok(result)
    }

    pub fn set_presentation(
        &mut self,
        presentation: SystemMenuPresentation,
    ) -> Result<(), SystemMenuError> {
        // SAFETY: the old menu is detached from the still-live owner HWND before destruction.
        unsafe {
            let replacement =
                create_menu(&presentation).map_err(|_| SystemMenuError::MenuCreateFailed)?;
            if let Some(state) = self.state.as_mut() {
                state.menu = replacement;
            }
            if let Some(previous) = self.menu.replace(replacement) {
                DestroyMenu(previous).map_err(|_| SystemMenuError::MenuItemCreateFailed)?;
            }
            self.presentation = presentation;
        }
        Ok(())
    }
    pub fn try_recv(&self) -> Option<SystemMenuAction> {
        self.receiver.try_recv().ok()
    }

    /// Present the same action menu used by the status icon at the current pointer location.
    pub fn show_context_menu(&self) -> Result<(), SystemMenuError> {
        let window = self.window.ok_or(SystemMenuError::WindowCreateFailed)?;
        let menu = self.menu.ok_or(SystemMenuError::MenuCreateFailed)?;
        show_menu(window, menu);
        Ok(())
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
    pub const fn is_visible(&self) -> bool {
        self.icon_added
    }
    pub fn set_visible(&mut self, visible: bool) -> Result<(), SystemMenuError> {
        unsafe {
            if visible {
                self.add_icon()
            } else {
                self.remove_icon()
            }
        }
        .map_err(|_| SystemMenuError::StatusItemUpdateFailed)
    }
    pub fn shutdown(mut self) -> Result<(), SystemMenuError> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> Result<(), SystemMenuError> {
        let mut failed = unsafe { self.remove_icon() }.is_err();
        if let Some(window) = self.window.take() {
            failed |= unsafe { DestroyWindow(window) }.is_err();
        }
        self.state.take();
        if let Some(menu) = self.menu.take() {
            failed |= unsafe { DestroyMenu(menu) }.is_err();
        }
        if self.class_registered {
            failed |= unsafe { UnregisterClassW(WINDOW_CLASS, Some(self.instance)) }.is_err();
            self.class_registered = false;
        }
        if failed {
            Err(SystemMenuError::ShutdownFailed)
        } else {
            Ok(())
        }
    }
    unsafe fn add_icon(&mut self) -> Result<(), ()> {
        if self.icon_added {
            return Ok(());
        }
        let window = self.window.ok_or(())?;
        let icon = unsafe {
            LoadIconW(
                Some(self.instance),
                PCWSTR(STATUS_ICON_RESOURCE_ID as usize as *const u16),
            )
        }
        .map_err(|_| ())?;
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: TRAY_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: CALLBACK_MESSAGE,
            hIcon: icon,
            ..Default::default()
        };
        copy_wide(&mut data.szTip, &self.presentation.tooltip);
        if !unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
            return Err(());
        }
        data.Anonymous = NOTIFYICONDATAW_0 {
            uVersion: NOTIFYICON_VERSION_4,
        };
        let _ = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) };
        self.icon_added = true;
        Ok(())
    }
    unsafe fn remove_icon(&mut self) -> Result<(), ()> {
        if !self.icon_added {
            return Ok(());
        }
        let window = self.window.ok_or(())?;
        let data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: TRAY_ID,
            ..Default::default()
        };
        if !unsafe { Shell_NotifyIconW(NIM_DELETE, &data) }.as_bool() {
            return Err(());
        }
        self.icon_added = false;
        Ok(())
    }
}

impl Drop for SystemMenu {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

unsafe fn create_menu(
    presentation: &SystemMenuPresentation,
) -> windows::core::Result<windows::Win32::UI::WindowsAndMessaging::HMENU> {
    let menu = unsafe { CreatePopupMenu()? };
    let result = (|| unsafe {
        append(
            menu,
            MF_STRING,
            OPEN_SETTINGS_ID,
            &presentation.open_settings,
        )?;
        append(
            menu,
            MF_STRING,
            TOGGLE_OVERLAY_VISIBILITY_ID,
            if presentation.overlay_visible {
                &presentation.hide_overlay
            } else {
                &presentation.show_overlay
            },
        )?;
        AppendMenuW(menu, MF_SEPARATOR, 0, None)?;
        append(
            menu,
            MF_STRING
                | if presentation.click_through_enabled {
                    MF_CHECKED
                } else {
                    Default::default()
                },
            TOGGLE_CLICK_THROUGH_ID,
            &presentation.click_through,
        )?;
        AppendMenuW(menu, MF_SEPARATOR, 0, None)?;
        append(
            menu,
            MF_STRING
                | if presentation.update_check_available {
                    Default::default()
                } else {
                    MF_GRAYED
                },
            CHECK_FOR_UPDATES_ID,
            &presentation.check_for_updates,
        )?;
        append(menu, MF_STRING, OPEN_SOURCE_ID, &presentation.open_source)?;
        AppendMenuW(menu, MF_SEPARATOR, 0, None)?;
        append(menu, MF_STRING | MF_GRAYED, 0, &presentation.version)?;
        append(menu, MF_STRING, RESTART_ID, &presentation.restart)?;
        append(menu, MF_STRING, QUIT_ID, &presentation.quit)
    })();
    if result.is_err() {
        let _ = unsafe { DestroyMenu(menu) };
    }
    result.map(|_| menu)
}

unsafe fn append(
    menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
    flags: windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS,
    id: usize,
    text: &str,
) -> windows::core::Result<()> {
    unsafe { AppendMenuW(menu, flags, id, &HSTRING::from(text)) }
}
fn copy_wide(destination: &mut [u16], value: &str) {
    for (slot, code_unit) in destination
        .iter_mut()
        .zip(value.encode_utf16().chain(std::iter::once(0)))
    {
        *slot = code_unit;
    }
}

unsafe extern "system" fn system_menu_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    let state = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut WindowState };
    if message == WM_NCDESTROY {
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, 0) };
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    if state.is_null() {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    let state = unsafe { &*state };
    match message {
        WM_COMMAND => {
            if let Some(action) = action_for_command(wparam.0 & 0xffff) {
                let _ = state.sender.send(action);
            }
            LRESULT(0)
        }
        CALLBACK_MESSAGE => {
            match lparam.0 as u32 & 0xffff {
                WM_LBUTTONUP | NIN_SELECT => {
                    let _ = state.sender.send(SystemMenuAction::OpenSettings);
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(window, state.menu),
                _ => {}
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn action_for_command(id: usize) -> Option<SystemMenuAction> {
    Some(match id {
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
fn show_menu(window: HWND, menu: windows::Win32::UI::WindowsAndMessaging::HMENU) {
    let mut point = POINT::default();
    unsafe {
        if GetCursorPos(&mut point).is_ok() {
            let _ = SetForegroundWindow(window);
            let _ = TrackPopupMenu(
                menu,
                TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                window,
                None,
            );
            let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        }
    }
}
