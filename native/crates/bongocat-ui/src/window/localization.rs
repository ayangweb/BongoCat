use super::{
    SettingsError, SettingsErrorCode, SettingsInputDiagnostics, SettingsLanguage,
    SettingsModelOrigin, ShortcutCaptureError, ShortcutCaptureTarget,
};

#[derive(Clone, Copy)]
pub(super) enum UiText {
    Settings,
    General,
    GeneralDescription,
    Models,
    ModelsDescription,
    ModelCatalog,
    InstalledModels,
    InstalledModelsDescription,
    ModelId,
    Preset,
    Installed,
    Active,
    Unavailable,
    Activate,
    Cancel,
    Confirm,
    Delete,
    ConfirmDeletion,
    PackageLayoutInvalid,
    PackageSafetyLimitsExceeded,
    ModelDefinitionUnsupported,
    TextureInvalid,
    ModelFilesUnavailable,
    ModelResourceInvalid,
    NoFolderSelected,
    FolderSelected,
    ChoosingFolder,
    SelectionCancelledPreviousRetained,
    SelectionCancelled,
    FolderPickerRequiresUiThread,
    SelectedFolderUnavailable,
    FolderPickerUnavailable,
    CancellingImport,
    StartingImport,
    Preparing,
    Copying,
    Validating,
    Committing,
    ImportComplete,
    ImportCancelled,
    LoadingModels,
    ModelCatalogUnavailable,
    NoModelsAvailable,
    ActivatingModel,
    DeletingModel,
    RefreshingModels,
    CatalogUnavailable,
    Choosing,
    ChooseFolder,
    Import,
    AvailableModels,
    Diagnostics,
    DiagnosticsDescription,
    RuntimeAndInput,
    RuntimeDiagnostics,
    RuntimeDiagnosticsDescription,
    DiagnosticsUnavailable,
    LoadingDiagnostics,
    InputReliabilityCounters,
    RuntimeRenderer,
    DiagnosticsExport,
    NoReportExported,
    Export,
    Shortcuts,
    RestoreDefaults,
    ClearAll,
    PressCommandShortcut,
    PressBehaviorShortcut,
    PressKey,
    Capture,
    InputService,
    Configuration,
    Backups,
    CurrentState,
    InputProcessing,
    SequenceRecovery,
    Transport,
    RestoreDefaultConfiguration,
    RestoreDefaultConfigurationDescription,
    OpenConfigurationBackupsFolder,
    OpenConfigurationBackupsFolderDescription,
    ExportDiagnostics,
    ExportDiagnosticsDescription,
    RestoreDefaultShortcuts,
    RestoreDefaultShortcutsDescription,
    ClearAllShortcuts,
    ClearAllShortcutsDescription,
    WaitingForKeyCombination,
    UnsupportedKey,
    ShortcutAlreadyAssigned,
    NotStarted,
    Running,
    PermissionRequired,
    BackendUnavailable,
    StartupFailed,
    GpuPreparationFailed,
    ModelLoadFailed,
    ModelEvaluationFailed,
    MotionLoadFailed,
    ExpressionLoadFailed,
    PlatformUnsupported,
    RuntimeTransportClosed,
    OverlaySettingsInvalid,
    MaximumFpsInvalid,
    NoRendererError,
    NoCommandFailures,
    ConfigurationUnavailable,
    DefaultsRestored,
    RestartToContinue,
    RecoveredFromBackup,
    LoadedNormally,
    NoRecovery,
    Appearance,
    Overlay,
    Theme,
    System,
    Light,
    Dark,
    ThemeDescription,
    Language,
    LanguageDescription,
    RuntimeStatus,
    RuntimeStatusDescription,
    ShowDesktopCat,
    ShowDesktopCatDescription,
    AlwaysOnTop,
    AlwaysOnTopDescription,
    ClickThroughOverlay,
    ClickThroughOverlayDescription,
    MotionAudio,
    MotionAudioDescription,
    OverlayScale,
    OverlayScaleDescription,
    OverlayOpacity,
    OverlayOpacityDescription,
    MaximumFps,
    MaximumFpsDescription,
    ModelInteraction,
    BehaviorShortcuts,
    BehaviorShortcutsDescription,
    MirrorModel,
    MirrorModelDescription,
    MirrorPointerTracking,
    MirrorPointerTrackingDescription,
    IgnorePointerInput,
    IgnorePointerInputDescription,
    Input,
    GamepadStickDeadZone,
    GamepadStickDeadZoneDescription,
    GamepadTriggerDeadZone,
    GamepadTriggerDeadZoneDescription,
    Application,
    ShowStatusIcon,
    ShowStatusIconDescription,
    #[cfg(target_os = "windows")]
    ShowTaskbarIcon,
    #[cfg(target_os = "windows")]
    ShowTaskbarIconDescription,
    OpenAtLogin,
    DecreaseOverlayScale,
    IncreaseOverlayScale,
    DecreaseOverlayOpacity,
    IncreaseOverlayOpacity,
    DecreaseMaximumFps,
    IncreaseMaximumFps,
    CheckingLoginStartup,
    LoginStartupStatusUnavailable,
    LoginStartupDisabled,
    LoginStartupEnabled,
    LoginStartupStale,
    LoginStartupRequiresApproval,
    LoginStartupNotFound,
    LoginStartupUnsupportedPlatform,
    LoginStartupUnsupportedOperatingSystem,
    LoginStartupUnsupportedBuild,
    Refreshing,
    Saving,
    Connecting,
    Starting,
    Ready,
    Degraded,
    Stopped,
    Refresh,
    Quit,
}

