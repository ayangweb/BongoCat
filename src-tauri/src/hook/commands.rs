use std::sync::Arc;

use tauri::State;

use super::{
    config::{AppConfig, SharedConfig},
    ipc::PendingRequests,
    permissions, settings,
};

#[tauri::command]
pub fn check_hook_status() -> Result<settings::HookHealth, String> {
    Ok(settings::verify_hook_integrity())
}

#[tauri::command]
pub fn install_notification_hook() -> Result<(), String> {
    settings::install_hook().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn uninstall_notification_hook() -> Result<(), String> {
    settings::uninstall_hook().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn check_pretooluse_hook_status() -> Result<bool, String> {
    settings::is_pretooluse_hook_installed().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn install_pretooluse_hook() -> Result<(), String> {
    settings::install_pretooluse_hook().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn uninstall_pretooluse_hook() -> Result<(), String> {
    settings::uninstall_pretooluse_hook().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_intercept_active(active: bool) -> Result<(), String> {
    permissions::set_intercept_active(active).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_intercept_active() -> bool {
    permissions::is_intercept_active()
}

#[tauri::command]
pub async fn respond_permission(
    request_id: String,
    decision: String,
    reason: Option<String>,
    pending: State<'_, Arc<PendingRequests>>,
) -> Result<(), String> {
    let allow = decision == "allow";
    let resolved = pending
        .resolve(&request_id, super::ipc::Decision { allow })
        .await;
    if !resolved {
        permissions::respond(&request_id, &decision, reason.as_deref())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_config(config: State<'_, SharedConfig>) -> AppConfig {
    config.read().map(|c| c.clone()).unwrap_or_default()
}

#[tauri::command]
pub fn update_config(
    config: State<'_, SharedConfig>,
    new_config: AppConfig,
) -> Result<(), String> {
    let path = dirs::home_dir()
        .map(|h| h.join(".BongoCat").join("config.toml"))
        .ok_or("Cannot find home directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let header = "# BongoCat 配置文件\n# 修改后自动生效，无需重启\n\n";
    let content = toml::to_string_pretty(&new_config).map_err(|e| e.to_string())?;
    std::fs::write(&path, format!("{}{}", header, content)).map_err(|e| e.to_string())?;
    if let Ok(mut guard) = config.write() {
        *guard = new_config;
    }
    Ok(())
}
