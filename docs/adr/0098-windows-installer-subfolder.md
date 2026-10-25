# ADR-0098: Windows Installer Subfolder Choice

状态：已接受；Windows 实机验收待完成

## 决策

Issue #1163 的目录选择有两种合理语义。Windows 目录页增加「在所选文件夹中创建 BongoCat
子文件夹」复选框：全新交互安装默认勾选，目录框表示父目录；取消勾选则直接安装到所选目录。
不根据用户输入的末级名称猜测意图。勾选时即使父目录也叫 BongoCat，仍创建子文件夹。

已有注册安装位置和显式 `/D` 指定的是最终路径，默认不勾选；用户仍可主动勾选。
只有目录页 Leave 回调追加路径，Show 回调撤销上次由本页追加的路径，避免 Back/Next 重复嵌套。
静默安装和 passive 更新跳过目录页，不改变既有默认目录、注册目录或 `/D` 语义。

`cargo-packager 0.11.8` 的 `preinstall_section` 出现在页面声明之后，不能配置目录页回调。
经维护者授权，采用其公开 `NsisConfig::template` 接口，限定扩展固定上游模板，不新建独立
安装器或复制构建、升级、卸载、签名实现。此项取代 ADR-0033 禁止自维护模板的绝对约束；
per-user、HKCU、免管理员权限与用户数据隔离边界不变。

## 来源与维护

- 基线：[cargo-packager 0.11.8 模板](https://github.com/crabnebula-dev/cargo-packager/blob/9e88b74ffb5531acf59c8bc371189a5e82b927d2/crates/packager/src/package/nsis/installer.nsi)。
- 许可：Apache-2.0 OR MIT，原始许可证随模板存放于打包 crate 的 installer 目录。
- 控件创建参考固定基线自带 `un.ConfirmShow` 以及
  [Tauri NSIS 安装器](https://github.com/tauri-apps/tauri/blob/1fc6d9c85c7d9c79f44afc408853e4b5d3ac1e42/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi)
  的卸载复选框。这里不引入 Tauri 依赖。
- 使用标准 MUI Directory Show/Leave 回调与 Win32 checkbox，保留原目录验证和 Browse 行为。
  `MapDialogRect` 按实际对话框字体换算坐标，Arabic 保留 RTL 样式；八种安装器语言均有文案。
- 所有自有差异以 `BongoCat extension begin/end` 标记；测试移除这些块后核对固定上游 SHA-256。
  更新依赖时须更新模板基线、许可证与 hash，并重新检查所有差异，不能只改版本号。

## 验收

macOS 可执行模板差异 contract、NSIS 编译检查和 workspace 检查；它们不能代替 Windows 行为验证。
Windows 维护者需验收：

- 全新安装选父目录，默认创建子目录；取消勾选直接安装，含末尾反斜杠和驱动器根目录。
- Back/Next、改目录、反复勾选、已存在的同名父目录不产生非预期嵌套。
- 标准目录和自定义名称目录的重装/升级沿用路径；`/S`、`/P`、`/D` 保持原语义。
- 八种语言、125/150/200% DPI 和 RTL 下复选框不裁剪、不覆盖磁盘空间信息。
- 不可写目录沿用原安装器报错，无新增提权；卸载保留无关文件及所有环境用户数据。

实机安装、升级、卸载和权限证据未取得前，不宣称安装器验证完成或可发布。
