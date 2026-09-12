# monio Input Backend Comparison

状态：完成源码对照，不引入生产依赖
日期：2026-08-29
审阅版本：`HuakunShen/monio@d1766e0dcd20dea0435be16cd80adaa749b86e30`

## Snapshot

审阅 commit 的提交日期为 2026-08-19，crate 版本为 `0.2.0`，使用 Rust 2024 edition。仓库仍在活跃开发，审阅时 GitHub 显示 7 stars、1 fork、1 个 open issue，未归档。

`Cargo.toml` 声明 Apache-2.0，README 也链接 `LICENSE`，但该 commit 根目录没有 `LICENSE` 文件，GitHub repository license 字段为空。该缺口不影响本次只读技术对照，但在任何再分发或复制代码前必须由上游补齐或单独确认许可证文本。

## Useful Evidence

monio 有几项实现可作为 BongoCat 的对照材料：

- macOS 使用 `objc2-core-graphics 0.3`，证明 listen/grab event tap、Core Foundation run loop 和 modifier device flag 可以不用旧 `core-graphics2` 实现；
- macOS `FlagsChanged` 使用左右 modifier 的 `NX_DEVICE*` bit，避免一侧释放被另一侧仍按下的通用 mask 遮蔽；
- event tap 收到 timeout/user-disable 后会 re-enable；
- hook、run loop、Raw Input mouse registration 和 stop/join 有显式资源恢复路径；
- Windows grab 模式仅为相对鼠标移动使用 Raw Input，并会保存/恢复进程原有 mouse registration，边界写得较清楚。

这些点可用于审阅 BongoCat 自有 wrapper，但不复制 monio 的公共事件模型或全局状态。

## Reliability Gaps

monio 当前不能满足 BongoCat issue #47 的输入不变量：

1. Windows 普通键盘监听使用 `WH_KEYBOARD_LL`，Raw Input 只用于 grab 模式的鼠标移动。键盘事件由 `WM_KEYDOWN/UP` 的 virtual-key 产生，没有 Raw Input scan code/E0/E1 主路径。
2. Windows 没有 `GetAsyncKeyState` pressed-set 校正，也没有设备移除、session lock、sleep、input desktop 或 UAC 返回 Reset。若 hook 没收到 KeyUp，库内没有机制恢复完整键盘状态。
3. bounded sync/tokio channel 对 `try_send` 结果使用 `let _ = ...`。文档明确队列满时丢弃新事件，但没有 overflow 计数、丢弃边沿分类或 Reset，因此 KeyUp 可以被静默丢弃。
4. macOS tap timeout/user-disable 只 re-enable tap，不向消费者发送 Reset，也没有 `CGEventSourceKeyState` 周期校正。disable 窗口内丢失 release 后仍可能留下调用方 pressed state。
5. Windows/macOS callback 直接在全局 handler mutex 内调用用户 handler，未见 callback panic boundary。慢 handler、panic 或锁竞争会扩大 hook/tap 被系统停用和 shutdown 竞态风险。
6. 全局 state 只维护 modifier/button bitmask，不是带 sequence、来源、校正时间和 Reset reason 的完整 pressed-set；`HookDisabled` 事件也不能替代按原因复位。

因此，monio 能正常上报大多数 down/up，不等于能证明“每个 pressed key 最终由 release、reconcile 或 Reset 清除”。它不能作为 issue #47 修复的验收 oracle。

## Decision

- 不把 monio 加入 BongoCat 产品或 spike dependency graph。
- Windows 继续使用自有 Raw Input keyboard/mouse wrapper、`GetAsyncKeyState` 校正和 lifecycle Reset。
- macOS 继续使用自有 listen-only CGEventTap、bounded reliable edge queue、`CGEventSourceKeyState` 校正和 disable/permission/session Reset。
- 可借鉴 monio 的 objc2 API 用法、modifier device flag 表和资源恢复测试，但任何采用都必须按 BongoCat 类型、队列和安全不变量重新实现与验证。
- 若未来重新评估 monio，最低条件是可靠 edge channel 不静默丢弃、双平台 pressed reconciliation、生命周期 Reset、callback panic isolation 和完整许可证文件；仅增加下载量或平台数量不改变结论。
