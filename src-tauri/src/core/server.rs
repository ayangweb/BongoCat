use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
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

// /config 无参数时回读的完整气泡配置（与 store 默认值保持一致）
#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
struct AiPublicConfig {
    enabled: bool,
    duration: u64,
    text_color: String,
    font_size: u32,
    bg_color: String,
    bg_opacity: u32,
}

// keep in sync with src/stores/ai.ts defaults
impl Default for AiPublicConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            duration: 3,
            text_color: "#333".into(),
            font_size: 14,
            bg_color: "#fff".into(),
            bg_opacity: 90,
        }
    }
}

// 可选样式覆盖：/say 一次性、/config 持久共用同一组字段。
// duration 单位为「秒」（与设置项一致）；/say 发事件时再转毫秒。
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct Overrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    font_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bg_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bg_opacity: Option<u32>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ShowChatPayload {
    text: String,
    // 毫秒；chat 页 `ms = duration ?? 默认 * 1000`
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    font_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bg_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bg_opacity: Option<u32>,
    // 消息来源；前端 say() 不带此字段，气泡窗口按 internal 兜底
    source: String,
}

fn is_hex_color(value: &str) -> bool {
    match value.strip_prefix('#') {
        Some(hex) => (hex.len() == 3 || hex.len() == 6) && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        None => false,
    }
}

// 解析并校验可选样式参数；任一非法返回错误说明。duration 保持「秒」。
fn parse_overrides(params: &HashMap<String, String>) -> Result<Overrides, String> {
    let mut out = Overrides::default();

    if let Some(raw) = params.get("duration") {
        let value = raw
            .parse::<u64>()
            .map_err(|_| "duration 必须是非负整数（秒）".to_string())?;
        out.duration = Some(value);
    }

    if let Some(raw) = params.get("textColor") {
        if !is_hex_color(raw) {
            return Err("textColor 必须是 hex 颜色（如 #ff0000）".into());
        }
        out.text_color = Some(raw.clone());
    }

    if let Some(raw) = params.get("fontSize") {
        let value = raw
            .parse::<u32>()
            .map_err(|_| "fontSize 必须是整数".to_string())?;
        if !(8..=64).contains(&value) {
            return Err("fontSize 必须在 8–64 之间".into());
        }
        out.font_size = Some(value);
    }

    if let Some(raw) = params.get("bgColor") {
        if !is_hex_color(raw) {
            return Err("bgColor 必须是 hex 颜色（如 #ffffff）".into());
        }
        out.bg_color = Some(raw.clone());
    }

    if let Some(raw) = params.get("bgOpacity") {
        let value = raw
            .parse::<u32>()
            .map_err(|_| "bgOpacity 必须是整数".to_string())?;
        if value > 100 {
            return Err("bgOpacity 必须在 0–100 之间".into());
        }
        out.bg_opacity = Some(value);
    }

    Ok(out)
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

fn respond(request: tiny_http::Request, status: u16, body: &str) {
    let _ = request.respond(Response::from_string(body).with_status_code(status));
}

fn respond_json(request: tiny_http::Request, body: String) {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("static header");
    let _ = request.respond(Response::from_string(body).with_header(header));
}

fn handle_request(app_handle: &AppHandle, token: &str, request: tiny_http::Request) {
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let path = path.to_string();

    if request.method() != &Method::Get {
        return respond(request, 404, "not found");
    }

    let params: HashMap<String, String> = form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();

    // token 非空时校验（两个接口共用）
    if !token.is_empty() && params.get("token").map(String::as_str) != Some(token) {
        return respond(request, 401, "unauthorized");
    }

    match path.as_str() {
        "/say" => handle_say(app_handle, &params, request),
        "/config" => handle_config(app_handle, &params, request),
        _ => respond(request, 404, "not found"),
    }
}

fn handle_say(app_handle: &AppHandle, params: &HashMap<String, String>, request: tiny_http::Request) {
    let text = match params.get("text") {
        Some(text) if !text.is_empty() => text.clone(),
        _ => return respond(request, 400, "missing text"),
    };

    let overrides = match parse_overrides(params) {
        Ok(overrides) => overrides,
        Err(err) => return respond(request, 400, &err),
    };

    let payload = ShowChatPayload {
        text,
        duration: overrides.duration.map(|seconds| seconds * 1000),
        text_color: overrides.text_color,
        font_size: overrides.font_size,
        bg_color: overrides.bg_color,
        bg_opacity: overrides.bg_opacity,
        source: "http".into(),
    };

    let _ = app_handle.emit("show-chat", payload);

    respond(request, 200, "ok");
}

fn handle_config(app_handle: &AppHandle, params: &HashMap<String, String>, request: tiny_http::Request) {
    let overrides = match parse_overrides(params) {
        Ok(overrides) => overrides,
        Err(err) => return respond(request, 400, &err),
    };

    // 无 setter 参数 → 回读当前配置
    if overrides == Overrides::default() {
        let current: AiPublicConfig = app_handle
            .with_store("ai", |store| store.try_get_or_default::<AiPublicConfig>("ai"))
            .unwrap_or_default();
        let body = serde_json::to_string(&current).unwrap_or_else(|_| "null".into());
        return respond_json(request, body);
    }

    if app_handle.get_webview_window("chat").is_none() {
        return respond(request, 503, "chat window not ready");
    }

    let _ = app_handle.emit("update-config", &overrides);

    let body = serde_json::json!({ "applied": overrides }).to_string();
    respond_json(request, body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_color_accepts_3_and_6_digits() {
        assert!(is_hex_color("#fff"));
        assert!(is_hex_color("#ffffff"));
        assert!(is_hex_color("#FF0000"));
    }

    #[test]
    fn hex_color_rejects_bad_input() {
        assert!(!is_hex_color("fff"));        // 缺 #
        assert!(!is_hex_color("#ff"));        // 长度错
        assert!(!is_hex_color("#gggggg"));    // 非 hex
        assert!(!is_hex_color("red"));
    }

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn parses_valid_overrides() {
        let out = parse_overrides(&params(&[
            ("duration", "5"),
            ("textColor", "#ff0000"),
            ("fontSize", "20"),
            ("bgColor", "#000"),
            ("bgOpacity", "80"),
        ]))
        .unwrap();
        assert_eq!(out.duration, Some(5));
        assert_eq!(out.text_color.as_deref(), Some("#ff0000"));
        assert_eq!(out.font_size, Some(20));
        assert_eq!(out.bg_color.as_deref(), Some("#000"));
        assert_eq!(out.bg_opacity, Some(80));
    }

    #[test]
    fn empty_params_give_default_overrides() {
        assert_eq!(parse_overrides(&params(&[])).unwrap(), Overrides::default());
    }

    #[test]
    fn rejects_out_of_range_and_bad_values() {
        assert!(parse_overrides(&params(&[("fontSize", "4")])).is_err());
        assert!(parse_overrides(&params(&[("fontSize", "999")])).is_err());
        assert!(parse_overrides(&params(&[("bgOpacity", "101")])).is_err());
        assert!(parse_overrides(&params(&[("textColor", "blue")])).is_err());
        assert!(parse_overrides(&params(&[("duration", "-1")])).is_err());
    }

    #[test]
    fn say_payload_serializes_http_source() {
        let payload = ShowChatPayload {
            text: "hi".into(),
            duration: None,
            text_color: None,
            font_size: None,
            bg_color: None,
            bg_opacity: None,
            source: "http".into(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains(r#""source":"http""#));
    }
}
