# Chat 历史消息设计

日期:2026-07-08
分支:feat/ai-chat-bubble

## 背景与目标

Chat 气泡消息目前只展示、不存储:消息来自前端 `say()`(内部调用,如设置页调试测试)和 HTTP `/say` 接口(Rust 发 `show-chat` 事件),气泡窗口展示后即丢弃。

目标:记录每条消息的历史,并在偏好设置的 Chat 标签页提供查询与展示界面。

## 需求

1. Chat 标签页新增「历史消息」按钮,点击后在设置窗口内弹出 Modal。
2. 查询条件:消息状态(已展示/未展示)、来源(HTTP/内部)、日期范围。
3. 列表展示:时间(精确到秒)、状态、来源、消息内容;内容过长时单行省略,每行提供「详情」按钮,点击后二级 Modal 展示完整消息。
4. 历史重启后保留,限制最近 500 条,超出淘汰最旧。

## 架构

记录点选在气泡窗口(`src/pages/chat/index.vue`)的 `showChat` 入口——所有来源的消息都汇聚于此,且总开关判断在这里,是唯一同时知道「消息内容」和「是否真的展示了」的位置。落盘与跨窗口同步复用现有 `@tauri-store/pinia`(`saveOnChange: true`),设置窗口直接读同一 store。

```
say() ──────────────┐
                    ├─ show-chat 事件 ─→ 气泡窗口 showChat()
Rust /say (source: ─┘                      ├─ push 到 chatHistory store(自动落盘+跨窗口同步)
        "http")                            └─ 现有展示逻辑(enabled 判断 → 气泡)

设置窗口 Chat 标签页 ─「历史消息」按钮 → Modal(筛选 + Table)← 读 chatHistory store
```

## 组件

### 1. 数据模型与存储 — 新建 `src/stores/chatHistory.ts`

独立于配置 store(`src/stores/chat.ts`),避免每条消息触发配置文件重写。

```ts
interface ChatMessage {
  time: number // Date.now() 时间戳,展示时格式化到秒
  text: string
  status: 'shown' | 'skipped' // skipped = 总开关关闭时收到
  source: 'http' | 'internal'
}
// state: { history: ChatMessage[] },上限 500 条,push 超出时移除最旧
```

在 `App.vue` 与其他 store 一致地调用 `$tauri.start()`。

### 2. 来源标记

- Rust `ShowChatPayload`(`src-tauri/src/core/server.rs`)增加 `source: "http"` 字段。
- 前端 `say()` 不带 source;气泡窗口收到 payload 时 `source ?? 'internal'`。

### 3. 记录点 — `src/pages/chat/index.vue`

`showChat` 入口处先记录(status 由 `chatStore.ai.enabled` 决定为 `shown` 或 `skipped`),再走现有展示逻辑。记录不影响展示流程的任何现有行为。

### 4. 历史消息界面 — `src/pages/preference/components/chat/index.vue`

Chat 标签页新增「历史消息」入口(ProListItem + Button),点击打开 antdv-next `Modal`:

- 筛选行:状态 `Select`(全部/已展示/未展示)+ 来源 `Select`(全部/HTTP/内部)+ 日期 `RangePicker`。
- 过滤在前端内存完成(最多 500 条,无需后端查询)。日期范围为闭区间:开始日 00:00:00 至 结束日 23:59:59。
- `Table` 列:时间(`YYYY-MM-DD HH:mm:ss`)、状态、来源、消息内容(单行省略)、操作(「详情」按钮 → 二级 Modal 展示完整文本,`whitespace-pre-wrap`)。
- 表格分页每页 20 条,按时间倒序(最新在前)。
- 文案走 i18n,补齐现有语言文件对应词条。

## 错误处理与边界

- 500 条上限是主要保护;写入失败不影响气泡展示(记录与展示互不阻塞)。
- 空历史/过滤无结果:Table 自带 empty 状态。
- 不做删除/清空功能(后续需要时加一个「清空」按钮即可,本期不做)。

## 测试

- 存储裁剪(超 500 淘汰最旧)与过滤逻辑抽为纯函数,各配小单测(参照 `chatPosition.test.ts` 的形式)。
- 手动验证:调试测试按钮与 HTTP curl 各发一条,总开关开/关各一次,确认状态与来源记录正确、重启后历史仍在。
