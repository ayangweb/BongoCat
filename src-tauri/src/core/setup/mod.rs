use tauri::{AppHandle, WebviewWindow};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub mod common;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub use common::*;

#[cfg(target_os = "windows")]
pub use windows::*;

pub fn default(
    app_handle: &AppHandle,
    main_window: WebviewWindow,
    preference_window: WebviewWindow,
) {
    #[cfg(debug_assertions)]
    main_window.open_devtools();

    platform(app_handle, main_window.clone(), preference_window.clone());
}
