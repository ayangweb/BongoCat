use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub idle_sleep_secs: u64,
    pub session_window_secs: u64,
    pub hook_timeout_secs: u64,
    pub jsonl_debounce_ms: u64,
    pub active_state_timeout_secs: u64,
    pub session_poll_fallback_secs: u64,
    pub cursor_track_near_ms: u64,
    pub cursor_track_far_ms: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            idle_sleep_secs: 300,
            session_window_secs: 600,
            hook_timeout_secs: 120,
            jsonl_debounce_ms: 300,
            active_state_timeout_secs: 30,
            session_poll_fallback_secs: 30,
            cursor_track_near_ms: 32,
            cursor_track_far_ms: 150,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".BongoCat").join("config.toml"))
}

pub fn load() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };
    if !path.exists() {
        let default = AppConfig::default();
        if let Ok(content) = toml::to_string_pretty(&default) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let header = "# BongoCat 配置文件\n# 修改后自动生效，无需重启\n\n";
            let _ = std::fs::write(&path, format!("{}{}", header, content));
        }
        return default;
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => toml::from_str::<AppConfig>(&content).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

pub type SharedConfig = Arc<RwLock<AppConfig>>;

pub fn shared() -> SharedConfig {
    Arc::new(RwLock::new(load()))
}

pub fn start_watching(
    config: SharedConfig,
    app_handle: tauri::AppHandle,
) -> Option<Debouncer<notify::RecommendedWatcher>> {
    let path = config_path()?;
    let dir = path.parent()?.to_path_buf();

    let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();

    let cfg = config;
    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            let events = match result {
                Ok(events) => events,
                Err(_) => continue,
            };
            let config_changed = events.iter().any(|e| {
                e.path.file_name().and_then(|n| n.to_str()) == Some("config.toml")
            });
            if !config_changed {
                continue;
            }
            let new_config = load();
            if let Ok(mut guard) = cfg.write() {
                *guard = new_config.clone();
            }
            let _ = app_handle.emit("hook-config-changed", &new_config);
        }
    });

    let mut debouncer = new_debouncer(Duration::from_millis(500), tx).ok()?;
    debouncer
        .watcher()
        .watch(&dir, RecursiveMode::NonRecursive)
        .ok()?;
    Some(debouncer)
}
