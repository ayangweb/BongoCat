use crate::{SystemMenuAction, SystemMenuError};
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
                IDI_APPLICATION, LoadIconW, MF_SEPARATOR, MF_STRING, PostMessageW, RegisterClassW,
                SetForegroundWindow, SetWindowLongPtrW, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
                TPM_RIGHTBUTTON, TrackPopupMenu, UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
                WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NCCREATE, WM_NCDESTROY,
                WM_NULL, WM_RBUTTONUP, WNDCLASSW,
            },
        },
    },
    core::w,
};

const WINDOW_CLASS: windows::core::PCWSTR = w!("BongoCatProductSystemMenuWindow");
const WINDOW_TITLE: windows::core::PCWSTR = w!("BongoCat System Menu");
const CALLBACK_MESSAGE: u32 = WM_APP + 47;
const TRAY_ID: u32 = 1;
const OPEN_SETTINGS_ID: usize = 1;
const QUIT_ID: usize = 2;

struct WindowState {
    sender: Sender<SystemMenuAction>,
    menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
}

pub struct SystemMenu {
    instance: HINSTANCE,
    window: Option<HWND>,
    menu: Option<windows::Win32::UI::WindowsAndMessaging::HMENU>,
    state: Option<Box<WindowState>>,
    receiver: Receiver<SystemMenuAction>,
    icon_added: bool,
    class_registered: bool,
}

impl SystemMenu {
    pub fn start() -> Result<Self, SystemMenuError> {
        Self::start_with_visibility(true)
    }

    pub fn start_with_visibility(visible: bool) -> Result<Self, SystemMenuError> {
        // SAFETY: creation and all later cleanup occur on the GPUI owner
        // thread. The boxed callback state outlives the hidden HWND, whose
        // WM_NCDESTROY clears GWLP_USERDATA before the Box is dropped.
        unsafe { Self::start_inner(visible) }
    }

    unsafe fn start_inner(visible: bool) -> Result<Self, SystemMenuError> {
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

        let menu = match unsafe { CreatePopupMenu() } {
            Ok(menu) => menu,
            Err(_) => {
                let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
                return Err(SystemMenuError::MenuCreateFailed);
            }
        };
        if unsafe { AppendMenuW(menu, MF_STRING, OPEN_SETTINGS_ID, w!("Open Settings")) }.is_err()
            || unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, None) }.is_err()
            || unsafe { AppendMenuW(menu, MF_STRING, QUIT_ID, w!("Quit BongoCat")) }.is_err()
        {
            let _ = unsafe { DestroyMenu(menu) };
            let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
            return Err(SystemMenuError::MenuItemCreateFailed);
        }

        let (sender, receiver) = mpsc::channel();
        let mut state = Box::new(WindowState {
            sender: sender.clone(),
            menu,
        });
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

        let mut system_menu = Self {
            instance,
            window: Some(window),
            menu: Some(menu),
            state: Some(state),
            receiver,
            icon_added: false,
            class_registered: true,
        };
        if visible && unsafe { system_menu.add_icon() }.is_err() {
            let _ = system_menu.cleanup();
            return Err(SystemMenuError::StatusItemCreateFailed);
        }
        Ok(system_menu)
    }

    pub fn try_recv(&self) -> Option<SystemMenuAction> {
        self.receiver.try_recv().ok()
    }

    pub const fn is_visible(&self) -> bool {
        self.icon_added
    }

    pub fn set_visible(&mut self, visible: bool) -> Result<(), SystemMenuError> {
        if visible == self.icon_added {
            return Ok(());
        }
        // SAFETY: status icon mutation occurs on the GPUI owner thread while
        // the hidden HWND remains live and owned by self.
        unsafe {
            if visible {
                self.add_icon()
                    .map_err(|_| SystemMenuError::StatusItemUpdateFailed)
            } else {
                self.remove_icon()
                    .map_err(|_| SystemMenuError::StatusItemUpdateFailed)
            }
        }
    }

    #[doc(hidden)]
    pub fn request_action_for_smoke(
        &self,
        action: SystemMenuAction,
    ) -> Result<(), SystemMenuError> {
        let window = self.window.ok_or(SystemMenuError::EventQueueClosed)?;
        let command = match action {
            SystemMenuAction::OpenSettings => OPEN_SETTINGS_ID,
            SystemMenuAction::Quit => QUIT_ID,
        };
        // SAFETY: the hidden HWND remains owned by self. Posting WM_COMMAND
        // exercises the same callback path as selecting the native menu item.
        unsafe { PostMessageW(Some(window), WM_COMMAND, WPARAM(command), LPARAM(0)) }
            .map_err(|_| SystemMenuError::EventQueueClosed)
    }

    pub fn shutdown(mut self) -> Result<(), SystemMenuError> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> Result<(), SystemMenuError> {
        let mut failed = false;
        failed |= unsafe { self.remove_icon() }.is_err();
        if let Some(window) = self.window.take() {
            // SAFETY: the hidden HWND is owned by self and destroyed once.
            failed |= unsafe { DestroyWindow(window) }.is_err();
        }
        self.state.take();
        if let Some(menu) = self.menu.take() {
            // SAFETY: the HMENU is owned by self and no HWND references it now.
            failed |= unsafe { DestroyMenu(menu) }.is_err();
        }
        if self.class_registered {
            // SAFETY: the class was registered by this owner and all windows
            // using it have already been destroyed.
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
        let icon = unsafe { LoadIconW(None, IDI_APPLICATION) }.map_err(|_| ())?;
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: TRAY_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: CALLBACK_MESSAGE,
            hIcon: icon,
            ..Default::default()
        };
        copy_wide(&mut data.szTip, "BongoCat");
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
        // SAFETY: CreateWindowExW passes a valid WindowState pointer that
        // outlives this HWND. WM_NCDESTROY clears it before the owner drops it.
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
    // SAFETY: the pointer was installed from the owner's live Box above.
    let state = unsafe { &*state };
    match message {
        WM_COMMAND => {
            match wparam.0 & 0xffff {
                OPEN_SETTINGS_ID => {
                    let _ = state.sender.send(SystemMenuAction::OpenSettings);
                }
                QUIT_ID => {
                    let _ = state.sender.send(SystemMenuAction::Quit);
                }
                _ => {}
            }
            LRESULT(0)
        }
        CALLBACK_MESSAGE => {
            let notification = lparam.0 as u32 & 0xffff;
            match notification {
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

fn show_menu(window: HWND, menu: windows::Win32::UI::WindowsAndMessaging::HMENU) {
    let mut point = POINT::default();
    // SAFETY: window and menu remain owned by the live SystemMenu; this code
    // runs synchronously on their owner thread from the window procedure.
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