pub(super) fn text(language: SettingsLanguage, key: UiText) -> &'static str {
    let values = match key {
        UiText::Settings => ["BongoCat Settings", "BongoCat 设置"],
        UiText::General => ["General", "通用"],
        UiText::GeneralDescription => [
            "Configure the overlay, model interaction, input and startup behavior.",
            "配置桌面猫、模型交互、输入和启动行为。",
        ],
        UiText::Models => ["Models", "模型"],
        UiText::ModelsDescription => [
            "Install, validate and activate Live2D model packages.",
            "安装、验证并启用 Live2D 模型包。",
        ],
        UiText::ModelCatalog => ["Model catalog", "模型列表"],
        UiText::InstalledModels => ["Installed models", "已安装模型"],
        UiText::InstalledModelsDescription => [
            "Import a model folder, validate package safety and choose the active model.",
            "导入模型文件夹，验证包安全性并选择当前模型。",
        ],
        UiText::ModelId => ["Model ID", "模型 ID"],
        UiText::Preset => ["Preset", "预置"],
        UiText::Installed => ["Installed", "已安装"],
        UiText::Active => ["Active", "当前使用"],
        UiText::Unavailable => ["Unavailable", "不可用"],
        UiText::Activate => ["Activate", "启用"],
        UiText::Cancel => ["Cancel", "取消"],
        UiText::Confirm => ["Confirm", "确认"],
        UiText::Delete => ["Delete", "删除"],
        UiText::ConfirmDeletion => ["Confirm deletion", "确认删除"],
        UiText::PackageLayoutInvalid => ["Package layout is invalid", "模型包结构无效"],
        UiText::PackageSafetyLimitsExceeded => {
            ["Package exceeds safety limits", "模型包超出安全限制"]
        }
        UiText::ModelDefinitionUnsupported => {
            ["Model definition is unsupported", "模型定义不受支持"]
        }
        UiText::TextureInvalid => ["Texture is invalid", "纹理无效"],
        UiText::ModelFilesUnavailable => ["Model files are unavailable", "模型文件不可用"],
        UiText::ModelResourceInvalid => ["Model resource is invalid", "模型资源无效"],
        UiText::NoFolderSelected => ["No folder selected", "尚未选择文件夹"],
        UiText::FolderSelected => ["Folder selected", "已选择文件夹"],
        UiText::ChoosingFolder => ["Choosing folder...", "正在选择文件夹..."],
        UiText::SelectionCancelledPreviousRetained => [
            "Selection cancelled; previous folder retained",
            "已取消选择；保留之前的文件夹",
        ],
        UiText::SelectionCancelled => ["Selection cancelled", "已取消选择"],
        UiText::FolderPickerRequiresUiThread => [
            "Folder picker requires the UI thread",
            "文件夹选择器必须在 UI 线程运行",
        ],
        UiText::SelectedFolderUnavailable => ["Selected folder is unavailable", "所选文件夹不可用"],
        UiText::FolderPickerUnavailable => ["Folder picker is unavailable", "文件夹选择器不可用"],
        UiText::CancellingImport => ["Cancelling import...", "正在取消导入..."],
        UiText::StartingImport => ["Starting import...", "正在开始导入..."],
        UiText::Preparing => ["Preparing", "正在准备"],
        UiText::Copying => ["Copying", "正在复制"],
        UiText::Validating => ["Validating", "正在验证"],
        UiText::Committing => ["Committing", "正在提交"],
        UiText::ImportComplete => ["Import complete", "导入完成"],
        UiText::ImportCancelled => ["Import cancelled", "已取消导入"],
        UiText::LoadingModels => ["Loading models...", "正在加载模型..."],
        UiText::ModelCatalogUnavailable => ["Model catalog is unavailable", "模型列表不可用"],
        UiText::NoModelsAvailable => ["No models available", "没有可用模型"],
        UiText::ActivatingModel => ["Activating model...", "正在启用模型..."],
        UiText::DeletingModel => ["Deleting model...", "正在删除模型..."],
        UiText::RefreshingModels => ["Refreshing models...", "正在刷新模型..."],
        UiText::CatalogUnavailable => ["Catalog unavailable", "模型列表不可用"],
        UiText::Choosing => ["Choosing...", "选择中..."],
        UiText::ChooseFolder => ["Choose folder", "选择文件夹"],
        UiText::Import => ["Import", "导入"],
        UiText::AvailableModels => ["Available models", "可用模型"],
        UiText::Diagnostics => ["Diagnostics", "诊断"],
        UiText::DiagnosticsDescription => [
            "Inspect runtime health, input reliability and shortcut bindings.",
            "检查运行状态、输入可靠性和快捷键绑定。",
        ],
        UiText::RuntimeAndInput => ["Runtime and input", "运行与输入"],
        UiText::RuntimeDiagnostics => ["Runtime diagnostics", "运行诊断"],
        UiText::RuntimeDiagnosticsDescription => [
            "Review renderer status, input counters and keyboard shortcuts. Search: renderer, input, shortcut.",
            "查看渲染状态、输入计数和键盘快捷键。搜索：渲染、输入、快捷键。",
        ],
        UiText::DiagnosticsUnavailable => ["Diagnostics unavailable", "诊断信息不可用"],
        UiText::LoadingDiagnostics => ["Loading diagnostics...", "正在加载诊断信息..."],
        UiText::InputReliabilityCounters => ["Input reliability counters", "输入可靠性计数"],
        UiText::RuntimeRenderer => ["Runtime renderer", "运行时渲染器"],
        UiText::DiagnosticsExport => ["Diagnostics export", "诊断导出"],
        UiText::NoReportExported => ["No report exported", "尚未导出报告"],
        UiText::Export => ["Export", "导出"],
        UiText::Shortcuts => ["Shortcuts", "快捷键"],
        UiText::RestoreDefaults => ["Restore defaults", "恢复默认值"],
        UiText::ClearAll => ["Clear all", "全部清除"],
        UiText::PressCommandShortcut => [
            "Press a key combination for this command",
            "请为此命令按下组合键",
        ],
        UiText::PressBehaviorShortcut => [
            "Press a key combination for this behavior",
            "请为此行为按下组合键",
        ],
        UiText::PressKey => ["Press key", "请按键"],
        UiText::Capture => ["Capture", "录入"],
        UiText::InputService => ["Input service", "输入服务"],
        UiText::Configuration => ["Configuration", "配置"],
        UiText::Backups => ["Backups", "备份"],
        UiText::CurrentState => ["Current state", "当前状态"],
        UiText::InputProcessing => ["Input processing", "输入处理"],
        UiText::SequenceRecovery => ["Sequence recovery", "序列恢复"],
        UiText::Transport => ["Transport", "传输"],
        UiText::RestoreDefaultConfiguration => ["Restore default configuration", "恢复默认配置"],
        UiText::RestoreDefaultConfigurationDescription => [
            "Archive the invalid configuration and create verified defaults",
            "归档无效配置并创建已验证的默认配置",
        ],
        UiText::OpenConfigurationBackupsFolder => {
            ["Open configuration backups folder", "打开配置备份文件夹"]
        }
        UiText::OpenConfigurationBackupsFolderDescription => [
            "Open the current environment's backup folder",
            "打开当前环境的备份文件夹",
        ],
        UiText::ExportDiagnostics => ["Export diagnostics", "导出诊断信息"],
        UiText::ExportDiagnosticsDescription => [
            "Write an anonymous diagnostics report to the current environment logs",
            "将匿名诊断报告写入当前环境的日志目录",
        ],
        UiText::RestoreDefaultShortcuts => ["Restore default shortcuts", "恢复默认快捷键"],
        UiText::RestoreDefaultShortcutsDescription => [
            "Replace custom shortcut bindings with the verified defaults",
            "使用已验证的默认值替换自定义快捷键绑定",
        ],
        UiText::ClearAllShortcuts => ["Clear all shortcuts", "清除全部快捷键"],
        UiText::ClearAllShortcutsDescription => [
            "Remove all custom shortcut bindings",
            "移除全部自定义快捷键绑定",
        ],
        UiText::WaitingForKeyCombination => ["Waiting for a key combination", "正在等待组合键"],
        UiText::UnsupportedKey => ["Unsupported key", "不支持的按键"],
        UiText::ShortcutAlreadyAssigned => ["Shortcut is already assigned", "该快捷键已被占用"],
        UiText::NotStarted => ["Not started", "尚未启动"],
        UiText::Running => ["Running", "运行中"],
        UiText::PermissionRequired => ["Permission required", "需要权限"],
        UiText::BackendUnavailable => ["Backend unavailable", "后端不可用"],
        UiText::StartupFailed => ["Startup failed", "启动失败"],
        UiText::GpuPreparationFailed => ["GPU preparation failed", "GPU 准备失败"],
        UiText::ModelLoadFailed => ["Model load failed", "模型加载失败"],
        UiText::ModelEvaluationFailed => ["Model evaluation failed", "模型计算失败"],
        UiText::MotionLoadFailed => ["Motion load failed", "动作加载失败"],
        UiText::ExpressionLoadFailed => ["Expression load failed", "表情加载失败"],
        UiText::PlatformUnsupported => ["Platform unsupported", "平台不受支持"],
        UiText::RuntimeTransportClosed => ["Runtime transport closed", "运行时传输已关闭"],
        UiText::OverlaySettingsInvalid => ["Overlay settings invalid", "桌面猫设置无效"],
        UiText::MaximumFpsInvalid => ["Maximum FPS invalid", "最大帧率无效"],
        UiText::NoRendererError => ["No renderer error", "没有渲染器错误"],
        UiText::NoCommandFailures => ["No command failures", "没有命令失败"],
        UiText::ConfigurationUnavailable => ["Configuration unavailable", "配置不可用"],
        UiText::DefaultsRestored => ["Defaults restored", "已恢复默认值"],
        UiText::RestartToContinue => ["Restart BongoCat to continue", "请重启 BongoCat 以继续"],
        UiText::RecoveredFromBackup => ["Recovered from backup", "已从备份恢复"],
        UiText::LoadedNormally => ["Loaded normally", "正常加载"],
        UiText::NoRecovery => ["No recovery", "未执行恢复"],
        UiText::Appearance => ["Appearance", "外观"],
        UiText::Overlay => ["Overlay", "桌面猫"],
        UiText::Theme => ["Theme", "主题"],
        UiText::System => ["System", "跟随系统"],
        UiText::Light => ["Light", "浅色"],
        UiText::Dark => ["Dark", "深色"],
        UiText::ThemeDescription => [
            "Follow the system appearance or use a fixed light or dark theme.",
            "跟随系统外观，或固定使用浅色或深色主题。",
        ],
        UiText::Language => ["Language", "语言"],
        UiText::LanguageDescription => [
            "Choose the language used by BongoCat.",
            "选择 BongoCat 使用的语言。",
        ],
        UiText::RuntimeStatus => ["Runtime status", "运行状态"],
        UiText::RuntimeStatusDescription => [
            "Current runtime connection and configuration revision.",
            "当前运行时连接状态和配置修订版本。",
        ],
        UiText::ShowDesktopCat => ["Show desktop cat", "显示桌面猫"],
        UiText::ShowDesktopCatDescription => [
            "Keep the Live2D overlay visible.",
            "保持 Live2D 桌面猫可见。",
        ],
        UiText::AlwaysOnTop => ["Always on top", "始终置顶"],
        UiText::AlwaysOnTopDescription => [
            "Keep the Live2D overlay above other windows.",
            "使 Live2D 桌面猫保持在其他窗口上方。",
        ],
        UiText::ClickThroughOverlay => ["Click-through overlay", "鼠标穿透"],
        UiText::ClickThroughOverlayDescription => [
            "Let pointer input pass through the Live2D overlay.",
            "允许指针事件穿过 Live2D 桌面猫。",
        ],
        UiText::MotionAudio => ["Motion audio", "动作音效"],
        UiText::MotionAudioDescription => [
            "Play audio attached to model motions.",
            "播放模型动作附带的音效。",
        ],
        UiText::OverlayScale => ["Overlay scale", "桌面猫缩放"],
        UiText::OverlayScaleDescription => [
            "Resize the Live2D overlay from 25% to 400%.",
            "在 25% 到 400% 之间调整桌面猫大小。",
        ],
        UiText::OverlayOpacity => ["Overlay opacity", "桌面猫不透明度"],
        UiText::OverlayOpacityDescription => [
            "Adjust the overlay opacity from 1% to 100%.",
            "在 1% 到 100% 之间调整桌面猫不透明度。",
        ],
        UiText::MaximumFps => ["Maximum FPS", "最大帧率"],
        UiText::MaximumFpsDescription => [
            "Limit animation and overlay updates from 15 to 240 FPS.",
            "将动画和桌面猫更新限制在每秒 15 到 240 帧。",
        ],
        UiText::ModelInteraction => ["Model interaction", "模型交互"],
        UiText::BehaviorShortcuts => ["Model behavior shortcuts", "模型行为快捷键"],
        UiText::BehaviorShortcutsDescription => [
            "Allow global shortcuts to trigger motions and expressions.",
            "允许全局快捷键触发模型动作和表情。",
        ],
        UiText::MirrorModel => ["Mirror model", "水平镜像模型"],
        UiText::MirrorModelDescription => [
            "Render the model mirrored horizontally.",
            "水平镜像渲染模型。",
        ],
        UiText::MirrorPointerTracking => ["Mirror pointer tracking", "镜像指针跟随"],
        UiText::MirrorPointerTrackingDescription => [
            "Mirror horizontal pointer movement with the model.",
            "随模型镜像水平方向的指针移动。",
        ],
        UiText::IgnorePointerInput => ["Ignore pointer input", "忽略指针输入"],
        UiText::IgnorePointerInputDescription => [
            "Do not apply pointer movement to the model.",
            "不将指针移动应用到模型。",
        ],
        UiText::Input => ["Input", "输入"],
        UiText::GamepadStickDeadZone => ["Gamepad stick dead zone", "手柄摇杆死区"],
        UiText::GamepadStickDeadZoneDescription => [
            "Ignore small analog stick movement.",
            "忽略摇杆的轻微移动。",
        ],
        UiText::GamepadTriggerDeadZone => ["Gamepad trigger dead zone", "手柄扳机死区"],
        UiText::GamepadTriggerDeadZoneDescription => {
            ["Ignore small trigger movement.", "忽略扳机键的轻微移动。"]
        }
        UiText::Application => ["Application", "应用"],
        UiText::ShowStatusIcon => ["Show status icon", "显示状态图标"],
        UiText::ShowStatusIconDescription => [
            "Show BongoCat in the system tray or menu bar.",
            "在系统托盘或菜单栏中显示 BongoCat。",
        ],
        #[cfg(target_os = "windows")]
        UiText::ShowTaskbarIcon => ["Show taskbar icon", "显示任务栏图标"],
        #[cfg(target_os = "windows")]
        UiText::ShowTaskbarIconDescription => [
            "Show the settings window in the Windows taskbar.",
            "在 Windows 任务栏中显示设置窗口。",
        ],
        UiText::OpenAtLogin => ["Open at login", "登录时启动"],
        UiText::DecreaseOverlayScale => ["Decrease overlay scale", "缩小桌面猫"],
        UiText::IncreaseOverlayScale => ["Increase overlay scale", "放大桌面猫"],
        UiText::DecreaseOverlayOpacity => ["Decrease overlay opacity", "降低桌面猫不透明度"],
        UiText::IncreaseOverlayOpacity => ["Increase overlay opacity", "提高桌面猫不透明度"],
        UiText::DecreaseMaximumFps => ["Decrease maximum FPS", "降低最大帧率"],
        UiText::IncreaseMaximumFps => ["Increase maximum FPS", "提高最大帧率"],
        UiText::CheckingLoginStartup => ["Checking login startup...", "正在检查登录启动状态..."],
        UiText::LoginStartupStatusUnavailable => [
            "Status unavailable; activate to retry",
            "状态不可用；激活控件以重试",
        ],
        UiText::LoginStartupDisabled => {
            ["Open BongoCat when you sign in", "登录系统时启动 BongoCat"]
        }
        UiText::LoginStartupEnabled => [
            "BongoCat opens when you sign in",
            "BongoCat 将在登录系统时启动",
        ],
        UiText::LoginStartupStale => [
            "Saved app location changed; enable to repair",
            "应用位置已变化；启用以修复",
        ],
        UiText::LoginStartupRequiresApproval => [
            "Approval required in System Settings",
            "需要在系统设置中批准",
        ],
        UiText::LoginStartupNotFound => [
            "App login item is missing; enable to repair",
            "登录项缺失；启用以修复",
        ],
        UiText::LoginStartupUnsupportedPlatform => [
            "Login startup is unavailable on this platform",
            "此平台不支持登录时启动",
        ],
        UiText::LoginStartupUnsupportedOperatingSystem => [
            "Login startup requires macOS 13 or later",
            "登录时启动需要 macOS 13 或更高版本",
        ],
        UiText::LoginStartupUnsupportedBuild => [
            "Login startup is unavailable in development builds",
            "开发构建不支持登录时启动",
        ],
        UiText::Refreshing => ["Refreshing runtime snapshot...", "正在刷新运行状态..."],
        UiText::Saving => ["Saving changes...", "正在保存更改..."],
        UiText::Connecting => ["Connecting to runtime...", "正在连接运行时..."],
        UiText::Starting => ["Starting", "正在启动"],
        UiText::Ready => ["Ready", "就绪"],
        UiText::Degraded => ["Degraded", "部分功能不可用"],
        UiText::Stopped => ["Stopped", "已停止"],
        UiText::Refresh => ["Refresh", "刷新"],
        UiText::Quit => ["Quit", "退出"],
    };
    values[match language {
        SettingsLanguage::ChineseSimplified => 1,
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => 0,
    }]
}

