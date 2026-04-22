use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use serde_json::Value;
use tauri::Emitter;

const SIGNAL_FILE_NAME: &str = "waiting-signal";
const SCRIPT_FILE_NAME: &str = "notify-waiting.sh";
const PRETOOLUSE_SCRIPT_NAME: &str = "pretooluse-hook.sh";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_BACKUPS: usize = 3;

// ---- 路径辅助 ----

fn claude_settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("settings.json"))
}

pub fn signal_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".BongoCat"))
}

pub fn signal_file_path() -> Option<PathBuf> {
    signal_dir().map(|d| d.join(SIGNAL_FILE_NAME))
}

fn signal_script_path() -> Option<PathBuf> {
    signal_dir().map(|d| d.join(SCRIPT_FILE_NAME))
}

fn pretooluse_script_path() -> Option<PathBuf> {
    signal_dir().map(|d| d.join(PRETOOLUSE_SCRIPT_NAME))
}

pub fn requests_dir() -> Option<PathBuf> {
    signal_dir().map(|d| d.join("requests"))
}

pub fn responses_dir() -> Option<PathBuf> {
    signal_dir().map(|d| d.join("responses"))
}

// ---- 原子写入 + 备份 ----

fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn backup_settings(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup_path = parent.join(format!("settings.json.bongocat.bak.{}", ts));
    std::fs::copy(path, &backup_path)?;

    let mut backups: Vec<_> = std::fs::read_dir(parent)?
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.starts_with("settings.json.bongocat.bak."))
                .unwrap_or(false)
        })
        .collect();
    if backups.len() > MAX_BACKUPS {
        backups.sort_by_key(|e| e.file_name());
        for old in &backups[..backups.len() - MAX_BACKUPS] {
            let _ = std::fs::remove_file(old.path());
        }
    }
    Ok(())
}

// ---- settings.json 读写 ----

fn read_claude_settings() -> Result<Value> {
    let path = claude_settings_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = std::fs::read(&path)?;
    let value: Value = serde_json::from_slice(&content)
        .map_err(|e| anyhow::anyhow!("settings.json is not valid JSON: {}", e))?;
    Ok(value)
}

fn write_claude_settings(value: &Value) -> Result<()> {
    let path = claude_settings_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    backup_settings(&path)?;
    let content = serde_json::to_string_pretty(value)?;
    serde_json::from_str::<Value>(&content)?;
    atomic_write(&path, content.as_bytes())?;
    Ok(())
}

// ---- managed 标记辅助 ----

fn is_managed(entry: &Value) -> bool {
    entry
        .get("_bongocat_managed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn make_managed_entry(matcher: &str, script_path: &Path) -> Value {
    serde_json::json!({
        "_bongocat_managed": true,
        "_bongocat_version": APP_VERSION,
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": script_path.display().to_string()
        }]
    })
}

fn remove_managed_entries(arr: &mut Vec<Value>) {
    arr.retain(|entry| !is_managed(entry));
}

