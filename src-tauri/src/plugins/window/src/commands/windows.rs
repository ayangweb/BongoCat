use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Runtime, WebviewWindow, command};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
};

static TOPMOST_RUNNING: OnceLock<Arc<AtomicBool>> = OnceLock::new();

#[command]
pub async fn show_window<R: Runtime>(_app_handle: AppHandle<R>, window: WebviewWindow<R>) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

#[command]
pub async fn hide_window<R: Runtime>(_app_handle: AppHandle<R>, window: WebviewWindow<R>) {
    let _ = window.hide();
}

#[command]
pub async fn set_always_on_top<R: Runtime>(
    _app_handle: AppHandle<R>,
    window: WebviewWindow<R>,
    always_on_top: bool,
) {
    let running = TOPMOST_RUNNING.get_or_init(|| Arc::new(AtomicBool::new(false)));

    let Ok(hwnd) = window.hwnd() else { return };
    let raw_hwnd = hwnd.0 as isize;

    if always_on_top {
        let Ok(_) = running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        else {
            return;
        };

        let running = Arc::clone(running);

        thread::spawn(move || {
            let hwnd = HWND(raw_hwnd as *mut _);

            while running.load(Ordering::SeqCst) {
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
                thread::sleep(Duration::from_millis(16));
            }
        });
    } else {
        running.store(false, Ordering::SeqCst);

        let hwnd = HWND(raw_hwnd as *mut _);

        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_NOTOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

#[command]
pub async fn set_taskbar_visibility<R: Runtime>(window: WebviewWindow<R>, visible: bool) {
    let _ = window.set_skip_taskbar(!visible);
}
