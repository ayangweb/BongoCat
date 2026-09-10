use crate::{SingleInstanceAction, SingleInstanceEnvironment, SingleInstanceError};
use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM,
            LRESULT, WPARAM,
        },
        System::{LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
        UI::WindowsAndMessaging::{
            CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW,
            GWLP_USERDATA, GetWindowLongPtrW, PostMessageW, RegisterClassW, RegisterWindowMessageW,
            SetWindowLongPtrW, UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_NCCREATE,
            WM_NCDESTROY, WNDCLASSW,
        },
    },
    core::{PCWSTR, w},
};

const DEVELOPMENT_MUTEX: PCWSTR = w!("Local\\com.ayangweb.bongo-cat.development.single-instance");
const PRODUCTION_MUTEX: PCWSTR = w!("Local\\com.ayangweb.bongo-cat.production.single-instance");
const DEVELOPMENT_WINDOW_CLASS: PCWSTR = w!("BongoCatDevelopmentSingleInstanceOwnerWindow");
const PRODUCTION_WINDOW_CLASS: PCWSTR = w!("BongoCatProductionSingleInstanceOwnerWindow");
const DEVELOPMENT_WINDOW_TITLE: PCWSTR = w!("BongoCat Development Instance Owner");
const PRODUCTION_WINDOW_TITLE: PCWSTR = w!("BongoCat Production Instance Owner");
const DEVELOPMENT_WAKE_MESSAGE: PCWSTR = w!("com.ayangweb.bongo-cat.development.open-settings");
const PRODUCTION_WAKE_MESSAGE: PCWSTR = w!("com.ayangweb.bongo-cat.production.open-settings");
const PRIMARY_WAIT_TIMEOUT: Duration = Duration::from_secs(2);
const PRIMARY_RETRY_INTERVAL: Duration = Duration::from_millis(10);

struct EnvironmentNames {
    mutex: PCWSTR,
    window_class: PCWSTR,
    window_title: PCWSTR,
    wake_message: PCWSTR,
}

impl SingleInstanceEnvironment {
    fn names(self) -> EnvironmentNames {
        match self {
            Self::Development => EnvironmentNames {
                mutex: DEVELOPMENT_MUTEX,
                window_class: DEVELOPMENT_WINDOW_CLASS,
                window_title: DEVELOPMENT_WINDOW_TITLE,
                wake_message: DEVELOPMENT_WAKE_MESSAGE,
            },
            Self::Production => EnvironmentNames {
                mutex: PRODUCTION_MUTEX,
                window_class: PRODUCTION_WINDOW_CLASS,
                window_title: PRODUCTION_WINDOW_TITLE,
                wake_message: PRODUCTION_WAKE_MESSAGE,
            },
        }
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this owner contains exactly one successful CreateMutexW
        // handle and Drop runs once after the instance decision is complete.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct WindowState {
    sender: Sender<SingleInstanceAction>,
    wake_message: u32,
}

pub enum SingleInstanceStart {
    Primary(SingleInstance),
    SecondaryNotified,
}

pub struct SingleInstance {
    instance: HINSTANCE,
    window_class: PCWSTR,
    window: Option<HWND>,
    state: Option<Box<WindowState>>,
    receiver: Receiver<SingleInstanceAction>,
    _mutex: OwnedHandle,
    class_registered: bool,
}

impl SingleInstance {
    pub fn acquire(
        environment: SingleInstanceEnvironment,
    ) -> Result<SingleInstanceStart, SingleInstanceError> {
        let names = environment.names();
        // The mutex is intentionally not owned by a thread. Its kernel object
        // lifetime, held by the primary process handle, is the uniqueness
        // boundary and avoids thread-affine ReleaseMutex during GPUI shutdown.
        let mutex = OwnedHandle(
            unsafe { CreateMutexW(None, false, names.mutex) }
                .map_err(|_| SingleInstanceError::MutexCreateFailed)?,
        );
        // SAFETY: CreateMutexW documents ERROR_ALREADY_EXISTS as the immediate
        // last-error value on a successful open of an existing named mutex.
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        // SAFETY: the message name is a process-lifetime static UTF-16 string.
        let wake_message = unsafe { RegisterWindowMessageW(names.wake_message) };
        if wake_message == 0 {
            return Err(SingleInstanceError::WakeMessageRegistrationFailed);
        }
        if already_exists {
            notify_primary(names.window_class, wake_message)?;
            return Ok(SingleInstanceStart::SecondaryNotified);
        }

        // SAFETY: the mutex establishes this process as primary. Creation and
        // all cleanup remain on the GPUI owner thread; callback state outlives
        // the hidden HWND and WM_NCDESTROY clears its borrowed pointer.
        unsafe { Self::create_primary(names, wake_message, mutex) }
            .map(SingleInstanceStart::Primary)
    }