fn extract_script_path(entry: &Value) -> Option<String> {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .and_then(|hooks| hooks.first())
        .and_then(|hook| hook.get("command"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
}

// ---- Hook 状态 ----

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookHealth {
    pub notification: HookEntryHealth,
    pub pre_tool_use: HookEntryHealth,
    pub settings_valid: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HookEntryHealth {
    Healthy,
    Inactive,
    Broken,
}

// ---- 公共 API ----

pub fn is_pretooluse_hook_installed() -> Result<bool> {
    let settings = read_claude_settings()?;
    Ok(has_managed_hook(&settings, "PreToolUse"))
}

fn has_managed_hook(settings: &Value, hook_type: &str) -> bool {
    settings
        .get("hooks")
        .and_then(|h| h.get(hook_type))
        .and_then(|n| n.as_array())
        .map(|arr| arr.iter().any(is_managed))
        .unwrap_or(false)
}

pub fn install_hook() -> Result<()> {
    let dir = signal_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    std::fs::create_dir_all(&dir)?;

    let script_path = signal_script_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    let signal_file = signal_file_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;

    let script_content = format!(
        "#!/bin/bash\n# BongoCat notification hook\nLOG_DIR=\"$HOME/.BongoCat/logs\"\nmkdir -p \"$LOG_DIR\"\nexec 2>>\"$LOG_DIR/hook.log\"\necho \"[$(date -u +%%FT%%TZ)] Notification hook invoked\" >&2\ndate +%s > \"{}\"\n",
        signal_file.display()
    );
    std::fs::write(&script_path, &script_content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;
    }

    let mut settings = read_claude_settings()?;
    let hook_entry = make_managed_entry("permission_prompt", &script_path);
    inject_hook(&mut settings, "Notification", hook_entry)?;
    write_claude_settings(&settings)?;
    Ok(())
}

pub fn uninstall_hook() -> Result<()> {
    let mut settings = read_claude_settings()?;
    if remove_hook(&mut settings, "Notification") {
        write_claude_settings(&settings)?;
    }
    if let Some(script) = signal_script_path() {
        let _ = std::fs::remove_file(&script);
    }
    if let Some(signal) = signal_file_path() {
        let _ = std::fs::remove_file(&signal);
    }
    Ok(())
}

pub fn install_pretooluse_hook() -> Result<()> {
    let dir = signal_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    std::fs::create_dir_all(&dir)?;

    if let Some(req_dir) = requests_dir() {
        std::fs::create_dir_all(&req_dir)?;
    }
    if let Some(resp_dir) = responses_dir() {
        std::fs::create_dir_all(&resp_dir)?;
    }

    let script_path = pretooluse_script_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    let intercept_flag = dir.join("intercept-active");
    let sock_path = dir.join("ipc.sock");

    let script_content = format!(
        r#"#!/bin/bash
# BongoCat PreToolUse hook — Unix Domain Socket IPC
LOG_DIR="$HOME/.BongoCat/logs"
mkdir -p "$LOG_DIR"
exec 2>>"$LOG_DIR/hook.log"
echo "[$(date -u +%FT%TZ)] PreToolUse hook invoked" >&2

SOCK="{sock_path}"

if [ ! -f "{intercept_flag}" ]; then
  exit 0
fi

if [ ! -S "$SOCK" ]; then
  echo "[$(date -u +%FT%TZ)] [warn] socket missing, pass-through" >&2
  exit 0
fi

INPUT=$(cat)
REQUEST_ID=$(uuidgen | tr '[:upper:]' '[:lower:]')
echo "[$(date -u +%FT%TZ)] request=$REQUEST_ID" >&2

RESP=$(printf '{{"id":"%s","payload":%s}}\n' "$REQUEST_ID" "$INPUT" \
  | timeout 125 nc -U "$SOCK" 2>/dev/null)

if [ -z "$RESP" ]; then
  echo "[$(date -u +%FT%TZ)] [warn] no response from socket, pass-through" >&2
  exit 0
fi

echo "[$(date -u +%FT%TZ)] response=$RESP" >&2

case "$RESP" in
  *'"allow"'*) DECISION="allow" ;;
  *'"deny"'*)  DECISION="deny" ;;
  *)           DECISION="ask" ;;
esac

if [ "$DECISION" = "deny" ]; then
  echo '{{"hookSpecificOutput":{{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"BongoCat user denied"}}}}'
  exit 2
fi

if [ "$DECISION" = "allow" ]; then
  echo '{{"hookSpecificOutput":{{"hookEventName":"PreToolUse","permissionDecision":"allow","permissionDecisionReason":"BongoCat user approved"}}}}'
  exit 0
fi

exit 0
"#,
        sock_path = sock_path.display(),
        intercept_flag = intercept_flag.display(),
    );

    std::fs::write(&script_path, &script_content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;
    }

    let mut settings = read_claude_settings()?;
    let hook_entry = make_managed_entry("", &script_path);
    inject_hook(&mut settings, "PreToolUse", hook_entry)?;
    write_claude_settings(&settings)?;
    Ok(())
}

