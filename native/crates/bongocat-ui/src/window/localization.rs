use super::SettingsLanguage;

#[derive(Clone, Copy)]
pub(super) enum UiText {
    Settings,
    General,
    GeneralDescription,
    Models,
    Diagnostics,
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
        UiText::Diagnostics => ["Diagnostics", "诊断"],
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
    };
    values[match language {
        SettingsLanguage::ChineseSimplified => 1,
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => 0,
    }]
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
            UiText::Diagnostics,
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
}