pub(super) fn model_availability_summary(
    language: SettingsLanguage,
    origin: SettingsModelOrigin,
    active: bool,
    texture_count: usize,
    expression_count: usize,
    motion_count: usize,
) -> String {
    let origin = text(
        language,
        match origin {
            SettingsModelOrigin::Preset => UiText::Preset,
            SettingsModelOrigin::Installed => UiText::Installed,
        },
    );
    let active = active.then(|| text(language, UiText::Active));
    match language {
        SettingsLanguage::ChineseSimplified => format!(
            "{origin}{} · {texture_count} 个纹理 · {expression_count} 个表情 · {motion_count} 个动作",
            active.map_or(String::new(), |active| format!(" · {active}"))
        ),
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => format!(
            "{origin}{} · {texture_count} textures · {expression_count} expressions · {motion_count} motions",
            active.map_or(String::new(), |active| format!(" · {active}"))
        ),
    }
}

pub(super) fn diagnostics_unavailable(language: SettingsLanguage, error: SettingsError) -> String {
    format!(
        "{} · {}",
        text(language, UiText::DiagnosticsUnavailable),
        settings_error(language, error)
    )
}

pub(super) fn diagnostics_export_status(
    language: SettingsLanguage,
    bytes_written: Option<u64>,
) -> String {
    match (language, bytes_written) {
        (SettingsLanguage::ChineseSimplified, Some(bytes)) => {
            format!("已导出 {bytes} 字节")
        }
        (SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates, Some(bytes)) => {
            format!("Exported {bytes} bytes")
        }
        (_, None) => text(language, UiText::NoReportExported).to_owned(),
    }
}

