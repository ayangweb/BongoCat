# 气泡 HTTP 控制接口 + 消息接口 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把气泡 HTTP 接口拆成「消息接口 `/say`（一次性样式覆盖）」与「控制接口 `/config`（持久写入默认值 + 设置页实时同步）」，并在设置页内联补全调用说明。

**Architecture:** Rust 端（`tiny_http`）按 path 路由两个 GET 接口，参数解析/校验抽成纯函数。`/say` 把临时样式随 `show-chat` 事件下发，chat 页用局部 override 渲染本条气泡且不写设置；`/config` 通过 `update-config` 事件交由「始终存活的 chat 窗口」写入 `aiStore`，由 `@tauri-store/pinia` 落盘并跨窗口同步到设置页。

**Tech Stack:** Tauri 2 / Rust / `tiny_http` / `serde_json`；Vue 3 + TypeScript + Pinia（`@tauri-store/pinia`）/ antdv-next。

## Global Constraints

- 参数名一律复用 `aiStore.ai.*` 字段名：`duration` `textColor` `fontSize` `bgColor` `bgOpacity`（设置项 ↔ API 一一对应）。
- 仅 GET + query 参数，不解析 POST body，不引入新 HTTP 框架/依赖。
- 仅监听 `127.0.0.1`；`token` 非空时两个接口都校验。
- 取值范围：`fontSize` 8–64；`bgOpacity` 0–100；`duration` ≥ 0（秒）；`textColor`/`bgColor` 为 `#rgb` 或 `#rrggbb`。非法 → `400` + 简短中文说明。
- `duration` 在 store 与 `/config` 中单位是「秒」；`/say` 下发 `show-chat` 时转毫秒（沿用 chat 页既有 `ms` 逻辑）。
- 不通过 HTTP 改 `enabled` 主开关、`httpPort`/`httpToken`/`debug` 等元配置。
- 本仓库无前端测试框架，**不**为本功能引入；前端用手动验证。Rust 纯函数用 `cargo test`（内置，无新依赖）。
- 提交信息走 commitlint：`type: 描述`，subject 不要大写开头/全大写（曾因 `HTTP` 被拒）。

## File Structure

- `src-tauri/src/core/server.rs` — 修改：新增 `Overrides`/`AiPublicConfig` 类型、`is_hex_color`/`parse_overrides` 纯函数 + 单测、按 path 路由 `/say`(扩展) 与 `/config`(新)。
- `src/constants/index.ts` — 修改：`LISTEN_KEY` 新增 `UPDATE_CONFIG`。
- `src/pages/chat/index.vue` — 修改：`ShowChatPayload` 扩展、局部 override 渲染、新增 `update-config` 监听写 store。
- `src/pages/preference/components/ai/index.vue` — 修改：HTTP 区块内联说明面板（替换现有单行 curl）。
- `src/locales/{zh-CN,zh-TW,en-US,vi-VN,pt-BR}.json` — 修改：`pages.preference.ai.labels.httpDocs` 与 `pages.preference.ai.hints.httpDocs`。

---

### Task 1: 后端 `/say` 扩展 + `/config` 新接口（含参数校验单测）

**Files:**
- Modify: `src-tauri/src/core/server.rs`（整文件替换为下方实现）

**Interfaces:**
- Produces 事件 `show-chat`，payload（camelCase，None 跳过）：`{ text: string, duration?: number(ms), textColor?: string, fontSize?: number, bgColor?: string, bgOpacity?: number }`
- Produces 事件 `update-config`，payload（camelCase，None 跳过）：`{ duration?: number(秒), textColor?: string, fontSize?: number, bgColor?: string, bgOpacity?: number }`
- `GET /config` 无 setter 参数时返回 store 内 `ai` 配置 JSON；带参数时返回 `{"applied": {<已应用的覆盖>}}`。

- [ ] **Step 1: 整文件替换 `server.rs`**

```rust
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
}
```

- [ ] **Step 2: 跑单测，确认通过**

