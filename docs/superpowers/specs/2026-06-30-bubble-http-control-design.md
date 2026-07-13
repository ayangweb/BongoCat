# 气泡 HTTP 接口：控制接口 + 消息接口

**日期：** 2026-06-30
**状态：** 设计已批准，待实现

## 背景

气泡的 HTTP 接口已存在：`src-tauri/src/core/server.rs` 用 `tiny_http` 在
`127.0.0.1:{httpPort}` 上监听，目前仅有 `GET /say?text=&duration=&token=`，
通过 `show-chat` 事件把文字推送到 chat 窗口。设置页
（`src/pages/preference/components/ai/index.vue`）已有 HTTP 开关、端口、Token，
以及一行 curl 示例。

本设计把接口拆成两类，并补全设置页的调用说明。

## 目标

1. **消息接口**（扩展现有 `GET /say`）：单条气泡可携带**临时、一次性**的样式覆盖
   （展示时长、文字颜色、文字大小、气泡底色、底色透明度），不影响已保存的设置。
2. **控制接口**（新增 `GET /config`）：修改**已保存的默认值**，持久化到磁盘，
   并实时同步到设置页 UI。
3. **设置页调用说明**：HTTP 区块展开后内联展示两类接口的完整调用介绍。

## 非目标

- 不引入新的 HTTP 框架（继续用 `tiny_http`，仅 GET + query 参数，不解析 POST body）。
- 不通过 HTTP 控制 `enabled` 主开关、`httpPort`/`httpToken`/`debug` 等元配置。
- 不做鉴权之外的访问控制（仍只监听 127.0.0.1）。

## 接口设计

参数名直接复用 `aiStore.ai.*` 字段名，使「设置项 ↔ API 参数」一一对应。

### 消息接口 `GET /say`

| 参数        | 必填 | 说明                           |
| ----------- | ---- | ------------------------------ |
| `text`      | 是   | 气泡文字（非空）               |
| `token`     | 否   | 服务端设置了 Token 时必填      |
| `duration`  | 否   | 本条气泡展示秒数（一次性覆盖） |
| `textColor` | 否   | 文字颜色 hex（一次性覆盖）     |
| `fontSize`  | 否   | 文字大小 px（一次性覆盖）      |
| `bgColor`   | 否   | 气泡底色 hex（一次性覆盖）     |
| `bgOpacity` | 否   | 底色透明度 0–100（一次性覆盖） |

覆盖仅作用于这一条气泡，不写入保存的设置；下一条气泡若不带覆盖则回到默认值。
成功返回 `200 OK`。

示例：

```
curl "http://127.0.0.1:7800/say?text=hi&textColor=%23ff0000&fontSize=20&duration=5"
```

### 控制接口 `GET /config`

| 参数        | 必填 | 说明                      |
| ----------- | ---- | ------------------------- |
| `token`     | 否   | 服务端设置了 Token 时必填 |
| `duration`  | 否   | 默认展示秒数              |
| `textColor` | 否   | 文字颜色 hex              |
| `fontSize`  | 否   | 文字大小 px               |
| `bgColor`   | 否   | 气泡底色 hex              |
| `bgOpacity` | 否   | 底色透明度 0–100          |

- 带任意设值参数：校验后持久化为新默认值，并实时同步到设置页；返回更新后的配置 JSON。
- 不带任何设值参数：仅返回当前配置 JSON（用于查询/验证）。

示例：

```
curl "http://127.0.0.1:7800/config?bgColor=%23000000&bgOpacity=80"
curl "http://127.0.0.1:7800/config"   # 读取当前配置
```

## 数据流

### 消息（临时覆盖）

```
GET /say?text=...&textColor=...&...
  → server.rs 校验参数
  → emit "show-chat" { text, duration?, textColor?, fontSize?, bgColor?, bgOpacity? }
  → chat/index.vue showChat(): 把携带的覆盖存入局部 ref（每次先重置）
  → bubbleStyle 取 override ?? aiStore.ai.*
  → 气泡按本条覆盖渲染，设置不变
```

`show-chat` 事件 payload 从 `{text, duration}` 扩展为附带可选样式覆盖字段。

### 控制（持久 + 同步）

```
GET /config?bgColor=...&...
  → server.rs 校验参数
  → emit "update-config" { 部分配置 }
  → chat/index.vue 监听 update-config，将值赋给 aiStore.ai.*
  → saveOnChange 持久化到磁盘
  → @tauri-store/pinia 跨窗口同步，设置页实时更新
  → 返回更新后的配置 JSON
```

写入走「始终存活的 chat 窗口」：chat 窗口在启动时创建、仅隐藏，其 JS 持续运行，
已持有 `aiStore` 并已监听事件，因此由它统一写 store，再由 pinia 插件同步到设置页
并落盘。不在 Rust 侧直接写 store，避免与前端为数据源的模型冲突。

## 校验

服务端在 emit 前校验，非法输入返回 `400` 加简短中文说明（当前服务仅有 404/401/200）：

- `textColor` / `bgColor`：必须是合法 hex（`#rgb` 或 `#rrggbb`）。
- `fontSize`：8–64。
- `bgOpacity`：0–100。
- `duration`：≥ 0。
- `/say` 的 `text`：必填且非空。

Token 校验逻辑不变：服务端设置了 Token 且请求 Token 不匹配 → `401`。

## 设置页调用说明

`src/pages/preference/components/ai/index.vue` 的 HTTP 区块在启用后，
内联展开一块说明面板，包含：

- 两类接口（消息 / 控制）的区别（一次性 vs 持久）。
- 参数表（名称、是否必填、含义、取值范围）。
- 可复制的 curl 示例，端口/Token 用当前实际值插值。

新增 i18n 字符串挂在 `ai.http.*` 下（zh-CN 及其它已存在 locale）。

## 涉及文件

- `src-tauri/src/core/server.rs`：拆出 `/say` 与 `/config` 处理；参数校验；400 响应；
  `/config` 返回配置 JSON。
- `src/pages/chat/index.vue`：`showChat` 支持一次性样式覆盖；新增 `update-config` 监听写 store。
- `src/pages/preference/components/ai/index.vue`：内联调用说明面板。
- `src/constants/index.ts`：新增 `update-config` 事件 key（`LISTEN_KEY`）。
- `src/locales/*.json`：新增 `ai.http.*` 文案。
- 事件 payload 类型（`show-chat` 扩展、`update-config` 新增）所在的类型定义处。