pub(super) fn input_service_attempts(language: SettingsLanguage, attempts: u64) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => format!("启动尝试：{attempts}"),
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => {
            format!("Start attempts: {attempts}")
        }
    }
}

pub(super) fn runtime_command_failure(
    language: SettingsLanguage,
    error: &str,
    sequence: u64,
) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => format!("{error} · 命令 #{sequence}"),
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => {
            format!("{error} · command #{sequence}")
        }
    }
}

pub(super) fn backup_candidates_checked(
    language: SettingsLanguage,
    checked_backups: u32,
) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => {
            format!("已检查 {checked_backups} 个备份候选")
        }
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => format!(
            "{} backup candidate{} checked",
            checked_backups,
            if checked_backups == 1 { "" } else { "s" }
        ),
    }
}

pub(super) fn recovered_backup_detail(
    language: SettingsLanguage,
    schema_version: u32,
    skipped_newer_backups: u32,
) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => {
            format!("Schema v{schema_version} · 已跳过 {skipped_newer_backups} 个较新的备份")
        }
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => format!(
            "Schema v{} · {} newer backup{} skipped",
            schema_version,
            skipped_newer_backups,
            if skipped_newer_backups == 1 { "" } else { "s" }
        ),
    }
}

pub(super) fn shortcut_accessibility_label(
    language: SettingsLanguage,
    target: &ShortcutCaptureTarget,
) -> String {
    let name = shortcut_target_name(language, target);
    match language {
        SettingsLanguage::ChineseSimplified => format!("为{name}录入快捷键"),
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => {
            format!("Capture shortcut for {name}")
        }
    }
}