Run: `cd src-tauri && cargo test --lib server::tests`
Expected: PASS（6 个 test：`hex_color_accepts_3_and_6_digits`、`hex_color_rejects_bad_input`、`parses_valid_overrides`、`empty_params_give_default_overrides`、`rejects_out_of_range_and_bad_values` 等全部 ok）

如报 `serde_json` 未引入：确认 `src-tauri/Cargo.toml` 已有 `serde_json`（tauri 传递依赖通常已含）；若缺，`cargo add serde_json`。

- [ ] **Step 3: 编译整个 crate**

Run: `cd src-tauri && cargo check`
Expected: 无 error（warning 可接受）

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/core/server.rs
git commit -m "feat(ai): split bubble http into say + config endpoints"
```

---

### Task 2: 前端 chat 页 —— 消息一次性覆盖 + `update-config` 持久同步

**Files:**
- Modify: `src/constants/index.ts:5-14`（`LISTEN_KEY` 加一项）
- Modify: `src/pages/chat/index.vue`

**Interfaces:**
- Consumes 事件 `show-chat`（含 Task 1 的可选样式字段）与 `update-config`。
- `LISTEN_KEY.UPDATE_CONFIG = 'update-config'`。

- [ ] **Step 1: 常量新增事件 key**

`src/constants/index.ts`，把 `SHOW_CHAT: 'show-chat',` 一行改为两行：

```ts
  SHOW_CHAT: 'show-chat',
  UPDATE_CONFIG: 'update-config',
```

- [ ] **Step 2: 扩展 `ShowChatPayload` 接口（chat/index.vue:14-17）**

```ts
interface ShowChatPayload {
  text: string
  duration?: number
  textColor?: string
  fontSize?: number
  bgColor?: string
  bgOpacity?: number
}
```

- [ ] **Step 3: 加入局部 override 状态（chat/index.vue，紧接 `const visible = ref(false)` 之后）**

```ts
// 本条气泡的一次性样式覆盖；每次 showChat 重置，不写入 aiStore（设置不变）
const override = reactive<{
  textColor?: string
  fontSize?: number
  bgColor?: string
  bgOpacity?: number
}>({})
```

并把顶部 `import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'` 补上 `reactive`：

```ts
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
```

- [ ] **Step 4: 样式计算改为「override 优先，回落 store」（chat/index.vue:41-47）**

```ts
const bgRgba = computed(() => hexToRgba(override.bgColor ?? aiStore.ai.bgColor, override.bgOpacity ?? aiStore.ai.bgOpacity))

const bubbleStyle = computed(() => ({
  color: override.textColor ?? aiStore.ai.textColor,
  fontSize: `${override.fontSize ?? aiStore.ai.fontSize}px`,
  background: bgRgba.value,
}))
```

- [ ] **Step 5: `showChat` 应用并重置 override（chat/index.vue:105-124，替换函数签名与开头）**

把函数前半段改为：

```ts
async function showChat({ text: nextText, duration, textColor, fontSize, bgColor, bgOpacity }: ShowChatPayload) {
  // 总开关唯一生效点
  if (!aiStore.ai.enabled) return

  // 一次性覆盖：赋 undefined 即回落到 store 默认
  override.textColor = textColor
  override.fontSize = fontSize
  override.bgColor = bgColor
  override.bgOpacity = bgOpacity

  text.value = nextText
  visible.value = true

  await resize()
  await reposition()
  await appWindow.show()

  // 默认时长唯一兜底点；0 表示常驻
  const ms = duration ?? aiStore.ai.duration * 1000

  clearTimeout(timer)

  if (ms > 0) {
    timer = setTimeout(hide, ms)
  }
}
```

- [ ] **Step 6: 新增 `update-config` 监听（chat/index.vue，紧接现有 `useTauriListen<ShowChatPayload>(...)` 之后）**

