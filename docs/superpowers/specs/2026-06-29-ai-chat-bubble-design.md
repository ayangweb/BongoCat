# 设计:猫咪头顶 AI 对话气泡（独立附着窗口）

- 日期：2026-06-29
- 状态：已确认，待实现
- 方案：B（动态尺寸窗口）

## 1. 目标与背景

在桌宠（Live2D 猫咪）头顶展示一个对话气泡，显示动态文字。

- 文字来源：**由代码/事件主动推送**。两类生产者：前端 `say()`（任意位置可调用）+ 本地 HTTP 接口（进程外 bash/curl/任意工具推送，见 6.1）；后续接一言、句库、交互触发都建在它之上。
- 消失行为：**定时自动消失**，接口可传 `duration=0` 表示常驻。
- 平台：**三端都要**（macOS / Windows / Linux），macOS 用 NSPanel 处理层级。

**为什么必须独立窗口**：主窗口尺寸紧贴模型且 `overflow-hidden`，气泡渲染在主窗口里会被裁切。独立窗口才能伸到猫咪上方边界之外。

## 2. 架构与权责

```
say(text, duration)              ← 任意位置可调用的推送 API
   └─ emit 'show-chat'（广播到所有窗口）
        └─▶ chat 窗口（/chat，独立透明穿透窗口）= 唯一生命周期权威
              ① 渲染文字 → 测量气泡真实尺寸
              ② setSize(自身 = 气泡尺寸)        ← 方案 B 的动态尺寸
              ③ reposition(摆到猫咪正上方居中)
              ④ show + 淡入
              ⑤ duration>0 → 计时 → 淡出 + hide
主窗口 move/resize → 通知 chat → chat reposition()
```

权责收敛原则：**chat 窗口自己管全部生命周期**（尺寸、定位、计时、动画），主窗口几乎零改动，从而把方案 B 的跨窗口同步坑降到最小。

主窗口仅有的改动：在 macOS 把已有的 `tauri://move` / `tauri://resize` 从「只发给 main」改成广播，好让 chat 也能收到。Windows/Linux 上 chat 直接用 `getByLabel('main').onMoved/onResized` 原生监听，无需改主窗口。

## 3. 新增窗口（`src-tauri/tauri.conf.json`）

在 `windows` 数组新增：

```jsonc
{
  "label": "chat",
  "url": "index.html/#/chat",
  "width": 200, "height": 100,     // 初始值，运行时被动态 setSize 覆盖
  "visible": false,
  "transparent": true,
  "decorations": false,
  "shadow": false,
  "alwaysOnTop": true,
  "skipTaskbar": true,
  "resizable": false,
  "maximizable": false,
  "focus": false
}
```

路由（`src/router`）：新增 `/chat` → `src/pages/chat/index.vue`。

## 4. chat 页面 `src/pages/chat/index.vue`

- **挂载时** `appWindow.setIgnoreCursorEvents(true)`（整窗鼠标穿透，透明空白区点不到）。
- **气泡 UI**：圆角矩形 + 朝下小三角（指向猫咪），底部居中锚定，文字向上换行；`<Transition>` 做淡入淡出。
- 样式从 `useAiStore()` 读取并绑到 `:style`：`textColor` / `fontSize` / `bgColor`+`bgOpacity`（合成 `rgba` 作气泡背景填充，窗口本身始终透明）。
- **监听 `show-chat {text, duration?}`**（`duration` 单位毫秒，可选）：
  0. `!aiStore.ai.enabled` → 直接忽略（**总开关唯一生效点**，所有生产者共用）
  1. 写入 `text` → `await nextTick()`
  2. `bubbleEl.getBoundingClientRect()` 量出 `w/h`（CSS 逻辑像素）
  3. `appWindow.setSize(new LogicalSize(w + 阴影留白, h + 三角高))`
  4. `reposition()` → `appWindow.show()` → 触发淡入
  5. `ms = duration ?? aiStore.ai.duration * 1000`（**默认时长唯一兜底点**）；重置计时器：`ms > 0` 时 `ms` 毫秒后淡出并 `hide()`；`=0` 常驻
- **监听主窗口几何变化** → 可见时 `reposition()`。
- **样式变化（字号等）且气泡可见时** → 重新测量 → `setSize` → `reposition`（字号改变会改变尺寸）。

## 5. 定位（纯函数，可测）`src/utils/chatPosition.ts`

```ts
// 全部用物理像素计算
computeBubblePosition(main{x,y,w,h}, bubble{w,h}, screen{x,y,w,h}, gap) {
  let x = main.x + (main.w - bubble.w) / 2               // 水平居中于猫
  let y = main.y - bubble.h - gap                         // 放猫咪正上方
  x = clamp(x, screen.x, screen.x + screen.w - bubble.w)  // 不超出屏幕
  if (y < screen.y) y = main.y + main.h + gap             // 上方没空间 → 翻到下方
  return { x, y }
}
```

