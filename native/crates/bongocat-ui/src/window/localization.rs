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
