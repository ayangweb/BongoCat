# Bark 客户端支持 — 设计文档

日期：2026-07-10
分支：feat/ai-chat-bubble

## 目标

BongoCat 作为 Bark 客户端，注册到自托管的 **htnanako/bark-server**（fork，带 SSE 通道），
实时接收推送消息并以聊天气泡展示、记入 chat-history。

不支持原版 finb/bark-server（仅 APNs 投递，桌面端无通道）与官方公共服务 api.day.app。

## 需求决策（已确认）

| 决策点 | 结论 |
|---|---|
| 目标服务端 | htnanako/bark-server fork（用户已部署） |
| 展示方式 | 只弹气泡：`title\nbody` 拼纯文本走现有 show-chat 路径，记入 chat-history（source: `bark`）；不发系统通知 |
| 断线回放 | 不回放：不发送 Last-Event-ID，断线期间消息直接丢弃 |
| 加密消息 | 支持 AES CBC/GCM（Web Crypto 原生），不支持 ECB |
| 实现位置 | 前端 fetch-SSE（方案 A）：零 Rust 改动、零新依赖 |

## 服务端协议（htnanako fork）

- `POST /register`，JSON body：
  `{ device_key: <已有则复用，首次为空>, platform: "macos", app_id: "me.fin.bark.macos", provider_id: "macos_sse", topic: "me.fin.bark.macos" }`
  → 响应 `data`: `{ key/device_key, stream_token, ... }`。同 key 重复注册复用 stream_token；身份冲突返回 409。
- `GET /events/{device_key}?stream_token=...`，`Accept: text/event-stream`。
  SSE 事件：`event: ready`（连上时）、`event: notification`（推送本体）；每 25s 一行 `: ping` 心跳。
  带 Last-Event-ID 时回放最多 200 条历史 —— 本设计**不发送**该头，故无回放。
- notification 的 `data:` 为 JSON：`{ id, device_key, title, subtitle, body, payload{...}, created_at }`。
  加密消息 payload 含 `ciphertext`（base64）与可选 `iv`。

## 架构与组件

### 新增

1. **`src/composables/useBark.ts`**（核心，约 100 行）
   - `register()`：fetch POST /register，成功后把 `deviceKey`/`streamToken` 写入 store
   - `connect()` / `disconnect()`：fetch + ReadableStream 手写极简 SSE 解析（按 `\n\n` 分帧，
     跳过 `:` 注释行，只处理 `event: notification`），AbortController 管理生命周期
   - 重连：指数退避 `[1,2,5,10,20,30]s` + 随机抖动，无限重试
   - 解密：payload 含 `ciphertext` → Web Crypto 按 store 配置（模式/key/IV）解密 → JSON
   - 出口：`title\nbody`（无 title 只取 body）emit `show-chat`，payload `source: 'bark'`

2. **设置页 Bark 卡片**（`src/pages/preference/components/chat/` 下）：
   启用开关、服务器地址、注册按钮（展示 device key 供复制）、加密配置（模式 CBC/GCM、key、IV）、
   连接状态指示（已连接 / 重连中 / 需重新注册 / 未启用）。

### 修改

- `src/stores/chat.ts`：新增 `bark: { enabled, serverUrl, deviceKey, streamToken, crypto: { mode, key, iv } }`
  （随现有 tauri-store 自动持久化、跨窗口同步）
- `src/utils/chatHistory.ts`：`ChatMessage.source` 联合类型加 `'bark'`；历史 Modal 筛选项同步
- `src/pages/chat/index.vue`：挂载时 `bark.enabled` 则 `connect()`（SSE 挂在常驻 chat 窗口）；
  watch bark 配置变更时断开重建
- locales 五个语言文件补文案

### 数据流

```
bark-server SSE → useBark 解析/解密 → emit show-chat (source:'bark')
              → 现有 showChat()：记 chat-history + 总开关判断 + 弹气泡
```

bark 消息不走任何特殊展示分支，与 `/say` 完全同路。Rust 端零改动。

## 错误处理

- **注册失败**：按钮旁展示错误；409 提示清空本地 key 重新注册；失败不写状态
- **SSE 断线**：指数退避无限重连（上限 30s + 抖动），状态显示"重连中"
- **同 key 被踢**（fork 同 key 单连接，新踢旧）：走重连路径；双实例互踢为已知限制，注明单实例使用，不做处理
- **401/403（stream_token 失效）**：停止重连，状态显示"需要重新注册"
- **解密失败 / 非法 JSON / 空消息**：丢弃该条，console.warn，不影响连接
- **配置变更 / 关闭开关**：AbortController 干净断开，需要时重建

## 测试

- SSE 分帧解析器抽纯函数 + 最小单测（跨 chunk 帧边界、心跳行、多行 data）
- 解密纯函数 + 单测（CBC/GCM 正例各一 + 坏密文一例）
- 端到端手动验证：docker 起 htnanako/bark-server → 注册 → curl 推送 → 气泡出现且
  历史 source=bark；断线重连、加密消息各验一次（补一份 bark 手动测试说明，
  参照现有 /say 手动测试脚本先例）

## 明确不做（YAGNI）

- 消息回放 / 离线补收
- AES-ECB、markdown 渲染、url/icon/level/sound 等 Bark 富字段
- 系统通知、Rust 端实现、多实例支持