    unsafe fn create_primary(
        names: EnvironmentNames,
        wake_message: u32,
        mutex: OwnedHandle,
    ) -> Result<Self, SingleInstanceError> {
        let module = unsafe { GetModuleHandleW(None) }
            .map_err(|_| SingleInstanceError::WindowClassRegistrationFailed)?;
        let instance = HINSTANCE(module.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(single_instance_window_proc),
            hInstance: instance,
            lpszClassName: names.window_class,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(SingleInstanceError::WindowClassRegistrationFailed);
        }

        let (sender, receiver) = mpsc::channel();
        let mut state = Box::new(WindowState {
            sender,
            wake_message,
        });
        let state_ptr = (&mut *state) as *mut WindowState;
        let window = match unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                names.window_class,
                names.window_title,
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
                let _ = unsafe { UnregisterClassW(names.window_class, Some(instance)) };
                return Err(SingleInstanceError::WindowCreateFailed);
            }
        };

        Ok(Self {
            instance,
            window_class: names.window_class,
            window: Some(window),
            state: Some(state),
            receiver,
            _mutex: mutex,
            class_registered: true,
        })
    }

    pub fn try_recv(&self) -> Option<SingleInstanceAction> {
        self.receiver.try_recv().ok()
    }

    pub fn shutdown(mut self) -> Result<(), SingleInstanceError> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> Result<(), SingleInstanceError> {
        let mut failed = false;
        if let Some(window) = self.window.take() {
            // SAFETY: the hidden HWND is owned by self and destroyed once on
            // its creation thread before the callback Box is dropped.
            failed |= unsafe { DestroyWindow(window) }.is_err();
        }
        self.state.take();
        if self.class_registered {
            // SAFETY: all windows using this process-local class are gone.
            failed |= unsafe { UnregisterClassW(self.window_class, Some(self.instance)) }.is_err();
            self.class_registered = false;
        }
        if failed {
            Err(SingleInstanceError::ShutdownFailed)
        } else {
            Ok(())
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn notify_primary(window_class: PCWSTR, wake_message: u32) -> Result<(), SingleInstanceError> {
    let deadline = Instant::now() + PRIMARY_WAIT_TIMEOUT;
    loop {
        // SAFETY: the class pointer is a process-lifetime UTF-16 string; a
        // successful HWND is used only for an asynchronous message post.
        if let Ok(window) = unsafe { FindWindowW(window_class, None) } {
            return unsafe { PostMessageW(Some(window), wake_message, WPARAM(0), LPARAM(0)) }
                .map_err(|_| SingleInstanceError::WakeFailed);
        }
        if Instant::now() >= deadline {
            return Err(SingleInstanceError::PrimaryUnavailable);
        }
        thread::sleep(PRIMARY_RETRY_INTERVAL);
    }
}

unsafe extern "system" fn single_instance_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        // SAFETY: CreateWindowExW passes the live WindowState pointer owned by
        // SingleInstance until after this HWND has received WM_NCDESTROY.
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
    // SAFETY: GWLP_USERDATA points to the owner's live Box until WM_NCDESTROY.
    let state = unsafe { &*state };
    if message == state.wake_message {
        let _ = state.sender.send(SingleInstanceAction::OpenSettings);
        LRESULT(0)
    } else {
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }
}
