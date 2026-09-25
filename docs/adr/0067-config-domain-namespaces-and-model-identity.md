# ADR-0067：配置领域命名空间与模型完整身份

- 状态：已接受
- 日期：2026-09-25
- 影响范围：`config.json` v1、settings protocol、Application/runtime shortcut 边界、schema/fixtures

## 背景

初版 Native Rewrite 的配置把不同领域的状态放在过于宽泛的 `application` 和 `model`
section 中：系统集成与更新策略没有独立归属，输入和 overlay 性能字段被放在模型 section，
模型来源使用 `preset`/`installed`，选中模型由两个可分别缺失的字段表示。这样会产生以下
问题：

- `model.maximum_fps` 看似模型属性，实际由 overlay 与 runtime frame scheduler 消费；
- `model.release_fallback_timeout_ms` 看似模型行为，实际只属于键盘输入可靠性；
- `application.show_*` 与 `application.check_for_updates_*` 属于不同领域；
- `installed` 描述存储实现，不能表达用户看到的模型来源；
- 单独的 `model_id` 不能区分同 id 的内置模型与导入模型，快捷键可能串到错误的模型；
- `overlay.visible` 是会话状态，不应被当作跨重启的用户偏好。

## 决策

当前 v1 直接采用以下领域结构，不保留旧字段 alias、迁移或兼容转换：

```text
schema_version
appearance
overlay
input { keyboard, gamepad }
logging
model
shortcuts
system
updates
```

具体规则如下：

1. 系统入口可见性放在 `system.show_taskbar_icon` / `system.show_status_icon`；自动更新策略放在
   `updates.check_automatically` / `updates.check_interval_hours`。
2. `overlay.maximum_fps` 属于 overlay/runtime；键盘释放保险属于
   `input.keyboard.release_fallback_timeout_ms`；手柄死区属于
   `input.gamepad.stick_dead_zone` / `input.gamepad.trigger_dead_zone`。
3. 模型来源统一为 `imported` / `built_in`。配置字段为 `model.imported_models` 与
   `model.built_in_models`；model-store 内部的 `InstalledModel`、`ModelOrigin::Installed` 和
   `ModelOrigin::Preset` 仍是技术层词汇，不直接序列化到配置。
4. 选中模型是一个 nullable 完整对象：`model.selected_model: null` 或
   `{ "id": "...", "source": "imported" | "built_in" }`。模型行为快捷键的 `model` 也必须保存
   同一完整身份，不能只保存 id。
5. 快捷键配置字段为 `shortcuts.command_bindings` 与
   `shortcuts.model_behavior_bindings`，两个独立门禁为 `shortcuts.commands_enabled` 与
   `shortcuts.model_behaviors_enabled`。活动模型投影和冲突检测以完整模型身份为作用域。
6. `overlay.visible` 从 `config.json` 移除。可见性是 runtime snapshot 的会话状态：每个新进程
   从可见状态启动，隐藏只持续到进程退出；runtime/settings snapshot 仍保留 `overlay_visible`。
7. 随机行为使用 `model.random_behavior.enabled` 与
   `model.random_behavior.interval_seconds`，并保持原有范围与默认值。
8. 模型响应来源门禁使用 `model.ignore_keyboard` 与 `model.ignore_gamepad`；它们属于模型
   交互表现，不属于 `input` 的采集/死区配置。门禁只作用于模型输入投影，不能关闭可靠输入、
   pressed-state 恢复、诊断或独立快捷键。三个模型输入忽略开关分别可由
   `toggle_ignore_mouse_input`、`toggle_ignore_keyboard_input` 和 `toggle_ignore_gamepad_input`
   application shortcut target 切换；这些 target 仍走 settings service 的 typed handoff 和
   revision-checked 持久化。

## 结果与边界

- Rust 类型、JSON Schema、默认值、fixtures、Application 映射、settings snapshot、platform
  shortcut target 和 runtime origin 投影必须使用同一组名称。
- 完整模型身份需要贯穿配置编译、平台注册和 Application dispatcher；runtime snapshot 公开
  当前模型的 storage origin，dispatcher 在触发行为前同时比较 id 与 origin。
- 配置损坏时仍按现有“最新有效备份 → 默认配置”规则处理。旧开发配置不会自动转换；结构不完整的
  文件按既有严格 v1 解析/恢复边界处理。
- `InstalledModel` 等技术类型不因本次用户-facing 命名调整而改名；只有配置和 settings-facing
  来源词汇使用 `imported` / `built_in`。

## 验证

- `shared/config/config.schema.json` 与 31 个 config fixtures 通过 Draft 2020-12 验证。
- `cargo test -p bongocat-config` 覆盖默认 round-trip、未知字段拒绝、完整模型身份、嵌套范围和
  fixture manifest。
- `cargo test -p bongocat-app --lib` 覆盖配置到 runtime、快捷键身份传播、可见性会话语义和
  重启边界。
- 独立 `spikes/config-store` contract 与 workspace 测试验证 Rust 实现和共享 fixture 一致。
