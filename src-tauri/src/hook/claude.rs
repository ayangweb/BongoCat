use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use super::config::SharedConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ClaudeEventSource {
    Claude,
    Signal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ClaudeEventState {
    Idle,
    Thinking,
    Coding,
    Success,
    Error,
    Waiting,
    Dormant,
}

impl ClaudeEventState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaudeEventState::Idle => "idle",
            ClaudeEventState::Thinking => "thinking",
            ClaudeEventState::Coding => "coding",
            ClaudeEventState::Success => "success",
            ClaudeEventState::Error => "error",
            ClaudeEventState::Waiting => "waiting",
            ClaudeEventState::Dormant => "dormant",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEvent {
    pub state: String,
    pub source: ClaudeEventSource,
    pub session_id: Option<String>,
    pub project_name: Option<String>,
    pub detail: Option<String>,
    pub raw_text: Option<String>,
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonlEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    message: Option<MessagePayload>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MessagePayload {
    content: Option<serde_json::Value>,
    stop_reason: Option<String>,
}

struct FileState {
    offset: u64,
    #[cfg(unix)]
    inode: u64,
    partial_line: String,
}

impl FileState {
    #[cfg(unix)]
    fn new(inode: u64) -> Self {
        Self {
            offset: 0,
            inode,
            partial_line: String::new(),
        }
    }

    #[cfg(not(unix))]
    fn new() -> Self {
        Self {
            offset: 0,
            partial_line: String::new(),
        }
    }
}

struct IncrementalParser {
    files: HashMap<PathBuf, FileState>,
}

impl IncrementalParser {
    fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    fn parse_new_entries(&mut self, path: &Path) -> Vec<JsonlEntry> {
        let mut entries = Vec::new();
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(_) => return entries,
        };
        let metadata = match file.metadata() {
            Ok(metadata) => metadata,
            Err(_) => return entries,
        };

        let file_size = metadata.len();

        #[cfg(unix)]
        let current_inode = {
            use std::os::unix::fs::MetadataExt;
            metadata.ino()
        };

        let state = self
            .files
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                #[cfg(unix)]
                {
                    FileState::new(current_inode)
                }
                #[cfg(not(unix))]
                {
                    FileState::new()
                }
            });

        #[cfg(unix)]
        if state.inode != current_inode {
            *state = FileState::new(current_inode);
        }

        if file_size < state.offset {
            state.offset = 0;
            state.partial_line.clear();
        }

        if file.seek(SeekFrom::Start(state.offset)).is_err() {
            return entries;
        }

        let mut buffer = String::new();
        if file.read_to_string(&mut buffer).is_err() || buffer.is_empty() {
            return entries;
        }

        let combined = format!("{}{}", state.partial_line, buffer);
        state.partial_line.clear();

        if !combined.ends_with('\n')
            && let Some((complete, partial)) = combined.rsplit_once('\n')
        {
            state.partial_line = partial.to_string();
            for line in complete.lines() {
                push_entry(&mut entries, line);
            }
            state.offset += buffer.len() as u64;
            return entries;
        }

        for line in combined.lines() {
            push_entry(&mut entries, line);
        }

        state.offset += buffer.len() as u64;
        entries
    }
}

fn push_entry(entries: &mut Vec<JsonlEntry>, line: &str) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }

    if let Ok(entry) = serde_json::from_str::<JsonlEntry>(trimmed) {
        entries.push(entry);
    }
}

fn claude_projects_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".claude").join("projects"))
}

fn project_name_from_path(path: &Path) -> Option<String> {
    let folder = path.parent()?.file_name()?.to_str()?;
    Some(folder.replace('-', "/"))
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}…", truncated)
    } else {
        truncated
    }
}

fn extract_text(content: &serde_json::Value) -> Option<String> {
    let items = content.as_array()?;
    items.iter().find_map(|item| {
        if item.get("type").and_then(|value| value.as_str()) != Some("text") {
            return None;
        }
        item.get("text")
            .and_then(|value| value.as_str())
            .map(|text| truncate_text(text, 36))
            .filter(|text| !text.is_empty())
    })
}

fn extract_tool_name(content: &serde_json::Value) -> Option<String> {
    let items = content.as_array()?;
    items.iter().find_map(|item| {
        if item.get("type").and_then(|value| value.as_str()) != Some("tool_use") {
            return None;
        }
        item.get("name")
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
    })
}