**DPI / 多屏处理（方案 B 的关键坑）**：
- `getBoundingClientRect` 是逻辑像素；`outerPosition` / 显示器 bounds 是物理像素。
- 定位前把 bubble 尺寸 × `scaleFactor` 转物理；`setSize` 用 `LogicalSize`、`setPosition` 用 `PhysicalPosition`。
- `screen` 取**猫当前所在显示器**：用主窗口 `currentMonitor()` 拿该显示器的 position/size/scaleFactor（不是主屏），bubble 尺寸 × 该显示器 scaleFactor 转物理再算。

## 6. 推送 API `src/composables/useChat.ts`

```ts
export function say(text: string, duration?: number) {
  emit('show-chat', { text, duration }) // duration 单位毫秒；undefined 时由 chat 页兜底默认值
}
```

- 全局 `emit` 广播，chat 窗口接收。任意页面/composable 直接 `say('你好~')`。
- **总开关 / 默认时长不在这里判断**，统一由 chat 页处理（见第 4 节 step 0 / 5），保证前端 `say()` 与 HTTP 接口两个生产者行为一致。
- 两类生产者最终都只是发同一个 `show-chat` 事件：
  - 前端任意位置：`say()`
  - 进程外（bash / curl / 任意工具）：HTTP 接口（见 6.1）

## 6.1 HTTP 外部推送接口（方案②）

进程内事件够不着外部。内嵌一个**本地 HTTP server**作为进程外入口，让 `curl` / 任意工具直接推送。

**依赖**：`tiny_http`（极轻量、无需 async 运行时，单独 std 线程跑阻塞循环；比 axum 省）。加入 workspace `Cargo.toml`：`tiny_http = "0.12"`。

**模块**：新增 `src-tauri/src/core/server.rs`，在 `setup` 阶段按配置启动。

```
GET http://127.0.0.1:<port>/say?text=<urlencoded>&duration=<秒，可选>&token=<可选>
```

- 启动时读 `aiStore` 配置：`httpEnabled` / `httpPort` / `httpToken`。
- 在独立线程 `std::thread::spawn` 跑 `tiny_http::Server::http("127.0.0.1:<port>")` 阻塞循环。
- 收到请求 → 解析 query：
  - `httpToken` 非空时校验 `token` 不匹配 → `401`
  - `text` 缺失 → `400`
  - `duration` 给了就 `秒 → 毫秒`；没给则**不带** `duration` 字段（让 chat 页用默认值）；`0` 表示常驻
  - 校验通过 → `app_handle.emit("show-chat", { text, duration })` → 返回 `200 ok`
- **复用同一个 `show-chat` 事件**：总开关 / 默认时长 / 定位 / 动画全部由 chat 页统一处理，HTTP 侧只管解析转发。

调用示例：

```bash
# 简单
curl "http://127.0.0.1:7800/say?text=%E4%BD%A0%E5%A5%BD%E5%91%80"
# 自动 urlencode + 指定 5 秒 + token
curl -G "http://127.0.0.1:7800/say" \
  --data-urlencode "text=你好呀~" \
  --data "duration=5" --data "token=abc123"
```

**安全（信任边界，不可省）**：
- **只绑 `127.0.0.1`**，不监听外网。
- 开关默认 **关闭**（`httpEnabled=false`）：开一个监听端口是用户应主动同意的行为；在「AI」设置里显式开启。
- 可选 `httpToken`：同机其它进程/用户也能访问 localhost，需要更强隔离时填 token 校验。默认空=仅靠 localhost。
- `// ponytail: 改端口/开关/token 后需重启 app 生效（不做热重启）`。

## 7. macOS NSPanel（`src-tauri/src/core/setup/macos.rs`）

- 取 `chat` 窗口 → `to_panel()`，与猫**同 level（Dock）+ 同 collection behavior**（跟随空间、全屏辅助），`non_activating`、`can_become_key=false`（永不抢焦点）。
- chat 创建晚于 main，show 时 order front 即在猫之上。`// ponytail: 同层 order-front；若层级不准再抬高 PanelLevel`。
- 把现有 `emit_position` / resize 的 `emit_to(main)` 改成广播（emit），使 chat 能收到主窗口移动/缩放。

## 8. 独立设置项「AI」

新 tab：`preference/index.vue` 的 `menus` 加一项
`{ key:'ai', label:'AI', icon:'i-solar:chat-round-bold', component: Ai }`，
新建 `src/pages/preference/components/ai/index.vue`。

新建独立 store `src/stores/ai.ts`（所有气泡配置集中在此，通过 `@tauri-store/pinia` 跨窗口同步）：

```ts
ai: {
  enabled:     boolean  // 总开关，默认 true
  duration:    number   // 默认展示秒数，默认 3
  textColor:   string   // 文字颜色，默认 '#333'
  fontSize:    number   // 文字大小(px)，默认 14
  bgColor:     string   // 气泡底色，默认 '#fff'
  bgOpacity:   number   // 底色透明度 0-100，默认 90
  debug:       boolean  // DEBUG 开关，默认 false
  // —— HTTP 外部接口（见 6.1）——
  httpEnabled: boolean  // HTTP 接口开关，默认 false（安全：默认不开端口）
  httpPort:    number   // 监听端口，默认 7800
  httpToken:   string   // 可选校验 token，默认 ''（空=不校验）
}
```