pub(super) fn shortcut_capture_error(
    language: SettingsLanguage,
    error: ShortcutCaptureError,
) -> &'static str {
    text(
        language,
        match error {
            ShortcutCaptureError::UnsupportedKey => UiText::UnsupportedKey,
            ShortcutCaptureError::AlreadyAssigned => UiText::ShortcutAlreadyAssigned,
        },
    )
}

pub(super) fn shortcut_target_name(
    language: SettingsLanguage,
    target: &ShortcutCaptureTarget,
) -> String {
    match target {
        ShortcutCaptureTarget::Command(command) => {
            let values = match command.as_str() {
                "toggle_overlay" => ["Show or hide desktop cat", "显示或隐藏桌面猫"],
                "open_settings" => ["Open settings", "打开设置"],
                "toggle_mirror" => ["Toggle model mirror", "切换模型镜像"],
                "toggle_click_through" => ["Toggle click-through", "切换鼠标穿透"],
                "toggle_always_on_top" => ["Toggle always on top", "切换始终置顶"],
                _ => return command.clone(),
            };
            values[match language {
                SettingsLanguage::ChineseSimplified => 1,
                SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => 0,
            }]
            .to_owned()
        }
        ShortcutCaptureTarget::ModelBehavior {
            model_id,
            behavior_id,
        } => format!("{model_id} ({behavior_id})"),
    }
}