pub fn uninstall_pretooluse_hook() -> Result<()> {
    let mut settings = read_claude_settings()?;
    if remove_hook(&mut settings, "PreToolUse") {
        write_claude_settings(&settings)?;
    }
    if let Some(script) = pretooluse_script_path() {
        let _ = std::fs::remove_file(&script);
    }
    Ok(())
}

pub fn verify_hook_integrity() -> HookHealth {
    let settings_valid = match read_claude_settings() {
        Ok(mut settings) => {
            let mut dirty = false;
            for hook_type in &["Notification", "PreToolUse"] {
                if let Some(arr) = settings
                    .get_mut("hooks")
                    .and_then(|h| h.get_mut(*hook_type))
                    .and_then(|n| n.as_array_mut())
                {
                    let before = arr.len();
                    arr.retain(|entry| {
                        if !is_managed(entry) {
                            return true;
                        }
                        if let Some(script) = extract_script_path(entry) {
                            Path::new(&script).exists()
                        } else {
                            false
                        }
                    });
                    if arr.len() != before {
                        dirty = true;
                    }
                }
            }
            if dirty {
                let _ = write_claude_settings(&settings);
            }
            true
        }
        Err(_) => false,
    };

    HookHealth {
        notification: check_entry_health("Notification"),
        pre_tool_use: check_entry_health("PreToolUse"),
        settings_valid,
    }
}

fn check_entry_health(hook_type: &str) -> HookEntryHealth {
    let settings = match read_claude_settings() {
        Ok(s) => s,
        Err(_) => return HookEntryHealth::Broken,
    };
    let entries = settings
        .get("hooks")
        .and_then(|h| h.get(hook_type))
        .and_then(|n| n.as_array());
    let Some(arr) = entries else {
        return HookEntryHealth::Inactive;
    };
    let managed: Vec<_> = arr.iter().filter(|e| is_managed(e)).collect();
    if managed.is_empty() {
        return HookEntryHealth::Inactive;
    }
    for entry in &managed {
        if let Some(script) = extract_script_path(entry) {
            if !Path::new(&script).exists() {
                return HookEntryHealth::Broken;
            }
        } else {
            return HookEntryHealth::Broken;
        }
    }
    HookEntryHealth::Healthy
}

fn inject_hook(settings: &mut Value, hook_type: &str, entry: Value) -> Result<()> {
    let hooks = settings
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json root is not an object"))?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let arr = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks is not an object"))?
        .entry(hook_type)
        .or_insert_with(|| serde_json::json!([]));
    let arr = arr
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("{} is not an array", hook_type))?;
    remove_managed_entries(arr);
    arr.push(entry);
    Ok(())
}

fn remove_hook(settings: &mut Value, hook_type: &str) -> bool {
    if let Some(arr) = settings
        .get_mut("hooks")
        .and_then(|h| h.get_mut(hook_type))
        .and_then(|n| n.as_array_mut())
    {
        let before = arr.len();
        remove_managed_entries(arr);
        arr.len() != before
    } else {
        false
    }
}

// ---- Signal 文件监听 ----

pub fn start_signal_watching(
    app_handle: tauri::AppHandle,
) -> anyhow::Result<Debouncer<notify::RecommendedWatcher>> {
    let watch_dir = signal_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    std::fs::create_dir_all(&watch_dir)?;

    let signal_path = signal_file_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;

    let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();

    let app = app_handle;
    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            let events = match result {
                Ok(events) => events,
                Err(_) => continue,
            };
            for event in events {
                if event.path != signal_path {
                    continue;
                }
                if !event.path.exists() {
                    continue;
                }
                crate::hook::claude::update_last_event();
                crate::hook::claude::set_active(true);
                let _ = app.emit("claude-event", crate::hook::claude::waiting_event());
            }
        }
    });

    let mut debouncer = new_debouncer(Duration::from_millis(100), tx)?;
    debouncer
        .watcher()
        .watch(&watch_dir, RecursiveMode::NonRecursive)?;
    Ok(debouncer)
}