```ts
interface UpdateConfigPayload {
  duration?: number
  textColor?: string
  fontSize?: number
  bgColor?: string
  bgOpacity?: number
}

// 控制接口：写入 aiStore 默认值，saveOnChange 落盘并跨窗口同步到设置页
useTauriListen<UpdateConfigPayload>(LISTEN_KEY.UPDATE_CONFIG, ({ payload }) => {
  const { duration, textColor, fontSize, bgColor, bgOpacity } = payload

  if (duration !== undefined) aiStore.ai.duration = duration
  if (textColor !== undefined) aiStore.ai.textColor = textColor
  if (fontSize !== undefined) aiStore.ai.fontSize = fontSize
  if (bgColor !== undefined) aiStore.ai.bgColor = bgColor
  if (bgOpacity !== undefined) aiStore.ai.bgOpacity = bgOpacity
})
```

- [ ] **Step 7: lint**

Run: `pnpm lint`
Expected: 无 error（自动 fix 后 `src/pages/chat/index.vue`、`src/constants/index.ts` 通过）

- [ ] **Step 8: 手动验证（需先在设置页开启 HTTP，重启 app）**

无前端测试框架，手动验证：

1. `pnpm tauri dev` 启动；设置页 → AI → 开启「启用 HTTP 接口」，重启 app。
2. 消息临时覆盖（设置不变）：
   ```bash
   curl "http://127.0.0.1:7800/say?text=红色大字&textColor=%23ff0000&fontSize=28&duration=4"
   ```
   预期：气泡红色 28px 显示 4 秒；随后
   ```bash
   curl "http://127.0.0.1:7800/say?text=恢复默认"
   ```
   预期：气泡回到默认颜色/字号；设置页数值未变。
3. 控制接口持久 + 同步：打开设置页，执行
   ```bash
   curl "http://127.0.0.1:7800/config?bgColor=%23000000&bgOpacity=70&fontSize=18"
   ```
   预期：设置页「气泡底色/底色透明度/文字大小」**实时**变化；返回 JSON 含 `"applied"`。重启 app 后值仍保留。
4. 回读：`curl "http://127.0.0.1:7800/config"` 预期返回当前配置 JSON。
5. 校验：`curl "http://127.0.0.1:7800/say?text=x&fontSize=999"` 预期 `400` + 「fontSize 必须在 8–64 之间」。

- [ ] **Step 9: 提交**

```bash
git add src/constants/index.ts src/pages/chat/index.vue
git commit -m "feat(ai): apply per-message style overrides and config sync in bubble"
```

---

### Task 3: 设置页内联调用说明 + i18n

**Files:**
- Modify: `src/pages/preference/components/ai/index.vue:106-110`（替换现有单行 curl 的 `ProListItem`）
- Modify: `src/locales/zh-CN.json` `src/locales/zh-TW.json` `src/locales/en-US.json` `src/locales/vi-VN.json` `src/locales/pt-BR.json`

**Interfaces:**
- Consumes 已存在的 i18n 标签 `pages.preference.ai.labels.{textColor,fontSize,bgColor,bgOpacity,duration}`（说明面板复用，不新增）。
- 新增 `pages.preference.ai.labels.httpDocs` 与 `pages.preference.ai.hints.httpDocs`。

- [ ] **Step 1: 5 个 locale 各加两个键**

在每个文件的 `pages.preference.ai.labels` 末尾加 `httpDocs`，`pages.preference.ai.hints` 末尾加 `httpDocs`：

`zh-CN.json`：
```json
"labels": { "...": "...", "httpDocs": "调用说明" },
"hints":  { "...": "...", "httpDocs": "消息接口 /say 的样式参数仅对本条气泡生效；控制接口 /config 写入默认值并持久保存。" }
```
`zh-TW.json`：
```json
"httpDocs": "呼叫說明"
"httpDocs": "訊息介面 /say 的樣式參數僅對本條氣泡生效；控制介面 /config 寫入預設值並持久保存。"
```
`en-US.json`：
```json
"httpDocs": "API reference"
"httpDocs": "Style params on /say apply to a single bubble only; /config writes the saved defaults."
```
`vi-VN.json`：
```json
"httpDocs": "Hướng dẫn gọi API"
"httpDocs": "Tham số kiểu dáng của /say chỉ áp dụng cho một bong bóng; /config ghi vào giá trị mặc định đã lưu."
```
`pt-BR.json`：
```json
"httpDocs": "Referência da API"
"httpDocs": "Os parâmetros de estilo de /say afetam apenas um balão; /config grava os padrões salvos."
```