pub(super) fn input_diagnostic_metrics(
    language: SettingsLanguage,
    diagnostics: SettingsInputDiagnostics,
) -> [(&'static str, u64); 25] {
    let labels = match language {
        SettingsLanguage::ChineseSimplified => [
            "按下的按键",
            "按下的鼠标按键",
            "按下的手柄按键",
            "已连接的手柄",
            "捕获的按下事件",
            "捕获的释放事件",
            "校正释放",
            "重置释放",
            "重复按下",
            "未匹配的释放",
            "无效来源",
            "重置次数",
            "序列缺口",
            "缺失事件",
            "重复事件",
            "乱序事件",
            "非单调时间戳",
            "手柄连接",
            "手柄断开",
            "过期手柄事件",
            "断开时释放",
            "入队事件",
            "队列溢出",
            "溢出恢复",
            "关闭后拒绝",
        ],
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => [
            "Pressed keys",
            "Pressed mouse buttons",
            "Pressed gamepad buttons",
            "Connected gamepads",
            "Captured presses",
            "Captured releases",
            "Reconciled releases",
            "Released by reset",
            "Duplicate presses",
            "Unmatched releases",
            "Invalid sources",
            "Resets",
            "Sequence gaps",
            "Missing events",
            "Duplicate events",
            "Out-of-order events",
            "Non-monotonic timestamps",
            "Gamepad connections",
            "Gamepad disconnections",
            "Stale gamepad events",
            "Released on disconnect",
            "Events enqueued",
            "Queue overflows",
            "Overflow recoveries",
            "Rejected after shutdown",
        ],
    };
    let values = [
        diagnostics.pressed_key_count as u64,
        diagnostics.pressed_mouse_button_count as u64,
        diagnostics.pressed_gamepad_button_count as u64,
        diagnostics.connected_gamepad_count as u64,
        diagnostics.captured_down,
        diagnostics.captured_up,
        diagnostics.reconciled_release,
        diagnostics.released_by_reset,
        diagnostics.duplicate_down,
        diagnostics.unmatched_release,
        diagnostics.invalid_source,
        diagnostics.reset_count,
        diagnostics.sequence_gap_count,
        diagnostics.missing_sequence_count,
        diagnostics.duplicate_sequence_count,
        diagnostics.out_of_order_sequence_count,
        diagnostics.non_monotonic_time_count,
        diagnostics.gamepad_connections,
        diagnostics.gamepad_disconnections,
        diagnostics.stale_gamepad_events,
        diagnostics.released_by_disconnect,
        diagnostics.transport_enqueued,
        diagnostics.transport_queue_full,
        diagnostics.transport_recovered_after_overflow,
        diagnostics.transport_runtime_stopped,
    ];
    std::array::from_fn(|index| (labels[index], values[index]))
}

pub(super) fn settings_error(language: SettingsLanguage, error: SettingsError) -> &'static str {
    let values = match error.code() {
        SettingsErrorCode::ServiceUnavailable => {
            ["settings service is unavailable", "设置服务不可用"]
        }
        SettingsErrorCode::SnapshotOutdated => [
            "settings changed in the background; review the latest values and retry",
            "设置已在后台更改；请检查最新值后重试",
        ],
        SettingsErrorCode::RuntimeUnavailable => {
            ["runtime did not apply the setting", "运行时未应用此设置"]
        }
        SettingsErrorCode::InvalidMaximumFps => [
            "maximum FPS must be between 15 and 240",
            "最大帧率必须在 15 到 240 之间",
        ],
        SettingsErrorCode::InvalidGamepadAxisSettings => [
            "gamepad dead-zone settings are out of range",
            "手柄死区设置超出范围",
        ],
        SettingsErrorCode::InvalidShortcutBindings => [
            "shortcut bindings are invalid or conflict",
            "快捷键绑定无效或存在冲突",
        ],
        SettingsErrorCode::ConfigPersistFailed => ["setting could not be saved", "无法保存设置"],
        SettingsErrorCode::ConfigPermissionDenied => [
            "configuration storage is not writable; check permissions and retry",
            "配置存储不可写；请检查权限后重试",
        ],
        SettingsErrorCode::ConfigStorageFull => [
            "configuration storage is full; free space and retry",
            "配置存储空间已满；请释放空间后重试",
        ],
        SettingsErrorCode::ConfigTargetOccupied => [
            "configuration storage is blocked; remove the blocking item and retry",
            "配置存储位置被占用；请移除占用项后重试",
        ],
        SettingsErrorCode::BackupLocationOpenFailed => [
            "configuration backup folder could not be opened",
            "无法打开配置备份文件夹",
        ],
        SettingsErrorCode::ConfigurationRecoveryRequired => [
            "configuration must be recovered before this action",
            "执行此操作前必须恢复配置",
        ],
        SettingsErrorCode::ConfigurationRecoveryFailed => [
            "default configuration could not be restored",
            "无法恢复默认配置",
        ],
        SettingsErrorCode::ModelUnavailable => ["selected model is unavailable", "所选模型不可用"],
        SettingsErrorCode::ModelSwitchFailed => {
            ["selected model could not be activated", "无法启用所选模型"]
        }
        SettingsErrorCode::InvalidModelId => ["model id is invalid", "模型 ID 无效"],
        SettingsErrorCode::ModelAlreadyInstalled => {
            ["model id is already installed", "该模型 ID 已安装"]
        }
        SettingsErrorCode::ModelImportInvalidPackage => ["model package is invalid", "模型包无效"],
        SettingsErrorCode::ModelImportSourceInvalid => {
            ["model source cannot be imported", "无法导入模型来源"]
        }
        SettingsErrorCode::ModelImportSourceChanged => [
            "model source changed during import",
            "模型来源在导入期间发生变化",
        ],
        SettingsErrorCode::ModelImportSourceUnsupported => [
            "model source contains an unsupported entry",
            "模型来源包含不支持的项目",
        ],
        SettingsErrorCode::ModelImportCancelled => ["model import was cancelled", "模型导入已取消"],
        SettingsErrorCode::ModelStoreBusy => ["model storage is busy", "模型存储正忙"],
        SettingsErrorCode::ModelImportFailed => ["model could not be imported", "无法导入模型"],
        SettingsErrorCode::PresetModelCannotBeDeleted => {
            ["preset model cannot be deleted", "无法删除预置模型"]
        }
        SettingsErrorCode::SelectedModelCannotBeDeleted => [
            "selected model must be replaced before deletion",
            "删除当前模型前必须先切换到其他模型",
        ],
        SettingsErrorCode::ModelNotInstalled => {
            ["installed model was not found", "找不到已安装模型"]
        }
        SettingsErrorCode::ModelDeleteFailed => {
            ["installed model could not be deleted", "无法删除已安装模型"]
        }
        SettingsErrorCode::DiagnosticsExportFailed => {
            ["diagnostics could not be exported", "无法导出诊断信息"]
        }
        SettingsErrorCode::StartupItemUpdateFailed => [
            "startup setting could not be updated",
            "无法更新登录启动设置",
        ],
        SettingsErrorCode::StatusIconUpdateFailed => [
            "status icon visibility could not be updated",
            "无法更新状态图标可见性",
        ],
        SettingsErrorCode::TaskbarIconUpdateFailed => [
            "taskbar icon visibility could not be updated",
            "无法更新任务栏图标可见性",
        ],
        SettingsErrorCode::WindowUnavailable => {
            ["settings window could not be hidden", "无法隐藏设置窗口"]
        }
        SettingsErrorCode::StatePersistFailed => {
            ["window layout could not be saved", "无法保存窗口布局"]
        }
        SettingsErrorCode::ShutdownFailed => {
            ["application shutdown did not complete", "应用未能完成关闭"]
        }
    };
    values[match language {
        SettingsLanguage::ChineseSimplified => 1,
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => 0,
    }]
}

