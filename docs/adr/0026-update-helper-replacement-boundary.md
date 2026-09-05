# ADR-0026: Update Helper Replacement Boundary

状态：已接受（2026-09-05）

## 背景

ADR-0021 至 ADR-0025 已将 manifest、artifact URL、下载和 staging 限制为不可变、已验证的
项目类型。它们不授权网络 worker、GPUI 或 installer 写入正在运行的 product files。若将这一
边界隐含在后续 helper 中，path substitution、部分替换、权限扩大和 rollback 降级都会绕过已有
的签名与 anti-rollback 结论。

## 威胁模型

- 不可信输入包括 update endpoint、manifest transport、artifact bytes、staging directory contents、
  product directory contents、环境变量、当前工作目录和用户可修改的 registry/launch arguments。
- 可信输入只包括编译进当前 app 的环境、target/arch、trust key、当前 installation root，和由
  VerifiedArtifact 只读 accessor 传递的 release identity、精确长度、digest 与 staging file。
  私钥、endpoint 配置和任意用户配置均不进入 helper。
- helper 与 app 同为当前用户权限；它不得请求 elevation、安装 service/driver、写入 HKLM、修改其他
  用户 profile，或以 Development/Production 用户数据目录作为 product replacement source/target。
- 被攻击者替换、删除或锁定的 app/helper/staging file 必须导致失败关闭和保留已知可运行 product，
  而不是重试到另一路径或启动未知 binary。

## 决策

- update helper 是独立、平台限定的 Rust owner，不属于 bongocat-update、runtime 或 GPUI。它只在
  app 完成 shutdown coordinator 的停止 frame tick、输入、runtime、config flush、audio/renderer/overlay
  release 后启动；app 必须等待 helper acknowledgement 或将本次更新报告为未安装。helper 不接受裸
  URL、manifest、target、版本、安装目录或 shell command-line replacement request。
- 调用方传入的 installation root 必须等于平台固定的 current-user product root。Windows 为
  $LOCALAPPDATA\Programs\BongoCat，macOS 为当前已签名 .app bundle；helper 在写入前重新解析
  root、拒绝 reparse point/symlink/非目录和任何与当前 target/arch 不一致的 payload。它绝不从
  StorageLayout 的 config、state、models、backups、logs 或 updates root 导出安装路径。
- helper 只接受同一环境 UpdateDownloadCoordinator 完成的唯一 staging file，重新检查 regular-file
  identity、所有权、无 link、精确 byte length 与 SHA-256，并执行对应 OS package signature/integrity
  verification。解包后的每个 product file 必须保持在 private same-volume work directory，拒绝 link、
  absolute path、path traversal 和已有 uninstaller；不允许把 archive entry 直接写入 product root。
- 替换采用 prepare -> validate -> commit：先在 product root 同卷创建全新的 private candidate directory，
  验证 candidate 的 target/arch、required resources、OS signature 与 executable launchability；再将
  current root 原子重命名为唯一 rollback directory，将 candidate 原子重命名为 current root。任何
  prepare/validate/commit failure 都不得删除 current root。commit 后 launch failure、health timeout 或
  explicit failure acknowledgement 必须在同一 helper invocation 内将 rollback directory 原子恢复；成功
  acknowledgement 后才删除 rollback directory。
- 同一 installation root 一次只允许一个 helper。lock/pid ownership 失效、父进程未退出、超时、unexpected
  child process 或 ambiguous recovery state 都停止替换并保留可诊断的 anonymous failure code。下一次
  launch 仅可在验证 current/rollback 两个固定 sibling 的签名与 target 后恢复其中一个；不得猜测、
  merge 或扫描其他目录。
- helper 不联网、不解析 JSON、不检查更新、不记录 URL、路径、按键、artifact 内容或签名材料。日志只记录
  stable replacement phase/error code、environment、target 和 release sequence；其保留策略服从应用
  diagnostics contract。Windows NSIS installer 继续只用于初始/显式 per-user packaging，不成为
  background update executor。

## 验证

- 平台 contract tests 覆盖拒绝裸输入、root/staging link substitution、path traversal、cross-environment、
  cross-target、invalid signature、concurrent helper、parent alive、prepare failure、atomic commit failure、
  launch/health failure rollback 与成功后 cleanup。
- Windows 10 1903+ 和 Windows 11 以非管理员 profile 验证真实 Authenticode、lock、upgrade、rollback、
  uninstall 后无残留和 Development/Production 数据保留；macOS 12+ 验证 signed/notarized bundle、
  quarantine、rollback 和 data preservation。
- 断电/kill fault injection 在每个 prepare/rename/acknowledgement 点重启，断言只会启动完整已验证的
  current 或 rollback product，且 never deletes user data。

## 后续边界

本 ADR 不实现 helper、OS signature verification、archive format、health protocol、endpoint、公钥或
release hosting。每个实现需要独立平台代码、自动化与实机证据；在上述验证通过前，更新安装和 stable
发布仍不可宣称完成。