fn has_tool_use(content: &serde_json::Value) -> bool {
    content
        .as_array()
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(|value| value.as_str()) == Some("tool_use")
            })
        })
}

fn contains_only_thinking(content: &serde_json::Value) -> bool {
    content
        .as_array()
        .is_some_and(|items| {
            !items.is_empty()
                && items.iter().all(|item| {
                    item.get("type").and_then(|value| value.as_str()) == Some("thinking")
                })
        })
}

fn has_error(content: &serde_json::Value) -> bool {
    content
        .as_array()
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("is_error")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
            })
        })
}

fn resolve_event(entry: &JsonlEntry, project_name: Option<String>) -> Option<ClaudeEvent> {
    let entry_type = entry.entry_type.as_deref()?;

    match entry_type {
        "file-history-snapshot" | "last-prompt" => None,
        "user" => {
            let raw_text = entry
                .message
                .as_ref()
                .and_then(|message| message.content.as_ref())
                .and_then(extract_text);
            Some(ClaudeEvent {
                state: ClaudeEventState::Thinking.as_str().to_string(),
                source: ClaudeEventSource::Claude,
                session_id: entry.session_id.clone(),
                project_name,
                detail: raw_text.clone(),
                raw_text,
                tool_name: None,
            })
        }
        "assistant" => {
            let message = entry.message.as_ref()?;
            let content = message.content.as_ref();
            let raw_text = content.and_then(extract_text);
            let tool_name = content.and_then(extract_tool_name);

            let state = match message.stop_reason.as_deref() {
                Some("end_turn") | Some("max_tokens") | Some("stop_sequence") => {
                    ClaudeEventState::Success
                }
                _ => {
                    if content.is_some_and(has_tool_use) {
                        ClaudeEventState::Coding
                    } else if content.is_some_and(contains_only_thinking) {
                        ClaudeEventState::Thinking
                    } else {
                        ClaudeEventState::Thinking
                    }
                }
            };

            Some(ClaudeEvent {
                state: state.as_str().to_string(),
                source: ClaudeEventSource::Claude,
                session_id: entry.session_id.clone(),
                project_name,
                detail: raw_text.clone().or_else(|| tool_name.clone()),
                raw_text,
                tool_name,
            })
        }
        "tool_result" => {
            let message = entry.message.as_ref()?;
            let content = message.content.as_ref();
            let is_error = content.is_some_and(has_error);
            let raw_text = if is_error {
                content.and_then(extract_text)
            } else {
                None
            };
            let state = if is_error {
                ClaudeEventState::Error
            } else {
                ClaudeEventState::Coding
            };

            Some(ClaudeEvent {
                state: state.as_str().to_string(),
                source: ClaudeEventSource::Claude,
                session_id: entry.session_id.clone(),
                project_name,
                detail: raw_text.clone(),
                raw_text,
                tool_name: None,
            })
        }
        _ => None,
    }
}

// ---- 超时检测状态 ----

static LAST_EVENT_MILLIS: AtomicU64 = AtomicU64::new(0);
static IS_ACTIVE: AtomicBool = AtomicBool::new(false);

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn update_last_event() {
    LAST_EVENT_MILLIS.store(now_millis(), Ordering::Relaxed);
}

pub fn set_active(active: bool) {
    IS_ACTIVE.store(active, Ordering::Relaxed);
}

fn is_active_state(state: &str) -> bool {
    !matches!(state, "idle" | "dormant")
}

pub fn waiting_event() -> ClaudeEvent {
    ClaudeEvent {
        state: ClaudeEventState::Waiting.as_str().to_string(),
        source: ClaudeEventSource::Signal,
        session_id: None,
        project_name: None,
        detail: Some("permission_prompt".to_string()),
        raw_text: None,
        tool_name: None,
    }
}

