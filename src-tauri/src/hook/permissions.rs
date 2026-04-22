use std::path::PathBuf;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use super::settings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub request_id: String,
    pub session_id: String,
    pub tool_name: String,
    pub tool_input: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponse {
    pub decision: String,
    pub reason: Option<String>,
}

fn sanitize_request_id(request_id: &str) -> Option<&str> {
    let valid = !request_id.is_empty()
        && request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    valid.then_some(request_id)
}

pub fn start_permission_watching(
    app_handle: tauri::AppHandle,
) -> anyhow::Result<Debouncer<notify::RecommendedWatcher>> {
    let requests_dir = settings::requests_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    std::fs::create_dir_all(&requests_dir)?;

    let watch_dir = requests_dir.clone();
    let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();

    let app = app_handle;
    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            let events = match result {
                Ok(events) => events,
                Err(_) => continue,
            };
            for event in events {
                let path = &event.path;
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if !path.exists() {
                    continue;
                }
                let content = match std::fs::read_to_string(path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let request: PermissionRequest = match serde_json::from_str(&content) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let _ = app.emit("permission-request", &request);
            }
        }
    });

    let mut debouncer = new_debouncer(Duration::from_millis(100), tx)?;
    debouncer
        .watcher()
        .watch(&watch_dir, RecursiveMode::NonRecursive)?;
    Ok(debouncer)
}

pub fn respond(request_id: &str, decision: &str, reason: Option<&str>) -> anyhow::Result<()> {
    let responses_dir = settings::responses_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    std::fs::create_dir_all(&responses_dir)?;

    let safe_request_id = sanitize_request_id(request_id)
        .ok_or_else(|| anyhow::anyhow!("Invalid request id"))?;

    let response = PermissionResponse {
        decision: decision.to_string(),
        reason: reason.map(|s| s.to_string()),
    };
    let response_path = responses_dir.join(format!("{}.json", safe_request_id));
    let tmp_path = responses_dir.join(format!("{}.tmp", safe_request_id));
    let content = serde_json::to_string(&response)?;
    std::fs::write(&tmp_path, &content)?;
    std::fs::rename(&tmp_path, &response_path)?;
    Ok(())
}

pub fn set_intercept_active(active: bool) -> anyhow::Result<()> {
    let flag_path = intercept_flag_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    if active {
        if let Some(parent) = flag_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&flag_path, "1")?;
    } else {
        let _ = std::fs::remove_file(&flag_path);
    }
    Ok(())
}

pub fn is_intercept_active() -> bool {
    intercept_flag_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

fn intercept_flag_path() -> Option<PathBuf> {
    settings::signal_dir().map(|d| d.join("intercept-active"))
}

pub fn cleanup_stale_files() {
    let now = std::time::SystemTime::now();
    let max_age = Duration::from_secs(120);
    for dir_fn in [settings::requests_dir, settings::responses_dir] {
        if let Some(dir) = dir_fn()
            && let Ok(entries) = std::fs::read_dir(&dir)
        {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata()
                    && let Ok(modified) = metadata.modified()
                    && now.duration_since(modified).unwrap_or_default() > max_age
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}
