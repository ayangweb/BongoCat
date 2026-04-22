use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::sync::{oneshot, Mutex};

use super::{config::SharedConfig, permissions::PermissionRequest};

#[derive(Debug, Deserialize)]
struct HookRequest {
    id: String,
    payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct HookResponse {
    decision: String,
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub allow: bool,
}

pub struct PendingRequests {
    senders: Mutex<HashMap<String, oneshot::Sender<Decision>>>,
}

impl PendingRequests {
    pub fn new() -> Self {
        Self {
            senders: Mutex::new(HashMap::new()),
        }
    }

    async fn register(&self, id: String) -> oneshot::Receiver<Decision> {
        let (tx, rx) = oneshot::channel();
        self.senders.lock().await.insert(id, tx);
        rx
    }

    pub async fn resolve(&self, id: &str, decision: Decision) -> bool {
        if let Some(sender) = self.senders.lock().await.remove(id) {
            let _ = sender.send(decision);
            true
        } else {
            false
        }
    }

    async fn remove(&self, id: &str) {
        self.senders.lock().await.remove(id);
    }
}

pub fn socket_path() -> Option<PathBuf> {
    super::settings::signal_dir().map(|d| d.join("ipc.sock"))
}

pub fn cleanup_socket() {
    if let Some(path) = socket_path() {
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(unix)]
pub async fn serve(
    app_handle: tauri::AppHandle,
    pending: Arc<PendingRequests>,
    config: SharedConfig,
) -> anyhow::Result<()> {
    use tokio::net::UnixListener;

    let sock_path = socket_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine socket path"))?;

    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(&sock_path);

    let listener = UnixListener::bind(&sock_path)?;

    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600))?;
    }

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let app = app_handle.clone();
                let pending = pending.clone();
                let config = config.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream, app, pending, config).await {
                        eprintln!("IPC client error: {e}");
                    }
                });
            }
            Err(e) => {
                eprintln!("IPC accept error: {e}");
            }
        }
    }
}

#[cfg(not(unix))]
pub async fn serve(
    _app_handle: tauri::AppHandle,
    _pending: Arc<PendingRequests>,
    _config: SharedConfig,
) -> anyhow::Result<()> {
    // Unix Domain Socket IPC 仅在 Unix 平台支持
    Ok(())
}

#[cfg(unix)]
async fn handle_client(
    stream: tokio::net::UnixStream,
    app: tauri::AppHandle,
    pending: Arc<PendingRequests>,
    config: SharedConfig,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);

    let mut line = String::new();
    buf_reader.read_line(&mut line).await?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let hook_req: HookRequest = serde_json::from_str(line)
        .map_err(|e| anyhow::anyhow!("Invalid hook request JSON: {e}"))?;

    let request_id = hook_req.id.clone();
    let valid_request_id = !request_id.is_empty()
        && request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !valid_request_id {
        let resp = serde_json::to_string(&HookResponse {
            decision: "deny".to_string(),
        })?;
        writer.write_all(resp.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.shutdown().await?;
        return Ok(());
    }

    let tool_name = hook_req
        .payload
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let session_id = hook_req
        .payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let tool_input = hook_req.payload.get("tool_input").map(|v| v.to_string());

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let perm_req = PermissionRequest {
        request_id: request_id.clone(),
        session_id,
        tool_name,
        tool_input,
        timestamp: now,
    };

    let rx = pending.register(request_id.clone()).await;

    if app.emit("permission-request", &perm_req).is_err() {
        pending.remove(&request_id).await;
        let resp = serde_json::to_string(&HookResponse {
            decision: "deny".to_string(),
        })?;
        writer.write_all(resp.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.shutdown().await?;
        return Ok(());
    }

    let timeout_secs = config.read().map(|c| c.hook_timeout_secs).unwrap_or(120);
    let decision = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx).await;

    let decision_str = match decision {
        Ok(Ok(d)) => {
            if d.allow {
                "allow"
            } else {
                "deny"
            }
        }
        Ok(Err(_)) => "deny",
        Err(_) => {
            pending.remove(&request_id).await;
            "ask"
        }
    };

    let resp = serde_json::to_string(&HookResponse {
        decision: decision_str.to_string(),
    })?;
    writer.write_all(resp.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.shutdown().await?;
    Ok(())
}