AI 设置页（用现有 `ProList`/`ProListItem` + antdv-next 控件）：
- **总开关** `enabled`（Switch）
- **默认秒数** `duration`（InputNumber + `s` 后缀）
- **文字颜色** `textColor`（ColorPicker）/ **文字大小** `fontSize`（InputNumber 或 Slider）
- **气泡底色** `bgColor`（ColorPicker）/ **透明度** `bgOpacity`（Slider 0-100）
- **HTTP 接口**子区：`httpEnabled`（Switch）/ `httpPort`（InputNumber）/ `httpToken`（Input.Password，可空）；开启时展示一条可复制的 `curl` 示例；旁注「改动后需重启生效」。
- **DEBUG** `debug`（Switch）；开启后**展开测试区**：
  - 文本输入框 + 「展示」按钮 → 调 `say(inputText)` 立即在猫咪头顶展示
  - 这块同时充当定位的手动验证工具（见第 9 节）

ColorPicker 用 antdv-next 自带；若缺失则回退原生 `<input type="color">`（`// ponytail`）。

i18n：5 个语言包补 key（zh-CN / zh-TW / en-US / vi-VN / pt-BR）。

## 9. 验证 —— 定位为重点

### (a) 纯函数 `chatPosition.ts` 单测（assert 自检，无框架，覆盖所有分支）

1. 正常居中：猫在屏幕中央 → x 居中、y 在上方
2. 贴左边缘 → x 夹到 `screen.x`
3. 贴右边缘 → x 夹到 `screen.x + screen.w - bubble.w`
4. 贴顶部、上方放不下 → 翻转到猫咪下方
5. 气泡比猫宽 → 仍以猫中心对齐
6. 气泡比屏幕还宽 → 夹取后不越界（取 `screen.x`）
7. 多显示器：猫在副屏（负坐标/偏移）→ 用猫所在显示器的 bounds 夹取
8. DPI=2：逻辑尺寸 × scaleFactor 后物理坐标正确

### (b) DPI / 多屏的真实数据来源

定位时用主窗口 `currentMonitor()` 拿到**猫当前所在显示器**的 position/size/scaleFactor，而不是主屏——这是多屏正确的关键。气泡尺寸（逻辑 px）× 该显示器 scaleFactor 转物理再算。

### (c) 手动验证清单（借 DEBUG 测试区逐项过）

- 短文 / 长文 / 多行换行 → 尺寸自适应且始终底边居中贴猫头顶
- 把猫拖到屏幕四边 + 四角 → 不裁切、贴边自动夹取、顶部不够时翻到下方
- 改猫咪 `scale` → 气泡跟随重新定位
- 跨显示器拖动、不同 DPI 的两块屏 → 位置/尺寸正确
- 拖动猫咪时气泡跟随（允许极轻微延迟）

### (d) 可见验证触发

模型加载完成后 `say(t('greeting'))` 打个招呼（受 `ai.enabled` 控制），启动即可看到气泡浮在猫咪头顶。

### (e) HTTP 接口验证

- 关闭 `httpEnabled` → 端口不监听（`curl` 连不上）。
- 开启后重启 → `curl ".../say?text=hi"` 返回 `200` 且猫咪头顶冒泡。
- 缺 `text` → `400`；设了 `httpToken` 且不带/错 token → `401`。
- 绑定确认：只在 `127.0.0.1` 可达，外网 IP 连不上。
- `duration=0` → 常驻；`duration=5` → 5 秒后消失。

## 10. 改动文件一览

新增：
- `src/pages/chat/index.vue`
- `src/utils/chatPosition.ts`（+ 自检）
- `src/composables/useChat.ts`
- `src/stores/ai.ts`
- `src/pages/preference/components/ai/index.vue`

- `src-tauri/src/core/server.rs`（HTTP 外部推送，tiny_http）

修改：
- `src-tauri/tauri.conf.json`（新增 chat 窗口）
- `src/router`（新增 /chat 路由）
- `src-tauri/src/core/setup/macos.rs`（chat NSPanel + move/resize 改广播）
- `src-tauri/src/core/mod.rs` + `lib.rs`（setup 阶段启动 HTTP server）
- `src-tauri/Cargo.toml`（+ `tiny_http`）
- `src/pages/preference/index.vue`（menus 加 AI tab）
- `src/locales/*`（5 个语言包补 key）
- `src/pages/main/index.vue`（模型加载后调一次 `say` 打招呼）

## 11. 已知简化（ponytail）

- macOS chat 与猫同 NSPanel level + order-front；若层级不准再抬高 `PanelLevel`。
- 拖动猫咪时气泡用 JS 重定位有极轻微跟随延迟；完全消除需原生子窗口（大量平台代码），不值。
- HTTP 接口用 `tiny_http` 单线程阻塞循环（够用），不引入 axum/tokio server 栈。
- HTTP 改端口/开关/token 后需重启 app 生效，不做配置热重载。
- HTTP 仅 `GET /say`，不做 REST/多路由/POST body（YAGNI，curl 一行就够）。