pub(super) fn model_invalid_summary(
    language: SettingsLanguage,
    origin: SettingsModelOrigin,
    diagnostic: &str,
) -> String {
    let origin = text(
        language,
        match origin {
            SettingsModelOrigin::Preset => UiText::Preset,
            SettingsModelOrigin::Installed => UiText::Installed,
        },
    );
    format!("{origin} · {diagnostic}")
}

pub(super) fn model_delete_confirmation(language: SettingsLanguage, status: &str) -> String {
    format!("{status} · {}", text(language, UiText::ConfirmDeletion))
}

pub(super) fn model_import_progress(
    language: SettingsLanguage,
    stage: &str,
    files_copied: u64,
    bytes_copied: u64,
) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => {
            format!("{stage} · {files_copied} 个文件 · {bytes_copied} 字节")
        }
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => {
            format!("{stage} · {files_copied} files · {bytes_copied} bytes")
        }
    }
}

pub(super) fn runtime_status(language: SettingsLanguage, health: &str, revision: u64) -> String {
    match language {
        SettingsLanguage::ChineseSimplified => {
            format!("运行状态：{health} - 修订 {revision}")
        }
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => {
            format!("Runtime {health} - revision {revision}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_display_languages_have_nonempty_shell_text() {
        let keys = [
            UiText::Settings,
            UiText::General,
            UiText::Models,
            UiText::ModelsDescription,
            UiText::ModelCatalog,
            UiText::InstalledModels,
            UiText::InstalledModelsDescription,
            UiText::ModelId,
            UiText::Preset,
            UiText::Installed,
            UiText::Active,
            UiText::Unavailable,
            UiText::Activate,
            UiText::Cancel,
            UiText::Confirm,
            UiText::Delete,
            UiText::ConfirmDeletion,
            UiText::PackageLayoutInvalid,
            UiText::PackageSafetyLimitsExceeded,
            UiText::ModelDefinitionUnsupported,
            UiText::TextureInvalid,
            UiText::ModelFilesUnavailable,
            UiText::ModelResourceInvalid,
            UiText::NoFolderSelected,
            UiText::FolderSelected,
            UiText::ChoosingFolder,
            UiText::SelectionCancelledPreviousRetained,
            UiText::SelectionCancelled,
            UiText::FolderPickerRequiresUiThread,
            UiText::SelectedFolderUnavailable,
            UiText::FolderPickerUnavailable,
            UiText::CancellingImport,
            UiText::StartingImport,
            UiText::Preparing,
            UiText::Copying,
            UiText::Validating,
            UiText::Committing,
            UiText::ImportComplete,
            UiText::ImportCancelled,
            UiText::LoadingModels,
            UiText::ModelCatalogUnavailable,
            UiText::NoModelsAvailable,
            UiText::ActivatingModel,
            UiText::DeletingModel,
            UiText::RefreshingModels,
            UiText::CatalogUnavailable,
            UiText::Choosing,
            UiText::ChooseFolder,
            UiText::Import,
            UiText::AvailableModels,
            UiText::Diagnostics,
            UiText::DiagnosticsDescription,
            UiText::RuntimeAndInput,
            UiText::RuntimeDiagnostics,
            UiText::RuntimeDiagnosticsDescription,
            UiText::DiagnosticsUnavailable,
            UiText::LoadingDiagnostics,
            UiText::InputReliabilityCounters,
            UiText::RuntimeRenderer,
            UiText::DiagnosticsExport,
            UiText::NoReportExported,
            UiText::Export,
            UiText::Shortcuts,
            UiText::RestoreDefaults,
            UiText::ClearAll,
            UiText::PressCommandShortcut,
            UiText::PressBehaviorShortcut,
            UiText::PressKey,
            UiText::Capture,
            UiText::InputService,
            UiText::Configuration,
            UiText::Backups,
            UiText::CurrentState,
            UiText::InputProcessing,
            UiText::SequenceRecovery,
            UiText::Transport,
            UiText::RestoreDefaultConfiguration,
            UiText::RestoreDefaultConfigurationDescription,
            UiText::OpenConfigurationBackupsFolder,
            UiText::OpenConfigurationBackupsFolderDescription,
            UiText::ExportDiagnostics,
            UiText::ExportDiagnosticsDescription,
            UiText::RestoreDefaultShortcuts,
            UiText::RestoreDefaultShortcutsDescription,
            UiText::ClearAllShortcuts,
            UiText::ClearAllShortcutsDescription,
            UiText::WaitingForKeyCombination,
            UiText::UnsupportedKey,
            UiText::ShortcutAlreadyAssigned,
            UiText::NotStarted,
            UiText::Running,
            UiText::PermissionRequired,
            UiText::BackendUnavailable,
            UiText::StartupFailed,
            UiText::GpuPreparationFailed,
            UiText::ModelLoadFailed,
            UiText::ModelEvaluationFailed,
            UiText::MotionLoadFailed,
            UiText::ExpressionLoadFailed,
            UiText::PlatformUnsupported,
            UiText::RuntimeTransportClosed,
            UiText::OverlaySettingsInvalid,
            UiText::MaximumFpsInvalid,
            UiText::NoRendererError,
            UiText::NoCommandFailures,
            UiText::ConfigurationUnavailable,
            UiText::DefaultsRestored,
            UiText::RestartToContinue,
            UiText::RecoveredFromBackup,
            UiText::LoadedNormally,
            UiText::NoRecovery,
            UiText::Language,
            UiText::RuntimeStatus,
            UiText::RuntimeStatusDescription,
            UiText::ShowDesktopCat,
            UiText::ShowDesktopCatDescription,
            UiText::AlwaysOnTop,
            UiText::AlwaysOnTopDescription,
            UiText::ClickThroughOverlay,
            UiText::ClickThroughOverlayDescription,
            UiText::MotionAudio,
            UiText::MotionAudioDescription,
            UiText::OverlayScale,
            UiText::OverlayScaleDescription,
            UiText::OverlayOpacity,
            UiText::OverlayOpacityDescription,
            UiText::MaximumFps,
            UiText::MaximumFpsDescription,
            UiText::ModelInteraction,
            UiText::BehaviorShortcuts,
            UiText::BehaviorShortcutsDescription,
            UiText::MirrorModel,
            UiText::MirrorModelDescription,
            UiText::MirrorPointerTracking,
            UiText::MirrorPointerTrackingDescription,
            UiText::IgnorePointerInput,
            UiText::IgnorePointerInputDescription,
            UiText::Input,
            UiText::GamepadStickDeadZone,
            UiText::GamepadStickDeadZoneDescription,
            UiText::GamepadTriggerDeadZone,
            UiText::GamepadTriggerDeadZoneDescription,
            UiText::Application,
            UiText::ShowStatusIcon,
            UiText::ShowStatusIconDescription,
            #[cfg(target_os = "windows")]
            UiText::ShowTaskbarIcon,
            #[cfg(target_os = "windows")]
            UiText::ShowTaskbarIconDescription,
            UiText::OpenAtLogin,
            UiText::DecreaseOverlayScale,
            UiText::IncreaseOverlayScale,
            UiText::DecreaseOverlayOpacity,
            UiText::IncreaseOverlayOpacity,
            UiText::DecreaseMaximumFps,
            UiText::IncreaseMaximumFps,
            UiText::CheckingLoginStartup,
            UiText::LoginStartupStatusUnavailable,
            UiText::LoginStartupDisabled,
            UiText::LoginStartupEnabled,
            UiText::LoginStartupStale,
            UiText::LoginStartupRequiresApproval,
            UiText::LoginStartupNotFound,
            UiText::LoginStartupUnsupportedPlatform,
            UiText::LoginStartupUnsupportedOperatingSystem,
            UiText::LoginStartupUnsupportedBuild,
            UiText::Refresh,
            UiText::Quit,
        ];
        for language in [
            SettingsLanguage::EnglishUnitedStates,
            SettingsLanguage::ChineseSimplified,
        ] {
            assert!(keys.into_iter().all(|key| !text(language, key).is_empty()));
        }
        assert!(keys.into_iter().any(|key| {
            text(SettingsLanguage::ChineseSimplified, key)
                != text(SettingsLanguage::EnglishUnitedStates, key)
        }));
        assert!(keys.into_iter().all(|key| {
            text(SettingsLanguage::System, key) == text(SettingsLanguage::EnglishUnitedStates, key)
        }));
    }

    #[test]
    fn stable_settings_errors_have_complete_localized_messages() {
        for code in SettingsErrorCode::ALL {
            let error = SettingsError::new(code);
            assert_eq!(
                settings_error(SettingsLanguage::EnglishUnitedStates, error),
                error.to_string()
            );
            assert_ne!(
                settings_error(SettingsLanguage::ChineseSimplified, error),
                settings_error(SettingsLanguage::EnglishUnitedStates, error)
            );
            assert_eq!(
                settings_error(SettingsLanguage::System, error),
                settings_error(SettingsLanguage::EnglishUnitedStates, error)
            );
        }
    }
}