（注意：实际编辑时把 `httpDocs` 作为新键追加进对应 `labels`/`hints` 对象，保留原有键不动；上面只展示新增项。）

- [ ] **Step 2: 替换说明面板（ai/index.vue:106-110）**

把现有 `<ProListItem title="curl"> ... </ProListItem>` 整块替换为：

```vue
      <ProListItem
        :description="$t('pages.preference.ai.hints.httpDocs')"
        :title="$t('pages.preference.ai.labels.httpDocs')"
        vertical
      >
        <Flex
          class="w-full text-3 color-text-tertiary"
          :gap="8"
          vertical
        >
          <div>
            <div class="color-text-secondary">{{ $t('pages.preference.ai.labels.basic') }} · /say</div>
            <code class="select-all break-all">curl "http://127.0.0.1:{{ aiStore.ai.httpPort }}/say?text=hi&textColor=%23ff0000&fontSize=20&duration=5"</code>
          </div>

          <div>
            <div class="color-text-secondary">{{ $t('pages.preference.ai.labels.http') }} · /config</div>
            <code class="select-all break-all">curl "http://127.0.0.1:{{ aiStore.ai.httpPort }}/config?bgColor=%23000000&bgOpacity=80"</code>
          </div>

          <div class="break-all">
            text · token? · duration · textColor · fontSize · bgColor · bgOpacity
          </div>
        </Flex>
      </ProListItem>
```

（`Flex` 已在该文件第 2 行从 `antdv-next` 导入，无需新增 import。）

- [ ] **Step 3: lint + 类型检查**

Run: `pnpm lint`
Expected: 无 error。

Run: `node -e "['zh-CN','zh-TW','en-US','vi-VN','pt-BR'].forEach(l=>JSON.parse(require('fs').readFileSync('src/locales/'+l+'.json','utf8')))"`
Expected: 无输出（5 个 JSON 均合法，无尾逗号等语法错）。

- [ ] **Step 4: 手动验证**

1. `pnpm tauri dev`，设置页 → AI → 开启 HTTP 接口。
2. 预期 HTTP 区块下方出现「调用说明」面板：含 `/say` 与 `/config` 两条可全选复制的 curl（端口随 `httpPort` 变化）、参数名一行、一句区别说明。
3. 切换语言（设置页语言项）→ 标题与说明文字随之切换。

- [ ] **Step 5: 提交**

```bash
git add src/pages/preference/components/ai/index.vue src/locales/zh-CN.json src/locales/zh-TW.json src/locales/en-US.json src/locales/vi-VN.json src/locales/pt-BR.json
git commit -m "feat(ai): inline http api docs in settings"
```

---

## Self-Review

- **Spec coverage：** 消息接口临时覆盖（Task 1 `/say` + Task 2 chat 渲染）✓；控制接口持久 + 实时同步（Task 1 `/config` + Task 2 `update-config` 监听）✓；5 个受控属性 duration/textColor/fontSize/bgColor/bgOpacity 全覆盖 ✓；设置页内联说明（Task 3）✓；校验 + 400（Task 1）✓；GET + 参数名复用字段名 + 不引新框架（Global Constraints）✓；回读 `/config`（Task 1）✓。
- **Placeholder scan：** 无 TBD/TODO；所有代码块为完整实现。
- **Type consistency：** `Overrides`/`ShowChatPayload`/`UpdateConfigPayload` 字段名与单位一致；`/say` duration 转毫秒、`/config` 与 store duration 同为秒，已在 Global Constraints 与各 payload 注释中标明；`LISTEN_KEY.UPDATE_CONFIG = 'update-config'` 与 Rust `emit("update-config")` 一致；`show-chat` 字段与 chat 页 `ShowChatPayload` 一致。
- **已知前提：** `pinia` 插件跨窗口同步 + `saveOnChange` 落盘，依赖既有 `src/main.ts:15` 配置；chat 窗口启动即创建（仅隐藏）故其 JS 持续运行——Task 2 Step 8 的「设置页实时同步」是对该前提的实测验证点，若不同步需在该步排查插件同步行为。