pub fn start_watching(
    app_handle: tauri::AppHandle,
    config: SharedConfig,
) -> anyhow::Result<Debouncer<notify::RecommendedWatcher>> {
    let projects_dir = claude_projects_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    std::fs::create_dir_all(&projects_dir)?;

    let debounce_ms = config
        .read()
        .map(|value| value.jsonl_debounce_ms)
        .unwrap_or(300)
        .max(50);

    let parser = Arc::new(Mutex::new(IncrementalParser::new()));
    let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();
    let app = app_handle.clone();

    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            let events = match result {
                Ok(events) => events,
                Err(_) => continue,
            };

            for event in events {
                let path = &event.path;
                if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                    continue;
                }

                let project_name = project_name_from_path(path);
                let entries = {
                    let mut parser = match parser.lock() {
                        Ok(parser) => parser,
                        Err(_) => continue,
                    };
                    parser.parse_new_entries(path)
                };

                for entry in entries {
                    let Some(event) = resolve_event(&entry, project_name.clone()) else {
                        continue;
                    };
                    update_last_event();
                    set_active(is_active_state(&event.state));
                    let _ = app.emit("claude-event", &event);
                }
            }
        }
    });

    // 超时检测线程：活跃状态超时回 idle，idle 超时进 dormant
    let timeout_app = app_handle;
    let timeout_config = config;
    std::thread::spawn(move || {
        let check_interval = std::time::Duration::from_secs(1);
        loop {
            std::thread::sleep(check_interval);

            let (active_timeout_secs, idle_timeout_secs) = timeout_config
                .read()
                .map(|c| (c.active_state_timeout_secs, c.idle_sleep_secs))
                .unwrap_or((30, 300));

            let now = now_millis();
            let last = LAST_EVENT_MILLIS.load(Ordering::Relaxed);
            let active = IS_ACTIVE.load(Ordering::Relaxed);

            if last == 0 {
                continue;
            }

            let elapsed_secs = (now.saturating_sub(last)) / 1000;

            if active && elapsed_secs >= active_timeout_secs {
                IS_ACTIVE.store(false, Ordering::Relaxed);
                let event = ClaudeEvent {
                    state: ClaudeEventState::Idle.as_str().to_string(),
                    source: ClaudeEventSource::Signal,
                    session_id: None,
                    project_name: None,
                    detail: None,
                    raw_text: None,
                    tool_name: None,
                };
                let _ = timeout_app.emit("claude-event", &event);
                update_last_event();
            } else if !active && elapsed_secs >= idle_timeout_secs {
                let event = ClaudeEvent {
                    state: ClaudeEventState::Dormant.as_str().to_string(),
                    source: ClaudeEventSource::Signal,
                    session_id: None,
                    project_name: None,
                    detail: None,
                    raw_text: None,
                    tool_name: None,
                };
                let _ = timeout_app.emit("claude-event", &event);
                update_last_event();
            }
        }
    });

    let mut debouncer = new_debouncer(std::time::Duration::from_millis(debounce_ms), tx)?;
    debouncer
        .watcher()
        .watch(&projects_dir, RecursiveMode::Recursive)?;
    Ok(debouncer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_entry(content: serde_json::Value, stop_reason: Option<&str>) -> JsonlEntry {
        JsonlEntry {
            entry_type: Some("assistant".to_string()),
            message: Some(MessagePayload {
                content: Some(content),
                stop_reason: stop_reason.map(ToString::to_string),
            }),
            session_id: Some("session-1".to_string()),
        }
    }

    #[test]
    fn assistant_tool_use_maps_to_coding_event() {
        let entry = assistant_entry(
            serde_json::json!([
                {"type": "text", "text": "先读一下文件"},
                {"type": "tool_use", "name": "Read"}
            ]),
            None,
        );

        let event = resolve_event(&entry, Some("demo/project".to_string())).unwrap();
        assert_eq!(event.state, "coding");
        assert_eq!(event.tool_name.as_deref(), Some("Read"));
        assert_eq!(event.raw_text.as_deref(), Some("先读一下文件"));
    }

    #[test]
    fn assistant_end_turn_maps_to_success_event() {
        let entry = assistant_entry(
            serde_json::json!([
                {"type": "text", "text": "已经完成了"}
            ]),
            Some("end_turn"),
        );

        let event = resolve_event(&entry, None).unwrap();
        assert_eq!(event.state, "success");
        assert_eq!(event.raw_text.as_deref(), Some("已经完成了"));
    }

    #[test]
    fn tool_result_error_maps_to_error_event() {
        let entry = JsonlEntry {
            entry_type: Some("tool_result".to_string()),
            message: Some(MessagePayload {
                content: Some(serde_json::json!([
                    {"type": "text", "text": "permission denied", "is_error": true}
                ])),
                stop_reason: None,
            }),
            session_id: None,
        };

        let event = resolve_event(&entry, None).unwrap();
        assert_eq!(event.state, "error");
        assert_eq!(event.raw_text.as_deref(), Some("permission denied"));
    }
}
