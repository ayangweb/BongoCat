use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_pinia::ManagerExt;
use tiny_http::{Method, Response, Server};

#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AiConfig {
    http_enabled: bool,
    http_port: u16,
    http_token: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            http_enabled: false,
            http_port: 7800,
            http_token: String::new(),
        }
    }
}

#[derive(Serialize, Clone)]
struct ShowChatPayload {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<u64>,
}

// ponytail: 改端口/开关/token 后需重启 app 生效（不做热重启）
pub fn start(app_handle: &AppHandle) {
    // 读持久化的 ai 配置（store id 与 key 均为 "ai"）；无文件时取默认（关闭）
    let config: AiConfig = app_handle
        .with_store("ai", |store| store.try_get_or_default::<AiConfig>("ai"))
        .unwrap_or_default();

    if !config.http_enabled {
        return;
    }

    let handle = app_handle.clone();
    let addr = format!("127.0.0.1:{}", config.http_port);
    let token = config.http_token;

    // ponytail: tiny_http 单线程阻塞循环，够用；不引入 axum/tokio
    std::thread::spawn(move || {
        let server = match Server::http(&addr) {
            Ok(server) => server,
            Err(err) => {
                log::error!("chat http server failed to bind {addr}: {err}");
                return;
            }
        };

        log::info!("chat http server listening on {addr}");

        for request in server.incoming_requests() {
            handle_request(&handle, &token, request);
        }
    });
}

fn handle_request(app_handle: &AppHandle, token: &str, request: tiny_http::Request) {
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));

    // 只支持 GET /say
    if request.method() != &Method::Get || path != "/say" {
        let _ = request.respond(Response::from_string("not found").with_status_code(404));
        return;
    }

    let params: HashMap<String, String> = form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();

    // token 非空时校验
    if !token.is_empty() && params.get("token").map(String::as_str) != Some(token) {
        let _ = request.respond(Response::from_string("unauthorized").with_status_code(401));
        return;
    }

    let text = match params.get("text") {
        Some(text) if !text.is_empty() => text.clone(),
        _ => {
            let _ = request.respond(Response::from_string("missing text").with_status_code(400));
            return;
        }
    };

    // duration：秒 → 毫秒；没给则不带（chat 页用默认）；0 表示常驻
    let duration = params
        .get("duration")
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| seconds * 1000);

    let _ = app_handle.emit("show-chat", ShowChatPayload { text, duration });

    let _ = request.respond(Response::from_string("ok"));
}
