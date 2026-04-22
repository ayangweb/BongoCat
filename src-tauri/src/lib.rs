mod core;
mod hook;
mod utils;

use std::sync::Arc;

use core::{
    device::start_device_listening,
    gamepad::{start_gamepad_listing, stop_gamepad_listing},
    label::{set_label_text, LabelState},
    prevent_default, setup,
};
use hook::{
    claude,
    commands::{
        check_hook_status, check_pretooluse_hook_status, get_config, get_intercept_active,
        install_notification_hook, install_pretooluse_hook, respond_permission,
        set_intercept_active, uninstall_notification_hook, uninstall_pretooluse_hook,
        update_config,
    },
    config, ipc, permissions,
};
use tauri::{Manager, WindowEvent, generate_handler};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_custom_window::{
    MAIN_WINDOW_LABEL, PREFERENCE_WINDOW_LABEL, show_preference_window,
};
use utils::fs_extra::copy_dir;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shared_config = config::shared();
    let pending_requests = Arc::new(ipc::PendingRequests::new());
    let label_state = Arc::new(LabelState::new());

    let app = tauri::Builder::default()
        .manage(shared_config.clone())
        .manage(pending_requests.clone())
        .manage(label_state.clone())
        .setup(move |app| {
            let app_handle = app.handle();

            let main_window = app.get_webview_window(MAIN_WINDOW_LABEL).unwrap();

            let preference_window = app.get_webview_window(PREFERENCE_WINDOW_LABEL).unwrap();

            setup::default(app_handle, main_window.clone(), preference_window.clone());

            // 启动配置热重载监听
            let _config_watcher = config::start_watching(shared_config.clone(), app_handle.clone());
            // 泄漏 watcher 以保持其在整个应用生命周期内存活
            Box::leak(Box::new(_config_watcher));

            // 启动权限请求文件监听
            if let Ok(watcher) = permissions::start_permission_watching(app_handle.clone()) {
                Box::leak(Box::new(watcher));
            }

            // 启动 Claude JSONL 事件监听
            if let Ok(watcher) = claude::start_watching(app_handle.clone(), shared_config.clone()) {
                Box::leak(Box::new(watcher));
            }

            // 启动通知 signal 文件监听
            if let Ok(watcher) = hook::settings::start_signal_watching(app_handle.clone()) {
                Box::leak(Box::new(watcher));
            }

            // 启动 IPC Unix Domain Socket 服务器
            let pending = pending_requests.clone();
            let app_h = app_handle.clone();
            let cfg_for_ipc = shared_config.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = ipc::serve(app_h, pending, cfg_for_ipc).await {
                    eprintln!("IPC server error: {e}");
                }
            });

            // 清理过期请求/响应文件
            permissions::cleanup_stale_files();

            Ok(())
        })
        .invoke_handler(generate_handler![
            copy_dir,
            start_device_listening,
            start_gamepad_listing,
            stop_gamepad_listing,
            set_label_text,
            check_hook_status,
            install_notification_hook,
            uninstall_notification_hook,
            check_pretooluse_hook_status,
            install_pretooluse_hook,
            uninstall_pretooluse_hook,
            set_intercept_active,
            get_intercept_active,
            respond_permission,
            get_config,
            update_config,
        ])
        .plugin(tauri_plugin_custom_window::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_pinia::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(prevent_default::init())
        .plugin(tauri_plugin_single_instance::init(
            |app_handle, _argv, _cwd| {
                show_preference_window(app_handle);
            },
        ))
        .plugin(
            tauri_plugin_log::Builder::new()
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .filter(|metadata| !metadata.target().contains("gilrs"))
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_macos_permissions::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_locale::init())
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();

                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(|app_handle, event| match event {
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            show_preference_window(app_handle);
        }
        tauri::RunEvent::Exit => {
            ipc::cleanup_socket();
            let _ = app_handle;
        }
        _ => {
            let _ = app_handle;
        }
    });
}
